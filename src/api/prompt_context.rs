//! `GET /api/v1/prompt-context/atoms` — the Passive Trajectory Index (L1).
//!
//! Phase 1a of the v0.4 trajectory-prompt-context roadmap
//! (`docs/roadmap/v0.4-trajectory-prompt-context-implementation.md`). This is
//! the deterministic, no-LLM read layer over the trajectory-signal fragments
//! that the Claude Code ingest extractor already stores
//! (`decision_made`, `failure`, `verification`, `read_context`, `evidence_ref`,
//! `prompt_directive`, `assumption`, `artifact`, `memory_candidate`,
//! `risk_flag`).
//!
//! Where `GET /api/v1/sessions/:id/trajectory` walks ONE session's fragments,
//! this endpoint walks the whole substrate and returns a flat, filterable list
//! of trajectory atoms across every session — the corpus a future prompt
//! compiler distils into capsules. It reuses `TRAJECTORY_KINDS` and
//! `parse_since` from the sessions module so the two stay in lockstep.
//!
//! Deliberately L1: pure metadata filtering + aggregation. No LLM call, no SQL,
//! no capsule injection. Those are later phases — see the roadmap's
//! "Earn the LLM call" principle.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use super::sessions::{parse_since, TRAJECTORY_KINDS};
use crate::services::ContextNestServices;

/// Hard ceiling on returned atoms regardless of the requested `limit`, so a
/// single request can't materialise an unbounded payload from a large corpus.
const MAX_LIMIT: usize = 1000;
const DEFAULT_LIMIT: usize = 200;
const DEFAULT_SINCE: &str = "30d";

#[derive(Debug, Deserialize)]
pub struct AtomsQuery {
    /// Restrict to a single trajectory kind (e.g. `decision_made`). Must be one
    /// of `TRAJECTORY_KINDS`; an unknown kind is a 400 rather than a silent
    /// empty result, so a typo surfaces immediately.
    pub kind: Option<String>,
    /// Case-sensitive substring match on `project_cwd`.
    pub project: Option<String>,
    /// Exact match on `src_session` (the originating session UUID).
    pub session_id: Option<String>,
    /// Age window suffix (`30d`, `24h`, `90m`). Defaults to `30d`.
    pub since: Option<String>,
    /// Max atoms returned (post-filter). Defaults to 200, capped at 1000.
    pub limit: Option<usize>,
}

/// One trajectory signal, flattened for cross-session consumption.
#[derive(Debug, Serialize)]
pub struct TrajectoryAtom {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub session_id: String,
    pub project: Option<String>,
    pub ts: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Serialize)]
pub struct AtomsResponse {
    /// Echoes the effective `since` window actually applied.
    pub since: String,
    /// Number of atoms in `atoms` (post-`limit`).
    pub count: usize,
    /// Number of atoms that matched the filters BEFORE `limit` truncation.
    /// `count < total_matched` means the page was capped.
    pub total_matched: usize,
    /// Histogram over the full matched set (not the truncated page), so the
    /// distribution stays honest when `atoms` is capped.
    pub by_kind: HashMap<String, usize>,
    pub atoms: Vec<TrajectoryAtom>,
}

/// `GET /api/v1/prompt-context/atoms`
pub async fn list_atoms(
    State(services): State<ContextNestServices>,
    Query(q): Query<AtomsQuery>,
) -> Result<Json<AtomsResponse>, StatusCode> {
    // Validate `kind` up front — an unknown kind never matches anything, so
    // returning it as a 400 is more useful than an empty 200.
    if let Some(k) = q.kind.as_deref() {
        if !TRAJECTORY_KINDS.contains(&k) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    let since_raw = q.since.as_deref().unwrap_or(DEFAULT_SINCE);
    let dur = parse_since(since_raw).unwrap_or_else(|| chrono::Duration::days(30));
    let cutoff = chrono::Utc::now() - dur;
    let limit = q.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);

    let texts = services.fragment_texts.read().await;
    let metadata = services.fragment_metadata.read().await;

    let mut atoms: Vec<TrajectoryAtom> = Vec::new();
    let mut by_kind: HashMap<String, usize> = HashMap::new();

    for (frag_id, meta) in metadata.iter() {
        let kind = meta.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if !TRAJECTORY_KINDS.contains(&kind) {
            continue;
        }
        if let Some(want) = q.kind.as_deref() {
            if kind != want {
                continue;
            }
        }

        let ts_str = meta.get("ts").and_then(|v| v.as_str());
        if let Some(ts) = ts_str {
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(ts) {
                if parsed.with_timezone(&chrono::Utc) < cutoff {
                    continue;
                }
            }
            // Unparseable ts → keep it; better to over-report than silently
            // drop a signal (mirrors `list_features`).
        }

        let project = meta
            .get("project_cwd")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(want) = q.project.as_deref() {
            match project.as_deref() {
                Some(p) if p.contains(want) => {}
                _ => continue,
            }
        }

        let session_id = meta
            .get("src_session")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        if let Some(want) = q.session_id.as_deref() {
            if session_id != want {
                continue;
            }
        }

        *by_kind.entry(kind.to_string()).or_insert(0) += 1;

        atoms.push(TrajectoryAtom {
            id: frag_id.clone(),
            kind: kind.to_string(),
            text: texts.get(frag_id).cloned().unwrap_or_default(),
            session_id,
            project,
            ts: ts_str.map(|s| s.to_string()),
            metadata: Value::Object(meta.clone().into_iter().collect()),
        });
    }
    drop(texts);
    drop(metadata);

    // Newest first; deterministic tiebreak on id so pages are stable across
    // calls (HashMap iteration order is otherwise non-deterministic).
    atoms.sort_by(|a, b| {
        b.ts.as_deref()
            .unwrap_or("")
            .cmp(a.ts.as_deref().unwrap_or(""))
            .then_with(|| a.id.cmp(&b.id))
    });

    let total_matched = atoms.len();
    atoms.truncate(limit);

    Ok(Json(AtomsResponse {
        since: since_raw.to_string(),
        count: atoms.len(),
        total_matched,
        by_kind,
        atoms,
    }))
}

/// Build the prompt-context router. Mounted in `create_simple_app`.
pub fn create_prompt_context_router() -> Router<ContextNestServices> {
    Router::new().route("/api/v1/prompt-context/atoms", get(list_atoms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ContextNestServices;
    use serde_json::json;

    async fn seed(services: &ContextNestServices) {
        let now = chrono::Utc::now();
        let recent = now.to_rfc3339();
        let old = (now - chrono::Duration::days(120)).to_rfc3339();

        let mut texts = services.fragment_texts.write().await;
        let mut meta = services.fragment_metadata.write().await;

        // Recent decision in project A, session S1.
        texts.insert("f1".into(), "chose hand-rolled JSON-RPC".into());
        meta.insert(
            "f1".into(),
            [
                ("kind".to_string(), json!("decision_made")),
                ("ts".to_string(), json!(recent)),
                ("src_session".to_string(), json!("S1")),
                ("project_cwd".to_string(), json!("/repo/alpha")),
            ]
            .into_iter()
            .collect(),
        );

        // Recent failure in project B, session S2.
        texts.insert("f2".into(), "cargo test failed on lock-across-await".into());
        meta.insert(
            "f2".into(),
            [
                ("kind".to_string(), json!("failure")),
                ("ts".to_string(), json!(recent)),
                ("src_session".to_string(), json!("S2")),
                ("project_cwd".to_string(), json!("/repo/beta")),
            ]
            .into_iter()
            .collect(),
        );

        // Old decision — should fall outside the default 30d window.
        texts.insert("f3".into(), "ancient choice".into());
        meta.insert(
            "f3".into(),
            [
                ("kind".to_string(), json!("decision_made")),
                ("ts".to_string(), json!(old)),
                ("src_session".to_string(), json!("S1")),
                ("project_cwd".to_string(), json!("/repo/alpha")),
            ]
            .into_iter()
            .collect(),
        );

        // Non-trajectory kind — must never appear.
        texts.insert("f4".into(), "shipped the endpoint".into());
        meta.insert(
            "f4".into(),
            [
                ("kind".to_string(), json!("feature")),
                ("ts".to_string(), json!(recent)),
                ("src_session".to_string(), json!("S1")),
                ("project_cwd".to_string(), json!("/repo/alpha")),
            ]
            .into_iter()
            .collect(),
        );
    }

    #[tokio::test]
    async fn returns_only_recent_trajectory_atoms() {
        let services = ContextNestServices::new_default().await.expect("services");
        seed(&services).await;

        let resp = list_atoms(
            State(services),
            Query(AtomsQuery {
                kind: None,
                project: None,
                session_id: None,
                since: None,
                limit: None,
            }),
        )
        .await
        .expect("ok")
        .0;

        // f1 + f2 only: f3 too old, f4 not a trajectory kind.
        assert_eq!(resp.count, 2);
        assert_eq!(resp.total_matched, 2);
        let ids: Vec<&str> = resp.atoms.iter().map(|a| a.id.as_str()).collect();
        assert!(ids.contains(&"f1"));
        assert!(ids.contains(&"f2"));
        assert!(!ids.contains(&"f3"));
        assert!(!ids.contains(&"f4"));
        assert_eq!(resp.by_kind.get("decision_made"), Some(&1));
        assert_eq!(resp.by_kind.get("failure"), Some(&1));
    }

    #[tokio::test]
    async fn filters_by_kind_and_project() {
        let services = ContextNestServices::new_default().await.expect("services");
        seed(&services).await;

        let resp = list_atoms(
            State(services),
            Query(AtomsQuery {
                kind: Some("decision_made".into()),
                project: Some("alpha".into()),
                session_id: None,
                since: None,
                limit: None,
            }),
        )
        .await
        .expect("ok")
        .0;

        assert_eq!(resp.count, 1);
        assert_eq!(resp.atoms[0].id, "f1");
        assert_eq!(resp.atoms[0].kind, "decision_made");
    }

    #[tokio::test]
    async fn unknown_kind_is_bad_request() {
        let services = ContextNestServices::new_default().await.expect("services");
        let err = list_atoms(
            State(services),
            Query(AtomsQuery {
                kind: Some("not_a_real_kind".into()),
                project: None,
                session_id: None,
                since: None,
                limit: None,
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn limit_truncates_but_total_matched_is_full() {
        let services = ContextNestServices::new_default().await.expect("services");
        seed(&services).await;

        let resp = list_atoms(
            State(services),
            Query(AtomsQuery {
                kind: None,
                project: None,
                session_id: None,
                since: None,
                limit: Some(1),
            }),
        )
        .await
        .expect("ok")
        .0;

        assert_eq!(resp.count, 1);
        assert_eq!(resp.total_matched, 2);
    }
}

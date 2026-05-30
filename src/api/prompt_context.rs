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

// =============================================================================
// `GET /api/v1/prompt-context/clusters` — Phase 1b deterministic dedup.
//
// `/atoms` returns every trajectory fragment that matches the filters. That's
// useful as raw material but noisy — a single insight that surfaced in 7
// sessions over 90d shows up as 7 distinct atoms. The clusters endpoint
// collapses by `(kind, normalized_text)`: same kind + same text after
// normalisation become one cluster. Each cluster carries the unique
// `sessions[]` it appeared in, so a cluster spanning multiple sessions is
// the deterministic version of "memory promotion" — the same lesson learned
// twice is a real pattern, not coincidence.
//
// Still L1.5 deterministic: no LLM, no SQL, no embedding cosine. Normalisation
// is lowercase + punctuation strip + whitespace collapse. Anything fancier
// (semantic dedup via embeddings, paraphrase clustering via LLM) belongs in
// later phases.
// =============================================================================

const DEFAULT_MIN_COUNT: usize = 2;
const CLUSTERS_DEFAULT_LIMIT: usize = 50;
const CLUSTERS_MAX_LIMIT: usize = 500;

#[derive(Debug, Deserialize)]
pub struct ClustersQuery {
    pub kind: Option<String>,
    pub project: Option<String>,
    pub session_id: Option<String>,
    pub since: Option<String>,
    /// Drop clusters whose total count is below this. Default 2 — single-
    /// occurrence atoms are not "patterns". Set to 1 to include them.
    pub min_count: Option<usize>,
    /// Max clusters returned (post-filter, post-sort). Default 50, cap 500.
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct TrajectoryCluster {
    pub kind: String,
    /// One representative atom's raw text (longest by char count among
    /// cluster members — preserves the most-detailed phrasing).
    pub representative_text: String,
    /// The exact post-normalisation key that the cluster was grouped by.
    /// Useful for debugging "why did these merge" without re-running the
    /// normaliser.
    pub normalized_text: String,
    /// Total atoms in the cluster across all sessions.
    pub count: usize,
    /// Distinct `src_session` UUIDs the cluster appeared in. Length
    /// `< count` when the same session emitted the atom multiple times;
    /// length `== count` when each occurrence was a different session
    /// (strongest promotion signal).
    pub sessions: Vec<String>,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ClustersResponse {
    pub since: String,
    /// Number of clusters returned (post-limit).
    pub count: usize,
    /// Clusters matching the filter pre-limit. `count < total_matched` means
    /// the page was capped.
    pub total_matched: usize,
    /// Total individual atoms scanned before grouping — same number `/atoms`
    /// would return for the same filter set.
    pub atoms_scanned: usize,
    pub clusters: Vec<TrajectoryCluster>,
}

/// Lowercase + replace any non-alphanumeric run with a single space, trim.
/// Lossy on punctuation but stable across rewordings like "fixed the bug."
/// vs "Fixed the bug" vs "Fixed the bug!" — all collapse to `fixed the bug`.
/// Returns an empty string when the input has no alphanumeric content.
fn normalize_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_space = true; // dedup leading whitespace
    for c in text.chars() {
        if c.is_alphanumeric() {
            for low in c.to_lowercase() {
                out.push(low);
            }
            prev_space = false;
        } else if !prev_space {
            out.push(' ');
            prev_space = true;
        }
    }
    out.truncate(out.trim_end().len());
    out
}

/// `GET /api/v1/prompt-context/clusters`
pub async fn list_clusters(
    State(services): State<ContextNestServices>,
    Query(q): Query<ClustersQuery>,
) -> Result<Json<ClustersResponse>, StatusCode> {
    if let Some(k) = q.kind.as_deref() {
        if !TRAJECTORY_KINDS.contains(&k) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    let since_raw = q.since.as_deref().unwrap_or(DEFAULT_SINCE);
    let dur = parse_since(since_raw).unwrap_or_else(|| chrono::Duration::days(30));
    let cutoff = chrono::Utc::now() - dur;
    let min_count = q.min_count.unwrap_or(DEFAULT_MIN_COUNT).max(1);
    let limit = q
        .limit
        .unwrap_or(CLUSTERS_DEFAULT_LIMIT)
        .min(CLUSTERS_MAX_LIMIT);

    struct ClusterAcc {
        kind: String,
        normalized: String,
        representative: String,
        count: usize,
        sessions: std::collections::BTreeSet<String>,
        first_ts: Option<String>,
        last_ts: Option<String>,
    }

    let texts = services.fragment_texts.read().await;
    let metadata = services.fragment_metadata.read().await;

    let mut clusters: HashMap<(String, String), ClusterAcc> = HashMap::new();
    let mut atoms_scanned: usize = 0;

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
        }
        if let Some(want) = q.project.as_deref() {
            match meta.get("project_cwd").and_then(|v| v.as_str()) {
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
        let text = texts.get(frag_id).cloned().unwrap_or_default();
        let normalized = normalize_text(&text);
        if normalized.is_empty() {
            // Atom with no alphanumeric content can't be deduped meaningfully;
            // also a noise signal — skip.
            continue;
        }
        atoms_scanned += 1;
        let key = (kind.to_string(), normalized.clone());
        let acc = clusters.entry(key).or_insert_with(|| ClusterAcc {
            kind: kind.to_string(),
            normalized,
            representative: text.clone(),
            count: 0,
            sessions: std::collections::BTreeSet::new(),
            first_ts: None,
            last_ts: None,
        });
        acc.count += 1;
        if !session_id.is_empty() {
            acc.sessions.insert(session_id);
        }
        // Representative = longest text seen so far (most detail).
        if text.chars().count() > acc.representative.chars().count() {
            acc.representative = text;
        }
        if let Some(ts) = ts_str {
            if acc.first_ts.as_deref().map(|cur| ts < cur).unwrap_or(true) {
                acc.first_ts = Some(ts.to_string());
            }
            if acc.last_ts.as_deref().map(|cur| ts > cur).unwrap_or(true) {
                acc.last_ts = Some(ts.to_string());
            }
        }
    }
    drop(texts);
    drop(metadata);

    let mut out: Vec<TrajectoryCluster> = clusters
        .into_values()
        .filter(|c| c.count >= min_count)
        .map(|c| TrajectoryCluster {
            kind: c.kind,
            representative_text: c.representative,
            normalized_text: c.normalized,
            count: c.count,
            sessions: c.sessions.into_iter().collect(),
            first_ts: c.first_ts,
            last_ts: c.last_ts,
        })
        .collect();

    // Rank: cross-session reach first (more distinct sessions = stronger
    // pattern), then raw count, then most recent activity, then
    // deterministic tiebreak on normalized_text so pages are stable.
    out.sort_by(|a, b| {
        b.sessions
            .len()
            .cmp(&a.sessions.len())
            .then_with(|| b.count.cmp(&a.count))
            .then_with(|| {
                b.last_ts
                    .as_deref()
                    .unwrap_or("")
                    .cmp(a.last_ts.as_deref().unwrap_or(""))
            })
            .then_with(|| a.normalized_text.cmp(&b.normalized_text))
    });

    let total_matched = out.len();
    out.truncate(limit);

    Ok(Json(ClustersResponse {
        since: since_raw.to_string(),
        count: out.len(),
        total_matched,
        atoms_scanned,
        clusters: out,
    }))
}

/// Build the prompt-context router. Mounted in `create_simple_app`.
pub fn create_prompt_context_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/api/v1/prompt-context/atoms", get(list_atoms))
        .route("/api/v1/prompt-context/clusters", get(list_clusters))
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

    #[test]
    fn normalize_collapses_punctuation_case_and_whitespace() {
        assert_eq!(normalize_text("Fixed the bug."), "fixed the bug");
        assert_eq!(normalize_text("Fixed   the bug!"), "fixed the bug");
        assert_eq!(normalize_text("FIXED THE BUG"), "fixed the bug");
        assert_eq!(normalize_text("  Fixed: the bug\n"), "fixed the bug");
        // No alphanumeric → empty (gets filtered out at the call site).
        assert_eq!(normalize_text("!!!"), "");
        assert_eq!(normalize_text(""), "");
        // Multibyte safe — unicode lowercase + alphanumeric.
        assert_eq!(normalize_text("Über München"), "über münchen");
    }

    /// Seed a corpus where three atoms collapse to one cluster, one is a
    /// solo, and one falls outside the default window.
    async fn seed_clusters_corpus(services: &ContextNestServices) {
        let now = chrono::Utc::now();
        let recent = now.to_rfc3339();
        let later = (now + chrono::Duration::seconds(5)).to_rfc3339();
        let later_2 = (now + chrono::Duration::seconds(10)).to_rfc3339();
        let old = (now - chrono::Duration::days(120)).to_rfc3339();

        let mut texts = services.fragment_texts.write().await;
        let mut meta = services.fragment_metadata.write().await;
        let put = |texts: &mut std::collections::HashMap<String, String>,
                   meta: &mut std::collections::HashMap<
            String,
            std::collections::HashMap<String, serde_json::Value>,
        >,
                   id: &str,
                   text: &str,
                   kv: &[(&str, serde_json::Value)]| {
            texts.insert(id.to_string(), text.to_string());
            let m: std::collections::HashMap<String, serde_json::Value> =
                kv.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
            meta.insert(id.to_string(), m);
        };

        // Three "decision_made" atoms whose normalised form is identical
        // (case + punctuation variants of the same words), emitted by THREE
        // different sessions — the strongest cluster. c3 is the longest
        // RAW text, so it wins the representative-text selection.
        put(
            &mut texts,
            &mut meta,
            "c1",
            "Always run cargo fmt",
            &[
                ("kind", json!("decision_made")),
                ("ts", json!(recent)),
                ("src_session", json!("S1")),
            ],
        );
        put(
            &mut texts,
            &mut meta,
            "c2",
            "Always RUN cargo fmt!",
            &[
                ("kind", json!("decision_made")),
                ("ts", json!(later)),
                ("src_session", json!("S2")),
            ],
        );
        put(
            &mut texts,
            &mut meta,
            "c3",
            "Always... run cargo fmt!!!  ",
            &[
                ("kind", json!("decision_made")),
                ("ts", json!(later_2)),
                ("src_session", json!("S3")),
            ],
        );

        // A solo "failure" atom — must be dropped by default min_count=2.
        put(
            &mut texts,
            &mut meta,
            "c4",
            "Lone failure that nobody else hit",
            &[
                ("kind", json!("failure")),
                ("ts", json!(recent)),
                ("src_session", json!("S1")),
            ],
        );

        // An old version of the cluster — must be dropped by default 30d.
        put(
            &mut texts,
            &mut meta,
            "c5",
            "Always run cargo fmt before committing.",
            &[
                ("kind", json!("decision_made")),
                ("ts", json!(old)),
                ("src_session", json!("S9")),
            ],
        );

        // Non-trajectory kind — must never appear regardless of dedup.
        put(
            &mut texts,
            &mut meta,
            "c6",
            "Same exact text as the cluster",
            &[
                ("kind", json!("feature")),
                ("ts", json!(recent)),
                ("src_session", json!("S1")),
            ],
        );
    }

    #[tokio::test]
    async fn clusters_collapse_by_normalized_text_across_sessions() {
        let services = ContextNestServices::new_default().await.expect("services");
        seed_clusters_corpus(&services).await;

        let resp = list_clusters(
            State(services),
            Query(ClustersQuery {
                kind: None,
                project: None,
                session_id: None,
                since: None,
                min_count: None, // default 2
                limit: None,
            }),
        )
        .await
        .expect("ok")
        .0;

        // The big cluster (c1+c2+c3) is the only one with count>=2. The lone
        // failure (c4) drops below min_count; c5 is too old; c6 is wrong kind.
        assert_eq!(resp.count, 1);
        assert_eq!(resp.atoms_scanned, 4, "c1+c2+c3+c4 inside window/kind");
        let c = &resp.clusters[0];
        assert_eq!(c.kind, "decision_made");
        assert_eq!(c.count, 3);
        assert_eq!(c.sessions, vec!["S1", "S2", "S3"]);
        // Representative is the longest RAW text variant (c3, with the
        // most punctuation/whitespace).
        assert!(c.representative_text.starts_with("Always..."));
        assert_eq!(c.normalized_text, "always run cargo fmt");
        // first_ts/last_ts span the cluster.
        assert!(c.first_ts.is_some());
        assert!(c.last_ts.is_some());
        assert!(c.first_ts < c.last_ts);
    }

    #[tokio::test]
    async fn clusters_min_count_one_keeps_solo_atoms() {
        let services = ContextNestServices::new_default().await.expect("services");
        seed_clusters_corpus(&services).await;

        let resp = list_clusters(
            State(services),
            Query(ClustersQuery {
                kind: None,
                project: None,
                session_id: None,
                since: None,
                min_count: Some(1),
                limit: None,
            }),
        )
        .await
        .expect("ok")
        .0;

        // Now the lone failure surfaces too: 2 clusters total.
        assert_eq!(resp.count, 2);
        // Sort: cross-session-reach desc first, so the 3-session cluster
        // beats the 1-session solo.
        assert_eq!(resp.clusters[0].count, 3);
        assert_eq!(resp.clusters[1].count, 1);
        assert_eq!(resp.clusters[1].kind, "failure");
    }

    #[tokio::test]
    async fn clusters_unknown_kind_is_bad_request() {
        let services = ContextNestServices::new_default().await.expect("services");
        let err = list_clusters(
            State(services),
            Query(ClustersQuery {
                kind: Some("not_a_real_kind".into()),
                project: None,
                session_id: None,
                since: None,
                min_count: None,
                limit: None,
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err, StatusCode::BAD_REQUEST);
    }
}

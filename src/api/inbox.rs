//! `GET /api/v1/inbox` — cross-session "what does Claude need from me?" feed.
//!
//! This endpoint replaces the old dashboard fan-out (one
//! `/api/v1/sessions` call followed by `2 × N` `/api/v1/tools/retrieve`
//! calls per polling tick) with a single server-side scan. The cost
//! profile drops from `O(S)` HTTP round-trips + `O(S × F)` lock
//! acquisitions per tick to a single request that walks `fragment_metadata`
//! once and looks up active session affinity from a single
//! `SessionIndex::active_fragments_session_map` snapshot.
//!
//! ## Inclusion rules
//!
//! A fragment is surfaced in the inbox iff:
//!
//! - it is currently **active** in `SessionIndex` (soft-deleted IDs are
//!   skipped — same visibility rule `retrieve` uses), **and**
//! - its stored metadata has `kind == "user_action"`, **or**
//! - its stored metadata has `kind == "decision"` AND
//!   `awaiting_decision == true`.
//!
//! These two filters mirror the previous frontend behaviour in
//! `web/src/hooks/useInbox.ts` so the dashboard semantics are unchanged.
//!
//! ## Response shape
//!
//! Returns a flat list of `InboxHit` items. The shape is intentionally
//! close to `RetrieveHit` so the frontend mapper does not need to be
//! rewritten — the only addition is the top-level `session_id` field,
//! and `similarity` is omitted because no query is run.
//!
//! Items are returned **sorted by metadata `ts` descending** (newest
//! first), with the empty/missing-`ts` case sorting last. The frontend
//! still applies its urgency-bucketed sort on top, so this server-side
//! order is a defensive default rather than a load-bearing contract.

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use serde::Serialize;
use std::collections::HashMap;

use crate::services::ContextNestServices;

/// One item in the dashboard inbox feed.
///
/// Mirrors [`crate::api::tools::RetrieveHit`] field-by-field except:
/// - `session_id` is added at the top level (the inbox is cross-session
///   by definition, so callers need to know where each item came from)
/// - `similarity` is omitted (no query, no scoring)
#[derive(Debug, Serialize)]
pub struct InboxHit {
    pub id: String,
    pub session_id: String,
    pub content: String,
    pub importance: f32,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct InboxResponse {
    pub items: Vec<InboxHit>,
}

/// `GET /api/v1/inbox`
///
/// See module docs for inclusion rules and ordering. Acquires three read
/// locks (fragment_metadata, session_index.active, fragment_texts) and
/// holds none of them across an `await` that could touch a writer.
pub async fn list_inbox(
    State(services): State<ContextNestServices>,
) -> Result<Json<InboxResponse>, StatusCode> {
    // Snapshot active-fragment → session affinity in one lock acquisition.
    // Soft-deleted fragments are intentionally excluded — they would still
    // resolve via `find_session` (reverse map) but they are not visible to
    // `retrieve`, so they should not be visible to the inbox either.
    let frag_to_session = services.session_index.active_fragments_session_map().await;

    if frag_to_session.is_empty() {
        return Ok(Json(InboxResponse { items: Vec::new() }));
    }

    // Walk metadata under a single read lock. For each entry, apply the
    // inclusion rules; only inbox-eligible fragments survive into the
    // intermediate vector.
    let metadata = services.fragment_metadata.read().await;
    let mut eligible: Vec<(String, String, HashMap<String, serde_json::Value>)> = Vec::new();

    for (frag_id, meta) in metadata.iter() {
        if !is_inbox_eligible(meta) {
            continue;
        }
        let Some(session_id) = frag_to_session.get(frag_id) else {
            // Metadata exists but the fragment is not active — likely
            // soft-deleted. Skip.
            continue;
        };
        eligible.push((frag_id.clone(), session_id.clone(), meta.clone()));
    }
    drop(metadata);

    if eligible.is_empty() {
        return Ok(Json(InboxResponse { items: Vec::new() }));
    }

    // Single bulk text lookup for survivors only.
    let texts = services.fragment_texts.read().await;

    // Importance lives on the canonical fragment, not in the metadata
    // sidecar. The substrate's `MemoryAttractorManager::get_fragment` is
    // the source of truth, but calling it N times here would re-introduce
    // the per-fragment lock acquisition we just eliminated. Importance is
    // a UI nice-to-have for the inbox (the frontend doesn't actually sort
    // by it), so default to a neutral 0.5 and accept the staleness rather
    // than pay the cost. A future iteration could mirror importance into
    // the metadata sidecar at store time if any dashboard view starts to
    // depend on it.
    let mut items: Vec<InboxHit> = eligible
        .into_iter()
        .map(|(id, session_id, meta)| {
            let content = texts.get(&id).cloned().unwrap_or_default();
            InboxHit {
                id,
                session_id,
                content,
                importance: 0.5,
                metadata: meta,
            }
        })
        .collect();
    drop(texts);

    // Sort by `ts` descending (newest first). Missing/empty ts sorts last
    // so stale entries never crowd out fresh ones in the default view.
    items.sort_by(|a, b| {
        let a_ts = a.metadata.get("ts").and_then(|v| v.as_str()).unwrap_or("");
        let b_ts = b.metadata.get("ts").and_then(|v| v.as_str()).unwrap_or("");
        match (a_ts.is_empty(), b_ts.is_empty()) {
            (true, true) => a.id.cmp(&b.id),
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => b_ts.cmp(a_ts).then_with(|| a.id.cmp(&b.id)),
        }
    });

    // View-layer dedup: long-running agents re-emit the same actionable
    // item across many turns while waiting for the user (e.g. researcher
    // autopilot's "Pick A/B/C" survives 7 turns until answered). The
    // substrate ingests each emission as a distinct fragment because
    // urgency/step/reason can drift across turns — losing that history
    // at ingest would be lossy. But the dashboard's attention queue only
    // needs "is this still pending?" once per (session, content, kind).
    //
    // Strategy: dedup by `(session_id, kind, content)`. Because we've
    // already sorted by ts desc, the FIRST occurrence of any key is the
    // newest — keep it, drop the rest. Cross-session duplicates are
    // intentionally preserved: "Run wrangler login" pending in two
    // different sessions is two distinct signals.
    let mut seen: std::collections::HashSet<(String, String, String)> =
        std::collections::HashSet::new();
    items.retain(|hit| {
        let kind = hit
            .metadata
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let key = (hit.session_id.clone(), kind, hit.content.clone());
        seen.insert(key)
    });

    Ok(Json(InboxResponse { items }))
}

/// Decide which memory kinds belong in the user's attention queue.
///
/// Three eligible kinds — keep the frontend filter in
/// `web/src/hooks/useInbox.ts` and the CLI renderer in
/// `src/inbox/mod.rs` in sync with this list:
///
/// - **`user_action`** — explicit `<user_action>` tag from a Claude
///   session. Always inbox-worthy.
/// - **`todo`** — extracted task / open item. Empirically these read as
///   "the user needs to do X" even without a tag (e.g. researcher
///   autopilot emits "User picks: ship / iterate / suspend"). A `todo`
///   carrying `task_status == "completed"` is dropped: the work is
///   already done, so it has no place in an attention queue. (The
///   ingest pipeline re-emits the same `(session_id, kind, content)`
///   triple with the new `task_status`, and the view-layer dedupe at
///   the bottom of `list_inbox` keeps the freshest copy — so once a
///   task flips to completed, this filter retires its inbox row.)
/// - **`decision`** — only when `awaiting_decision: true`, since
///   already-resolved decisions are historical, not actionable.
///
/// Any other kind (accomplishment, learning, state, goal_phase, …) is
/// historical / contextual and excluded.
pub(crate) fn is_inbox_eligible(meta: &HashMap<String, serde_json::Value>) -> bool {
    match meta.get("kind").and_then(|v| v.as_str()) {
        Some("user_action") => true,
        Some("todo") => {
            // Drop completed todos; everything else (pending,
            // in_progress, failed, or missing task_status entirely)
            // stays inbox-eligible. `failed` is deliberately kept —
            // regressions are valuable attention signal.
            !matches!(
                meta.get("task_status").and_then(|v| v.as_str()),
                Some("completed")
            )
        }
        Some("decision") => meta
            .get("awaiting_decision")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        _ => false,
    }
}

/// Build the inbox router. Mounted alongside the tools / cc_hooks /
/// sessions routers in `create_simple_app`.
pub fn create_inbox_router() -> Router<ContextNestServices> {
    Router::new().route("/api/v1/inbox", get(list_inbox))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta(kv: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
        kv.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
    }

    #[test]
    fn user_action_is_eligible() {
        assert!(is_inbox_eligible(&meta(&[("kind", json!("user_action"))])));
    }

    #[test]
    fn todo_is_eligible() {
        // researcher-style ingest emits `todo` for "user needs to do X"
        // records that would otherwise be tagged `user_action` explicitly.
        assert!(is_inbox_eligible(&meta(&[("kind", json!("todo"))])));
    }

    #[test]
    fn todo_pending_is_eligible() {
        assert!(is_inbox_eligible(&meta(&[
            ("kind", json!("todo")),
            ("task_status", json!("pending")),
        ])));
    }

    #[test]
    fn todo_in_progress_is_eligible() {
        assert!(is_inbox_eligible(&meta(&[
            ("kind", json!("todo")),
            ("task_status", json!("in_progress")),
        ])));
    }

    #[test]
    fn todo_failed_is_eligible() {
        // Regressions are useful attention signal — keep them visible.
        assert!(is_inbox_eligible(&meta(&[
            ("kind", json!("todo")),
            ("task_status", json!("failed")),
        ])));
    }

    #[test]
    fn todo_completed_is_ineligible() {
        // Regression: a completed task should not occupy an inbox slot.
        // This is the bug that caused "Add BE route POST
        // /api/publisher-style-synthesis/:jobId" to keep showing 14
        // minutes after the assistant marked task #22 done.
        assert!(!is_inbox_eligible(&meta(&[
            ("kind", json!("todo")),
            ("task_status", json!("completed")),
        ])));
    }

    #[test]
    fn decision_awaiting_is_eligible() {
        assert!(is_inbox_eligible(&meta(&[
            ("kind", json!("decision")),
            ("awaiting_decision", json!(true)),
        ])));
    }

    #[test]
    fn decision_not_awaiting_is_ineligible() {
        assert!(!is_inbox_eligible(&meta(&[
            ("kind", json!("decision")),
            ("awaiting_decision", json!(false)),
        ])));
    }

    #[test]
    fn decision_without_awaiting_flag_is_ineligible() {
        assert!(!is_inbox_eligible(&meta(&[("kind", json!("decision"))])));
    }

    #[test]
    fn unknown_kind_is_ineligible() {
        assert!(!is_inbox_eligible(&meta(&[("kind", json!("note"))])));
    }

    #[test]
    fn missing_kind_is_ineligible() {
        assert!(!is_inbox_eligible(&HashMap::new()));
    }
}

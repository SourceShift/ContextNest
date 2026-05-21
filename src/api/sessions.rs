//! `GET /api/v1/sessions` — list every session the substrate knows about.
//!
//! Returns a sorted (newest-first by `last_ts`) array of session summary
//! objects. Each entry carries the fragment count, the most-common
//! `project_cwd`, the most-common `src_session` UUID, and the latest
//! `ts` seen across that session's active fragments.
//!
//! This endpoint is intentionally read-only and requires no request body.
//! It is the first building block for the cross-session Inbox and Sessions
//! list UI routes.

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use serde::Serialize;
use tracing::warn;

use crate::services::ContextNestServices;

// =============================================================================
// Response shapes
// =============================================================================

/// Summary of a single session as returned by `GET /api/v1/sessions`.
#[derive(Debug, Serialize)]
pub struct SessionSummary {
    /// The substrate session ID (e.g. `"cc-879fccc6"`).
    pub id: String,
    /// Count of ACTIVE (non-soft-deleted) fragments in this session.
    pub fragment_count: usize,
    /// Most-common non-empty `project_cwd` value across active fragments.
    /// `null` when no fragment carries this metadata key.
    pub project_cwd: Option<String>,
    /// Most-common non-empty `src_session` value across active fragments.
    /// `null` when no fragment carries this metadata key.
    pub src_session_uuid: Option<String>,
    /// ISO 8601 timestamp of the latest `ts` value seen across active
    /// fragments. Lexicographic max works correctly for ISO 8601 strings.
    /// `null` when no fragment carries a `ts` metadata key.
    pub last_ts: Option<String>,
    /// Per-session count of active fragments grouped by `metadata.kind`.
    /// Fragments without a `kind` go under `"unknown"`. Always populated
    /// (possibly with a single `"unknown"` entry) so dashboard consumers
    /// can render the per-row count strip without a separate request.
    pub by_kind: std::collections::HashMap<String, usize>,
}

#[derive(Debug, Serialize)]
pub struct SessionsResponse {
    pub sessions: Vec<SessionSummary>,
}

// =============================================================================
// Handler
// =============================================================================

/// `GET /api/v1/sessions`
///
/// Enumerates every session in the substrate's `SessionIndex` (union of
/// `active` and `deleted` key sets), aggregates per-session metadata from the
/// `fragment_metadata` sidecar, and returns the result sorted descending by
/// `last_ts` (newest first). Sessions with `null` `last_ts` sort last.
/// Ties are broken by session `id` ascending for a stable sort.
pub async fn list_sessions(
    State(services): State<ContextNestServices>,
) -> Result<Json<SessionsResponse>, StatusCode> {
    // Enumerate all known session IDs (active ∪ deleted, sorted).
    let session_ids = services.session_index.list_all_sessions().await;

    // Acquire a single read lock over fragment_metadata for the whole loop
    // rather than one lock per session — reduces contention and avoids
    // repeated lock/unlock overhead.
    let metadata_map = services.fragment_metadata.read().await;

    let mut summaries: Vec<SessionSummary> = Vec::with_capacity(session_ids.len());

    for session_id in &session_ids {
        // Only ACTIVE fragment IDs contribute to the count and aggregation.
        let active_ids = services.session_index.list_active(session_id).await;
        let fragment_count = active_ids.len();

        // Accumulators for the three metadata fields + per-kind counts.
        let mut cwd_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut src_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut by_kind: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        let mut last_ts: Option<String> = None;

        for frag_id in &active_ids {
            let Some(meta) = metadata_map.get(frag_id) else {
                // Fragment has no metadata entry at all — count under "unknown"
                // and skip the rest of the metadata aggregation for this row.
                *by_kind.entry("unknown".to_string()).or_insert(0) += 1;
                continue;
            };

            // kind bucketing — drives the dashboard's per-session counts strip.
            let kind = meta
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            *by_kind.entry(kind).or_insert(0) += 1;

            // project_cwd
            if let Some(cwd_val) = meta.get("project_cwd") {
                if let Some(cwd_str) = cwd_val.as_str() {
                    if !cwd_str.is_empty() {
                        *cwd_counts.entry(cwd_str.to_owned()).or_insert(0) += 1;
                    }
                }
            }

            // src_session
            if let Some(src_val) = meta.get("src_session") {
                if let Some(src_str) = src_val.as_str() {
                    if !src_str.is_empty() {
                        *src_counts.entry(src_str.to_owned()).or_insert(0) += 1;
                    }
                }
            }

            // ts — lexicographic max works for ISO 8601
            if let Some(ts_val) = meta.get("ts") {
                if let Some(ts_str) = ts_val.as_str() {
                    if !ts_str.is_empty() {
                        last_ts = Some(match last_ts.take() {
                            None => ts_str.to_owned(),
                            Some(prev) => {
                                if ts_str > prev.as_str() {
                                    ts_str.to_owned()
                                } else {
                                    prev
                                }
                            }
                        });
                    }
                }
            }
        }

        // Pick the most-common value for each field; None when map is empty.
        let project_cwd = most_common(cwd_counts);
        let src_session_uuid = most_common(src_counts);

        if fragment_count == 0 && project_cwd.is_none() && src_session_uuid.is_none() {
            // Session exists only in deleted map (all fragments hard-removed or
            // never active). Still emit the entry so the caller knows the
            // session existed.
            warn!(session_id = %session_id, "session has no active fragments");
        }

        summaries.push(SessionSummary {
            id: session_id.clone(),
            fragment_count,
            project_cwd,
            src_session_uuid,
            last_ts,
            by_kind,
        });
    }

    // Sort: newest last_ts first (None sorts last), tiebreak by id ascending.
    summaries.sort_by(|a, b| match (&b.last_ts, &a.last_ts) {
        (Some(bt), Some(at)) => bt.cmp(at).then_with(|| a.id.cmp(&b.id)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.id.cmp(&b.id),
    });

    Ok(Json(SessionsResponse {
        sessions: summaries,
    }))
}

// =============================================================================
// Helpers
// =============================================================================

/// Return the key with the highest count from a frequency map, or `None` if
/// the map is empty. Ties are broken by key value (smallest wins) for
/// determinism.
fn most_common(counts: std::collections::HashMap<String, usize>) -> Option<String> {
    counts
        .into_iter()
        .max_by(|(k1, v1), (k2, v2)| v1.cmp(v2).then_with(|| k2.cmp(k1)))
        .map(|(k, _)| k)
}

// =============================================================================
// Router
// =============================================================================

/// Build the sessions router. Mounted alongside the tools and cc_hooks routers
/// in `create_simple_app`.
pub fn create_sessions_router() -> Router<ContextNestServices> {
    Router::new().route("/api/v1/sessions", get(list_sessions))
}

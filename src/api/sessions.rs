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

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::Serialize;
use serde_json::Value;
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

// =============================================================================
// `GET /api/v1/sessions/by-file?path=<substring>`
//
// Returns the sessions whose `files_touched` fragment contains a path
// that contains the given substring (case-insensitive). Backed by the
// MemoryKind::FilesTouched records emitted at ingest time — see
// `src/ingest/claude_code/extractor.rs`.
// =============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct ByFileQuery {
    /// Substring to match against any file path in any session's
    /// `files_touched` array. Case-insensitive. A bare basename like
    /// `"AgentStreamRail.tsx"` matches any session that touched a file
    /// containing that substring; a more-specific path like
    /// `"web/src/components/AgentStreamRail.tsx"` narrows the match.
    pub path: String,
}

#[derive(Debug, serde::Serialize)]
pub struct SessionFileMatch {
    pub session_id: String,
    /// Subset of the session's full `files_touched` list — only the
    /// paths that actually matched the query substring. Lets the
    /// caller see which file in the session matched (useful when the
    /// query is a partial basename and several files in the session
    /// could plausibly match).
    pub matched_files: Vec<String>,
    pub total_files: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct ByFileResponse {
    pub query: String,
    pub matches: Vec<SessionFileMatch>,
}

pub async fn sessions_by_file(
    State(services): State<ContextNestServices>,
    Query(q): Query<ByFileQuery>,
) -> Result<Json<ByFileResponse>, StatusCode> {
    let needle = q.path.trim();
    if needle.is_empty() {
        return Ok(Json(ByFileResponse {
            query: q.path,
            matches: Vec::new(),
        }));
    }
    let needle_low = needle.to_lowercase();

    // Walk fragment_metadata once, pulling every `files_touched`
    // fragment. The substrate doesn't have a secondary index here so
    // this is an O(N) scan — fine for the sub-1k files_touched
    // fragments a typical substrate has (one per session).
    let metadata = services.fragment_metadata.read().await;
    let mut by_session: std::collections::HashMap<String, SessionFileMatch> =
        std::collections::HashMap::new();

    for (frag_id, meta) in metadata.iter() {
        if meta.get("kind").and_then(|v| v.as_str()) != Some("files_touched") {
            continue;
        }
        let Some(files) = meta.get("files").and_then(|v| v.as_array()) else {
            continue;
        };
        let matched: Vec<String> = files
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|p| p.to_lowercase().contains(&needle_low))
            .map(|p| p.to_string())
            .collect();
        if matched.is_empty() {
            continue;
        }
        // Recover the session id from the metadata sidecar. cc_hooks
        // ingest writes `src_session` as the canonical full UUID; on
        // the rare path where it's missing (e.g. records ingested
        // before that field was added) we skip the row rather than
        // pollute the response with "unknown" entries.
        let Some(session_id) = meta
            .get("src_session")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
        else {
            // Silence the warn — single-line drop is fine for a rare
            // malformed-record edge case.
            let _ = frag_id;
            continue;
        };
        // If multiple files_touched fragments exist for the same
        // session (shouldn't happen on the current ingest path, but
        // belt-and-braces), keep the union of matched files.
        by_session
            .entry(session_id.clone())
            .and_modify(|m| {
                for p in &matched {
                    if !m.matched_files.contains(p) {
                        m.matched_files.push(p.clone());
                    }
                }
                m.total_files = m.total_files.max(files.len());
            })
            .or_insert(SessionFileMatch {
                session_id,
                matched_files: matched,
                total_files: files.len(),
            });
    }
    drop(metadata);

    let mut matches: Vec<SessionFileMatch> = by_session.into_values().collect();
    matches.sort_by(|a, b| {
        b.matched_files
            .len()
            .cmp(&a.matched_files.len())
            .then_with(|| a.session_id.cmp(&b.session_id))
    });

    Ok(Json(ByFileResponse {
        query: q.path,
        matches,
    }))
}

// =============================================================================
// `GET /api/v1/sessions/by-feature?q=<substring>`
//
// Returns the sessions that declared a feature (in their
// `z-insight.delivered_features[]`) whose name contains the substring.
// Higher-signal than by-file because the agent named the feature itself.
// =============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct ByFeatureQuery {
    pub q: String,
}

#[derive(Debug, serde::Serialize)]
pub struct FeatureHit {
    pub session_id: String,
    pub feature: String,
    pub ts: Option<String>,
    pub files: Vec<String>,
    pub refs: Vec<Value>,
    pub layer: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct ByFeatureResponse {
    pub query: String,
    pub hits: Vec<FeatureHit>,
}

pub async fn sessions_by_feature(
    State(services): State<ContextNestServices>,
    Query(q): Query<ByFeatureQuery>,
) -> Result<Json<ByFeatureResponse>, StatusCode> {
    let needle = q.q.trim();
    if needle.is_empty() {
        return Ok(Json(ByFeatureResponse {
            query: q.q,
            hits: Vec::new(),
        }));
    }
    let needle_low = needle.to_lowercase();

    let texts = services.fragment_texts.read().await;
    let metadata = services.fragment_metadata.read().await;
    let mut hits: Vec<FeatureHit> = Vec::new();

    for (frag_id, meta) in metadata.iter() {
        if meta.get("kind").and_then(|v| v.as_str()) != Some("feature") {
            continue;
        }
        let feature_text = texts.get(frag_id).cloned().unwrap_or_default();
        if !feature_text.to_lowercase().contains(&needle_low) {
            continue;
        }
        let session_id = meta
            .get("src_session")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let files: Vec<String> = meta
            .get("files")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let refs: Vec<Value> = meta
            .get("refs")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let layer = meta
            .get("layer")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let ts = meta
            .get("ts")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        hits.push(FeatureHit {
            session_id,
            feature: feature_text,
            ts,
            files,
            refs,
            layer,
        });
    }
    drop(texts);
    drop(metadata);

    // Most recent first when both have ts; deterministic on session_id otherwise.
    hits.sort_by(|a, b| {
        b.ts.as_deref()
            .unwrap_or("")
            .cmp(a.ts.as_deref().unwrap_or(""))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });

    Ok(Json(ByFeatureResponse { query: q.q, hits }))
}

/// Build the sessions router. Mounted alongside the tools and cc_hooks routers
/// in `create_simple_app`.
pub fn create_sessions_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/api/v1/sessions", get(list_sessions))
        .route("/api/v1/sessions/by-file", get(sessions_by_file))
        .route("/api/v1/sessions/by-feature", get(sessions_by_feature))
}

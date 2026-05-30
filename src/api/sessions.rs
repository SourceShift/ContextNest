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
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

use crate::api::inbox::is_inbox_eligible;
use crate::services::ContextNestServices;

pub(crate) const TRAJECTORY_KINDS: &[&str] = &[
    "read_context",
    "verification",
    "evidence_ref",
    "decision_made",
    "failure",
    "prompt_directive",
    "assumption",
    "artifact",
    "memory_candidate",
    "risk_flag",
];

const PROMOTION_KINDS: &[&str] = &["memory_candidate", "prompt_directive", "risk_flag"];

// =============================================================================
// Response shapes
// =============================================================================

/// Summary of a single session as returned by `GET /api/v1/sessions`.
#[derive(Debug, Serialize)]
pub struct SessionSummary {
    /// The substrate session ID — the bare Claude Code session UUID
    /// (e.g. `"879fccc6-3a1f-4b2c-9d8e-1234567890ab"`).
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

// =============================================================================
// `GET /api/v1/features?since=<duration>&layer=<layer>`
//
// Time-windowed catalogue of every Feature record (one per
// `z-insight.delivered_features[]` entry) the substrate has seen.
// The daily-driver query: "what shipped today, and how do I test it?"
// =============================================================================

#[derive(Debug, serde::Deserialize)]
pub struct FeaturesQuery {
    /// Duration suffix: `5m`, `2h`, `24h`, `7d`, `30d`. When omitted,
    /// defaults to `24h` — the "what shipped today" answer most callers
    /// want without parameters. Unparseable values fall through to the
    /// default so a typo doesn't break a dashboard widget.
    pub since: Option<String>,
    /// Optional `layer` filter (`frontend`/`backend`/`infra`/`docs`/
    /// `tests`/`other`). Matches the `layer` field the agent supplied
    /// on `delivered_features[]`. Case-insensitive.
    pub layer: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct FeatureEntry {
    pub session_id: String,
    pub feature: String,
    pub ts: Option<String>,
    pub files: Vec<String>,
    pub refs: Vec<Value>,
    pub layer: Option<String>,
    /// Free-form recipe the agent supplied — shell command, curl
    /// snippet, "click X then look for Y", etc. Omitted when the
    /// agent didn't include a `how_to_test` for this feature.
    pub how_to_test: Option<String>,
    /// Symbol names (e.g. `fn retrieve()`, `struct BasinSnapshot`).
    /// Empty array when the agent didn't enumerate them.
    pub defs: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct FeaturesResponse {
    pub since: String,
    pub layer: Option<String>,
    pub count: usize,
    pub features: Vec<FeatureEntry>,
}

/// Parse a duration suffix (`5m`, `2h`, `24h`, `7d`, `30d`) into a
/// chrono::Duration. Returns `None` on parse failure so the caller
/// falls back to its default.
pub(crate) fn parse_since(s: &str) -> Option<chrono::Duration> {
    let s = s.trim();
    if s.len() < 2 {
        return None;
    }
    let (num, unit) = s.split_at(s.len() - 1);
    let n: i64 = num.parse().ok()?;
    if n < 0 {
        return None;
    }
    match unit {
        "m" => Some(chrono::Duration::minutes(n)),
        "h" => Some(chrono::Duration::hours(n)),
        "d" => Some(chrono::Duration::days(n)),
        _ => None,
    }
}

pub async fn list_features(
    State(services): State<ContextNestServices>,
    Query(q): Query<FeaturesQuery>,
) -> Result<Json<FeaturesResponse>, StatusCode> {
    let since_raw = q.since.as_deref().unwrap_or("24h");
    let dur = parse_since(since_raw).unwrap_or_else(|| chrono::Duration::hours(24));
    let cutoff = chrono::Utc::now() - dur;
    let layer_low = q.layer.as_deref().map(str::to_lowercase);

    let texts = services.fragment_texts.read().await;
    let metadata = services.fragment_metadata.read().await;
    let mut entries: Vec<FeatureEntry> = Vec::new();

    for (frag_id, meta) in metadata.iter() {
        if meta.get("kind").and_then(|v| v.as_str()) != Some("feature") {
            continue;
        }
        let ts_str = meta.get("ts").and_then(|v| v.as_str());
        if let Some(ts) = ts_str {
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(ts) {
                if parsed.with_timezone(&chrono::Utc) < cutoff {
                    continue;
                }
            }
            // Unparseable ts → don't exclude; better to over-report
            // than silently drop something the dashboard expected.
        }
        let layer = meta
            .get("layer")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(want) = &layer_low {
            let got = layer.as_deref().unwrap_or("").to_lowercase();
            if got != *want {
                continue;
            }
        }
        let session_id = meta
            .get("src_session")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        if session_id.is_empty() {
            continue;
        }
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
        let how_to_test = meta
            .get("how_to_test")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let defs: Vec<String> = meta
            .get("defs")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let feature_text = texts.get(frag_id).cloned().unwrap_or_default();
        entries.push(FeatureEntry {
            session_id,
            feature: feature_text,
            ts: ts_str.map(|s| s.to_string()),
            files,
            refs,
            layer,
            how_to_test,
            defs,
        });
    }
    drop(texts);
    drop(metadata);

    // Newest first; deterministic tiebreak on session_id then feature.
    entries.sort_by(|a, b| {
        b.ts.as_deref()
            .unwrap_or("")
            .cmp(a.ts.as_deref().unwrap_or(""))
            .then_with(|| a.session_id.cmp(&b.session_id))
            .then_with(|| a.feature.cmp(&b.feature))
    });

    Ok(Json(FeaturesResponse {
        since: since_raw.to_string(),
        layer: q.layer,
        count: entries.len(),
        features: entries,
    }))
}

// =============================================================================
// `GET /api/v1/sessions/:id/top-feature`
//
// Returns the single highest-scoring `Feature` record for this session,
// ranking every `delivered_features[]` entry the agent emitted against
// the session's `files_touched` aggregate. The downstream consumer
// (z-dashboard's categorize.ts) uses this as the anchor signal for
// tab routing — features ship, goals drift.
//
// Score components (defaults; surfaced in the response for transparency):
//   freq          0.35 · ln1p(count of records sharing this feature name)
//   file_overlap  0.40 · |feature.files ∩ session.files_touched| / |feature.files|
//   recency       0.15 · normalized 0..1 rank by ts within the session
//   defs          0.05 · ln1p(len(metadata.defs))
//   has_refs      0.05 · constant if feature carries PR/commit refs
// =============================================================================
//
// Tie-break order on equal scores: recency desc, then feature text asc
// (deterministic so the dashboard never flip-flops between two equally-
// good features turn-to-turn).
// =============================================================================

/// Default ranking weights. Tunable later via config; kept as a struct
/// (rather than free constants) so the response payload can echo back
/// the exact weights that produced the score.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct TopFeatureWeights {
    pub freq: f32,
    pub file_overlap: f32,
    pub recency: f32,
    pub defs: f32,
    pub refs: f32,
}

impl Default for TopFeatureWeights {
    fn default() -> Self {
        Self {
            freq: 0.35,
            file_overlap: 0.40,
            recency: 0.15,
            defs: 0.05,
            refs: 0.05,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TopFeatureCandidate {
    pub feature: String,
    pub score: f32,
    pub freq: u32,
    pub file_overlap: f32,
    pub recency: f32,
    pub layer: Option<String>,
    pub ts: Option<String>,
    pub files: Vec<String>,
    pub refs: Vec<Value>,
    pub defs: Vec<String>,
    pub how_to_test: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TopFeatureResponse {
    pub session_id: String,
    pub top_feature: Option<TopFeatureCandidate>,
    pub candidate_count: usize,
    pub weights: TopFeatureWeights,
}

pub async fn top_feature_for_session(
    State(services): State<ContextNestServices>,
    Path(session_id): Path<String>,
) -> Result<Json<TopFeatureResponse>, StatusCode> {
    let weights = TopFeatureWeights::default();
    let texts = services.fragment_texts.read().await;
    let metadata = services.fragment_metadata.read().await;

    // Pass 1: collect this session's Feature records + the session-level
    // files_touched aggregate. Walking metadata once keeps this O(N) over
    // the substrate, same complexity as the sibling handlers above.
    struct RawFeature {
        text: String,
        ts: Option<String>,
        files: Vec<String>,
        refs: Vec<Value>,
        layer: Option<String>,
        defs: Vec<String>,
        how_to_test: Option<String>,
    }
    let mut feats: Vec<RawFeature> = Vec::new();
    let mut session_files: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Use session_index to enumerate THIS session's fragment IDs. The
    // raw `src_session` metadata stores the UUID *without* the `cc-`
    // prefix, while the path param carries the full `cc-<uuid>` form —
    // matching on the metadata sidecar would silently miss every real
    // session. The index is the canonical mapping cc_session_id →
    // Vec<frag_id>; reuse it here the same way list_sessions does.
    let active_ids = services.session_index.list_active(&session_id).await;
    for frag_id in &active_ids {
        let Some(meta) = metadata.get(frag_id) else {
            continue;
        };
        let kind = meta.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        match kind {
            "feature" => {
                let text = texts.get(frag_id).cloned().unwrap_or_default();
                if text.trim().is_empty() {
                    continue;
                }
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
                let defs: Vec<String> = meta
                    .get("defs")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let how_to_test = meta
                    .get("how_to_test")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let ts = meta
                    .get("ts")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                feats.push(RawFeature {
                    text,
                    ts,
                    files,
                    refs,
                    layer,
                    defs,
                    how_to_test,
                });
            }
            "files_touched" => {
                if let Some(arr) = meta.get("files").and_then(|v| v.as_array()) {
                    for f in arr.iter().filter_map(|v| v.as_str()) {
                        if !f.is_empty() {
                            session_files.insert(f.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    drop(texts);
    drop(metadata);

    if feats.is_empty() {
        return Ok(Json(TopFeatureResponse {
            session_id,
            top_feature: None,
            candidate_count: 0,
            weights,
        }));
    }

    // Pass 2: group by lowercased feature text for the freq signal.
    // Lowercasing absorbs trivial casing drift across turns ("GET …" vs
    // "Get …") without merging semantically-different features that
    // happen to share a token.
    let mut freq_by_key: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for f in &feats {
        let key = f.text.to_lowercase();
        *freq_by_key.entry(key).or_insert(0) += 1;
    }

    // Pass 3: assign recency rank from ts ordering. Empty ts sorts oldest.
    // Single-record sessions get recency=1.0 (no ordering signal lost).
    let mut indices: Vec<usize> = (0..feats.len()).collect();
    indices.sort_by(|&a, &b| {
        feats[a]
            .ts
            .as_deref()
            .unwrap_or("")
            .cmp(feats[b].ts.as_deref().unwrap_or(""))
    });
    let mut recency_rank = vec![0.0_f32; feats.len()];
    let n = feats.len();
    for (rank, &idx) in indices.iter().enumerate() {
        recency_rank[idx] = if n == 1 {
            1.0
        } else {
            rank as f32 / (n - 1) as f32
        };
    }

    // Pass 4: score every candidate, find argmax.
    let mut best: Option<(usize, f32, u32, f32)> = None;
    for (i, f) in feats.iter().enumerate() {
        let freq = *freq_by_key.get(&f.text.to_lowercase()).unwrap_or(&1);
        let file_overlap = if f.files.is_empty() || session_files.is_empty() {
            0.0
        } else {
            let hit = f
                .files
                .iter()
                .filter(|p| session_files.contains(*p))
                .count() as f32;
            hit / f.files.len() as f32
        };
        let recency = recency_rank[i];
        let defs_signal = (f.defs.len() as f32).ln_1p();
        let has_refs = !f.refs.is_empty();
        let score = weights.freq * (freq as f32).ln_1p()
            + weights.file_overlap * file_overlap
            + weights.recency * recency
            + weights.defs * defs_signal
            + if has_refs { weights.refs } else { 0.0 };
        let candidate = (i, score, freq, file_overlap);
        best = Some(match best {
            None => candidate,
            Some(prev) => {
                // Tiebreak: higher score wins; on equal score prefer
                // higher recency; on equal recency prefer lexicographically
                // smaller text for deterministic output.
                if score > prev.1 {
                    candidate
                } else if (score - prev.1).abs() < f32::EPSILON {
                    if recency > recency_rank[prev.0] {
                        candidate
                    } else if (recency - recency_rank[prev.0]).abs() < f32::EPSILON
                        && f.text < feats[prev.0].text
                    {
                        candidate
                    } else {
                        prev
                    }
                } else {
                    prev
                }
            }
        });
    }

    let (best_idx, best_score, best_freq, best_overlap) = best.expect("non-empty feats");
    let chosen = &feats[best_idx];
    let top = TopFeatureCandidate {
        feature: chosen.text.clone(),
        score: best_score,
        freq: best_freq,
        file_overlap: best_overlap,
        recency: recency_rank[best_idx],
        layer: chosen.layer.clone(),
        ts: chosen.ts.clone(),
        files: chosen.files.clone(),
        refs: chosen.refs.clone(),
        defs: chosen.defs.clone(),
        how_to_test: chosen.how_to_test.clone(),
    };

    Ok(Json(TopFeatureResponse {
        session_id,
        top_feature: Some(top),
        candidate_count: feats.len(),
        weights,
    }))
}

// =============================================================================
// `GET /api/v1/sessions/:id/summary`
//
// Aggregated session summary in the shape downstream routers (notably
// the z-dashboard categorizer) want when treating ContextNest as the
// single source of truth — replacing the daemon's own LLM transcript
// summarisation. Walks every active fragment for the session ONCE and
// pulls out:
//
//   domain         — text of the session's Domain record (latest seen)
//   progress       — Domain.metadata.progress
//   topics         — Domain.metadata.topics (deduped union)
//   goal           — latest GoalPhase by ts
//   current_state  — latest State by ts
//   top_jobs       — last 5 Accomplishment texts (newest first)
//   facts          — last 5 Learning texts (newest first)
//   tasks          — every Todo with id/subject/status, dedup-keyed
//   started_at     — min ts across all fragments
//   last_ts        — max ts across all fragments
//
// Same complexity as `top_feature_for_session` — single read-lock pass
// over `fragment_metadata`. No LLM, no async beyond the lock.
// =============================================================================

#[derive(Debug, Serialize)]
pub struct SessionSummaryTask {
    pub id: Option<String>,
    pub subject: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct SessionSummaryPayload {
    pub domain: Option<String>,
    pub progress: Option<String>,
    pub topics: Vec<String>,
    pub goal: Option<String>,
    pub current_state: Option<String>,
    pub top_jobs: Vec<String>,
    pub facts: Vec<String>,
    pub tasks: Vec<SessionSummaryTask>,
    pub started_at: Option<String>,
    pub last_ts: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SessionSummaryResponse {
    pub session_id: String,
    pub fragment_count: usize,
    pub summary: SessionSummaryPayload,
}

/// Limit applied to top_jobs and facts — the downstream Insight.summary
/// schema in z-dashboard takes up to 3 of each in the prompt but the
/// daemon may also use these for non-categorize purposes (e.g. session
/// hover cards). 5 strikes a balance: enough for hovers, small enough
/// that the payload stays sub-kilobyte for the common case.
const SUMMARY_KEEP: usize = 5;

pub async fn session_summary(
    State(services): State<ContextNestServices>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionSummaryResponse>, StatusCode> {
    let texts = services.fragment_texts.read().await;
    let metadata = services.fragment_metadata.read().await;

    // Per-kind accumulators. We walk metadata once and route each
    // matching fragment into the correct bucket.
    let mut domain_text: Option<String> = None;
    let mut domain_ts: Option<String> = None;
    let mut progress: Option<String> = None;
    let mut topics: Vec<String> = Vec::new();
    // GoalPhase / State are timestamp-ranked: keep (ts, text) tuples and
    // pick the latest at the end. ts comparison is lexicographic ISO 8601.
    let mut goals: Vec<(String, String)> = Vec::new();
    let mut states: Vec<(String, String)> = Vec::new();
    let mut top_jobs: Vec<(String, String)> = Vec::new();
    let mut facts: Vec<(String, String)> = Vec::new();
    // Tasks dedup keyed by id-if-present-else-subject — mirrors the
    // ingest-time dedup so we don't return the same logical task twice.
    let mut tasks: std::collections::HashMap<String, SessionSummaryTask> =
        std::collections::HashMap::new();
    let mut started_at: Option<String> = None;
    let mut last_ts: Option<String> = None;

    // session_index is the canonical cc_session_id → frag_ids map.
    // Walking metadata.iter() and filtering by `src_session` would silently
    // miss every real session because the metadata sidecar stores the
    // raw UUID without the `cc-` prefix that the path param carries.
    let active_ids = services.session_index.list_active(&session_id).await;
    let fragment_count = active_ids.len();

    for frag_id in &active_ids {
        let Some(meta) = metadata.get(frag_id) else {
            continue;
        };
        let kind = meta.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let ts = meta
            .get("ts")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Track started_at (min ts) and last_ts (max ts) across all
        // fragments — independent of kind.
        if let Some(ts_str) = &ts {
            started_at = Some(match started_at.take() {
                None => ts_str.clone(),
                Some(prev) => {
                    if ts_str < &prev {
                        ts_str.clone()
                    } else {
                        prev
                    }
                }
            });
            last_ts = Some(match last_ts.take() {
                None => ts_str.clone(),
                Some(prev) => {
                    if ts_str > &prev {
                        ts_str.clone()
                    } else {
                        prev
                    }
                }
            });
        }

        match kind {
            "domain" => {
                if let Some(t) = texts.get(frag_id) {
                    if !t.is_empty() {
                        // Take the record with the latest ts. The
                        // extractor already emits only one per session,
                        // but be defensive against ingest-time bugs.
                        let take = match (&domain_ts, &ts) {
                            (None, _) => true,
                            (Some(_), None) => false,
                            (Some(prev), Some(cur)) => cur > prev,
                        };
                        if take {
                            domain_text = Some(t.clone());
                            domain_ts = ts.clone();
                            progress = meta
                                .get("progress")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            topics = meta
                                .get("topics")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                        .collect()
                                })
                                .unwrap_or_default();
                        }
                    }
                }
            }
            "goal_phase" => {
                if let (Some(t), Some(ts_str)) = (texts.get(frag_id), &ts) {
                    if !t.is_empty() {
                        goals.push((ts_str.clone(), t.clone()));
                    }
                }
            }
            "state" => {
                if let (Some(t), Some(ts_str)) = (texts.get(frag_id), &ts) {
                    if !t.is_empty() {
                        states.push((ts_str.clone(), t.clone()));
                    }
                }
            }
            "accomplishment" => {
                if let (Some(t), Some(ts_str)) = (texts.get(frag_id), &ts) {
                    if !t.is_empty() {
                        top_jobs.push((ts_str.clone(), t.clone()));
                    }
                }
            }
            "learning" => {
                if let (Some(t), Some(ts_str)) = (texts.get(frag_id), &ts) {
                    if !t.is_empty() {
                        facts.push((ts_str.clone(), t.clone()));
                    }
                }
            }
            "todo" => {
                if let Some(subject) = texts.get(frag_id).filter(|s| !s.is_empty()) {
                    let id = meta
                        .get("task_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let status = meta
                        .get("task_status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("pending")
                        .to_string();
                    let dedup_key = id.clone().unwrap_or_else(|| subject.clone());
                    tasks.insert(
                        dedup_key,
                        SessionSummaryTask {
                            id,
                            subject: subject.clone(),
                            status,
                        },
                    );
                }
            }
            _ => {}
        }
    }
    drop(texts);
    drop(metadata);

    if fragment_count == 0 {
        // Session not found in the substrate — distinguish from "found
        // but empty" by returning 404 so the daemon can fall through to
        // a local-cache lookup if it wants to.
        return Err(StatusCode::NOT_FOUND);
    }

    // Pick latest by ts for single-value fields.
    goals.sort_by(|a, b| b.0.cmp(&a.0));
    states.sort_by(|a, b| b.0.cmp(&a.0));
    top_jobs.sort_by(|a, b| b.0.cmp(&a.0));
    facts.sort_by(|a, b| b.0.cmp(&a.0));

    let goal = goals.into_iter().next().map(|(_, t)| t);
    let current_state = states.into_iter().next().map(|(_, t)| t);
    let top_jobs_out: Vec<String> = top_jobs
        .into_iter()
        .take(SUMMARY_KEEP)
        .map(|(_, t)| t)
        .collect();
    let facts_out: Vec<String> = facts
        .into_iter()
        .take(SUMMARY_KEEP)
        .map(|(_, t)| t)
        .collect();
    let mut tasks_out: Vec<SessionSummaryTask> = tasks.into_values().collect();
    // Stable task order: pending first (they're what the dashboard wants
    // to highlight), then in_progress, completed, failed, others. Within
    // a status bucket sort alphabetically by subject for determinism.
    fn status_rank(s: &str) -> u8 {
        match s {
            "in_progress" => 0,
            "pending" => 1,
            "failed" => 2,
            "completed" => 3,
            _ => 4,
        }
    }
    tasks_out.sort_by(|a, b| {
        status_rank(&a.status)
            .cmp(&status_rank(&b.status))
            .then_with(|| a.subject.cmp(&b.subject))
    });

    Ok(Json(SessionSummaryResponse {
        session_id,
        fragment_count,
        summary: SessionSummaryPayload {
            domain: domain_text,
            progress,
            topics,
            goal,
            current_state,
            top_jobs: top_jobs_out,
            facts: facts_out,
            tasks: tasks_out,
            started_at,
            last_ts,
        },
    }))
}

// =============================================================================
// `GET /api/v1/sessions/:id/trajectory`
//
// Session-level trajectory surface for the dashboard. This is deliberately
// aggregation-only: no LLM, no writes, no promotion side effects. It turns the
// per-turn z-insight trajectory records into a chronological stream plus phase
// buckets and a review queue for candidate long-term memory.
// =============================================================================

#[derive(Debug, Serialize, Clone)]
pub struct TrajectoryRecord {
    pub id: String,
    pub kind: String,
    pub content: String,
    pub ts: Option<String>,
    pub phase_idx: Option<usize>,
    pub metadata: Value,
}

#[derive(Debug, Serialize)]
pub struct TrajectoryPhase {
    pub idx: usize,
    pub goal: String,
    pub start_ts: Option<String>,
    pub end_ts: Option<String>,
    pub counts: std::collections::HashMap<String, usize>,
    pub decisions: Vec<TrajectoryRecord>,
    pub failures: Vec<TrajectoryRecord>,
    pub verifications: Vec<TrajectoryRecord>,
    pub risks: Vec<TrajectoryRecord>,
    pub prompt_directives: Vec<TrajectoryRecord>,
    pub assumptions: Vec<TrajectoryRecord>,
}

#[derive(Debug, Serialize)]
pub struct TrajectoryCostProfile {
    pub trajectory_records: usize,
    pub turns_estimate: usize,
    pub records_per_turn: f32,
    pub prompt_directives: usize,
    pub memory_candidates: usize,
    pub risk_flags: usize,
}

/// One row of session-to-basin overlap. Emitted on the trajectory response so
/// the dashboard can surface the substrate's clustering judgement without
/// re-querying `/field/basins`. `members_in_session` answers "how many of THIS
/// session's fragments did the substrate cluster into this basin"; `heat_24h`
/// answers "is this basin alive right now" (write-time, basin-wide).
#[derive(Debug, Serialize)]
pub struct BasinLink {
    pub basin_id: String,
    pub members_in_session: usize,
    pub total_members: usize,
    pub heat_24h: usize,
    pub hottest_kind: Option<String>,
}

/// Basins that are NOT in this session but are connected to the session's
/// own basins via the learned connection graph. This is the "resonance"
/// signal — emergent patterns across multiple weakly-related basins that
/// would never come back from a flat similarity query (§5 of
/// `docs/architecture.md`).
///
/// `coherence` is the mean edge weight from session-fragments to neighbors
/// in this basin. `sessions_touching` counts distinct sessions that own
/// the neighbor fragments — surfaces the "seen together" UI ("this
/// debugging session resonates with 3 prior sessions about WAL safety").
#[derive(Debug, Serialize)]
pub struct ResonantBasin {
    pub basin_id: String,
    pub edge_count: usize,
    pub coherence: f32,
    pub sessions_touching: usize,
}

/// Promotion-queue candidates clustered by the basin they live in. Lets the
/// dashboard surface "earned promotion" — three candidates from three
/// sessions resonating into one basin is a strong signal vs. nine
/// candidates spread across nine basins (noise). `coherence` here is the
/// share of the promotion queue concentrated in this basin (0.0–1.0).
#[derive(Debug, Serialize)]
pub struct PromotionCluster {
    pub basin_id: String,
    pub candidates: Vec<TrajectoryRecord>,
    pub coherence: f32,
}

#[derive(Debug, Serialize)]
pub struct TrajectoryResponse {
    pub session_id: String,
    pub trajectory_count: usize,
    pub phases: Vec<TrajectoryPhase>,
    pub records: Vec<TrajectoryRecord>,
    pub promotion_queue: Vec<TrajectoryRecord>,
    pub cost_profile: TrajectoryCostProfile,
    /// Basins this session's fragments live in, ranked by `members_in_session`
    /// desc. Empty when the consolidation worker hasn't crystallised any of
    /// the session's fragments into basins yet (cold-substrate / pre-Phase-3
    /// state, see `docs/architecture-honest.md`).
    pub basin_links: Vec<BasinLink>,
    /// External basins connected to this session's basins via the learned
    /// connection graph, ranked by `coherence × sessions_touching` desc.
    /// Capped at the top 5 to keep response payload bounded. Empty when
    /// the session has no own basins (cold substrate) or when no neighbors
    /// land in foreign basins (isolated session).
    pub resonant_basins: Vec<ResonantBasin>,
    /// `promotion_queue` grouped by the basin each candidate lives in.
    /// Candidates without a basin assignment are omitted (legacy data
    /// stored before consolidation; the flat `promotion_queue` still
    /// surfaces them). Sorted by `candidates.len()` desc.
    pub promotion_clusters: Vec<PromotionCluster>,
}

#[derive(Debug, Clone)]
struct PhaseWindow {
    idx: usize,
    goal: String,
    start_ts: Option<String>,
    end_ts: Option<String>,
}

pub async fn session_trajectory(
    State(services): State<ContextNestServices>,
    Path(session_id): Path<String>,
) -> Result<Json<TrajectoryResponse>, StatusCode> {
    let active_ids = services.session_index.list_active(&session_id).await;
    if active_ids.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    let texts = services.fragment_texts.read().await;
    let metadata = services.fragment_metadata.read().await;

    let mut phases: Vec<PhaseWindow> = Vec::new();
    let mut records: Vec<TrajectoryRecord> = Vec::new();
    let mut state_turns = 0usize;

    for frag_id in &active_ids {
        let Some(meta) = metadata.get(frag_id) else {
            continue;
        };
        let kind = meta.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let ts = meta
            .get("ts")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let text = texts.get(frag_id).cloned().unwrap_or_default();

        if kind == "state" {
            state_turns += 1;
        }

        if kind == "goal_phase" {
            phases.push(PhaseWindow {
                idx: phases.len(),
                goal: text,
                start_ts: meta
                    .get("start_ts")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| ts.clone()),
                end_ts: meta
                    .get("end_ts")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| ts.clone()),
            });
            continue;
        }

        if TRAJECTORY_KINDS.contains(&kind) {
            records.push(TrajectoryRecord {
                id: frag_id.clone(),
                kind: kind.to_string(),
                content: text,
                ts,
                phase_idx: None,
                metadata: Value::Object(meta.clone().into_iter().collect()),
            });
        }
    }
    drop(texts);
    drop(metadata);

    // Basin overlap — answers "which substrate-clustered groups does this
    // session's content live in?" Geometry comes from the manager; per-fragment
    // ts (write-time) gives the heat signal. Holding the metadata read guard
    // through the synchronous loop is intentional — never await inside.
    let basin_snapshots = services.attractor_manager.list_basin_snapshots().await;
    let basin_links: Vec<BasinLink> = if basin_snapshots.is_empty() {
        Vec::new()
    } else {
        let session_id_set: std::collections::HashSet<&String> = active_ids.iter().collect();
        let metadata = services.fragment_metadata.read().await;
        let now = chrono::Utc::now();
        let cutoff = now - chrono::Duration::hours(24);

        let mut links: Vec<BasinLink> = Vec::new();
        for snapshot in &basin_snapshots {
            let overlap_count = snapshot
                .fragment_ids
                .iter()
                .filter(|id| session_id_set.contains(id))
                .count();
            if overlap_count == 0 {
                continue;
            }

            let heat_24h = snapshot
                .fragment_ids
                .iter()
                .filter(|id| {
                    metadata
                        .get(*id)
                        .and_then(|m| m.get("ts"))
                        .and_then(|v| v.as_str())
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|t| t.with_timezone(&chrono::Utc) >= cutoff)
                        .unwrap_or(false)
                })
                .count();

            let mut kind_counts: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            for id in snapshot
                .fragment_ids
                .iter()
                .filter(|id| session_id_set.contains(id))
            {
                if let Some(rec) = records.iter().find(|r| &r.id == id) {
                    *kind_counts.entry(rec.kind.clone()).or_insert(0) += 1;
                }
            }
            let hottest_kind = kind_counts
                .into_iter()
                .max_by_key(|(_, c)| *c)
                .map(|(k, _)| k);

            links.push(BasinLink {
                basin_id: snapshot.id.clone(),
                members_in_session: overlap_count,
                total_members: snapshot.fragment_ids.len(),
                heat_24h,
                hottest_kind,
            });
        }
        drop(metadata);
        links.sort_by(|a, b| {
            b.members_in_session
                .cmp(&a.members_in_session)
                .then_with(|| b.heat_24h.cmp(&a.heat_24h))
        });
        links
    };

    // Resonance — basins connected to ours via the learned-graph that
    // are NOT ours, ranked by mean edge weight × distinct foreign
    // sessions. The fragment→basin and fragment→session reverse maps
    // are built once; the loop is O(active_ids × avg_neighbors).
    let resonant_basins: Vec<ResonantBasin> = if basin_links.is_empty() {
        Vec::new()
    } else {
        let mut fragment_to_basin: std::collections::HashMap<String, String> =
            std::collections::HashMap::with_capacity(
                basin_snapshots.iter().map(|s| s.fragment_ids.len()).sum(),
            );
        for snapshot in &basin_snapshots {
            for fid in &snapshot.fragment_ids {
                fragment_to_basin.insert(fid.clone(), snapshot.id.clone());
            }
        }
        let fragment_to_session = services.session_index.active_fragments_session_map().await;
        let own_basin_ids: std::collections::HashSet<&String> =
            basin_links.iter().map(|b| &b.basin_id).collect();
        let own_fragment_ids: std::collections::HashSet<&String> = active_ids.iter().collect();

        struct ResonanceAccum {
            total_weight: f32,
            edge_count: usize,
            sessions: std::collections::HashSet<String>,
        }
        let mut by_basin: std::collections::HashMap<String, ResonanceAccum> =
            std::collections::HashMap::new();

        for fid in active_ids.iter() {
            let neighbors = services.attractor_manager.list_neighbors(fid).await;
            for (neighbor_id, weight) in neighbors {
                if own_fragment_ids.contains(&neighbor_id) {
                    continue;
                }
                let Some(neighbor_basin) = fragment_to_basin.get(&neighbor_id) else {
                    continue;
                };
                if own_basin_ids.contains(neighbor_basin) {
                    continue;
                }
                let entry =
                    by_basin
                        .entry(neighbor_basin.clone())
                        .or_insert_with(|| ResonanceAccum {
                            total_weight: 0.0,
                            edge_count: 0,
                            sessions: std::collections::HashSet::new(),
                        });
                entry.total_weight += weight;
                entry.edge_count += 1;
                if let Some(session) = fragment_to_session.get(&neighbor_id) {
                    entry.sessions.insert(session.clone());
                }
            }
        }

        let mut basins: Vec<ResonantBasin> = by_basin
            .into_iter()
            .map(|(basin_id, accum)| ResonantBasin {
                basin_id,
                edge_count: accum.edge_count,
                coherence: if accum.edge_count > 0 {
                    accum.total_weight / accum.edge_count as f32
                } else {
                    0.0
                },
                sessions_touching: accum.sessions.len(),
            })
            .collect();
        basins.sort_by(|a, b| {
            let a_score = a.coherence * a.sessions_touching.max(1) as f32;
            let b_score = b.coherence * b.sessions_touching.max(1) as f32;
            b_score
                .partial_cmp(&a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.edge_count.cmp(&a.edge_count))
        });
        basins.truncate(5);
        basins
    };

    phases.sort_by(|a, b| {
        a.start_ts
            .as_deref()
            .unwrap_or("")
            .cmp(b.start_ts.as_deref().unwrap_or(""))
            .then_with(|| a.goal.cmp(&b.goal))
    });
    for (idx, phase) in phases.iter_mut().enumerate() {
        phase.idx = idx;
    }

    for rec in &mut records {
        rec.phase_idx = assign_phase(rec.ts.as_deref(), &phases);
    }
    records.sort_by(|a, b| {
        a.ts.as_deref()
            .unwrap_or("")
            .cmp(b.ts.as_deref().unwrap_or(""))
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.content.cmp(&b.content))
    });

    let mut phase_views: Vec<TrajectoryPhase> = phases
        .iter()
        .map(|p| TrajectoryPhase {
            idx: p.idx,
            goal: p.goal.clone(),
            start_ts: p.start_ts.clone(),
            end_ts: p.end_ts.clone(),
            counts: std::collections::HashMap::new(),
            decisions: Vec::new(),
            failures: Vec::new(),
            verifications: Vec::new(),
            risks: Vec::new(),
            prompt_directives: Vec::new(),
            assumptions: Vec::new(),
        })
        .collect();

    for rec in &records {
        let Some(idx) = rec.phase_idx else {
            continue;
        };
        let Some(phase) = phase_views.get_mut(idx) else {
            continue;
        };
        *phase.counts.entry(rec.kind.clone()).or_insert(0) += 1;
        match rec.kind.as_str() {
            "decision_made" => push_limited(&mut phase.decisions, rec.clone(), 3),
            "failure" => push_limited(&mut phase.failures, rec.clone(), 3),
            "verification" => push_limited(&mut phase.verifications, rec.clone(), 3),
            "risk_flag" => push_limited(&mut phase.risks, rec.clone(), 3),
            "prompt_directive" => push_limited(&mut phase.prompt_directives, rec.clone(), 2),
            "assumption" => push_limited(&mut phase.assumptions, rec.clone(), 2),
            _ => {}
        }
    }

    let promotion_queue: Vec<TrajectoryRecord> = records
        .iter()
        .filter(|r| PROMOTION_KINDS.contains(&r.kind.as_str()))
        .cloned()
        .collect();

    // Cluster the flat promotion queue by basin so the dashboard can
    // surface "earned promotion" (multiple candidates resonating into one
    // basin) vs. one-off candidates. Rebuilds fragment_to_basin from
    // basin_snapshots — small dup with the resonance block, accepted for
    // commit atomicity; a follow-up refactor can hoist it once.
    let promotion_clusters: Vec<PromotionCluster> =
        if promotion_queue.is_empty() || basin_snapshots.is_empty() {
            Vec::new()
        } else {
            let mut fragment_to_basin: std::collections::HashMap<&str, &str> =
                std::collections::HashMap::with_capacity(
                    basin_snapshots.iter().map(|s| s.fragment_ids.len()).sum(),
                );
            for snapshot in &basin_snapshots {
                for fid in &snapshot.fragment_ids {
                    fragment_to_basin.insert(fid.as_str(), snapshot.id.as_str());
                }
            }

            let mut by_basin: std::collections::HashMap<String, Vec<TrajectoryRecord>> =
                std::collections::HashMap::new();
            for rec in &promotion_queue {
                if let Some(basin_id) = fragment_to_basin.get(rec.id.as_str()) {
                    by_basin
                        .entry((*basin_id).to_string())
                        .or_default()
                        .push(rec.clone());
                }
            }

            let clustered_total: usize = by_basin.values().map(|v| v.len()).sum();
            let mut clusters: Vec<PromotionCluster> = by_basin
                .into_iter()
                .map(|(basin_id, candidates)| {
                    let coherence = if clustered_total > 0 {
                        candidates.len() as f32 / clustered_total as f32
                    } else {
                        0.0
                    };
                    PromotionCluster {
                        basin_id,
                        candidates,
                        coherence,
                    }
                })
                .collect();
            clusters.sort_by(|a, b| b.candidates.len().cmp(&a.candidates.len()));
            clusters
        };

    let prompt_directives = records
        .iter()
        .filter(|r| r.kind == "prompt_directive")
        .count();
    let memory_candidates = records
        .iter()
        .filter(|r| r.kind == "memory_candidate")
        .count();
    let risk_flags = records.iter().filter(|r| r.kind == "risk_flag").count();
    let turns_estimate = state_turns.max(1);
    let records_per_turn = records.len() as f32 / turns_estimate as f32;
    let trajectory_count = records.len();

    Ok(Json(TrajectoryResponse {
        session_id,
        trajectory_count,
        phases: phase_views,
        records,
        promotion_queue,
        cost_profile: TrajectoryCostProfile {
            trajectory_records: trajectory_count,
            turns_estimate,
            records_per_turn,
            prompt_directives,
            memory_candidates,
            risk_flags,
        },
        basin_links,
        resonant_basins,
        promotion_clusters,
    }))
}

// =============================================================================
// `GET /api/v1/sessions/:id/prompt-preview`
//
// Deterministic "what would ContextNest inject?" preview. This is intentionally
// conservative and only surfaces trajectory kinds that are directly useful in a
// future prompt capsule.
// =============================================================================

#[derive(Debug, Serialize)]
pub struct PromptPreviewSection {
    pub key: String,
    pub title: String,
    pub kind: String,
    pub items: Vec<TrajectoryRecord>,
}

#[derive(Debug, Serialize)]
pub struct PromptPreviewResponse {
    pub session_id: String,
    pub sections: Vec<PromptPreviewSection>,
    pub item_count: usize,
}

pub async fn session_prompt_preview(
    State(services): State<ContextNestServices>,
    Path(session_id): Path<String>,
) -> Result<Json<PromptPreviewResponse>, StatusCode> {
    let active_ids = services.session_index.list_active(&session_id).await;
    if active_ids.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    let texts = services.fragment_texts.read().await;
    let metadata = services.fragment_metadata.read().await;
    let mut by_kind: std::collections::HashMap<String, Vec<TrajectoryRecord>> =
        std::collections::HashMap::new();

    for frag_id in &active_ids {
        let Some(meta) = metadata.get(frag_id) else {
            continue;
        };
        let kind = meta.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        if !matches!(
            kind,
            "decision_made"
                | "verification"
                | "failure"
                | "risk_flag"
                | "prompt_directive"
                | "assumption"
                | "memory_candidate"
        ) {
            continue;
        }
        if kind == "verification"
            && matches!(meta.get("status").and_then(|v| v.as_str()), Some("not_run"))
        {
            continue;
        }
        let text = texts.get(frag_id).cloned().unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        let rec = TrajectoryRecord {
            id: frag_id.clone(),
            kind: kind.to_string(),
            content: text,
            ts: meta
                .get("ts")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            phase_idx: None,
            metadata: Value::Object(meta.clone().into_iter().collect()),
        };
        by_kind.entry(kind.to_string()).or_default().push(rec);
    }
    drop(texts);
    drop(metadata);

    for records in by_kind.values_mut() {
        records.sort_by(|a, b| {
            b.ts.as_deref()
                .unwrap_or("")
                .cmp(a.ts.as_deref().unwrap_or(""))
                .then_with(|| a.content.cmp(&b.content))
        });
    }

    let specs = [
        (
            "decisions",
            "Relevant prior decisions",
            "decision_made",
            5usize,
        ),
        (
            "verified",
            "Verified workflows and checks",
            "verification",
            5usize,
        ),
        ("failures", "Known failure patterns", "failure", 5usize),
        ("risks", "Active risk flags", "risk_flag", 5usize),
        (
            "directives",
            "Candidate prompt directives",
            "prompt_directive",
            5usize,
        ),
        (
            "assumptions",
            "Assumptions to revalidate",
            "assumption",
            3usize,
        ),
        (
            "candidates",
            "Memory candidates pending review",
            "memory_candidate",
            5usize,
        ),
    ];

    let mut sections = Vec::new();
    let mut item_count = 0usize;
    for (key, title, kind, limit) in specs {
        let items: Vec<TrajectoryRecord> = by_kind
            .remove(kind)
            .unwrap_or_default()
            .into_iter()
            .take(limit)
            .collect();
        item_count += items.len();
        sections.push(PromptPreviewSection {
            key: key.to_string(),
            title: title.to_string(),
            kind: kind.to_string(),
            items,
        });
    }

    Ok(Json(PromptPreviewResponse {
        session_id,
        sections,
        item_count,
    }))
}

fn assign_phase(ts: Option<&str>, phases: &[PhaseWindow]) -> Option<usize> {
    let ts = ts?;
    for phase in phases {
        let starts_before = phase.start_ts.as_deref().map(|s| s <= ts).unwrap_or(true);
        let ends_after = phase.end_ts.as_deref().map(|e| ts <= e).unwrap_or(true);
        if starts_before && ends_after {
            return Some(phase.idx);
        }
    }
    phases
        .iter()
        .filter(|p| p.start_ts.as_deref().map(|s| s <= ts).unwrap_or(false))
        .max_by(|a, b| a.start_ts.cmp(&b.start_ts))
        .map(|p| p.idx)
}

fn push_limited<T>(items: &mut Vec<T>, item: T, limit: usize) {
    if items.len() < limit {
        items.push(item);
    }
}

// =============================================================================
// `GET /api/v1/sessions/attention`
//
// Cross-session attention queue grouped by session. Walks `fragment_metadata`,
// keeps fragments where `is_inbox_eligible(meta)` (user_action, todo with
// task_status != completed, decision with awaiting_decision == true), groups
// by `src_session`, ranks sessions by max ts desc. Filterable by
// `?project=<substring>&since=<duration>&limit=<n>`.
//
// Distinct from `/api/v1/inbox`: inbox returns a flat per-fragment list
// across the substrate; this returns a per-session aggregate so a dashboard
// can render "sessions with open work" without a second roll-up pass.
// Eligibility is shared with the inbox via `is_inbox_eligible`, so the two
// views never drift on what counts as attention.
// =============================================================================

const ATTENTION_DEFAULT_LIMIT: usize = 20;
const ATTENTION_MAX_LIMIT: usize = 200;
const ATTENTION_DEFAULT_SINCE: &str = "30d";
const ATTENTION_ITEMS_PER_SESSION: usize = 5;

#[derive(Debug, Deserialize)]
pub struct AttentionQuery {
    pub project: Option<String>,
    pub since: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct AttentionItem {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub ts: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Serialize)]
pub struct AttentionSession {
    pub session_id: String,
    pub project_cwd: Option<String>,
    pub last_ts: Option<String>,
    pub attention_count: usize,
    pub by_kind: std::collections::HashMap<String, usize>,
    /// Newest-first; capped at `ATTENTION_ITEMS_PER_SESSION` so dashboards
    /// get a representative preview without paying for the full list. Call
    /// `GET /api/v1/sessions/:id` for the complete grouped view.
    pub items: Vec<AttentionItem>,
}

#[derive(Debug, Serialize)]
pub struct AttentionResponse {
    pub since: String,
    pub project: Option<String>,
    pub count: usize,
    pub sessions: Vec<AttentionSession>,
}

pub async fn list_attention(
    State(services): State<ContextNestServices>,
    Query(q): Query<AttentionQuery>,
) -> Result<Json<AttentionResponse>, StatusCode> {
    let since_raw = q.since.as_deref().unwrap_or(ATTENTION_DEFAULT_SINCE);
    let dur = parse_since(since_raw).unwrap_or_else(|| chrono::Duration::days(30));
    let cutoff = chrono::Utc::now() - dur;
    let limit = q
        .limit
        .unwrap_or(ATTENTION_DEFAULT_LIMIT)
        .min(ATTENTION_MAX_LIMIT);
    let project = q.project.clone();

    let texts = services.fragment_texts.read().await;
    let metadata = services.fragment_metadata.read().await;

    let mut by_session: std::collections::HashMap<String, AttentionSession> =
        std::collections::HashMap::new();

    for (frag_id, meta) in metadata.iter() {
        if !is_inbox_eligible(meta) {
            continue;
        }
        let ts_str = meta.get("ts").and_then(|v| v.as_str());
        if let Some(ts) = ts_str {
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(ts) {
                if parsed.with_timezone(&chrono::Utc) < cutoff {
                    continue;
                }
            }
        }
        let session_id = meta
            .get("src_session")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        if session_id.is_empty() {
            continue;
        }
        let project_cwd = meta
            .get("project_cwd")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(want) = project.as_deref() {
            match project_cwd.as_deref() {
                Some(p) if p.contains(want) => {}
                _ => continue,
            }
        }
        let kind = meta
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let entry = by_session
            .entry(session_id.clone())
            .or_insert_with(|| AttentionSession {
                session_id: session_id.clone(),
                project_cwd: project_cwd.clone(),
                last_ts: None,
                attention_count: 0,
                by_kind: std::collections::HashMap::new(),
                items: Vec::new(),
            });
        entry.attention_count += 1;
        *entry.by_kind.entry(kind.clone()).or_insert(0) += 1;
        if let Some(ts) = ts_str {
            if entry.last_ts.as_deref().map(|cur| ts > cur).unwrap_or(true) {
                entry.last_ts = Some(ts.to_string());
            }
        }
        entry.items.push(AttentionItem {
            id: frag_id.clone(),
            kind,
            text: texts.get(frag_id).cloned().unwrap_or_default(),
            ts: ts_str.map(|s| s.to_string()),
            metadata: Value::Object(meta.clone().into_iter().collect()),
        });
    }
    drop(texts);
    drop(metadata);

    let mut sessions: Vec<AttentionSession> = by_session.into_values().collect();
    // Per-session: newest items first, then truncate to the preview cap.
    for s in &mut sessions {
        s.items.sort_by(|a, b| {
            b.ts.as_deref()
                .unwrap_or("")
                .cmp(a.ts.as_deref().unwrap_or(""))
                .then_with(|| a.id.cmp(&b.id))
        });
        s.items.truncate(ATTENTION_ITEMS_PER_SESSION);
    }
    // Cross-session: newest activity first, deterministic tiebreak on id.
    sessions.sort_by(|a, b| {
        b.last_ts
            .as_deref()
            .unwrap_or("")
            .cmp(a.last_ts.as_deref().unwrap_or(""))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    sessions.truncate(limit);

    Ok(Json(AttentionResponse {
        since: since_raw.to_string(),
        project,
        count: sessions.len(),
        sessions,
    }))
}

// =============================================================================
// `GET /api/v1/sessions/:id` — full session detail.
//
// Returns every fragment owned by the session grouped by kind, with the
// actionable kinds listed first so a dashboard can render the "still open"
// part of the session at the top. Distinct from `/sessions/:id/summary`
// (top_jobs / facts / tasks digest) and `/sessions/:id/trajectory`
// (phase-windowed trajectory atoms): this is the raw grouped view, useful
// when an agent or operator wants every record the session produced.
// =============================================================================

/// Stable ordering of kinds in the response. Actionable kinds first, then
/// trajectory signals (the L1 prompt-context atoms), then narrative kinds.
/// Anything not listed lands at the end in lexicographic order so the
/// response remains deterministic even when new kinds appear.
const DETAIL_KIND_PRIORITY: &[&str] = &[
    // Actionable — same set as `is_inbox_eligible` plus `blocker` (which the
    // extractor doesn't currently emit but is reserved by spec).
    "user_action",
    "todo",
    "blocker",
    "decision",
    // Trajectory — atoms that prompt-context Phase 1a indexes.
    "decision_made",
    "failure",
    "risk_flag",
    "verification",
    "evidence_ref",
    "read_context",
    "prompt_directive",
    "assumption",
    "artifact",
    "memory_candidate",
    // Narrative.
    "session_title",
    "goal_phase",
    "current_task",
    "accomplishment",
    "learning",
    "feature",
    "state",
    "summary",
    "domain",
    "files_touched",
    "initial_prompt_window",
];

#[derive(Debug, Serialize)]
pub struct DetailItem {
    pub id: String,
    pub text: String,
    pub ts: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Serialize)]
pub struct DetailGroup {
    pub kind: String,
    pub items: Vec<DetailItem>,
}

#[derive(Debug, Serialize)]
pub struct SessionDetailResponse {
    pub session_id: String,
    pub project_cwd: Option<String>,
    pub last_ts: Option<String>,
    pub total_fragments: usize,
    pub by_kind: std::collections::HashMap<String, usize>,
    pub groups: Vec<DetailGroup>,
}

pub async fn session_detail(
    State(services): State<ContextNestServices>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionDetailResponse>, StatusCode> {
    let active_ids = services.session_index.list_active(&session_id).await;
    if active_ids.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    let texts = services.fragment_texts.read().await;
    let metadata = services.fragment_metadata.read().await;

    let mut grouped: std::collections::HashMap<String, Vec<DetailItem>> =
        std::collections::HashMap::new();
    let mut by_kind: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut last_ts: Option<String> = None;
    let mut project_cwd: Option<String> = None;

    for frag_id in &active_ids {
        let Some(meta) = metadata.get(frag_id) else {
            continue;
        };
        let kind = meta
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let ts = meta
            .get("ts")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if let Some(ref t) = ts {
            if last_ts
                .as_deref()
                .map(|cur| t.as_str() > cur)
                .unwrap_or(true)
            {
                last_ts = Some(t.clone());
            }
        }
        if project_cwd.is_none() {
            project_cwd = meta
                .get("project_cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
        *by_kind.entry(kind.clone()).or_insert(0) += 1;
        grouped.entry(kind).or_default().push(DetailItem {
            id: frag_id.clone(),
            text: texts.get(frag_id).cloned().unwrap_or_default(),
            ts,
            metadata: Value::Object(meta.clone().into_iter().collect()),
        });
    }
    drop(texts);
    drop(metadata);

    // Within each group: newest first; deterministic tiebreak on id.
    for items in grouped.values_mut() {
        items.sort_by(|a, b| {
            b.ts.as_deref()
                .unwrap_or("")
                .cmp(a.ts.as_deref().unwrap_or(""))
                .then_with(|| a.id.cmp(&b.id))
        });
    }

    // Emit groups in priority order; any unknown kinds at the end in
    // lexicographic order. A kind with zero items is omitted entirely.
    let mut groups: Vec<DetailGroup> = Vec::new();
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for &kind in DETAIL_KIND_PRIORITY {
        if let Some(items) = grouped.remove(kind) {
            seen.insert(kind);
            groups.push(DetailGroup {
                kind: kind.to_string(),
                items,
            });
        }
    }
    let mut leftover_keys: Vec<String> = grouped.keys().cloned().collect();
    leftover_keys.sort();
    for kind in leftover_keys {
        if let Some(items) = grouped.remove(&kind) {
            groups.push(DetailGroup { kind, items });
        }
    }
    let _ = seen; // future: detect orphan kinds

    Ok(Json(SessionDetailResponse {
        session_id,
        project_cwd,
        last_ts,
        total_fragments: active_ids.len(),
        by_kind,
        groups,
    }))
}

// =============================================================================
// `POST /api/v1/sessions/find` — NL session search.
//
// Embeds the caller's query, cosine-scores every candidate fragment whose
// kind is `goal_phase` or `session_title` (the two kinds that name a
// session's *intent*), groups by `src_session`, keeps the max-cosine
// fragment per session, and returns the top-N sessions ranked by that
// score. Optional `project` substring + `since` window filters mirror
// `/sessions/attention`.
//
// Why `goal_phase` + `session_title` and nothing else: this endpoint
// answers "find the session where I worked on X", which is a question
// about session intent, not about every fragment the session produced.
// Indexing on intent kinds keeps the search precision-heavy — a session
// whose decisions/failures happened to mention "auth" won't shadow the
// session whose stated goal was "auth refactor".
//
// Failure semantics:
//   - empty query                → 400 BAD_REQUEST
//   - embedder failure           → 503 SERVICE_UNAVAILABLE (transient)
//   - zero candidates            → 200 with `sessions: []`
// =============================================================================

/// Kinds the embedding-cosine search indexes against. Intent kinds only.
const FIND_INDEXED_KINDS: &[&str] = &["goal_phase", "session_title"];
const FIND_DEFAULT_LIMIT: usize = 10;
const FIND_MAX_LIMIT: usize = 50;
const FIND_DEFAULT_SINCE: &str = "90d";
/// Hard cap on candidate fragments scored per request. With ~5-30 intent
/// fragments per session at typical scale, 5000 covers ~200-1000 sessions
/// — plenty for the current corpus and prevents a worst-case 50k-fragment
/// substrate from pinning the embedder. Above this we score the most
/// recent N and surface `truncated:true` so the caller knows.
const FIND_CANDIDATE_CAP: usize = 5000;
/// `match_text` preview length so a 10-result page stays under ~5 KB.
const FIND_MATCH_TEXT_LIMIT: usize = 280;

#[derive(Debug, Deserialize)]
pub struct FindRequest {
    pub query: String,
    pub project: Option<String>,
    pub since: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct FindHit {
    pub session_id: String,
    pub project_cwd: Option<String>,
    pub last_ts: Option<String>,
    pub score: f32,
    /// Which indexed kind the top-scoring fragment was. Useful so the
    /// caller can tell whether the session matched by goal narrative
    /// vs. by literal title.
    pub match_kind: String,
    pub match_text: String,
    pub match_ts: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FindResponse {
    pub query: String,
    pub since: String,
    pub project: Option<String>,
    pub count: usize,
    pub total_scored: usize,
    pub truncated: bool,
    pub sessions: Vec<FindHit>,
}

pub async fn find_sessions(
    State(services): State<ContextNestServices>,
    Json(req): Json<FindRequest>,
) -> Result<Json<FindResponse>, StatusCode> {
    let query = req.query.trim();
    if query.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let since_raw = req.since.as_deref().unwrap_or(FIND_DEFAULT_SINCE);
    let dur = parse_since(since_raw).unwrap_or_else(|| chrono::Duration::days(90));
    let cutoff = chrono::Utc::now() - dur;
    let limit = req.limit.unwrap_or(FIND_DEFAULT_LIMIT).min(FIND_MAX_LIMIT);
    let project = req.project.clone();

    // 1. Collect candidate ids from the metadata sidecar. We capture
    //    (id, ts) so we can keep the most-recent N if we hit the cap.
    let mut candidates: Vec<(String, Option<String>)> = {
        let metadata = services.fragment_metadata.read().await;
        let mut out: Vec<(String, Option<String>)> = Vec::new();
        for (frag_id, meta) in metadata.iter() {
            let kind = meta.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            if !FIND_INDEXED_KINDS.contains(&kind) {
                continue;
            }
            let ts_str = meta.get("ts").and_then(|v| v.as_str());
            if let Some(ts) = ts_str {
                if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(ts) {
                    if parsed.with_timezone(&chrono::Utc) < cutoff {
                        continue;
                    }
                }
            }
            if let Some(want) = project.as_deref() {
                match meta.get("project_cwd").and_then(|v| v.as_str()) {
                    Some(p) if p.contains(want) => {}
                    _ => continue,
                }
            }
            // `src_session` empty → can't group; skip.
            if meta
                .get("src_session")
                .and_then(|v| v.as_str())
                .map(str::is_empty)
                .unwrap_or(true)
            {
                continue;
            }
            out.push((frag_id.clone(), ts_str.map(|s| s.to_string())));
        }
        out
    };

    // Cap candidate set. Sort newest-first so the cap keeps the most recent.
    let truncated = candidates.len() > FIND_CANDIDATE_CAP;
    if truncated {
        candidates.sort_by(|a, b| {
            b.1.as_deref()
                .unwrap_or("")
                .cmp(a.1.as_deref().unwrap_or(""))
                .then_with(|| a.0.cmp(&b.0))
        });
        candidates.truncate(FIND_CANDIDATE_CAP);
    }
    let total_scored = candidates.len();

    if total_scored == 0 {
        return Ok(Json(FindResponse {
            query: query.to_string(),
            since: since_raw.to_string(),
            project,
            count: 0,
            total_scored: 0,
            truncated: false,
            sessions: Vec::new(),
        }));
    }

    // 2. Embed the query. Embedder failure → 503; callers can retry.
    let query_embedding = match services.embedding.generate_embedding(query).await {
        Ok(v) => v,
        Err(e) => {
            warn!(error = ?e, "find_sessions: embedder failure");
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
    };

    // 3. For each candidate, hydrate via the attractor manager (which
    //    carries the embedding under `fragment.content` — load-bearing
    //    naming quirk: `.content` is the vector, NOT the text), score
    //    cosine, retain the best per session. Fragments the manager has
    //    no embedding for (sidecar-only — post-WAL-replay state) are
    //    silently skipped here; they have no embedding to cosine against.
    struct Best {
        score: f32,
        frag_id: String,
        kind: String,
        ts: Option<String>,
    }
    let mut best_by_session: std::collections::HashMap<String, Best> =
        std::collections::HashMap::new();
    let metadata = services.fragment_metadata.read().await;

    for (frag_id, ts) in &candidates {
        let fragment = match services.attractor_manager.get_fragment(frag_id).await {
            Ok(Some(f)) => f,
            _ => continue,
        };
        let score = services
            .embedding
            .calculate_similarity(&query_embedding, &fragment.content);
        let Some(meta) = metadata.get(frag_id) else {
            continue;
        };
        let session_id = match meta.get("src_session").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let kind = meta
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        match best_by_session.get(&session_id) {
            Some(prev) if prev.score >= score => {}
            _ => {
                best_by_session.insert(
                    session_id,
                    Best {
                        score,
                        frag_id: frag_id.clone(),
                        kind,
                        ts: ts.clone(),
                    },
                );
            }
        }
    }

    // 4. Rank sessions by score desc, materialise the text preview.
    let texts = services.fragment_texts.read().await;
    let mut hits: Vec<FindHit> = best_by_session
        .into_iter()
        .map(|(session_id, b)| {
            let project_cwd = metadata
                .get(&b.frag_id)
                .and_then(|m| m.get("project_cwd"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let text_raw = texts.get(&b.frag_id).cloned().unwrap_or_default();
            let match_text = if text_raw.chars().count() > FIND_MATCH_TEXT_LIMIT {
                let truncated: String = text_raw.chars().take(FIND_MATCH_TEXT_LIMIT).collect();
                format!("{truncated}…")
            } else {
                text_raw
            };
            FindHit {
                session_id,
                project_cwd,
                last_ts: b.ts.clone(),
                score: b.score,
                match_kind: b.kind,
                match_text,
                match_ts: b.ts,
            }
        })
        .collect();
    drop(texts);
    drop(metadata);

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    hits.truncate(limit);

    Ok(Json(FindResponse {
        query: query.to_string(),
        since: since_raw.to_string(),
        project,
        count: hits.len(),
        total_scored,
        truncated,
        sessions: hits,
    }))
}

/// Build the sessions router. Mounted alongside the tools and cc_hooks routers
/// in `create_simple_app`.
///
/// Route ordering matters: Axum's matchit gives static segments priority over
/// `:id` parameters, so `/sessions/attention`, `/sessions/by-file`, and
/// `/sessions/by-feature` all resolve correctly even though they sit next to
/// `/sessions/:id`.
pub fn create_sessions_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/api/v1/sessions", get(list_sessions))
        .route("/api/v1/sessions/attention", get(list_attention))
        .route("/api/v1/sessions/find", axum::routing::post(find_sessions))
        .route("/api/v1/sessions/by-file", get(sessions_by_file))
        .route("/api/v1/sessions/by-feature", get(sessions_by_feature))
        .route(
            "/api/v1/sessions/:id/top-feature",
            get(top_feature_for_session),
        )
        .route("/api/v1/sessions/:id/summary", get(session_summary))
        .route("/api/v1/sessions/:id/trajectory", get(session_trajectory))
        .route(
            "/api/v1/sessions/:id/prompt-preview",
            get(session_prompt_preview),
        )
        .route("/api/v1/sessions/:id", get(session_detail))
        .route("/api/v1/features", get(list_features))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Seed fragment_texts + fragment_metadata + session_index for a
    /// canned cross-session corpus shared by the attention / detail tests.
    /// - S1: one pending todo, one user_action, one completed todo (drop),
    ///       all recent.
    /// - S2: one pending todo, old enough to fall outside default since.
    /// - S3: one pending todo + one goal_phase (non-attention), recent,
    ///       different project.
    async fn seed_attention_corpus(services: &ContextNestServices) {
        let now = chrono::Utc::now();
        let recent = now.to_rfc3339();
        let old = (now - chrono::Duration::days(120)).to_rfc3339();

        let mut texts = services.fragment_texts.write().await;
        let mut meta = services.fragment_metadata.write().await;

        let put = |texts: &mut std::collections::HashMap<String, String>,
                   meta: &mut std::collections::HashMap<
            String,
            std::collections::HashMap<String, Value>,
        >,
                   id: &str,
                   text: &str,
                   kv: &[(&str, Value)]| {
            texts.insert(id.to_string(), text.to_string());
            let m: std::collections::HashMap<String, Value> =
                kv.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
            meta.insert(id.to_string(), m);
        };

        put(
            &mut texts,
            &mut meta,
            "f1",
            "pending todo in S1 alpha",
            &[
                ("kind", json!("todo")),
                ("task_status", json!("pending")),
                ("ts", json!(recent)),
                ("src_session", json!("S1")),
                ("project_cwd", json!("/repo/alpha")),
            ],
        );
        put(
            &mut texts,
            &mut meta,
            "f2",
            "user action in S1",
            &[
                ("kind", json!("user_action")),
                ("ts", json!(recent)),
                ("src_session", json!("S1")),
                ("project_cwd", json!("/repo/alpha")),
            ],
        );
        put(
            &mut texts,
            &mut meta,
            "f3",
            "completed todo in S1 (must be excluded)",
            &[
                ("kind", json!("todo")),
                ("task_status", json!("completed")),
                ("ts", json!(recent)),
                ("src_session", json!("S1")),
                ("project_cwd", json!("/repo/alpha")),
            ],
        );
        put(
            &mut texts,
            &mut meta,
            "f4",
            "old pending todo in S2 (since cutoff drops it)",
            &[
                ("kind", json!("todo")),
                ("task_status", json!("pending")),
                ("ts", json!(old)),
                ("src_session", json!("S2")),
                ("project_cwd", json!("/repo/beta")),
            ],
        );
        put(
            &mut texts,
            &mut meta,
            "f5",
            "pending todo in S3 gamma",
            &[
                ("kind", json!("todo")),
                ("task_status", json!("pending")),
                ("ts", json!(recent)),
                ("src_session", json!("S3")),
                ("project_cwd", json!("/repo/gamma")),
            ],
        );
        put(
            &mut texts,
            &mut meta,
            "f6",
            "goal_phase in S3 (non-attention)",
            &[
                ("kind", json!("goal_phase")),
                ("ts", json!(recent)),
                ("src_session", json!("S3")),
                ("project_cwd", json!("/repo/gamma")),
            ],
        );
        drop(meta);
        drop(texts);

        // Wire the session_index so /sessions/:id can resolve S1.
        services.session_index.add("S1", "f1").await;
        services.session_index.add("S1", "f2").await;
        services.session_index.add("S1", "f3").await;
        services.session_index.add("S3", "f5").await;
        services.session_index.add("S3", "f6").await;
    }

    #[tokio::test]
    async fn attention_groups_eligible_kinds_by_session() {
        let services = ContextNestServices::new_default().await.expect("services");
        seed_attention_corpus(&services).await;

        let resp = list_attention(
            State(services),
            Query(AttentionQuery {
                project: None,
                since: None,
                limit: None,
            }),
        )
        .await
        .expect("ok")
        .0;

        // S2 is too old; S1 + S3 only. S1 carries the todo + user_action,
        // S3 carries the lone pending todo (goal_phase is filtered out).
        assert_eq!(resp.count, 2);
        let by_id: std::collections::HashMap<&str, &AttentionSession> = resp
            .sessions
            .iter()
            .map(|s| (s.session_id.as_str(), s))
            .collect();
        let s1 = by_id.get("S1").expect("S1 present");
        assert_eq!(
            s1.attention_count, 2,
            "todo + user_action, completed dropped"
        );
        let s3 = by_id.get("S3").expect("S3 present");
        assert_eq!(s3.attention_count, 1);
        // f3 (completed) must not surface anywhere.
        for s in &resp.sessions {
            for item in &s.items {
                assert_ne!(item.id, "f3", "completed todo must be excluded");
            }
        }
    }

    #[tokio::test]
    async fn attention_filters_by_project_substring() {
        let services = ContextNestServices::new_default().await.expect("services");
        seed_attention_corpus(&services).await;

        let resp = list_attention(
            State(services),
            Query(AttentionQuery {
                project: Some("gamma".into()),
                since: None,
                limit: None,
            }),
        )
        .await
        .expect("ok")
        .0;

        assert_eq!(resp.count, 1);
        assert_eq!(resp.sessions[0].session_id, "S3");
    }

    #[tokio::test]
    async fn session_detail_groups_by_kind_in_priority_order() {
        let services = ContextNestServices::new_default().await.expect("services");
        seed_attention_corpus(&services).await;

        let resp = session_detail(State(services), Path("S1".into()))
            .await
            .expect("ok")
            .0;

        assert_eq!(resp.total_fragments, 3);
        assert_eq!(resp.project_cwd.as_deref(), Some("/repo/alpha"));
        // by_kind keeps the completed todo (raw view; filtering belongs to
        // /attention). user_action=1, todo=2 (f1 pending + f3 completed).
        assert_eq!(resp.by_kind.get("user_action"), Some(&1));
        assert_eq!(resp.by_kind.get("todo"), Some(&2));

        // Group order: actionable kinds (user_action, todo) must come before
        // anything else. S1 only carries user_action + todo so we expect
        // exactly those two groups in that order.
        let kinds: Vec<&str> = resp.groups.iter().map(|g| g.kind.as_str()).collect();
        assert_eq!(kinds, vec!["user_action", "todo"]);
    }

    #[tokio::test]
    async fn session_detail_unknown_session_is_not_found() {
        let services = ContextNestServices::new_default().await.expect("services");
        let err = session_detail(State(services), Path("nope".into()))
            .await
            .unwrap_err();
        assert_eq!(err, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn find_sessions_rejects_empty_query() {
        let services = ContextNestServices::new_default().await.expect("services");
        let err = find_sessions(
            State(services),
            Json(FindRequest {
                query: "   ".into(), // whitespace-only counts as empty
                project: None,
                since: None,
                limit: None,
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn find_sessions_returns_empty_list_when_no_candidates() {
        // No goal_phase / session_title fragments seeded → handler must
        // short-circuit BEFORE touching the embedder. This is also how we
        // assert the candidate-collection prefilter is correct: if it
        // didn't filter to indexed kinds, the seed's attention todos would
        // wrongly become candidates and the embedder would be called.
        let services = ContextNestServices::new_default().await.expect("services");
        seed_attention_corpus(&services).await; // only todos/user_actions/goal_phase
                                                // Note: seed has f6 with kind=goal_phase but only in S3 (recent).
                                                // To exercise the zero-candidate path we filter to a project
                                                // with no goal_phase / session_title fragments.
        let resp = find_sessions(
            State(services),
            Json(FindRequest {
                query: "anything".into(),
                project: Some("__no_such_project__".into()),
                since: None,
                limit: None,
            }),
        )
        .await
        .expect("ok")
        .0;
        assert_eq!(resp.count, 0);
        assert_eq!(resp.total_scored, 0);
        assert!(!resp.truncated);
        assert!(resp.sessions.is_empty());
    }
}

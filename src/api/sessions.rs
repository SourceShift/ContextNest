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
fn parse_since(s: &str) -> Option<chrono::Duration> {
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
    let mut session_files: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for (frag_id, meta) in metadata.iter() {
        let src = meta.get("src_session").and_then(|v| v.as_str());
        if src != Some(session_id.as_str()) {
            continue;
        }
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
    let mut freq_by_key: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
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
    let mut fragment_count: usize = 0;

    for (frag_id, meta) in metadata.iter() {
        let src = meta.get("src_session").and_then(|v| v.as_str());
        if src != Some(session_id.as_str()) {
            continue;
        }
        fragment_count += 1;
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

/// Build the sessions router. Mounted alongside the tools and cc_hooks routers
/// in `create_simple_app`.
pub fn create_sessions_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/api/v1/sessions", get(list_sessions))
        .route("/api/v1/sessions/by-file", get(sessions_by_file))
        .route("/api/v1/sessions/by-feature", get(sessions_by_feature))
        .route(
            "/api/v1/sessions/:id/top-feature",
            get(top_feature_for_session),
        )
        .route("/api/v1/sessions/:id/summary", get(session_summary))
        .route("/api/v1/features", get(list_features))
}

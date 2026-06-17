//! `POST /api/v1/agent/outcome` — PR-6 of the agent-context-pack epic.
//!
//! Outcome feedback loop (EvoMem pattern, arXiv:2511.01912). After a
//! mini-ork worker subagent stops, mini-ork POSTs the set of CN atom
//! ids the worker consumed in its prefetch alongside the run outcome
//! (success/failure) and optional evidence text. CN bumps the
//! `last_accessed` timestamp on each atom (always — even on failure)
//! and adjusts a `confidence_signal` scalar in fragment_metadata,
//! capped at ±0.05 per call to prevent a single noisy worker from
//! pinning a basin.
//!
//! The endpoint is **side-effect-light**: it does NOT trigger
//! re-consolidation, re-embedding, basin re-formation, or anything
//! that touches the hot path. All it does is write metadata that
//! retrieve already reads (`last_accessed`, plus a new `confidence_signal`
//! scalar that future scoring passes can multiply in).
//!
//! ## Body
//!
//! ```json
//! {
//!   "atom_ids": ["cn-...", "cn-..."],
//!   "outcome": "success" | "failure",
//!   "evidence": "optional free-text snippet (≤480 chars stored)",
//!   "session_id": "optional consumer session id (for traceability)"
//! }
//! ```
//!
//! ## Response
//!
//! `200 OK` with JSON:
//! ```json
//! { "updated": <count>, "skipped_unknown": <count>, "delta_applied": ±0.05 }
//! ```
//!
//! `400 Bad Request` when body is malformed.
//!
//! ## Why string-typed outcome (not enum)
//!
//! Future mini-ork versions may want richer outcome shapes (e.g.
//! `partial_success`, `silent_failure_caught_in_review`). String + a
//! small parse keeps the wire format extensible without a v2 endpoint.
//! Anything other than `success`/`failure` is treated as no-confidence-
//! change (still updates last_accessed for trajectory tracking).
//!
//! ## What's intentionally NOT here
//!
//! - **No retroactive decay sweep.** Atoms that never get outcome
//!   feedback don't decay just because we now track outcomes. A
//!   separate consolidation-worker pass could handle that; out of
//!   scope for PR-6.
//! - **No per-session aggregation.** Each call applies its delta
//!   independently; a worker that runs twice and consumes the same
//!   atom both times produces 2x the delta. Higher-level aggregation
//!   (e.g. one outcome per atom per session) is the caller's
//!   responsibility.
//! - **No new sidecar table.** The confidence signal lives in
//!   `fragment_metadata` (the existing sidecar) as a new key
//!   `_cn_confidence_signal: f64` defaulting to 0.0. Bounded ±1.0
//!   total to prevent runaway accumulation.

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::Json as JsonResponse,
    routing::post,
    Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::services::ContextNestServices;

/// Per-call confidence delta cap. A single outcome report can shift
/// an atom's confidence_signal by at most ±0.05 — even an enthusiastic
/// caller can't pin a basin without 20+ rounds of consistent feedback.
const CONFIDENCE_DELTA_PER_CALL: f64 = 0.05;

/// Bounded range for the cumulative confidence signal. Atoms can't
/// drift past ±1.0 regardless of how many outcomes get reported.
/// Future scoring can normalize via tanh or similar without losing
/// signal at the bounds.
const CONFIDENCE_SIGNAL_MIN: f64 = -1.0;
const CONFIDENCE_SIGNAL_MAX: f64 = 1.0;

/// Metadata key for the confidence signal scalar. Prefixed with `_cn_`
/// (like other CN-controlled metadata keys) so callers know not to
/// overwrite it from outside the substrate.
const CONFIDENCE_SIGNAL_FIELD: &str = "_cn_confidence_signal";

/// Metadata key for the count of outcome reports applied. Helps debug
/// "why is this atom's signal so high" later.
const CONFIDENCE_REPORTS_COUNT_FIELD: &str = "_cn_confidence_reports_count";

/// Metadata key for the latest outcome evidence string (capped at 480
/// chars — same length budget as cc_hooks result_excerpt).
const LATEST_OUTCOME_EVIDENCE_FIELD: &str = "_cn_latest_outcome_evidence";

/// Metadata key for the last_accessed timestamp, kept in sync with
/// the retrieve-time bump but written here independently for the
/// outcome-feedback path.
const LAST_ACCESSED_FIELD: &str = "last_accessed";

#[derive(Debug, Deserialize)]
pub struct OutcomeRequest {
    pub atom_ids: Vec<String>,
    pub outcome: String,
    #[serde(default)]
    pub evidence: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OutcomeResponse {
    /// Number of atoms whose metadata was updated.
    pub updated: usize,
    /// Number of atoms in the request that didn't exist in metadata
    /// (silent skip, not an error — atom_ids may be stale).
    pub skipped_unknown: usize,
    /// Per-atom delta applied to confidence_signal (signed).
    /// +0.05 on success, -0.05 on failure, 0.0 otherwise.
    pub delta_applied: f64,
}

fn classify_outcome(outcome: &str) -> f64 {
    match outcome.to_ascii_lowercase().as_str() {
        "success" | "ok" | "passed" => CONFIDENCE_DELTA_PER_CALL,
        "failure" | "failed" | "fail" => -CONFIDENCE_DELTA_PER_CALL,
        _ => 0.0,
    }
}

fn truncate_to_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

pub async fn handle_outcome(
    State(services): State<ContextNestServices>,
    Json(req): Json<OutcomeRequest>,
) -> Result<JsonResponse<OutcomeResponse>, StatusCode> {
    if req.atom_ids.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let delta = classify_outcome(&req.outcome);
    let now_rfc = Utc::now().to_rfc3339();
    let evidence_capped = req.evidence.as_deref().map(|s| truncate_to_chars(s, 480));

    let mut updated = 0usize;
    let mut skipped_unknown = 0usize;

    {
        let mut meta_w = services.fragment_metadata.write().await;
        for atom_id in &req.atom_ids {
            // Skip blank ids defensively — mini-ork's hook code may
            // sometimes emit empty strings if extraction failed.
            if atom_id.is_empty() {
                skipped_unknown += 1;
                continue;
            }

            // Only update atoms we already know about. Unknown ids
            // are NOT inserted — that would silently inflate the
            // substrate with empty-metadata entries.
            let Some(entry) = meta_w.get_mut(atom_id) else {
                skipped_unknown += 1;
                continue;
            };

            // Always bump last_accessed — even on neutral outcomes —
            // so trajectory analyses can see this atom was consumed.
            entry.insert(
                LAST_ACCESSED_FIELD.to_string(),
                Value::String(now_rfc.clone()),
            );

            // Apply confidence delta (clipped to ±1.0 cumulative range).
            // Default to 0.0 when the field is absent or non-numeric;
            // defensive against legacy atoms.
            let current = entry
                .get(CONFIDENCE_SIGNAL_FIELD)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let next = (current + delta).clamp(CONFIDENCE_SIGNAL_MIN, CONFIDENCE_SIGNAL_MAX);
            entry.insert(CONFIDENCE_SIGNAL_FIELD.to_string(), Value::from(next));

            // Bump the reports-count counter for debug visibility.
            let prev_count = entry
                .get(CONFIDENCE_REPORTS_COUNT_FIELD)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            entry.insert(
                CONFIDENCE_REPORTS_COUNT_FIELD.to_string(),
                Value::from(prev_count + 1),
            );

            // Store the latest evidence snippet if provided.
            if let Some(ev) = &evidence_capped {
                entry.insert(
                    LATEST_OUTCOME_EVIDENCE_FIELD.to_string(),
                    Value::String(ev.clone()),
                );
            }

            updated += 1;
        }
    }

    tracing::info!(
        outcome = %req.outcome,
        delta = delta,
        updated,
        skipped_unknown,
        consumer_session = ?req.session_id,
        "agent_outcome: feedback applied"
    );

    Ok(JsonResponse(OutcomeResponse {
        updated,
        skipped_unknown,
        delta_applied: delta,
    }))
}

pub fn create_agent_outcome_router() -> Router<ContextNestServices> {
    Router::new().route("/api/v1/agent/outcome", post(handle_outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_outcome_maps_known_words_to_signed_delta() {
        assert_eq!(classify_outcome("success"), CONFIDENCE_DELTA_PER_CALL);
        assert_eq!(classify_outcome("ok"), CONFIDENCE_DELTA_PER_CALL);
        assert_eq!(classify_outcome("PASSED"), CONFIDENCE_DELTA_PER_CALL);
        assert_eq!(classify_outcome("failure"), -CONFIDENCE_DELTA_PER_CALL);
        assert_eq!(classify_outcome("failed"), -CONFIDENCE_DELTA_PER_CALL);
        assert_eq!(classify_outcome("Fail"), -CONFIDENCE_DELTA_PER_CALL);
    }

    #[test]
    fn classify_outcome_unknown_yields_zero_delta() {
        // Unknown outcome strings still update last_accessed (handler
        // does that unconditionally) but don't shift confidence.
        assert_eq!(classify_outcome("partial_success"), 0.0);
        assert_eq!(classify_outcome("idk"), 0.0);
        assert_eq!(classify_outcome(""), 0.0);
    }

    #[test]
    fn truncate_caps_at_char_boundary_not_byte() {
        // 1000 'é' chars = 2000 bytes. cap to 480 chars = 960 bytes.
        let long: String = std::iter::repeat('é').take(1000).collect();
        let capped = truncate_to_chars(&long, 480);
        assert_eq!(capped.chars().count(), 480);
        // No partial-codepoint truncation — capped is valid UTF-8.
        assert!(std::str::from_utf8(capped.as_bytes()).is_ok());
    }

    #[test]
    fn confidence_signal_bounded_by_min_max() {
        // Verify the algebra: 20 success deltas should saturate at +1.0.
        let mut signal = 0.0;
        for _ in 0..25 {
            signal = (signal + CONFIDENCE_DELTA_PER_CALL)
                .clamp(CONFIDENCE_SIGNAL_MIN, CONFIDENCE_SIGNAL_MAX);
        }
        assert_eq!(signal, CONFIDENCE_SIGNAL_MAX);

        // 25 failures from saturation should land at +1.0 - 25*0.05
        // clipped to -1.0 (since 1.0 - 1.25 = -0.25 doesn't hit floor;
        // 30 hits = -0.50, 40 hits = -1.0 exact, 41 hits clipped).
        for _ in 0..40 {
            signal = (signal - CONFIDENCE_DELTA_PER_CALL)
                .clamp(CONFIDENCE_SIGNAL_MIN, CONFIDENCE_SIGNAL_MAX);
        }
        assert_eq!(signal, CONFIDENCE_SIGNAL_MIN);
    }
}

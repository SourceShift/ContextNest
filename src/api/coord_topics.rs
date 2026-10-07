//! Concord P3 — topic-overlap advisory surface.
//!
//! Two read-only GET endpoints plus a small set of pure helpers, kept
//! out of `coord_footprints.rs` (1243 lines) and `coord_turn.rs` so
//! the per-turn hook stays a thin shim. Notice emission lives in the
//! `coord_turn` handler; the calibration endpoints here serve both
//! states (notices on OR off), so an operator can tune the threshold
//! while the user-visible feature is suppressed.
//!
//! ## Env knobs
//!
//! | var | default | effect |
//! |---|---|---|
//! | `CONTEXTNEST_CONCORD_TOPIC` | unset (false) | Master switch for advisory line emission |
//! | `CONTEXTNEST_CONCORD_TOPIC_THRESHOLD` | 0.85 | Minimum cosine to consider a peer worth noticing |
//! | `CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS` | 3600 | Live-intent lookback for `best_topic_match` |
//! | `CONTEXTNEST_CONCORD_TOPIC_DEDUP_SECS` | 3600 | Same-pair notice cooldown |
//!
//! All four are read fresh on every call (mirrors `parse_digest_enabled`).

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::error;

use crate::services::ContextNestServices;

// ───────────────────────── env helpers ─────────────────────────

/// Pure parser for the master switch. Returns `true` only when the
/// trimmed, lowercased value is `"1"`, `"true"`, or `"on"`. Anything
/// else (including unset) is `false` — the calibration endpoints stay
/// on, but no notice is emitted.
pub fn parse_topic_enabled(raw: Option<&str>) -> bool {
    match raw {
        Some(v) => {
            let t = v.trim().to_ascii_lowercase();
            matches!(t.as_str(), "1" | "true" | "on")
        }
        None => false,
    }
}

/// Read `CONTEXTNEST_CONCORD_TOPIC` fresh on every call.
pub fn topic_enabled() -> bool {
    parse_topic_enabled(std::env::var("CONTEXTNEST_CONCORD_TOPIC").ok().as_deref())
}

/// Pure parser behind [`topic_threshold`]. `f32` parse + finite check
/// — anything else falls back to the default.
pub fn parse_topic_threshold(raw: Option<&str>) -> f32 {
    match raw.and_then(|s| s.trim().parse::<f32>().ok()) {
        Some(v) if v.is_finite() => v,
        _ => DEFAULT_TOPIC_THRESHOLD,
    }
}

/// Default cosine above which a peer is "near-identical enough" to
/// notice. 0.85 is conservative — local embedder outputs in this
/// codebase cluster tighter than hosted models, so 0.85 is the lower
/// bound for two semantically-different prompts to register.
const DEFAULT_TOPIC_THRESHOLD: f32 = 0.85;

/// Read `CONTEXTNEST_CONCORD_TOPIC_THRESHOLD` fresh on every call.
/// Unparseable or non-finite values → default.
pub fn topic_threshold() -> f32 {
    parse_topic_threshold(
        std::env::var("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD")
            .ok()
            .as_deref(),
    )
}

/// Pure parser behind [`topic_window_secs`]. `i64` parse + positive
/// check — anything else falls back to the default.
pub fn parse_topic_window_secs(raw: Option<&str>) -> i64 {
    match raw.and_then(|s| s.trim().parse::<i64>().ok()) {
        Some(v) if v > 0 => v,
        _ => DEFAULT_TOPIC_WINDOW_SECS,
    }
}

/// Default lookback window (seconds) for live intents. One hour is
/// enough to keep two same-project loops in each other's peer set
/// during a multi-turn session without letting yesterday's intent
/// leak through.
const DEFAULT_TOPIC_WINDOW_SECS: i64 = 3600;

/// Read `CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS` fresh on every call.
pub fn topic_window_secs() -> i64 {
    parse_topic_window_secs(
        std::env::var("CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS")
            .ok()
            .as_deref(),
    )
}

/// Pure parser behind [`topic_dedup_secs`]. Same shape as
/// `parse_topic_window_secs`.
pub fn parse_topic_dedup_secs(raw: Option<&str>) -> i64 {
    match raw.and_then(|s| s.trim().parse::<i64>().ok()) {
        Some(v) if v > 0 => v,
        _ => DEFAULT_TOPIC_DEDUP_SECS,
    }
}

/// Default dedup window (seconds). One hour is the same default as
/// the live-intent lookback so a fresh intent's peer is at most one
/// notice-window old.
const DEFAULT_TOPIC_DEDUP_SECS: i64 = 3600;

/// Read `CONTEXTNEST_CONCORD_TOPIC_DEDUP_SECS` fresh on every call.
pub fn topic_dedup_secs() -> i64 {
    parse_topic_dedup_secs(
        std::env::var("CONTEXTNEST_CONCORD_TOPIC_DEDUP_SECS")
            .ok()
            .as_deref(),
    )
}

// ───────────────────────── capture helper ─────────────────────────

/// Pick the captured prompt string. Returns `None` when there are
/// fewer than 20 non-whitespace characters — short prompts aren't
/// worth embedding, and would otherwise pin a noisy 0-vector into the
/// store. Otherwise returns the first 500 characters by Unicode
/// scalar value (no byte slicing, so a 4-byte emoji at position 498
/// doesn't panic).
pub fn capture_text(prompt: Option<&str>) -> Option<String> {
    let s = prompt?;
    let non_ws = s.chars().filter(|c| !c.is_whitespace()).count();
    if non_ws < 20 {
        return None;
    }
    Some(s.chars().take(500).collect())
}

/// Render the `[concord]` advisory line for one match. Exact wire
/// format the brief mandates:
///
/// `[concord] ↔ {other} may be working on the same thing: "{first 120
/// chars of other_text}" (similarity {:.2}). If so, coordinate: mini-ork
/// concord send {other} "..."`
pub fn render_topic_notice(other: &str, other_text: &str, similarity: f32) -> String {
    let quoted: String = other_text.chars().take(120).collect();
    format!(
        "[concord] ↔ {other} may be working on the same thing: \"{quoted}\" \
         (similarity {:.2}). If so, coordinate: mini-ork concord send {other} \"...\"",
        similarity
    )
}

// ───────────────────────── handlers ─────────────────────────

/// `GET /api/v1/coord/intents` — list every live intent for the
/// calibration UI. Never includes the embedding blob; the operator
/// needs the text + timestamp, not the vector. `samples` (Concord
/// P3b) is the count of blended prompts feeding the stored vector
/// (1 on a replace-only row, >= 2 after a blend).
async fn list_intents_handler(
    State(services): State<ContextNestServices>,
) -> (StatusCode, Json<Value>) {
    let now = Utc::now();
    let intents = match services
        .coord_store
        .list_live_intents(topic_window_secs(), now)
    {
        Ok(v) => v,
        Err(e) => {
            error!(error = %e, "coord_topics: list_intents failed");
            Vec::new()
        }
    };
    let arr: Vec<Value> = intents
        .into_iter()
        .map(|i| {
            json!({
                "principal_id": i.principal_id,
                "text": i.text,
                "samples": i.samples,
                "updated_at": i.updated_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            })
        })
        .collect();
    (StatusCode::OK, Json(json!({ "intents": arr })))
}

#[derive(Debug, Default, Deserialize)]
struct TopicPairsQuery {
    #[serde(default)]
    min: Option<String>,
}

/// `GET /api/v1/coord/topic-pairs?min=` — every i<j same-dim pair of
/// live intents with `similarity >= min`, sorted descending, capped at
/// 50. The optional `min` is parsed leniently (anything unparseable
/// falls back to 0.5) so a typo on the dashboard URL doesn't 4xx.
async fn topic_pairs_handler(
    State(services): State<ContextNestServices>,
    Query(q): Query<TopicPairsQuery>,
) -> (StatusCode, Json<Value>) {
    let now = Utc::now();
    let min = q
        .min
        .as_deref()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(0.5);
    let sim_fn = |a: &[f32], b: &[f32]| services.embedding.calculate_similarity(a, b);
    let pairs = match services
        .coord_store
        .topic_pairs(topic_window_secs(), now, min, 50, sim_fn)
    {
        Ok(v) => v,
        Err(e) => {
            error!(error = %e, "coord_topics: topic_pairs failed");
            Vec::new()
        }
    };
    let arr: Vec<Value> = pairs
        .into_iter()
        .map(|p| {
            json!({
                "a": p.a,
                "b": p.b,
                "similarity": p.similarity,
                "a_text": p.a_text,
                "b_text": p.b_text,
            })
        })
        .collect();
    (StatusCode::OK, Json(json!({ "pairs": arr })))
}

/// Mount point for both endpoints. Returns `Router<ContextNestServices>`
/// so `simple.rs` can `.merge(...)` it next to the other coord routers.
pub fn create_coord_topics_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/api/v1/coord/intents", get(list_intents_handler))
        .route("/api/v1/coord/topic-pairs", get(topic_pairs_handler))
}

// ───────────────────────── tests ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_topic_enabled_is_strict() {
        assert!(parse_topic_enabled(Some("1")));
        assert!(parse_topic_enabled(Some("true")));
        assert!(parse_topic_enabled(Some("TRUE")));
        assert!(parse_topic_enabled(Some(" on ")));
        assert!(!parse_topic_enabled(None));
        assert!(!parse_topic_enabled(Some("")));
        assert!(!parse_topic_enabled(Some("0")));
        assert!(!parse_topic_enabled(Some("false")));
        assert!(!parse_topic_enabled(Some("off")));
        assert!(!parse_topic_enabled(Some("yes")));
    }

    #[test]
    fn parse_topic_threshold_parses_and_falls_back() {
        assert_eq!(parse_topic_threshold(None), DEFAULT_TOPIC_THRESHOLD);
        assert_eq!(parse_topic_threshold(Some("")), DEFAULT_TOPIC_THRESHOLD);
        assert_eq!(parse_topic_threshold(Some("0.9")), 0.9);
        assert_eq!(parse_topic_threshold(Some(" 0.95 ")), 0.95);
        // Non-finite (NaN) → default.
        assert_eq!(parse_topic_threshold(Some("nan")), DEFAULT_TOPIC_THRESHOLD);
        // Unparseable → default.
        assert_eq!(
            parse_topic_threshold(Some("not-a-number")),
            DEFAULT_TOPIC_THRESHOLD
        );
    }

    #[test]
    fn parse_topic_window_secs_parses_and_falls_back() {
        assert_eq!(parse_topic_window_secs(None), DEFAULT_TOPIC_WINDOW_SECS);
        assert_eq!(parse_topic_window_secs(Some("")), DEFAULT_TOPIC_WINDOW_SECS);
        assert_eq!(parse_topic_window_secs(Some("60")), 60);
        assert_eq!(parse_topic_window_secs(Some(" 120 ")), 120);
        // Non-positive → default.
        assert_eq!(
            parse_topic_window_secs(Some("0")),
            DEFAULT_TOPIC_WINDOW_SECS
        );
        assert_eq!(
            parse_topic_window_secs(Some("-5")),
            DEFAULT_TOPIC_WINDOW_SECS
        );
        assert_eq!(
            parse_topic_window_secs(Some("not-a-number")),
            DEFAULT_TOPIC_WINDOW_SECS
        );
    }

    #[test]
    fn parse_topic_dedup_secs_parses_and_falls_back() {
        assert_eq!(parse_topic_dedup_secs(None), DEFAULT_TOPIC_DEDUP_SECS);
        assert_eq!(parse_topic_dedup_secs(Some("")), DEFAULT_TOPIC_DEDUP_SECS);
        assert_eq!(parse_topic_dedup_secs(Some("30")), 30);
        assert_eq!(parse_topic_dedup_secs(Some("0")), DEFAULT_TOPIC_DEDUP_SECS);
    }

    #[test]
    fn capture_text_rejects_short_and_truncates_long() {
        // < 20 non-whitespace chars → None.
        assert!(capture_text(None).is_none());
        assert!(capture_text(Some("")).is_none());
        assert!(capture_text(Some("         a            ")).is_none()); // 1 non-ws
        let nineteen = "a".repeat(19);
        assert!(capture_text(Some(&nineteen)).is_none());
        // Exactly 20 → Some.
        let twenty = "a".repeat(20);
        let got = capture_text(Some(&twenty)).expect("captured");
        assert_eq!(got.chars().count(), 20);
        // Long input → truncated to 500 chars.
        let long: String = "a".repeat(800);
        let got = capture_text(Some(&long)).expect("captured");
        assert_eq!(got.chars().count(), 500);
        // Multi-byte passthrough: emoji at position 498 should not panic.
        let mut s: String = "a".repeat(498);
        s.push('🦀'); // 4 bytes
        s.push('b');
        let got = capture_text(Some(&s)).expect("captured");
        assert_eq!(got.chars().count(), 500);
        assert!(got.contains('🦀'));
    }

    #[test]
    fn render_topic_notice_format_is_byte_exact() {
        let s = render_topic_notice("loop:b", "hello world", 0.97);
        assert!(
            s.starts_with("[concord] ↔ loop:b may be working on the same thing: \"hello world\"")
        );
        assert!(s.contains("(similarity 0.97)"));
        assert!(s.contains("mini-ork concord send loop:b \"...\""));
    }

    #[test]
    fn render_topic_notice_truncates_long_other_text() {
        let long: String = "x".repeat(500);
        let s = render_topic_notice("loop:b", &long, 0.5);
        // Exactly the first 120 chars are quoted, closed right before the similarity.
        assert!(
            s.contains(&format!("\"{}\" (similarity", "x".repeat(120))),
            "first 120 chars quoted; got {s:?}"
        );
        assert!(!s.contains(&"x".repeat(121)), "truncated at 120; got {s:?}");
    }
}

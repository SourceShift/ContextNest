//! `GET /api/v1/stats` — substrate-wide aggregate counts for the
//! dashboard's Substrate page.
//!
//! Returns total active-fragment count plus a `by_kind` breakdown so
//! the dashboard can render the kind-distribution bar without making
//! eight separate `/api/v1/tools/retrieve` calls (or worse, walking
//! `/api/v1/inbox` and inferring).
//!
//! ## Why a dedicated endpoint
//!
//! Inbox returns only inbox-eligible kinds. Sessions returns per-session
//! fragment counts but not kind breakdowns. Retrieve takes a query
//! string and runs similarity scoring. None of these answer "how many
//! `learning` records does the substrate hold right now?" cheaply.
//!
//! The implementation is a **single read-lock scan** over
//! `fragment_metadata`, intersected with `session_index.active` to
//! exclude soft-deleted entries. No similarity, no embedding, no
//! per-fragment lock churn.

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use serde::Serialize;
use std::collections::HashMap;

use crate::services::ContextNestServices;

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    /// Total number of currently-active fragments across all sessions
    /// (i.e. the size of `session_index.active`'s union, equivalent to
    /// `fragment_texts.len()` modulo soft-delete drift).
    pub total_fragments: usize,
    /// Total number of distinct sessions known to the substrate
    /// (active fragments only — soft-deleted-only sessions are skipped).
    pub total_sessions: usize,
    /// Counts grouped by `metadata.kind`. Fragments without a kind are
    /// bucketed under `"unknown"` so the histogram still sums to
    /// `total_fragments`. Always-present keys make it safe for the
    /// frontend to render a stable color palette without falling back
    /// to "missing" rendering.
    pub by_kind: HashMap<String, usize>,
}

pub async fn get_stats(
    State(services): State<ContextNestServices>,
) -> Result<Json<StatsResponse>, StatusCode> {
    let active = services.session_index.active_fragments_session_map().await;
    let metadata = services.fragment_metadata.read().await;

    let mut by_kind: HashMap<String, usize> = HashMap::new();
    let mut total_fragments = 0usize;
    for (frag_id, _session_id) in &active {
        total_fragments += 1;
        let kind = metadata
            .get(frag_id)
            .and_then(|m| m.get("kind"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        *by_kind.entry(kind).or_insert(0) += 1;
    }
    drop(metadata);

    // Distinct sessions = unique values in the active map.
    let total_sessions = active
        .values()
        .collect::<std::collections::HashSet<_>>()
        .len();

    Ok(Json(StatsResponse {
        total_fragments,
        total_sessions,
        by_kind,
    }))
}

pub fn create_stats_router() -> Router<ContextNestServices> {
    Router::new().route("/api/v1/stats", get(get_stats))
}

//! `GET /api/v1/substrate/consolidation` — observability for the
//! background consolidation worker (Phase 1 of the neural-field epic,
//! see `docs/roadmap/epics/neural-field-real.md`).
//!
//! Returns a snapshot of the queue + worker progress so operators can
//! answer "is the substrate keeping up?" without reading server logs:
//!
//! ```json
//! {
//!   "total_fragments": 25079,
//!   "consolidated_count": 25079,
//!   "lag": 0,
//!   "queued": 0,
//!   "succeeded_total": 25079,
//!   "failed_total": 0,
//!   "last_lap_ms": 412,
//!   "initial_scan_complete": true
//! }
//! ```
//!
//! `lag` is the practical answer: `total_fragments - consolidated_count`.
//! It includes both fragments still queued AND fragments that the
//! worker hasn't gotten to yet (no scan deficit). The dashboard's
//! substrate page reads this every few seconds to render the
//! consolidation progress band.

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use serde::Serialize;

use crate::services::ContextNestServices;

#[derive(Debug, Serialize)]
pub struct ConsolidationStatus {
    /// Total fragments the substrate knows about (sidecars + canonical).
    pub total_fragments: usize,
    /// Fragments whose metadata carries the `_cn_consolidated` flag.
    /// Equal to `total_fragments` once the worker has fully caught up.
    pub consolidated_count: usize,
    /// `total_fragments - consolidated_count`. The single number ops
    /// look at to answer "how far behind is the worker?"
    pub lag: usize,
    /// Live queue depth (post-dedup). May be smaller than `lag`
    /// briefly when the worker has just drained a batch but hasn't
    /// flipped the metadata flags yet.
    pub queued: usize,
    /// Cumulative successful consolidations since the server started.
    /// Monotonic; reset only by process restart.
    pub succeeded_total: usize,
    /// Cumulative failures since startup. Each one is a warn-level log
    /// line with the fragment id + error.
    pub failed_total: usize,
    /// Wall-clock duration of the most recent non-empty batch (ms).
    /// Useful for spotting embedder slowdowns.
    pub last_lap_ms: u64,
    /// Set to true after the worker has scanned all
    /// `fragment_metadata` entries at startup and enqueued anything
    /// missing the consolidation flag. Tests poll on this before
    /// asserting "everything that existed is queued."
    pub initial_scan_complete: bool,
}

pub async fn get_consolidation_status(
    State(services): State<ContextNestServices>,
) -> Result<Json<ConsolidationStatus>, StatusCode> {
    let metrics = services.consolidation_queue.snapshot_metrics();

    // Walk the metadata sidecar once for total + consolidated counts.
    // Two passes in one loop = same lock acquisition cost as the
    // single-stat answer.
    let texts = services.fragment_texts.read().await;
    let metadata = services.fragment_metadata.read().await;
    // Total fragments = union of ids known to either sidecar. Counting
    // texts alone misses metadata-only ghosts; counting metadata alone
    // misses sidecar inserts that didn't carry metadata.
    let mut ids: std::collections::HashSet<&String> = texts.keys().collect();
    ids.extend(metadata.keys());
    let total_fragments = ids.len();

    let mut consolidated_count = 0usize;
    for id in &ids {
        if let Some(meta) = metadata.get(*id) {
            if meta
                .get("_cn_consolidated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                consolidated_count += 1;
            }
        }
    }
    drop(metadata);
    drop(texts);

    let lag = total_fragments.saturating_sub(consolidated_count);

    Ok(Json(ConsolidationStatus {
        total_fragments,
        consolidated_count,
        lag,
        queued: metrics.queued,
        succeeded_total: metrics.consolidated,
        failed_total: metrics.failed,
        last_lap_ms: metrics.last_lap_ms,
        initial_scan_complete: metrics.initial_scan_complete,
    }))
}

pub fn create_substrate_router() -> Router<ContextNestServices> {
    Router::new().route(
        "/api/v1/substrate/consolidation",
        get(get_consolidation_status),
    )
}

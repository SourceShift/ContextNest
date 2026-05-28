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

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

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

/// Phase 7 of the neural-field epic
/// (`docs/roadmap/epics/neural-field-real.md`). Aggregate substrate
/// observability — answers "is everything ContextNest's tagline
/// claims to do actually happening?" in one read.
///
/// Operators look at this to verify (a) the worker is keeping up,
/// (b) basins and edges are forming for the live ingest path,
/// (c) the decay knob is set sanely. Failure mode: if any nested
/// stat is zero on a populated substrate, the corresponding code
/// path is dormant.
#[derive(Debug, Serialize)]
pub struct SubstrateHealth {
    pub fragments: FragmentStats,
    pub basins: BasinStats,
    pub connections: ConnectionStats,
    pub decay: DecayStats,
}

#[derive(Debug, Serialize)]
pub struct FragmentStats {
    pub total: usize,
    pub consolidated: usize,
    pub lag: usize,
}

#[derive(Debug, Serialize)]
pub struct BasinStats {
    /// Count of basins currently in `MemoryAttractorManager.basin_manager`.
    /// Zero on a cold substrate; rises as the worker drains.
    pub count: usize,
    /// Mean members per basin. 0.0 when count is 0.
    pub avg_mass: f32,
    /// Maximum members in any single basin — sanity-check for
    /// pathological clustering where one basin absorbs everything.
    pub max_mass: usize,
}

#[derive(Debug, Serialize)]
pub struct ConnectionStats {
    /// Edge count in the learned-graph. Forms automatically via
    /// similarity-driven auto-connection inside `add_node` calls
    /// the worker makes.
    pub edges: usize,
    /// Mean degree across nodes. 0.0 when the graph is empty.
    pub avg_degree: f32,
    /// Node count — should track fragment count modulo ingest-to-
    /// consolidation lag.
    pub nodes: usize,
}

#[derive(Debug, Serialize)]
pub struct DecayStats {
    /// Currently-active half-life (env-overridable). Defaults to
    /// the same 60-day default the retrieve handler uses.
    pub half_life_days: f64,
    /// Median age in days across all fragments carrying a `ts`
    /// metadata field. Operators set the half-life relative to
    /// this number — half-life ≈ median age means the median
    /// fragment is at ~50% of its peak similarity.
    pub median_fragment_age_days: f64,
}

pub async fn get_substrate_health(
    State(services): State<ContextNestServices>,
) -> Result<Json<SubstrateHealth>, StatusCode> {
    // Fragments — same union-of-sidecars logic as the
    // /consolidation endpoint, plus consolidation flag.
    let metadata = services.fragment_metadata.read().await;
    let texts = services.fragment_texts.read().await;
    let mut ids: std::collections::HashSet<&String> = texts.keys().collect();
    ids.extend(metadata.keys());
    let total = ids.len();
    let mut consolidated = 0usize;
    let mut ages: Vec<f64> = Vec::new();
    let now = chrono::Utc::now();
    for id in &ids {
        if let Some(meta) = metadata.get(*id) {
            if meta
                .get("_cn_consolidated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                consolidated += 1;
            }
            if let Some(ts) = meta.get("ts").and_then(|v| v.as_str()) {
                if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(ts) {
                    let secs = (now - parsed.with_timezone(&chrono::Utc)).num_seconds() as f64;
                    if secs >= 0.0 {
                        ages.push(secs / 86_400.0);
                    }
                }
            }
        }
    }
    drop(texts);
    drop(metadata);

    let lag = total.saturating_sub(consolidated);

    // Basins.
    let snapshots = services.attractor_manager.list_basin_snapshots().await;
    let basin_count = snapshots.len();
    let (basin_total_members, basin_max_mass) = snapshots
        .iter()
        .map(|s| s.fragment_ids.len())
        .fold((0usize, 0usize), |(sum, max), n| (sum + n, max.max(n)));
    let basin_avg_mass = if basin_count > 0 {
        basin_total_members as f32 / basin_count as f32
    } else {
        0.0
    };

    // Connections — pull GraphMetrics directly. Avoids walking
    // every edge again; the manager keeps these counters up to
    // date inside add_node / create_connection.
    let metrics = services
        .attractor_manager
        .connection_network_graph_metrics()
        .await;

    // Decay knob — same parse path the retrieve handler uses; if
    // someone exports a different value it shows up here so the
    // dashboard's substrate page never disagrees with the actual
    // applied multiplier.
    let half_life_days: f64 = std::env::var("CONTEXTNEST_DECAY_HALF_LIFE_DAYS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|v: &f64| v.is_finite() && *v > 0.0)
        .unwrap_or(60.0);

    // Median age — sort ages once, take middle. Ignores fragments
    // without a parseable `ts`; not all fragments have one (e.g.
    // legacy stored content), so this is a "best estimate from
    // available data" rather than an authoritative median.
    let median_age = if ages.is_empty() {
        0.0
    } else {
        ages.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = ages.len() / 2;
        if ages.len() % 2 == 0 {
            (ages[mid - 1] + ages[mid]) / 2.0
        } else {
            ages[mid]
        }
    };

    Ok(Json(SubstrateHealth {
        fragments: FragmentStats {
            total,
            consolidated,
            lag,
        },
        basins: BasinStats {
            count: basin_count,
            avg_mass: basin_avg_mass,
            max_mass: basin_max_mass,
        },
        connections: ConnectionStats {
            edges: metrics.total_edges,
            avg_degree: metrics.avg_degree,
            nodes: metrics.total_nodes,
        },
        decay: DecayStats {
            half_life_days,
            median_fragment_age_days: median_age,
        },
    }))
}

/// Admin endpoint that collapses near-duplicate basins via
/// [`crate::memory::attractors::memory_attractor_manager::MemoryAttractorManager::merge_nearby_basins`].
/// Designed to clean up degenerate substrates produced by the pre-fix
/// `process_memories` Step 1 (every fragment seeded its own basin).
///
/// `distance_threshold` is passed straight to `should_merge_with`:
/// `distance < merged_radius * threshold` is the merge predicate. The
/// default (1.0) means "merge basins whose centers are closer than their
/// combined radius" — a conservative pass that catches obvious singletons
/// without collapsing semantically-distinct clusters. Operators tuning
/// against the substrate should run with a small threshold first and
/// inspect `basins_merged` before raising it.
///
/// O(N²) runtime — at 100K basins expect several minutes of wall clock.
#[derive(Debug, Deserialize)]
pub struct MergeBasinsQuery {
    #[serde(default = "default_merge_threshold")]
    pub distance_threshold: f32,
}

fn default_merge_threshold() -> f32 {
    1.0
}

#[derive(Debug, Serialize)]
pub struct MergeBasinsResponse {
    pub basins_merged: usize,
    pub basins_remaining: usize,
    pub elapsed_ms: u64,
    pub distance_threshold: f32,
}

pub async fn admin_merge_nearby_basins(
    State(services): State<ContextNestServices>,
    Query(query): Query<MergeBasinsQuery>,
) -> Result<Json<MergeBasinsResponse>, StatusCode> {
    let start = std::time::Instant::now();
    let basins_merged = services
        .attractor_manager
        .merge_nearby_basins(query.distance_threshold)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let basins_remaining = services
        .attractor_manager
        .list_basin_snapshots()
        .await
        .len();
    Ok(Json(MergeBasinsResponse {
        basins_merged,
        basins_remaining,
        elapsed_ms: start.elapsed().as_millis() as u64,
        distance_threshold: query.distance_threshold,
    }))
}

pub fn create_substrate_router() -> Router<ContextNestServices> {
    Router::new()
        .route(
            "/api/v1/substrate/consolidation",
            get(get_consolidation_status),
        )
        .route("/api/v1/substrate/health", get(get_substrate_health))
        .route(
            "/api/v1/admin/merge-nearby-basins",
            post(admin_merge_nearby_basins),
        )
}

//! Graph-view backend — two read-only endpoints over the canonical
//! connection network:
//!
//! - `GET /api/v1/graph/neighbors` — 1-hop neighbours of a node, sorted
//!   by edge weight desc. Forwards to
//!   [`MemoryAttractorManager::list_neighbors`].
//! - `GET /api/v1/graph/path` — shortest path between two nodes, via
//!   Dijkstra (A* currently delegates to it). Forwards to
//!   [`MemoryAttractorManager::find_graph_path`].
//!
//! ## Three-outcome path semantics
//!
//! `find_graph_path` distinguishes three cases that must NOT be
//! collapsed into each other:
//!
//! | outcome | meaning | response |
//! |---|---|---|
//! | `Err(ContextNestError::NotFound)` | an endpoint is absent from the graph | `404` |
//! | `Ok(None)` | both endpoints exist, no route connects them | `200 {found:false}` |
//! | `Err(other)` | internal failure | `500` |
//!
//! Returning 404 for the `Ok(None)` case would lie to the caller about
//! node existence — "no such node" and "no path between these nodes"
//! are different questions with different operator remedies.
//!
//! ## Empty graph is a valid state
//!
//! A cold substrate has no nodes and no edges. Neighbours of any id is
//! then an empty list, not an error: `neighbors_of` has no error channel
//! by design, so `GET /api/v1/graph/neighbors` never 404s.
//!
//! [`MemoryAttractorManager::list_neighbors`]: crate::memory::attractors::memory_attractor_manager::MemoryAttractorManager::list_neighbors
//! [`MemoryAttractorManager::find_graph_path`]: crate::memory::attractors::memory_attractor_manager::MemoryAttractorManager::find_graph_path

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};

use crate::error::ContextNestError;
use crate::services::ContextNestServices;

// =============================================================================
// /api/v1/graph/neighbors
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct GraphNeighborsQuery {
    /// Node to expand. Unknown ids are not an error — the graph simply
    /// has no incident edges for them.
    pub node_id: String,
    #[serde(default = "default_graph_limit")]
    pub limit: usize,
}

fn default_graph_limit() -> usize {
    10
}

#[derive(Debug, Serialize)]
pub struct NeighborRow {
    pub id: String,
    pub weight: f32,
}

#[derive(Debug, Serialize)]
pub struct GraphNeighborsResponse {
    pub node_id: String,
    pub neighbors: Vec<NeighborRow>,
    /// Neighbour count before `limit` truncation.
    pub total: usize,
}

pub async fn list_graph_neighbors(
    State(services): State<ContextNestServices>,
    Query(q): Query<GraphNeighborsQuery>,
) -> Result<Json<GraphNeighborsResponse>, StatusCode> {
    if q.limit == 0 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let limit = q.limit.min(200);

    let mut neighbors = services.attractor_manager.list_neighbors(&q.node_id).await;
    let total = neighbors.len();
    neighbors.truncate(limit);

    Ok(Json(GraphNeighborsResponse {
        node_id: q.node_id,
        neighbors: neighbors
            .into_iter()
            .map(|(id, weight)| NeighborRow { id, weight })
            .collect(),
        total,
    }))
}

// =============================================================================
// /api/v1/graph/path
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct GraphPathQuery {
    /// Path start node.
    pub from: String,
    /// Path end node.
    pub to: String,
}

#[derive(Debug, Serialize)]
pub struct GraphPathResponse {
    /// False when both endpoints exist but no route connects them.
    pub found: bool,
    pub nodes: Vec<String>,
    /// Edge traversals — `nodes.len() - 1` on a found path.
    pub hops: usize,
    pub total_weight: f32,
    pub confidence: f32,
    pub algorithm: String,
}

pub async fn graph_path(
    State(services): State<ContextNestServices>,
    Query(q): Query<GraphPathQuery>,
) -> Result<Json<GraphPathResponse>, StatusCode> {
    match services
        .attractor_manager
        .find_graph_path(&q.from, &q.to)
        .await
    {
        Ok(Some(path)) => {
            // `Path.length` counts nodes with both endpoints inclusive;
            // a hop is an edge traversal, so report one less.
            let hops = path.nodes.len().saturating_sub(1);
            Ok(Json(GraphPathResponse {
                found: true,
                nodes: path.nodes,
                hops,
                total_weight: path.total_weight,
                confidence: path.confidence,
                algorithm: path.algorithm,
            }))
        }
        Ok(None) => Ok(Json(GraphPathResponse {
            found: false,
            nodes: Vec::new(),
            hops: 0,
            total_weight: 0.0,
            confidence: 0.0,
            algorithm: String::new(),
        })),
        Err(ContextNestError::NotFound(_)) => Err(StatusCode::NOT_FOUND),
        Err(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

// =============================================================================
// Router
// =============================================================================

pub fn create_graph_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/api/v1/graph/neighbors", get(list_graph_neighbors))
        .route("/api/v1/graph/path", get(graph_path))
}

//! Graph-view backend.
//!
//! Two read-only endpoints over the canonical connection network:
//!
//! - `GET /api/v1/graph/neighbors` — 1-hop neighbours of a node, sorted
//!   by edge weight desc. Forwards to
//!   [`MemoryAttractorManager::list_neighbors`].
//! - `GET /api/v1/graph/path` — shortest path between two nodes, via
//!   Dijkstra (A* currently delegates to it). Forwards to
//!   [`MemoryAttractorManager::find_graph_path`].
//!
//! Plus four over the optional Neo4j projection
//! ([`crate::services::graph_backend`]):
//!
//! - `POST /api/v1/graph/upsert` — best-effort batch projection of
//!   learning entities. Answers `200 {skipped:true}` rather than failing
//!   when no backend is attached.
//! - `GET /api/v1/graph/credit-chain`, `GET /api/v1/graph/siblings`,
//!   `GET /api/v1/graph/traverse` — reads against that projection;
//!   `404` when the capability is not present in this build, `503` when a
//!   live database failed the query.
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
    routing::{get, post},
    Extension, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::error::ContextNestError;
use crate::services::graph_backend::{
    is_known_edge_type, is_known_node_label, GraphEdge, GraphNode,
};
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
// /api/v1/graph/upsert
// =============================================================================
//
// The projection the caller is asking for is **best-effort**: the graph is
// a view over the canonical substrate, not a system of record, so a write
// that cannot be projected is reported as `skipped` (200) rather than as a
// failure. The caller never has to care whether a database is attached.

/// Largest batch accepted in one request — bounds both the transaction and
/// the request body, so one caller cannot pin a connection indefinitely.
const MAX_UPSERT_ITEMS: usize = 2000;

/// Concurrent upserts allowed in flight. Serialising writes keeps a burst
/// from exhausting the driver's pool; it is a throughput knob, not a
/// correctness one — the backend's single-transaction batch is what makes
/// an upsert atomic.
const UPSERT_CONCURRENCY: usize = 4;

/// `traverse`'s hop default and hard ceiling. The clamped value is
/// interpolated into a variable-length Cypher pattern, so an unbounded one
/// would let a single request walk the entire graph.
const DEFAULT_MAX_HOPS: u8 = 3;
const MIN_MAX_HOPS: u8 = 1;
const MAX_MAX_HOPS: u8 = 8;

fn default_max_hops() -> u8 {
    DEFAULT_MAX_HOPS
}

#[derive(Debug, Deserialize)]
pub struct GraphNodeInput {
    pub id: String,
    /// Must be one of [`crate::services::graph_backend::NODE_LABELS`].
    pub label: String,
    #[serde(default)]
    pub props: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct GraphEdgeInput {
    pub from: String,
    pub from_label: String,
    #[serde(rename = "type")]
    pub edge_type: String,
    pub to: String,
    pub to_label: String,
    #[serde(default)]
    pub props: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct GraphUpsertRequest {
    #[serde(default)]
    pub nodes: Vec<GraphNodeInput>,
    #[serde(default)]
    pub edges: Vec<GraphEdgeInput>,
    /// Which producer sent this batch. Not part of the projection — it
    /// exists to make the log line attributable.
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GraphUpsertResponse {
    /// Always the request's node + edge count, whatever the backend did
    /// with them.
    pub accepted: usize,
    pub nodes_upserted: usize,
    pub edges_upserted: usize,
    /// True when nothing was projected: no backend is attached, or the
    /// write failed. The caller must not treat this as an error to retry.
    pub skipped: bool,
}

impl GraphUpsertResponse {
    fn skipped(accepted: usize) -> Self {
        Self {
            accepted,
            nodes_upserted: 0,
            edges_upserted: 0,
            skipped: true,
        }
    }
}

pub async fn upsert_graph(
    State(services): State<ContextNestServices>,
    Extension(semaphore): Extension<Arc<Semaphore>>,
    Json(req): Json<GraphUpsertRequest>,
) -> Result<Json<GraphUpsertResponse>, StatusCode> {
    // Validation comes before the availability check on purpose: a batch
    // naming an unknown label is the caller's mistake whether or not a
    // database is attached, and silently reporting it as `skipped` would
    // hide a bug in the producer until the day the feature gets enabled.
    for node in &req.nodes {
        if !is_known_node_label(&node.label) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    for edge in &req.edges {
        if !is_known_edge_type(&edge.edge_type, &edge.from_label, &edge.to_label) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    let accepted = req.nodes.len() + req.edges.len();
    if accepted > MAX_UPSERT_ITEMS {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    if let Some(source) = req.source.as_deref() {
        tracing::debug!(source, accepted, "graph upsert received");
    }

    if !services.graph.is_available() {
        return Ok(Json(GraphUpsertResponse::skipped(accepted)));
    }

    let nodes: Vec<GraphNode> = req
        .nodes
        .into_iter()
        .map(|n| GraphNode {
            id: n.id,
            label: n.label,
            props: n.props,
        })
        .collect();
    let edges: Vec<GraphEdge> = req
        .edges
        .into_iter()
        .map(|e| GraphEdge {
            from: e.from,
            from_label: e.from_label,
            edge_type: e.edge_type,
            to: e.to,
            to_label: e.to_label,
            props: e.props,
        })
        .collect();

    let _permit = semaphore
        .acquire()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match services.graph.upsert(&nodes, &edges).await {
        Ok(counts) => Ok(Json(GraphUpsertResponse {
            accepted,
            nodes_upserted: counts.nodes,
            edges_upserted: counts.edges,
            skipped: false,
        })),
        Err(e) => {
            tracing::warn!(
                error = %e,
                accepted,
                "graph upsert failed; nothing projected"
            );
            Ok(Json(GraphUpsertResponse::skipped(accepted)))
        }
    }
}

// =============================================================================
// /api/v1/graph/credit-chain
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct CreditChainQuery {
    pub trace_id: String,
}

#[derive(Debug, Serialize)]
pub struct CreditChainResponse {
    pub trace_id: String,
    pub nodes: Vec<serde_json::Value>,
    pub total: usize,
}

pub async fn graph_credit_chain(
    State(services): State<ContextNestServices>,
    Query(q): Query<CreditChainQuery>,
) -> Result<Json<CreditChainResponse>, StatusCode> {
    if !services.graph.is_available() {
        return Err(StatusCode::NOT_FOUND);
    }

    match services.graph.credit_chain(&q.trace_id).await {
        Ok(nodes) => {
            let total = nodes.len();
            Ok(Json(CreditChainResponse {
                trace_id: q.trace_id,
                nodes,
                total,
            }))
        }
        Err(ContextNestError::NotFound(_)) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(error = %e, "graph credit-chain query failed");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

// =============================================================================
// /api/v1/graph/siblings
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct SiblingsQuery {
    pub target: String,
}

#[derive(Debug, Serialize)]
pub struct SiblingsResponse {
    pub target: String,
    /// The `GradientTarget → TaskClass` fan-out.
    pub classes: Vec<serde_json::Value>,
    pub total: usize,
}

pub async fn graph_siblings(
    State(services): State<ContextNestServices>,
    Query(q): Query<SiblingsQuery>,
) -> Result<Json<SiblingsResponse>, StatusCode> {
    if !services.graph.is_available() {
        return Err(StatusCode::NOT_FOUND);
    }

    match services.graph.siblings(&q.target).await {
        Ok(classes) => {
            let total = classes.len();
            Ok(Json(SiblingsResponse {
                target: q.target,
                classes,
                total,
            }))
        }
        Err(ContextNestError::NotFound(_)) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(error = %e, "graph siblings query failed");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

// =============================================================================
// /api/v1/graph/traverse
// =============================================================================
//
// Same three-outcome split as [`graph_path`]: an absent endpoint is a
// `404`, both endpoints existing with no route between them is a
// `200 {found:false}`, and a query that failed against a live database is
// a `503` the caller may retry.

#[derive(Debug, Deserialize)]
pub struct TraverseQuery {
    pub from: String,
    pub to: String,
    #[serde(default = "default_max_hops")]
    pub max_hops: u8,
}

#[derive(Debug, Serialize)]
pub struct TraverseResponse {
    pub found: bool,
    pub nodes: Vec<String>,
    /// Edge traversals — `nodes.len() - 1` on a found route.
    pub hops: usize,
}

pub async fn graph_traverse(
    State(services): State<ContextNestServices>,
    Query(q): Query<TraverseQuery>,
) -> Result<Json<TraverseResponse>, StatusCode> {
    if !services.graph.is_available() {
        return Err(StatusCode::NOT_FOUND);
    }

    // Clamped here, not in the backend: the ceiling is a request-shape
    // policy, and the backend should be able to trust its input.
    let max_hops = q.max_hops.clamp(MIN_MAX_HOPS, MAX_MAX_HOPS);

    match services.graph.traverse(&q.from, &q.to, max_hops).await {
        Ok(Some(nodes)) => {
            let hops = nodes.len().saturating_sub(1);
            Ok(Json(TraverseResponse {
                found: true,
                nodes,
                hops,
            }))
        }
        Ok(None) => Ok(Json(TraverseResponse {
            found: false,
            nodes: Vec::new(),
            hops: 0,
        })),
        Err(ContextNestError::NotFound(_)) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            tracing::warn!(error = %e, "graph traverse query failed");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

// =============================================================================
// Router
// =============================================================================

pub fn create_graph_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/api/v1/graph/neighbors", get(list_graph_neighbors))
        .route("/api/v1/graph/path", get(graph_path))
        .route("/api/v1/graph/upsert", post(upsert_graph))
        .route("/api/v1/graph/credit-chain", get(graph_credit_chain))
        .route("/api/v1/graph/siblings", get(graph_siblings))
        .route("/api/v1/graph/traverse", get(graph_traverse))
        // The upsert permit pool rides as an Extension rather than router
        // state: `create_simple_app` applies `.with_state(services)`
        // *after* merging this router, so a second state type here would
        // change every handler's `State<S>`.
        .layer(Extension(Arc::new(Semaphore::new(UPSERT_CONCURRENCY))))
}

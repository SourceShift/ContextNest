//! Connection Network for Optimized Memory Retrieval
//! Implements a sophisticated connection network that enables optimized
//! retrieval patterns through intelligent memory association and pathfinding.
//!
//! Storage layout (disk-first substrate epic): node vectors live once in the
//! shared [`VectorArena`] and a node's index *is* its arena row. Edges are
//! 64-byte records in a slab addressed by `u32` slot; each node keeps the
//! slots of its incident edges. The previous layout stored every edge UUID
//! five times and every endpoint id twice more as separate heap strings —
//! ~870 bytes per edge, ~6.7 GB at 7.8 M edges.

use crate::error::ContextNestError;
use crate::error::ContextNestResult;
use crate::memory::attractors::vector_arena::{ArenaView, Holder, VectorArena};
use crate::memory::attractors::{utils, ComponentStatus, MemoryAttractorConfig};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BinaryHeap, HashMap};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use uuid::Uuid;

/// Connection network for optimized memory retrieval
#[derive(Debug)]
pub struct ConnectionNetwork {
    connection_policy: RwLock<(usize, f32)>,
    /// Configuration
    config: MemoryAttractorConfig,
    /// Network graph
    graph: Arc<RwLock<MemoryGraph>>,
    /// Retrieval optimizer
    retrieval_optimizer: Arc<RetrievalOptimizer>,
    /// Path finder
    path_finder: Arc<PathFinder>,
    /// Network statistics
    statistics: Arc<RwLock<NetworkStatistics>>,
    /// Component status
    status: Arc<RwLock<ComponentStatus>>,
}

/// Slot marker for a freed edge record.
const DEAD: u32 = u32::MAX;

/// Node state minus its vector (which lives in the arena row).
#[derive(Debug, Clone)]
struct NodeSlot {
    node_type: MemoryNodeType,
    importance: f32,
    last_accessed: DateTime<Utc>,
    access_frequency: f32,
    metadata: HashMap<String, String>,
    created_at: DateTime<Utc>,
    fragment_ids: Vec<String>,
    /// Slots of edges touching this node.
    incident: Vec<u32>,
}

/// One edge, 64 bytes. `uuid` is the durable edge id; ids that are not
/// UUIDs (legacy/test data) are kept in `MemoryGraph::edge_names`.
#[derive(Debug, Clone, Copy)]
struct EdgeRecord {
    uuid: u128,
    source: u32,
    target: u32,
    weight: f32,
    strength: f32,
    created_at: DateTime<Utc>,
    last_reinforced: DateTime<Utc>,
    usage_count: u32,
    connection_type: ConnectionType,
    bidirectional: bool,
}

impl EdgeRecord {
    fn is_live(&self) -> bool {
        self.source != DEAD
    }

    /// The endpoint opposite `row`, honouring direction: a directed edge is
    /// only walkable from its source.
    fn other(&self, row: u32) -> Option<u32> {
        if self.source == row {
            Some(self.target)
        } else if self.target == row && self.bidirectional {
            Some(self.source)
        } else {
            None
        }
    }
}

/// Memory graph representing connections between memories
#[derive(Debug)]
pub struct MemoryGraph {
    arena: Arc<VectorArena>,
    /// Indexed by arena row; `None` for rows that are not graph nodes.
    nodes: Vec<Option<NodeSlot>>,
    node_count: usize,
    edges: Vec<EdgeRecord>,
    free_edges: Vec<u32>,
    edge_count: usize,
    edge_names: HashMap<u32, String>,
    /// Graph metrics
    metrics: GraphMetrics,
}

/// Memory node in the graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNode {
    /// Node ID
    pub id: String,
    /// Node type
    pub node_type: MemoryNodeType,
    /// Memory content vector
    pub content: Vec<f32>,
    /// Node importance
    pub importance: f32,
    /// Last access timestamp
    pub last_accessed: DateTime<Utc>,
    /// Access frequency
    pub access_frequency: f32,
    /// Node metadata
    pub metadata: HashMap<String, String>,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Associated fragment IDs
    pub fragment_ids: Vec<String>,
}

/// Types of memory nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryNodeType {
    /// Primary memory node
    Primary,
    /// Fragment node
    Fragment,
    /// Concept node
    Concept,
    /// Context node
    Context,
    /// Meta node (contains other nodes)
    Meta,
}

/// Connection edge between memory nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionEdge {
    /// Edge ID
    pub id: String,
    /// Source node ID
    pub source: String,
    /// Target node ID
    pub target: String,
    /// Connection weight
    pub weight: f32,
    /// Connection type
    pub connection_type: ConnectionType,
    /// Connection strength
    pub strength: f32,
    /// Bidirectional flag
    pub bidirectional: bool,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last reinforcement
    pub last_reinforced: DateTime<Utc>,
    /// Usage count
    pub usage_count: usize,
}

/// Types of connections between memories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionType {
    /// Semantic similarity
    Semantic,
    /// Temporal relationship
    Temporal,
    /// Causal relationship
    Causal,
    /// Hierarchical relationship
    Hierarchical,
    /// Associative relationship
    Associative,
    /// User-defined relationship
    UserDefined,
}

/// Graph metrics
#[derive(Debug, Clone, Default)]
pub struct GraphMetrics {
    /// Total nodes
    pub total_nodes: usize,
    /// Total edges
    pub total_edges: usize,
    /// Average degree
    pub avg_degree: f32,
    /// Clustering coefficient
    pub clustering_coefficient: f32,
    /// Average path length
    pub avg_path_length: f32,
    /// Graph density
    pub density: f32,
    /// Number of connected components
    pub connected_components: usize,
    /// Largest component size
    pub largest_component_size: usize,
}

/// Retrieval optimizer for efficient memory access
#[derive(Debug)]
pub struct RetrievalOptimizer {
    /// Retrieval strategies
    strategies: Vec<Box<dyn RetrievalStrategy>>,
    /// Cache for frequently accessed memories
    retrieval_cache: Arc<RwLock<HashMap<String, CachedRetrieval>>>,
    /// Performance metrics.
    /// Wrapped in `std::sync::RwLock` (not tokio) because metric updates are
    /// short critical sections that are never held across an `.await` point.
    /// Using a sync lock avoids the overhead of an async lock for a task that
    /// completes in nanoseconds.
    metrics: RwLock<RetrievalMetrics>,
}

/// Trait for retrieval strategies
pub trait RetrievalStrategy: Send + Sync + std::fmt::Debug {
    /// Find memories using this strategy
    fn find_memories(&self, query: &RetrievalQuery, graph: &MemoryGraph) -> Vec<RetrievalResult>;

    /// Get strategy name
    fn name(&self) -> &str;

    /// Get strategy confidence
    fn confidence(&self) -> f32;
}

/// Retrieval query
#[derive(Debug, Clone)]
pub struct RetrievalQuery {
    /// Query ID
    pub id: String,
    /// Query content
    pub content: Vec<f32>,
    /// Query type
    pub query_type: QueryType,
    /// Max results
    pub max_results: usize,
    /// Minimum confidence
    pub min_confidence: f32,
    /// Context filters
    pub context_filters: HashMap<String, String>,
    /// Retrieval strategies to use
    pub allowed_strategies: Vec<String>,
}

/// Types of retrieval queries
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum QueryType {
    /// Similarity search
    Similarity,
    /// Path-based search
    Path,
    /// Association search
    Association,
    /// Contextual search
    Contextual,
    /// Hybrid search
    Hybrid,
}

/// Retrieval result
#[derive(Debug, Clone)]
pub struct RetrievalResult {
    /// Memory ID
    pub memory_id: String,
    /// Confidence score
    pub confidence: f32,
    /// Retrieval path (if applicable)
    pub retrieval_path: Option<Vec<String>>,
    /// Strategy used
    pub strategy: String,
    /// Additional metadata
    pub metadata: HashMap<String, f32>,
}

/// Cached retrieval result
#[derive(Debug, Clone)]
pub struct CachedRetrieval {
    /// Results
    pub results: Vec<RetrievalResult>,
    /// Cache timestamp
    pub timestamp: DateTime<Utc>,
    /// Access count
    pub access_count: usize,
    /// Query hash
    pub query_hash: u64,
}

/// Retrieval performance metrics
#[derive(Debug, Clone, Default)]
pub struct RetrievalMetrics {
    /// Total retrievals
    pub total_retrievals: usize,
    /// Average retrieval time
    pub avg_retrieval_time: Duration,
    /// Cache hit rate
    pub cache_hit_rate: f32,
    /// Strategy success rates
    pub strategy_success_rates: HashMap<String, f32>,
    /// Average result confidence
    pub avg_confidence: f32,
}

/// Path finder for optimal memory traversal
#[derive(Debug)]
pub struct PathFinder {
    /// Pathfinding algorithms
    algorithms: Vec<Box<dyn PathfindingAlgorithm>>,
    /// Path cache
    path_cache: Arc<RwLock<HashMap<String, CachedPath>>>,
    /// Metrics.
    /// Wrapped in `std::sync::RwLock` (not tokio) because metric updates are
    /// short critical sections that are never held across an `.await` point.
    /// Using a sync lock avoids the overhead of an async lock for a task that
    /// completes in nanoseconds.
    metrics: RwLock<PathFindingMetrics>,
}

/// Trait for pathfinding algorithms
pub trait PathfindingAlgorithm: Send + Sync + std::fmt::Debug {
    /// Find path between nodes
    fn find_path(&self, graph: &MemoryGraph, start: &str, end: &str) -> Option<Path>;

    /// Get algorithm name
    fn name(&self) -> &str;

    /// Get algorithm complexity
    fn complexity(&self) -> AlgorithmComplexity;
}

/// Path between memory nodes
#[derive(Debug, Clone)]
pub struct Path {
    /// Path nodes
    pub nodes: Vec<String>,
    /// Total path weight
    pub total_weight: f32,
    /// Path length
    pub length: usize,
    /// Path confidence
    pub confidence: f32,
    /// Algorithm used
    pub algorithm: String,
}

/// Cached path
#[derive(Debug, Clone)]
pub struct CachedPath {
    /// Path
    pub path: Path,
    /// Cache timestamp
    pub timestamp: DateTime<Utc>,
    /// Access count
    pub access_count: usize,
}

/// Algorithm complexity metrics
#[derive(Debug, Clone)]
pub enum AlgorithmComplexity {
    Linear,
    Logarithmic,
    Quadratic,
    Exponential,
}

/// Path finding metrics
#[derive(Debug, Clone, Default)]
pub struct PathFindingMetrics {
    /// Total path searches
    pub total_searches: usize,
    /// Average search time
    pub avg_search_time: Duration,
    /// Path found rate
    pub path_found_rate: f32,
    /// Average path length
    pub avg_path_length: f32,
    /// Algorithm success rates
    pub algorithm_success_rates: HashMap<String, f32>,
}

/// Network statistics
#[derive(Debug, Clone, Default)]
pub struct NetworkStatistics {
    /// Total nodes added
    pub total_nodes_added: usize,
    /// Total nodes removed
    pub total_nodes_removed: usize,
    /// Total connections created
    pub total_connections_created: usize,
    /// Total retrievals performed
    pub total_retrievals: usize,
    /// Network efficiency score
    pub network_efficiency: f32,
    /// Average connection strength
    pub avg_connection_strength: f32,
    /// Network growth rate
    pub growth_rate: f32,
}

impl ConnectionNetwork {
    /// Create a new connection network with a private vector arena.
    pub fn new(config: MemoryAttractorConfig) -> Self {
        Self::with_arena(config, Arc::new(VectorArena::new()))
    }

    /// Create a network whose node vectors live in `arena`, shared with the
    /// fragment store.
    pub fn with_arena(config: MemoryAttractorConfig, arena: Arc<VectorArena>) -> Self {
        Self {
            config: config.clone(),
            graph: Arc::new(RwLock::new(MemoryGraph::new(arena))),
            connection_policy: RwLock::new((
                std::env::var("CONTEXTNEST_MAX_CONNECTIONS_PER_NODE")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(32),
                std::env::var("CONTEXTNEST_CONNECTION_SIMILARITY_THRESHOLD")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0.7),
            )),
            retrieval_optimizer: Arc::new(RetrievalOptimizer::new()),
            path_finder: Arc::new(PathFinder::new()),
            statistics: Arc::new(RwLock::new(NetworkStatistics::default())),
            status: Arc::new(RwLock::new(ComponentStatus::Initializing)),
        }
    }

    /// Initialize the connection network

    pub async fn initialize(&self) -> ContextNestResult<()> {
        *self.status.write().unwrap() = ComponentStatus::Running;
        Ok(())
    }

    /// Add a memory node to the network

    pub async fn add_node(&self, node: MemoryNode) -> ContextNestResult<()> {
        let node_id = node.id.clone();

        // Phase 1: insert under the write lock. We deliberately scope the
        // guard so it drops before we call `create_connections_for_node`,
        // which itself takes `graph.read()` — without the drop, that read
        // would deadlock on `std::sync::RwLock` (writer waiting for itself).
        {
            let mut graph = self.graph.write().unwrap();
            graph.insert_node(node)?;
        }

        // Phase 2: create connections (uses read+write internally, see fix in
        // `create_connections_for_node` which collects candidates first then
        // releases the read lock before issuing write-side `create_connection`
        // calls).
        self.create_connections_for_node(&node_id).await?;

        // Phase 3: refresh metrics + stats under a fresh write lock.
        {
            let mut graph = self.graph.write().unwrap();
            graph.update_metrics();
        }
        self.update_statistics(|stats| {
            stats.total_nodes_added += 1;
        });

        Ok(())
    }

    /// Remove a memory node and every edge touching it. O(degree).

    pub async fn remove_node(&self, node_id: &str) -> ContextNestResult<()> {
        let mut graph = self.graph.write().unwrap();
        let Some(row) = graph.row_of(node_id) else {
            return Err(ContextNestError::NotFound(format!(
                "Node {} not found",
                node_id
            )));
        };
        graph.remove_node_row(row);
        graph.arena.release(node_id, Holder::Node);
        graph.update_metrics();
        drop(graph);

        self.update_statistics(|stats| {
            stats.total_nodes_removed += 1;
        });

        Ok(())
    }

    /// Create connection between two nodes

    pub async fn create_connection(
        &self,
        source_id: &str,
        target_id: &str,
        connection_type: ConnectionType,
        weight: f32,
    ) -> ContextNestResult<String> {
        let mut graph = self.graph.write().unwrap();

        let Some(source) = graph.row_of(source_id) else {
            return Err(ContextNestError::NotFound(format!(
                "Source node {} not found",
                source_id
            )));
        };
        let Some(target) = graph.row_of(target_id) else {
            return Err(ContextNestError::NotFound(format!(
                "Target node {} not found",
                target_id
            )));
        };

        let now = Utc::now();
        let uuid = Uuid::new_v4();
        graph.push_edge(
            EdgeRecord {
                uuid: uuid.as_u128(),
                source,
                target,
                weight,
                strength: weight,
                created_at: now,
                last_reinforced: now,
                usage_count: 0,
                connection_type,
                bidirectional: true,
            },
            None,
        );
        graph.update_metrics();
        drop(graph);

        self.update_statistics(|stats| {
            stats.total_connections_created += 1;
        });

        Ok(uuid.to_string())
    }

    /// 1-hop neighbors of `node_id` with their edge weights, sorted
    /// strongest first. Used by Phase 5 of the neural-field epic
    /// (`docs/roadmap/epics/neural-field-real.md`) to surface
    /// learned-graph siblings of a query's top hit at retrieve time,
    /// independent of the heavier `retrieve_memories` cache path.
    ///
    /// Walks the node's incident edges (O(degree)) and emits both
    /// directions of any bidirectional edge so callers get a symmetric
    /// neighbor list. Returns an empty Vec when the node has no edges yet
    /// (cold substrate, or no peer fragments survived similarity-driven
    /// auto-connection thresholds).
    pub async fn neighbors_of(&self, node_id: &str) -> Vec<(String, f32)> {
        let graph = self.graph.read().unwrap();
        let Some(row) = graph.row_of(node_id) else {
            return Vec::new();
        };
        let view = graph.arena.read();
        let mut out: Vec<(String, f32)> = graph
            .neighbor_rows(row)
            .filter_map(|(other, weight)| Some((view.id(other)?.to_string(), weight)))
            .collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    /// Retrieve memories based on query

    pub async fn retrieve_memories(
        &self,
        query: RetrievalQuery,
    ) -> ContextNestResult<Vec<RetrievalResult>> {
        let start_time = Utc::now();

        // Update statistics
        self.update_statistics(|stats| {
            stats.total_retrievals += 1;
        });

        // Check cache first
        let query_hash = self.calculate_query_hash(&query);
        if let Some(cached) = self.get_cached_retrieval(query_hash) {
            return Ok(cached.results.clone());
        }

        // Perform retrieval using optimizer
        let graph = self.graph.read().unwrap();
        let results = self.retrieval_optimizer.retrieve(&query, &graph)?;

        // Cache results
        self.cache_retrieval(query_hash, &results);

        let retrieval_time = Utc::now()
            .signed_duration_since(start_time)
            .to_std()
            .unwrap_or_default();

        // Update retrieval metrics. `update_metrics` takes `&self` and locks
        // `RetrievalOptimizer.metrics` (RwLock<RetrievalMetrics>) internally,
        // so this is safe through the Arc without requiring `Arc::get_mut`.
        self.retrieval_optimizer
            .update_metrics(retrieval_time, &results);

        Ok(results)
    }

    /// Find path between two memories

    pub async fn find_path(&self, start_id: &str, end_id: &str) -> ContextNestResult<Option<Path>> {
        let graph = self.graph.read().unwrap();

        // Check if nodes exist
        if graph.row_of(start_id).is_none() {
            return Err(ContextNestError::NotFound(format!(
                "Start node {} not found",
                start_id
            )));
        }

        if graph.row_of(end_id).is_none() {
            return Err(ContextNestError::NotFound(format!(
                "End node {} not found",
                end_id
            )));
        }

        // Use path finder
        let path = self.path_finder.find_path(&graph, start_id, end_id)?;

        // Update path-finding metrics. `update_metrics` takes `&self` and
        // locks `PathFinder.metrics` (RwLock<PathFindingMetrics>) internally,
        // so this is safe through the Arc without requiring `Arc::get_mut`.
        self.path_finder.update_metrics(&path);

        Ok(path)
    }

    /// Reinforce connections based on usage. O(degree) per path step.

    pub async fn reinforce_connections(&self, retrieval_path: &[String]) -> ContextNestResult<()> {
        let mut graph = self.graph.write().unwrap();

        for window in retrieval_path.windows(2) {
            let (Some(source), Some(target)) = (graph.row_of(&window[0]), graph.row_of(&window[1]))
            else {
                continue;
            };
            let now = Utc::now();
            let slots: Vec<u32> = graph.nodes[source as usize]
                .as_ref()
                .map(|slot| slot.incident.clone())
                .unwrap_or_default();
            let mut node_bump = false;
            for slot in slots {
                let edge = &mut graph.edges[slot as usize];
                if (edge.source == source && edge.target == target)
                    || (edge.source == target && edge.target == source)
                {
                    edge.usage_count = edge.usage_count.saturating_add(1);
                    edge.last_reinforced = now;
                    edge.strength = (edge.strength * 0.9 + 0.1).min(1.0);
                    node_bump = true;
                }
            }

            if node_bump {
                for row in [source, target] {
                    if let Some(node) = graph.nodes[row as usize].as_mut() {
                        node.last_accessed = now;
                        node.access_frequency += 1.0;
                    }
                }
            }
        }

        Ok(())
    }

    /// Optimize network structure

    pub async fn optimize_network(&self) -> ContextNestResult<NetworkOptimizationResult> {
        let mut graph = self.graph.write().unwrap();
        let mut optimizations = Vec::new();

        // Remove weak connections, keeping both endpoints' incident lists
        // consistent (the previous map-retain left stale edge ids behind).
        let weak: Vec<u32> = graph
            .edges
            .iter()
            .enumerate()
            .filter(|(_, edge)| edge.is_live() && edge.strength <= 0.1)
            .map(|(slot, _)| slot as u32)
            .collect();
        let edges_removed = weak.len();
        for slot in weak {
            graph.remove_edge(slot);
        }

        if edges_removed > 0 {
            optimizations.push(NetworkOptimization {
                optimization_type: OptimizationType::RemoveWeakConnections,
                impact: edges_removed as f32,
                description: format!("Removed {} weak connections", edges_removed),
            });
        }

        // Update metrics
        graph.update_metrics();

        // Calculate overall improvement
        let improvement_score = if optimizations.is_empty() {
            0.0
        } else {
            optimizations.iter().map(|o| o.impact).sum::<f32>() / optimizations.len() as f32
        };

        Ok(NetworkOptimizationResult {
            optimizations,
            improvement_score,
            final_node_count: graph.node_count,
            final_edge_count: graph.edge_count,
        })
    }

    /// Get network statistics

    pub fn get_statistics(&self) -> NetworkStatistics {
        self.statistics.read().unwrap().clone()
    }

    /// Get graph metrics

    pub fn get_graph_metrics(&self) -> GraphMetrics {
        self.graph.read().unwrap().metrics.clone()
    }

    /// Get a snapshot of the path-finder metrics.
    /// Delegates to `PathFinder::metrics()` which clones the locked value so
    /// callers do not have to manage lock lifetimes.
    pub fn path_finder_metrics(&self) -> PathFindingMetrics {
        self.path_finder.metrics()
    }

    /// Get a snapshot of the retrieval-optimizer metrics.
    /// Delegates to `RetrievalOptimizer::metrics()` which clones the locked
    /// value so callers do not have to manage lock lifetimes.
    pub fn retrieval_optimizer_metrics(&self) -> RetrievalMetrics {
        self.retrieval_optimizer.metrics()
    }

    pub(crate) fn validate_vectors<'a>(
        &self,
        vectors: impl Iterator<Item = &'a [f32]>,
    ) -> ContextNestResult<()> {
        let graph = self.graph.read().unwrap();
        let mut dimensions = graph.arena.dim();
        for vector in vectors {
            if vector.len() > 8192
                || crate::services::exact::norm(vector).is_none()
                || dimensions.is_some_and(|dim| dim != vector.len())
            {
                return Err(ContextNestError::Validation(
                    "invalid vector or embedding dimension mismatch".into(),
                ));
            }
            dimensions = Some(vector.len());
        }
        Ok(())
    }

    pub(crate) fn set_connection_policy(&self, max: usize, threshold: f32) {
        *self.connection_policy.write().unwrap() = (max, threshold);
    }

    /// Durable form of one node (`Some(id)`: the node and its incident
    /// edges) or of the whole graph (`None`). Hydrates vectors from the
    /// arena, so whole-graph snapshots allocate one vector per node.
    pub(crate) fn durable_graph(&self, id: Option<&str>) -> (Vec<MemoryNode>, Vec<ConnectionEdge>) {
        let graph = self.graph.read().unwrap();
        let view = graph.arena.read();
        match id {
            None => (
                graph
                    .node_rows()
                    .filter_map(|(row, _)| graph.to_node(row, &view))
                    .collect(),
                (0..graph.edges.len() as u32)
                    .filter_map(|slot| graph.to_edge(slot, &view))
                    .collect(),
            ),
            Some(id) => {
                // Resolve through the held view: a second `arena.read()` on
                // this thread can deadlock behind a queued writer.
                let Some(row) = view.row_of(id).filter(|&row| graph.has_row(row)) else {
                    return (Vec::new(), Vec::new());
                };
                let incident = graph.nodes[row as usize]
                    .as_ref()
                    .map(|slot| slot.incident.as_slice())
                    .unwrap_or_default();
                (
                    graph.to_node(row, &view).into_iter().collect(),
                    incident
                        .iter()
                        .filter_map(|&slot| graph.to_edge(slot, &view))
                        .collect(),
                )
            }
        }
    }

    /// Restore durable nodes and edges. A node whose `content` is empty
    /// reuses the arena row its fragment already claimed. Edges whose
    /// endpoints are not nodes, or whose id is already present, are skipped.
    pub(crate) fn restore_graph(&self, nodes: Vec<MemoryNode>, edges: Vec<ConnectionEdge>) {
        let mut graph = self.graph.write().unwrap();
        for node in nodes {
            graph.restore_node(node);
        }
        graph.edges.reserve(edges.len());
        for edge in edges {
            graph.restore_edge(edge);
        }
        graph.update_metrics();
    }

    // Helper methods

    async fn create_connections_for_node(&self, node_id: &str) -> ContextNestResult<()> {
        let (max_connections, similarity_threshold) = *self.connection_policy.read().unwrap();

        // Phase 1: score candidates under read locks (graph, then arena —
        // the crate-wide lock order). One contiguous pass over the arena
        // rows; ids are cloned only for the final top-K. Locks drop before
        // phase 2 because `create_connection` takes the graph write lock.
        let candidates = {
            let graph = self.graph.read().unwrap();
            let Some(row) = graph.row_of(node_id) else {
                return Ok(());
            };
            let view = graph.arena.read();
            let Some(query) = view.vector(row) else {
                return Ok(());
            };
            view.top_k(query, max_connections, similarity_threshold, |other| {
                other != row && graph.has_row(other)
            })
        };

        // Phase 2: issue connection creates with no read lock held.
        for (other_id, similarity) in candidates {
            let _ = self
                .create_connection(node_id, &other_id, ConnectionType::Semantic, similarity)
                .await;
        }

        Ok(())
    }

    fn calculate_query_hash(&self, query: &RetrievalQuery) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        use std::hash::{Hash, Hasher};

        // Vec<f32> doesn't impl Hash (f32 is not Hash because of NaN). Fold the
        // content into a single u64 via the shared `calculate_simple_hash`,
        // then feed that into the rest of the query-key derivation.
        utils::calculate_simple_hash(&query.content).hash(&mut hasher);
        query.query_type.hash(&mut hasher);
        query.max_results.hash(&mut hasher);
        hasher.finish()
    }

    fn get_cached_retrieval(&self, query_hash: u64) -> Option<CachedRetrieval> {
        let mut cache = self.retrieval_optimizer.retrieval_cache.write().unwrap();
        if let Some(cached) = cache.get_mut(&query_hash.to_string()) {
            cached.access_count += 1;
            return Some(cached.clone());
        }
        None
    }

    fn cache_retrieval(&self, query_hash: u64, results: &[RetrievalResult]) {
        let mut cache = self.retrieval_optimizer.retrieval_cache.write().unwrap();

        // Manage cache size
        if cache.len() > 1000 {
            let to_remove = cache.len() - 1000;
            let keys: Vec<String> = cache.keys().take(to_remove).cloned().collect();
            for key in keys {
                cache.remove(&key);
            }
        }

        cache.insert(
            query_hash.to_string(),
            CachedRetrieval {
                results: results.to_vec(),
                timestamp: Utc::now(),
                access_count: 1,
                query_hash,
            },
        );
    }

    fn update_statistics<F>(&self, update_fn: F)
    where
        F: FnOnce(&mut NetworkStatistics),
    {
        let mut stats = self.statistics.write().unwrap();
        update_fn(&mut stats);

        // Update derived metrics
        if stats.total_nodes_added > 0 {
            stats.growth_rate = (stats.total_nodes_added as f32 - stats.total_nodes_removed as f32)
                / stats.total_nodes_added as f32;
        }
    }
}

impl MemoryGraph {
    fn new(arena: Arc<VectorArena>) -> Self {
        Self {
            arena,
            nodes: Vec::new(),
            node_count: 0,
            edges: Vec::new(),
            free_edges: Vec::new(),
            edge_count: 0,
            edge_names: HashMap::new(),
            metrics: GraphMetrics::default(),
        }
    }

    fn has_row(&self, row: u32) -> bool {
        self.nodes.get(row as usize).is_some_and(Option::is_some)
    }

    /// Row of `id` when it is a graph node (an arena row alone is not).
    fn row_of(&self, id: &str) -> Option<u32> {
        self.arena.row(id).filter(|&row| self.has_row(row))
    }

    fn node_rows(&self) -> impl Iterator<Item = (u32, &NodeSlot)> {
        self.nodes
            .iter()
            .enumerate()
            .filter_map(|(row, slot)| Some((row as u32, slot.as_ref()?)))
    }

    /// Walkable neighbours of `row` with edge weights.
    fn neighbor_rows(&self, row: u32) -> impl Iterator<Item = (u32, f32)> + '_ {
        self.nodes
            .get(row as usize)
            .and_then(Option::as_ref)
            .map(|slot| slot.incident.as_slice())
            .unwrap_or_default()
            .iter()
            .filter_map(move |&slot| {
                let edge = &self.edges[slot as usize];
                Some((edge.other(row)?, edge.weight))
            })
    }

    fn slot_for(node: &MemoryNode) -> NodeSlot {
        NodeSlot {
            node_type: node.node_type.clone(),
            importance: node.importance,
            last_accessed: node.last_accessed,
            access_frequency: node.access_frequency,
            metadata: node.metadata.clone(),
            created_at: node.created_at,
            fragment_ids: node.fragment_ids.clone(),
            incident: Vec::new(),
        }
    }

    fn place(&mut self, row: u32, slot: NodeSlot) {
        let index = row as usize;
        if self.nodes.len() <= index {
            self.nodes.resize_with(index + 1, || None);
        }
        if self.nodes[index].replace(slot).is_none() {
            self.node_count += 1;
        }
    }

    fn insert_node(&mut self, node: MemoryNode) -> ContextNestResult<u32> {
        if self.row_of(&node.id).is_some() {
            return Err(ContextNestError::Validation(format!(
                "Node {} already exists",
                node.id
            )));
        }
        let row = self
            .arena
            .claim(&node.id, Holder::Node, &node.content)
            .map_err(|e| ContextNestError::Validation(e.to_string()))?;
        self.place(row, Self::slot_for(&node));
        Ok(row)
    }

    /// Restore path: replaces an existing node's metadata (keeping its
    /// edges); an empty `content` reuses the fragment's arena row.
    fn restore_node(&mut self, node: MemoryNode) {
        let row = if node.content.is_empty() {
            self.arena.claim_existing(&node.id, Holder::Node)
        } else {
            self.arena.claim(&node.id, Holder::Node, &node.content).ok()
        };
        let Some(row) = row else {
            return;
        };
        let mut slot = Self::slot_for(&node);
        if let Some(existing) = self.nodes.get_mut(row as usize).and_then(Option::take) {
            self.node_count -= 1;
            slot.incident = existing.incident;
        }
        self.place(row, slot);
    }

    fn restore_edge(&mut self, edge: ConnectionEdge) {
        let (Some(source), Some(target)) = (self.row_of(&edge.source), self.row_of(&edge.target))
        else {
            return;
        };
        let uuid = Uuid::parse_str(&edge.id).ok().map(|u| u.as_u128());
        let duplicate = self.nodes[source as usize].as_ref().is_some_and(|slot| {
            slot.incident.iter().any(|&s| match uuid {
                Some(u) => self.edges[s as usize].uuid == u && !self.edge_names.contains_key(&s),
                None => self.edge_names.get(&s) == Some(&edge.id),
            })
        });
        if duplicate {
            return;
        }
        self.push_edge(
            EdgeRecord {
                uuid: uuid.unwrap_or(0),
                source,
                target,
                weight: edge.weight,
                strength: edge.strength,
                created_at: edge.created_at,
                last_reinforced: edge.last_reinforced,
                usage_count: u32::try_from(edge.usage_count).unwrap_or(u32::MAX),
                connection_type: edge.connection_type,
                bidirectional: edge.bidirectional,
            },
            uuid.is_none().then_some(edge.id),
        );
    }

    fn push_edge(&mut self, record: EdgeRecord, name: Option<String>) -> u32 {
        let slot = match self.free_edges.pop() {
            Some(slot) => {
                self.edges[slot as usize] = record;
                slot
            }
            None => {
                self.edges.push(record);
                (self.edges.len() - 1) as u32
            }
        };
        if let Some(name) = name {
            self.edge_names.insert(slot, name);
        }
        for row in [record.source, record.target] {
            if let Some(node) = self.nodes[row as usize].as_mut() {
                if !node.incident.contains(&slot) {
                    node.incident.push(slot);
                }
            }
        }
        self.edge_count += 1;
        slot
    }

    fn remove_edge(&mut self, slot: u32) {
        let edge = self.edges[slot as usize];
        if !edge.is_live() {
            return;
        }
        for row in [edge.source, edge.target] {
            if let Some(node) = self.nodes[row as usize].as_mut() {
                node.incident.retain(|&s| s != slot);
            }
        }
        self.edges[slot as usize].source = DEAD;
        self.edge_names.remove(&slot);
        self.free_edges.push(slot);
        self.edge_count -= 1;
    }

    fn remove_node_row(&mut self, row: u32) {
        let incident = self.nodes[row as usize]
            .as_mut()
            .map(|slot| std::mem::take(&mut slot.incident))
            .unwrap_or_default();
        for slot in incident {
            self.remove_edge(slot);
        }
        if self.nodes[row as usize].take().is_some() {
            self.node_count -= 1;
        }
    }

    fn edge_id(&self, slot: u32) -> String {
        self.edge_names
            .get(&slot)
            .cloned()
            .unwrap_or_else(|| Uuid::from_u128(self.edges[slot as usize].uuid).to_string())
    }

    fn to_edge(&self, slot: u32, view: &ArenaView<'_>) -> Option<ConnectionEdge> {
        let edge = self.edges.get(slot as usize).filter(|e| e.is_live())?;
        Some(ConnectionEdge {
            id: self.edge_id(slot),
            source: view.id(edge.source)?.to_string(),
            target: view.id(edge.target)?.to_string(),
            weight: edge.weight,
            connection_type: edge.connection_type,
            strength: edge.strength,
            bidirectional: edge.bidirectional,
            created_at: edge.created_at,
            last_reinforced: edge.last_reinforced,
            usage_count: edge.usage_count as usize,
        })
    }

    fn to_node(&self, row: u32, view: &ArenaView<'_>) -> Option<MemoryNode> {
        let slot = self.nodes.get(row as usize)?.as_ref()?;
        Some(MemoryNode {
            id: view.id(row)?.to_string(),
            node_type: slot.node_type.clone(),
            content: view.vector(row)?.to_vec(),
            importance: slot.importance,
            last_accessed: slot.last_accessed,
            access_frequency: slot.access_frequency,
            metadata: slot.metadata.clone(),
            created_at: slot.created_at,
            fragment_ids: slot.fragment_ids.clone(),
        })
    }

    fn update_metrics(&mut self) {
        self.metrics.total_nodes = self.node_count;
        self.metrics.total_edges = self.edge_count;

        // avg_degree via a closed form: every edge contributes one incident
        // entry to each endpoint, so total degree is 2 * total_edges.
        if self.metrics.total_nodes > 0 {
            self.metrics.avg_degree =
                (2 * self.metrics.total_edges) as f32 / self.metrics.total_nodes as f32;
        }

        // Calculate graph density. Guard against the zero-node case: `total_nodes - 1`
        // underflows `usize` when the graph is empty (no nodes), which panics in
        // debug builds. The density of an empty graph is 0 by convention.
        let max_possible_edges = if self.metrics.total_nodes > 1 {
            self.metrics.total_nodes * (self.metrics.total_nodes - 1) / 2
        } else {
            0
        };
        if max_possible_edges > 0 {
            self.metrics.density = self.metrics.total_edges as f32 / max_possible_edges as f32;
        }

        // Simplified calculations for other metrics
        self.metrics.clustering_coefficient = 0.3; // Placeholder
        self.metrics.avg_path_length = 3.5; // Placeholder
        self.metrics.connected_components = 1; // Assuming connected graph
        self.metrics.largest_component_size = self.metrics.total_nodes;
    }
}

impl RetrievalOptimizer {
    fn new() -> Self {
        Self {
            strategies: vec![
                Box::new(SimilarityRetrieval::new()),
                Box::new(AssociationRetrieval::new()),
                Box::new(ContextualRetrieval::new()),
            ],
            retrieval_cache: Arc::new(RwLock::new(HashMap::new())),
            metrics: RwLock::new(RetrievalMetrics::default()),
        }
    }

    /// Return a clone of the current retrieval metrics.
    /// Clones the locked value rather than returning a guard so that callers
    /// (including tests) do not have to manage the lock lifetime.
    pub fn metrics(&self) -> RetrievalMetrics {
        self.metrics.read().unwrap().clone()
    }

    fn retrieve(
        &self,
        query: &RetrievalQuery,
        graph: &MemoryGraph,
    ) -> ContextNestResult<Vec<RetrievalResult>> {
        let mut all_results = Vec::new();

        // Try each allowed strategy
        for strategy in &self.strategies {
            if query.allowed_strategies.is_empty()
                || query
                    .allowed_strategies
                    .contains(&strategy.name().to_string())
            {
                let strategy_results = strategy.find_memories(query, graph);
                all_results.extend(strategy_results);
            }
        }

        // Sort by confidence and limit results
        all_results.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        all_results.truncate(query.max_results);

        // Filter by minimum confidence
        all_results.retain(|result| result.confidence >= query.min_confidence);

        Ok(all_results)
    }

    fn update_metrics(&self, retrieval_time: Duration, results: &[RetrievalResult]) {
        let mut metrics = self.metrics.write().unwrap();
        metrics.total_retrievals += 1;

        // Update average retrieval time
        metrics.avg_retrieval_time = (metrics.avg_retrieval_time
            * (metrics.total_retrievals - 1) as u32
            + Duration::from_millis(retrieval_time.as_millis() as u64))
            / metrics.total_retrievals as u32;

        // Update average confidence
        if !results.is_empty() {
            let avg_confidence: f32 =
                results.iter().map(|r| r.confidence).sum::<f32>() / results.len() as f32;
            metrics.avg_confidence =
                (metrics.avg_confidence * (metrics.total_retrievals - 1) as f32 + avg_confidence)
                    / metrics.total_retrievals as f32;
        }

        // Update cache hit rate
        let cache_size = self.retrieval_cache.read().unwrap().len();
        if cache_size > 0 {
            metrics.cache_hit_rate = cache_size as f32 / metrics.total_retrievals as f32;
        }
    }
}

// Retrieval strategy implementations

#[derive(Debug)]
struct SimilarityRetrieval {
    similarity_threshold: f32,
}

impl SimilarityRetrieval {
    fn new() -> Self {
        Self {
            similarity_threshold: 0.5,
        }
    }
}

impl RetrievalStrategy for SimilarityRetrieval {
    fn find_memories(&self, query: &RetrievalQuery, graph: &MemoryGraph) -> Vec<RetrievalResult> {
        let view = graph.arena.read();
        let mut results: Vec<RetrievalResult> = view
            .all_at_least(&query.content, self.similarity_threshold, |row| {
                graph.has_row(row)
            })
            .into_iter()
            .map(|(memory_id, similarity)| RetrievalResult {
                memory_id,
                confidence: similarity,
                retrieval_path: None,
                strategy: self.name().to_string(),
                metadata: HashMap::new(),
            })
            .collect();

        results.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        results
    }

    fn name(&self) -> &str {
        "similarity"
    }

    fn confidence(&self) -> f32 {
        0.8
    }
}

#[derive(Debug)]
struct AssociationRetrieval {
    max_depth: usize,
}

impl AssociationRetrieval {
    fn new() -> Self {
        Self { max_depth: 3 }
    }
}

impl RetrievalStrategy for AssociationRetrieval {
    fn find_memories(&self, query: &RetrievalQuery, graph: &MemoryGraph) -> Vec<RetrievalResult> {
        let view = graph.arena.read();

        // Find nodes with highest degree (most connected); ties by id so the
        // ranking is deterministic.
        let mut node_degrees: Vec<(&str, usize)> = graph
            .node_rows()
            .filter_map(|(row, slot)| Some((view.id(row)?.as_ref(), slot.incident.len())))
            .collect();
        node_degrees.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

        // Return top connected nodes
        node_degrees
            .into_iter()
            .take(query.max_results)
            .filter_map(|(node_id, degree)| {
                let confidence = (degree as f32 / graph.metrics.total_nodes as f32).min(1.0);
                (confidence >= query.min_confidence).then(|| RetrievalResult {
                    memory_id: node_id.to_string(),
                    confidence,
                    retrieval_path: None,
                    strategy: self.name().to_string(),
                    metadata: HashMap::new(),
                })
            })
            .collect()
    }

    fn name(&self) -> &str {
        "association"
    }

    fn confidence(&self) -> f32 {
        0.6
    }
}

#[derive(Debug)]
struct ContextualRetrieval {
    context_weight: f32,
}

impl ContextualRetrieval {
    fn new() -> Self {
        Self {
            context_weight: 0.3,
        }
    }
}

impl RetrievalStrategy for ContextualRetrieval {
    fn find_memories(&self, query: &RetrievalQuery, graph: &MemoryGraph) -> Vec<RetrievalResult> {
        let view = graph.arena.read();
        let mut results = Vec::new();

        for (row, node) in graph.node_rows() {
            // Calculate context-based confidence
            let mut confidence = 0.5; // Base confidence

            // Boost confidence based on recent access
            let time_since_access = Utc::now()
                .signed_duration_since(node.last_accessed)
                .to_std()
                .unwrap_or_default()
                .as_secs_f32()
                / 3600.0; // Hours
            let recency_factor = (-time_since_access / 24.0).exp();
            confidence += recency_factor * self.context_weight;

            // Boost confidence based on access frequency
            let frequency_factor = (node.access_frequency / 10.0).min(1.0);
            confidence += frequency_factor * self.context_weight;

            // Apply context filters
            let matches_filters = query
                .context_filters
                .iter()
                .all(|(key, value)| node.metadata.get(key) == Some(value));

            if matches_filters && confidence >= query.min_confidence {
                let Some(node_id) = view.id(row) else {
                    continue;
                };
                results.push(RetrievalResult {
                    memory_id: node_id.to_string(),
                    confidence: confidence.min(1.0),
                    retrieval_path: None,
                    strategy: self.name().to_string(),
                    metadata: HashMap::new(),
                });
            }
        }

        results.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        results
    }

    fn name(&self) -> &str {
        "contextual"
    }

    fn confidence(&self) -> f32 {
        0.7
    }
}

impl PathFinder {
    fn new() -> Self {
        Self {
            algorithms: vec![
                Box::new(DijkstraPathfinding::new()),
                Box::new(AStarPathfinding::new()),
            ],
            path_cache: Arc::new(RwLock::new(HashMap::new())),
            metrics: RwLock::new(PathFindingMetrics::default()),
        }
    }

    /// Return a clone of the current path-finding metrics.
    /// Clones the locked value rather than returning a guard so that callers
    /// (including tests) do not have to manage the lock lifetime.
    pub fn metrics(&self) -> PathFindingMetrics {
        self.metrics.read().unwrap().clone()
    }

    fn find_path(
        &self,
        graph: &MemoryGraph,
        start: &str,
        end: &str,
    ) -> ContextNestResult<Option<Path>> {
        // Check cache first
        let cache_key = format!("{}_{}", start, end);
        if let Some(cached) = self.path_cache.read().unwrap().get(&cache_key) {
            return Ok(Some(cached.path.clone()));
        }

        // Try each algorithm
        for algorithm in &self.algorithms {
            if let Some(path) = algorithm.find_path(graph, start, end) {
                // Cache the path
                let mut cache = self.path_cache.write().unwrap();
                cache.insert(
                    cache_key,
                    CachedPath {
                        path: path.clone(),
                        timestamp: Utc::now(),
                        access_count: 1,
                    },
                );

                return Ok(Some(path));
            }
        }

        Ok(None)
    }

    fn update_metrics(&self, path: &Option<Path>) {
        let mut metrics = self.metrics.write().unwrap();
        metrics.total_searches += 1;

        match path {
            Some(path) => {
                metrics.path_found_rate =
                    (metrics.path_found_rate * (metrics.total_searches - 1) as f32 + 1.0)
                        / metrics.total_searches as f32;

                metrics.avg_path_length = (metrics.avg_path_length
                    * (metrics.total_searches - 1) as f32
                    + path.length as f32)
                    / metrics.total_searches as f32;
            }
            None => {
                metrics.path_found_rate = (metrics.path_found_rate
                    * (metrics.total_searches - 1) as f32)
                    / metrics.total_searches as f32;
            }
        }
    }
}

// Pathfinding algorithm implementations

#[derive(Debug)]
struct DijkstraPathfinding;

impl DijkstraPathfinding {
    fn new() -> Self {
        Self
    }
}

impl PathfindingAlgorithm for DijkstraPathfinding {
    fn find_path(&self, graph: &MemoryGraph, start: &str, end: &str) -> Option<Path> {
        // Unit-weight Dijkstra over node rows. Distances are tracked lazily
        // for visited rows only — the previous version seeded a map entry
        // (and an id clone) for every node in the graph per query. f32 is
        // PartialOrd-only, so distances ride in `NotNan` for the heap.
        use ordered_float::NotNan;
        let start_row = graph.row_of(start)?;
        let end_row = graph.row_of(end)?;
        let zero = NotNan::new(0.0_f32).expect("0.0 is not NaN");
        let step = NotNan::new(1.0_f32).expect("constant 1.0 is not NaN");
        let mut distances: HashMap<u32, NotNan<f32>> = HashMap::from([(start_row, zero)]);
        let mut previous: HashMap<u32, u32> = HashMap::new();
        let mut unvisited = BinaryHeap::from([(std::cmp::Reverse(zero), start_row)]);

        while let Some((std::cmp::Reverse(dist), current)) = unvisited.pop() {
            if current == end_row {
                break;
            }
            if distances.get(&current).is_some_and(|&d| dist > d) {
                continue;
            }
            for (neighbor, _) in graph.neighbor_rows(current) {
                let alt = dist + step;
                let improves = match distances.get(&neighbor) {
                    Some(&known) => alt < known,
                    None => true,
                };
                if improves {
                    distances.insert(neighbor, alt);
                    previous.insert(neighbor, current);
                    unvisited.push((std::cmp::Reverse(alt), neighbor));
                }
            }
        }

        let total_weight = distances.get(&end_row)?.into_inner();
        let mut rows = vec![end_row];
        let mut current = end_row;
        while current != start_row {
            current = *previous.get(&current)?;
            rows.push(current);
        }
        rows.reverse();

        let view = graph.arena.read();
        let path_nodes: Vec<String> = rows
            .into_iter()
            .map(|row| view.id(row).map(|id| id.to_string()))
            .collect::<Option<_>>()?;

        Some(Path {
            length: path_nodes.len(),
            nodes: path_nodes,
            total_weight,
            confidence: 0.8,
            algorithm: self.name().to_string(),
        })
    }

    fn name(&self) -> &str {
        "dijkstra"
    }

    fn complexity(&self) -> AlgorithmComplexity {
        AlgorithmComplexity::Quadratic
    }
}

#[derive(Debug)]
struct AStarPathfinding;

impl AStarPathfinding {
    fn new() -> Self {
        Self
    }
}

impl PathfindingAlgorithm for AStarPathfinding {
    fn find_path(&self, graph: &MemoryGraph, start: &str, end: &str) -> Option<Path> {
        // Simplified A* implementation (uses same logic as Dijkstra for now)
        // In practice, would use heuristic function
        let dijkstra = DijkstraPathfinding;
        dijkstra.find_path(graph, start, end)
    }

    fn name(&self) -> &str {
        "astar"
    }

    fn complexity(&self) -> AlgorithmComplexity {
        AlgorithmComplexity::Logarithmic
    }
}

/// Network optimization result
#[derive(Debug, Clone)]
pub struct NetworkOptimizationResult {
    /// Optimizations applied
    pub optimizations: Vec<NetworkOptimization>,
    /// Overall improvement score
    pub improvement_score: f32,
    /// Final node count
    pub final_node_count: usize,
    /// Final edge count
    pub final_edge_count: usize,
}

/// Individual network optimization
#[derive(Debug, Clone)]
pub struct NetworkOptimization {
    /// Type of optimization
    pub optimization_type: OptimizationType,
    /// Impact of optimization
    pub impact: f32,
    /// Description
    pub description: String,
}

/// Types of network optimizations
#[derive(Debug, Clone)]
pub enum OptimizationType {
    /// Remove weak connections
    RemoveWeakConnections,
    /// Merge similar nodes
    MergeSimilarNodes,
    /// Rebalance network
    RebalanceNetwork,
    /// Update connection weights
    UpdateConnectionWeights,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::attractors::MemoryAttractorConfig;
    use std::time::Instant;

    fn node(id: &str, content: Vec<f32>) -> MemoryNode {
        MemoryNode {
            id: id.to_string(),
            node_type: MemoryNodeType::Primary,
            content,
            importance: 0.8,
            last_accessed: Utc::now(),
            access_frequency: 1.0,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![],
        }
    }

    fn edge(graph: &mut MemoryGraph, source: &str, target: &str, weight: f32) -> u32 {
        let now = Utc::now();
        let record = EdgeRecord {
            uuid: Uuid::new_v4().as_u128(),
            source: graph.row_of(source).unwrap(),
            target: graph.row_of(target).unwrap(),
            weight,
            strength: weight,
            created_at: now,
            last_reinforced: now,
            usage_count: 0,
            connection_type: ConnectionType::Semantic,
            bidirectional: true,
        };
        graph.push_edge(record, None)
    }

    #[tokio::test]
    async fn test_connection_network() {
        let config = MemoryAttractorConfig::default();
        let network = ConnectionNetwork::new(config);
        network.initialize().await.unwrap();

        network
            .add_node(node("test_node", vec![0.1; 64]))
            .await
            .unwrap();

        // Get statistics
        let stats = network.get_statistics();
        assert_eq!(stats.total_nodes_added, 1);
    }

    #[tokio::test]
    async fn test_connection_creation() {
        let config = MemoryAttractorConfig::default();
        let network = ConnectionNetwork::new(config);
        network.initialize().await.unwrap();

        network
            .add_node(node("node1", vec![0.1; 32]))
            .await
            .unwrap();
        network
            .add_node(node("node2", vec![0.2; 32]))
            .await
            .unwrap();

        // Create connection
        let edge_id = network
            .create_connection("node1", "node2", ConnectionType::Semantic, 0.8)
            .await
            .unwrap();

        assert!(Uuid::parse_str(&edge_id).is_ok());

        // `add_node(node2)` auto-creates a Semantic connection because
        // `create_connections_for_node` finds node1 highly similar
        // (vec![0.1; 32] vs vec![0.2; 32] has cosine similarity > 0.7), so
        // when we then *manually* create_connection the count is 2, not 1.
        let stats = network.get_statistics();
        assert_eq!(stats.total_connections_created, 2);
        assert_eq!(network.get_graph_metrics().total_edges, 2);
    }

    #[tokio::test]
    async fn test_memory_retrieval() {
        let config = MemoryAttractorConfig::default();
        let network = ConnectionNetwork::new(config);
        network.initialize().await.unwrap();

        network
            .add_node(node("test_node", vec![0.1; 64]))
            .await
            .unwrap();

        // Create retrieval query
        let query = RetrievalQuery {
            id: "test_query".to_string(),
            content: vec![0.1; 64],
            query_type: QueryType::Similarity,
            max_results: 10,
            min_confidence: 0.5,
            context_filters: HashMap::new(),
            allowed_strategies: vec!["similarity".to_string()],
        };

        let results = network.retrieve_memories(query).await.unwrap();
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn test_path_finding() {
        let config = MemoryAttractorConfig::default();
        let network = ConnectionNetwork::new(config);
        network.initialize().await.unwrap();

        network
            .add_node(node("node1", vec![0.1; 32]))
            .await
            .unwrap();
        network
            .add_node(node("node2", vec![0.2; 32]))
            .await
            .unwrap();

        network
            .create_connection("node1", "node2", ConnectionType::Semantic, 0.8)
            .await
            .unwrap();

        // Find path
        let path = network.find_path("node1", "node2").await.unwrap();
        assert!(path.is_some());

        let path = path.unwrap();
        assert_eq!(path.nodes, vec!["node1".to_string(), "node2".to_string()]);
    }

    #[test]
    fn test_similarity_retrieval() {
        let strategy = SimilarityRetrieval::new();

        let query = RetrievalQuery {
            id: "test".to_string(),
            content: vec![0.1; 32],
            query_type: QueryType::Similarity,
            max_results: 5,
            min_confidence: 0.5,
            context_filters: HashMap::new(),
            allowed_strategies: vec![],
        };

        let arena = Arc::new(VectorArena::new());
        let mut graph = MemoryGraph::new(arena.clone());
        graph.insert_node(node("node1", vec![0.1; 32])).unwrap();
        // An arena row that is not a graph node must not be retrieved.
        arena
            .claim("fragment-only", Holder::Fragment, &[0.1; 32])
            .unwrap();

        let results = strategy.find_memories(&query, &graph);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].memory_id, "node1");
        assert!(results[0].confidence > 0.9);
    }

    #[test]
    fn test_dijkstra_pathfinding() {
        let algorithm = DijkstraPathfinding;

        let mut graph = MemoryGraph::new(Arc::new(VectorArena::new()));

        // Create a simple graph: node1 -> node2 -> node3
        for i in 1..=3 {
            graph
                .insert_node(node(&format!("node{}", i), vec![0.1; 32]))
                .unwrap();
        }
        edge(&mut graph, "node1", "node2", 0.9);
        edge(&mut graph, "node2", "node3", 0.9);

        let path = algorithm.find_path(&graph, "node1", "node3");
        assert!(path.is_some());

        let path = path.unwrap();
        assert_eq!(path.nodes, vec!["node1", "node2", "node3"]);
        assert_eq!(path.total_weight, 2.0);
        assert!(algorithm.find_path(&graph, "node1", "missing").is_none());
    }

    #[tokio::test]
    async fn remove_node_drops_incident_edges_and_releases_the_row() {
        let arena = Arc::new(VectorArena::new());
        let network =
            ConnectionNetwork::with_arena(MemoryAttractorConfig::default(), arena.clone());
        network.set_connection_policy(0, 0.7); // no auto-connections
        for (id, x) in [("a", 1.0), ("b", 2.0), ("c", 3.0)] {
            network.add_node(node(id, vec![x, 1.0])).await.unwrap();
        }
        network
            .create_connection("a", "b", ConnectionType::Semantic, 0.9)
            .await
            .unwrap();
        network
            .create_connection("b", "c", ConnectionType::Semantic, 0.8)
            .await
            .unwrap();
        network
            .create_connection("a", "c", ConnectionType::Semantic, 0.7)
            .await
            .unwrap();

        network.remove_node("b").await.unwrap();

        assert_eq!(network.get_graph_metrics().total_edges, 1);
        assert_eq!(
            network.neighbors_of("a").await,
            vec![("c".to_string(), 0.7)]
        );
        assert_eq!(
            network.neighbors_of("c").await,
            vec![("a".to_string(), 0.7)]
        );
        assert!(!arena.contains("b"));
        assert!(network.remove_node("b").await.is_err());

        // Freed edge slots and the arena row are reused.
        network.add_node(node("d", vec![4.0, 1.0])).await.unwrap();
        network
            .create_connection("d", "a", ConnectionType::Semantic, 0.6)
            .await
            .unwrap();
        let graph = network.graph.read().unwrap();
        assert_eq!(graph.edges.len(), 3, "a dead slot was reused");
        assert_eq!(graph.edge_count, 2);
    }

    #[tokio::test]
    async fn node_shares_the_fragment_row() {
        let arena = Arc::new(VectorArena::new());
        let row = arena.claim("f", Holder::Fragment, &[1.0, 0.0]).unwrap();
        let network =
            ConnectionNetwork::with_arena(MemoryAttractorConfig::default(), arena.clone());
        network.add_node(node("f", vec![1.0, 0.0])).await.unwrap();
        assert_eq!(arena.row("f"), Some(row));
        assert_eq!(arena.len(), 1);
        network.remove_node("f").await.unwrap();
        assert!(arena.contains("f"), "fragment still holds the row");
    }

    #[tokio::test]
    async fn durable_graph_round_trips_through_restore() {
        let network = ConnectionNetwork::new(MemoryAttractorConfig::default());
        network.set_connection_policy(0, 0.7);
        network.add_node(node("a", vec![1.0, 0.0])).await.unwrap();
        network.add_node(node("b", vec![0.0, 1.0])).await.unwrap();
        let id = network
            .create_connection("a", "b", ConnectionType::Causal, 0.42)
            .await
            .unwrap();
        network
            .reinforce_connections(&["a".into(), "b".into()])
            .await
            .unwrap();

        let (nodes, edges) = network.durable_graph(None);
        assert_eq!(nodes.len(), 2);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].id, id);
        assert_eq!(edges[0].usage_count, 1);
        assert_eq!(edges[0].connection_type, ConnectionType::Causal);

        let restored = ConnectionNetwork::new(MemoryAttractorConfig::default());
        let mut legacy = edges[0].clone();
        legacy.id = "legacy-edge".to_string();
        restored.restore_graph(nodes, vec![edges[0].clone(), edges[0].clone(), legacy]);
        let (_, again) = restored.durable_graph(Some("a"));
        let mut ids: Vec<_> = again.iter().map(|e| e.id.clone()).collect();
        ids.sort();
        assert_eq!(
            ids,
            vec![id.clone(), "legacy-edge".to_string()],
            "duplicate skipped"
        );
        assert_eq!(restored.get_graph_metrics().total_edges, 2);
        assert_eq!(
            restored.neighbors_of("b").await,
            vec![("a".to_string(), 0.42), ("a".to_string(), 0.42)]
        );
    }

    /// `durable_graph(Some(..))` runs on every checkpoint persist while the
    /// consolidation worker writes to the arena. It must not re-enter the
    /// arena lock under its own read view (deadlocks behind a queued writer).
    #[tokio::test]
    async fn durable_graph_does_not_deadlock_with_concurrent_arena_writers() {
        let arena = Arc::new(VectorArena::new());
        let network = Arc::new(ConnectionNetwork::with_arena(
            MemoryAttractorConfig::default(),
            arena.clone(),
        ));
        network.set_connection_policy(0, 0.7);
        network.add_node(node("a", vec![1.0, 0.0])).await.unwrap();

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let writer = {
            let (arena, stop) = (arena.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut i = 0u64;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    let id = format!("w{}", i % 64);
                    arena
                        .claim(&id, Holder::Fragment, &[1.0, i as f32])
                        .unwrap();
                    i += 1;
                }
            })
        };
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let reader = {
            let network = network.clone();
            std::thread::spawn(move || {
                for _ in 0..20_000 {
                    assert_eq!(network.durable_graph(Some("a")).0.len(), 1);
                }
                done_tx.send(()).unwrap();
            })
        };
        let finished = done_rx.recv_timeout(Duration::from_secs(20));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        writer.join().unwrap();
        assert!(
            finished.is_ok(),
            "durable_graph deadlocked against arena writers"
        );
        reader.join().unwrap();
    }

    #[tokio::test]
    async fn optimize_network_keeps_incident_lists_consistent() {
        let network = ConnectionNetwork::new(MemoryAttractorConfig::default());
        network.set_connection_policy(0, 0.7);
        network.add_node(node("a", vec![1.0, 0.0])).await.unwrap();
        network.add_node(node("b", vec![0.0, 1.0])).await.unwrap();
        network
            .create_connection("a", "b", ConnectionType::Semantic, 0.05)
            .await
            .unwrap();
        network
            .create_connection("a", "b", ConnectionType::Semantic, 0.9)
            .await
            .unwrap();
        let result = network.optimize_network().await.unwrap();
        assert_eq!(result.final_edge_count, 1);
        assert_eq!(
            network.neighbors_of("a").await,
            vec![("b".to_string(), 0.9)]
        );
    }

    /// Verify that `CONTEXTNEST_MAX_CONNECTIONS_PER_NODE` bounds the fan-out
    /// when adding a new node into a graph where many existing peers exceed
    /// the similarity threshold. Without the cap, `add_node` would create
    /// one edge per qualifying peer, causing avg_degree (and consolidation
    /// CPU) to grow with substrate size. With the cap set to 3 here we
    /// expect at most 3 connections to be created for the inbound node even
    /// though 10 peers qualify.
    ///
    /// This test mutates a process-global env var; serialize against other
    /// env-mutating tests via the conventional Mutex pattern if more land.
    #[tokio::test]
    async fn test_create_connections_respects_top_k_cap() {
        // Snapshot + override the cap. Restore on drop via a scope guard
        // pattern so a panic mid-test doesn't leak env to siblings.
        struct EnvGuard {
            key: &'static str,
            prev: Option<String>,
        }
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                match &self.prev {
                    Some(v) => std::env::set_var(self.key, v),
                    None => std::env::remove_var(self.key),
                }
            }
        }
        let _g = EnvGuard {
            key: "CONTEXTNEST_MAX_CONNECTIONS_PER_NODE",
            prev: std::env::var("CONTEXTNEST_MAX_CONNECTIONS_PER_NODE").ok(),
        };
        std::env::set_var("CONTEXTNEST_MAX_CONNECTIONS_PER_NODE", "3");

        let config = MemoryAttractorConfig::default();
        let network = ConnectionNetwork::new(config);
        network.initialize().await.unwrap();

        // Seed 10 peers whose content is essentially identical to the
        // inbound node — every one sits well above the 0.7 similarity floor.
        for i in 0..10 {
            network
                .add_node(node(&format!("peer{}", i), vec![0.1; 32]))
                .await
                .unwrap();
        }

        // Baseline: capture connection count after the seeding completes
        // (peers connect to each other as they're added).
        let stats_before = network.get_statistics();

        // Inbound node — same content vector, so all 10 peers qualify.
        network
            .add_node(node("inbound", vec![0.1; 32]))
            .await
            .unwrap();

        let stats_after = network.get_statistics();
        let new_connections =
            stats_after.total_connections_created - stats_before.total_connections_created;

        assert!(
            new_connections <= 3,
            "expected at most 3 new connections (top-K cap), got {}",
            new_connections
        );
    }

    /// Verify that calling `find_path` once increments the path-finder search
    /// counter from 0 to 1. The counter is stored inside
    /// `PathFinder.metrics: RwLock<PathFindingMetrics>` which is shared via
    /// `Arc<PathFinder>`, so the update must go through an interior-mutable
    /// write lock rather than `&mut self`.
    #[tokio::test]
    async fn test_path_finder_metrics_increment() {
        let config = MemoryAttractorConfig::default();
        let network = ConnectionNetwork::new(config);
        network.initialize().await.unwrap();

        // Confirm counter starts at zero.
        assert_eq!(network.path_finder_metrics().total_searches, 0);

        // Set up two nodes connected by an edge so find_path has a valid path.
        network
            .add_node(node("pf_node1", vec![0.1; 32]))
            .await
            .unwrap();
        network
            .add_node(node("pf_node2", vec![0.9; 32]))
            .await
            .unwrap();
        network
            .create_connection("pf_node1", "pf_node2", ConnectionType::Semantic, 0.8)
            .await
            .unwrap();

        // One find_path call should push total_searches to 1.
        let _ = network.find_path("pf_node1", "pf_node2").await.unwrap();
        assert_eq!(
            network.path_finder_metrics().total_searches,
            1,
            "expected total_searches == 1 after one find_path call"
        );
    }

    /// Verify that calling `retrieve_memories` once increments the retrieval
    /// counter from 0 to 1. The counter is stored inside
    /// `RetrievalOptimizer.metrics: RwLock<RetrievalMetrics>` which is shared
    /// via `Arc<RetrievalOptimizer>`, so the update must go through an
    /// interior-mutable write lock rather than `&mut self`.
    #[tokio::test]
    async fn test_retrieval_optimizer_metrics_increment() {
        let config = MemoryAttractorConfig::default();
        let network = ConnectionNetwork::new(config);
        network.initialize().await.unwrap();

        // Confirm counter starts at zero.
        assert_eq!(network.retrieval_optimizer_metrics().total_retrievals, 0);

        // Add a node so there is at least one candidate to retrieve.
        network
            .add_node(node("ro_node1", vec![0.5; 32]))
            .await
            .unwrap();

        let query = RetrievalQuery {
            id: "ro_query".to_string(),
            content: vec![0.5; 32],
            query_type: QueryType::Similarity,
            max_results: 5,
            min_confidence: 0.5,
            context_filters: HashMap::new(),
            allowed_strategies: vec!["similarity".to_string()],
        };

        // One retrieve_memories call should push total_retrievals to 1.
        let _ = network.retrieve_memories(query).await.unwrap();
        assert_eq!(
            network.retrieval_optimizer_metrics().total_retrievals,
            1,
            "expected total_retrievals == 1 after one retrieve_memories call"
        );
    }

    /// Scale benchmark for the contiguous exact scan (the operation that
    /// drove consolidation CPU). Run with:
    /// `cargo test --release --lib connection_scan_benchmark -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn connection_scan_benchmark() {
        let rows: usize = std::env::var("CN_BENCH_ROWS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(334_000);
        let dim = 1024;
        let arena = VectorArena::new();
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 40) as f32 / (1u64 << 24) as f32 - 0.5
        };
        let mut vector = vec![0.0f32; dim];
        for i in 0..rows {
            vector.iter_mut().for_each(|v| *v = next());
            arena
                .claim(&format!("frag-{i:08}"), Holder::Node, &vector)
                .unwrap();
        }
        let view = arena.read();
        let query = view.vector(7).unwrap().to_vec();
        let started = Instant::now();
        let iterations = 10;
        for _ in 0..iterations {
            std::hint::black_box(view.top_k(&query, 32, 0.7, |r| r != 7));
        }
        let per_scan = started.elapsed() / iterations;
        println!("exact top-32 over {rows} x {dim}: {per_scan:?} per scan");
    }
}

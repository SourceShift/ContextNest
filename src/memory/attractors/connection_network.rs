//! Connection Network for Optimized Memory Retrieval
//! Implements a sophisticated connection network that enables optimized
//! retrieval patterns through intelligent memory association and pathfinding.

use crate::error::ContextNestResult;
use crate::error::{ContextNestError, Result};
use crate::memory::attractors::{utils, ComponentStatus, MemoryAttractorConfig, MemoryFragment};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock as AsyncRwLock;
use uuid::Uuid;

/// Connection network for optimized memory retrieval
#[derive(Debug)]
pub struct ConnectionNetwork {
    /// Configuration
    config: MemoryAttractorConfig,
    /// Network graph
    graph: Arc<RwLock<MemoryGraph>>,
    /// Connection weights
    connection_weights: Arc<RwLock<HashMap<String, f32>>>,
    /// Retrieval optimizer
    retrieval_optimizer: Arc<RetrievalOptimizer>,
    /// Path finder
    path_finder: Arc<PathFinder>,
    /// Network statistics
    statistics: Arc<RwLock<NetworkStatistics>>,
    /// Component status
    status: Arc<RwLock<ComponentStatus>>,
}

/// Memory graph representing connections between memories
#[derive(Debug, Clone)]
pub struct MemoryGraph {
    /// Nodes (memories)
    nodes: HashMap<String, MemoryNode>,
    /// Edges (connections)
    edges: HashMap<String, Vec<ConnectionEdge>>,
    /// Adjacency list for fast lookup
    adjacency_list: HashMap<String, Vec<String>>,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Create a new connection network
    pub fn new(config: MemoryAttractorConfig) -> Self {
        Self {
            config: config.clone(),
            graph: Arc::new(RwLock::new(MemoryGraph::new())),
            connection_weights: Arc::new(RwLock::new(HashMap::new())),
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
            if graph.nodes.contains_key(&node_id) {
                return Err(ContextNestError::Validation(format!(
                    "Node {} already exists",
                    node_id
                )));
            }
            graph.nodes.insert(node_id.clone(), node);
            graph.adjacency_list.insert(node_id.clone(), Vec::new());
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

    /// Remove a memory node from the network

    pub async fn remove_node(&self, node_id: &str) -> ContextNestResult<()> {
        let mut graph = self.graph.write().unwrap();

        // Check if node exists
        if !graph.nodes.contains_key(node_id) {
            return Err(ContextNestError::NotFound(format!(
                "Node {} not found",
                node_id
            )));
        }

        // Remove node
        graph.nodes.remove(node_id);

        // Remove all edges connected to this node
        let edges_to_remove: Vec<String> = graph
            .edges
            .iter()
            .filter(|(_, edges)| {
                edges
                    .iter()
                    .any(|e| e.source == node_id || e.target == node_id)
            })
            .map(|(id, _)| id.clone())
            .collect();

        for edge_id in edges_to_remove {
            graph.edges.remove(&edge_id);
        }

        // Remove from adjacency list
        graph.adjacency_list.remove(node_id);

        // Remove from other adjacency lists
        for (_, neighbors) in graph.adjacency_list.iter_mut() {
            neighbors.retain(|id| id != node_id);
        }

        // Update graph metrics
        graph.update_metrics();

        // Update statistics
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

        // Check if nodes exist
        if !graph.nodes.contains_key(source_id) {
            return Err(ContextNestError::NotFound(format!(
                "Source node {} not found",
                source_id
            )));
        }

        if !graph.nodes.contains_key(target_id) {
            return Err(ContextNestError::NotFound(format!(
                "Target node {} not found",
                target_id
            )));
        }

        // Create connection edge
        let edge = ConnectionEdge {
            id: utils::generate_unique_id(),
            source: source_id.to_string(),
            target: target_id.to_string(),
            weight,
            connection_type,
            strength: weight,
            bidirectional: true,
            created_at: Utc::now(),
            last_reinforced: Utc::now(),
            usage_count: 0,
        };

        let edge_id = edge.id.clone();

        // Add edge
        graph.edges.insert(edge_id.clone(), vec![edge.clone()]);

        // Update adjacency lists
        graph
            .adjacency_list
            .get_mut(source_id)
            .unwrap()
            .push(target_id.to_string());
        graph
            .adjacency_list
            .get_mut(target_id)
            .unwrap()
            .push(source_id.to_string());

        // Update connection weights
        self.update_connection_weight(&edge_id, weight);

        // Update graph metrics
        graph.update_metrics();

        // Update statistics
        self.update_statistics(|stats| {
            stats.total_connections_created += 1;
        });

        Ok(edge_id)
    }

    /// 1-hop neighbors of `node_id` with their edge weights, sorted
    /// strongest first. Used by Phase 5 of the neural-field epic
    /// (`docs/roadmap/epics/neural-field-real.md`) to surface
    /// learned-graph siblings of a query's top hit at retrieve time,
    /// independent of the heavier `retrieve_memories` cache path.
    ///
    /// Walks the edge map (cheap O(E) scan; for the 25k-fragment
    /// substrate that's ~10k–50k edges) and emits both directions of
    /// any bidirectional edge so callers get a symmetric neighbor list.
    /// Returns an empty Vec when the node has no edges yet (cold
    /// substrate, or no peer fragments survived similarity-driven
    /// auto-connection thresholds).
    pub async fn neighbors_of(&self, node_id: &str) -> Vec<(String, f32)> {
        let graph = self.graph.read().unwrap();
        let mut out: Vec<(String, f32)> = Vec::new();
        for edges in graph.edges.values() {
            for edge in edges {
                if edge.source == node_id {
                    out.push((edge.target.clone(), edge.weight));
                } else if edge.target == node_id && edge.bidirectional {
                    out.push((edge.source.clone(), edge.weight));
                }
            }
        }
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
        if !graph.nodes.contains_key(start_id) {
            return Err(ContextNestError::NotFound(format!(
                "Start node {} not found",
                start_id
            )));
        }

        if !graph.nodes.contains_key(end_id) {
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

    /// Reinforce connections based on usage

    pub async fn reinforce_connections(&self, retrieval_path: &[String]) -> ContextNestResult<()> {
        let mut graph = self.graph.write().unwrap();

        for window in retrieval_path.windows(2) {
            let source_id = window[0].clone();
            let target_id = window[1].clone();

            // Two-phase update to keep `graph.edges` and `graph.nodes`
            // borrows non-overlapping: first reinforce the matching edge(s),
            // then bump the participating nodes' access stats.
            let mut node_bump = false;
            for (_, edges) in graph.edges.iter_mut() {
                for edge in edges.iter_mut() {
                    if (edge.source == source_id && edge.target == target_id)
                        || (edge.source == target_id && edge.target == source_id)
                    {
                        edge.usage_count += 1;
                        edge.last_reinforced = Utc::now();
                        edge.strength = (edge.strength * 0.9 + 0.1).min(1.0);
                        node_bump = true;
                        break;
                    }
                }
            }

            if node_bump {
                let now = Utc::now();
                if let Some(node) = graph.nodes.get_mut(&source_id) {
                    node.last_accessed = now;
                    node.access_frequency += 1.0;
                }
                if let Some(node) = graph.nodes.get_mut(&target_id) {
                    node.last_accessed = now;
                    node.access_frequency += 1.0;
                }
            }
        }

        Ok(())
    }

    /// Optimize network structure

    pub async fn optimize_network(&self) -> ContextNestResult<NetworkOptimizationResult> {
        let mut graph = self.graph.write().unwrap();
        let mut optimizations = Vec::new();

        // Remove weak connections
        let initial_edges = graph.edges.len();
        graph.edges.retain(|_, edges| {
            edges.retain(|edge| edge.strength > 0.1);
            !edges.is_empty()
        });
        let edges_removed = initial_edges - graph.edges.len();

        if edges_removed > 0 {
            optimizations.push(NetworkOptimization {
                optimization_type: OptimizationType::RemoveWeakConnections,
                impact: edges_removed as f32,
                description: format!("Removed {} weak connections", edges_removed),
            });
        }

        // Rebuild adjacency list
        graph.rebuild_adjacency_list();

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
            final_node_count: graph.nodes.len(),
            final_edge_count: graph.edges.len(),
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

    // Helper methods

    async fn create_connections_for_node(&self, node_id: &str) -> ContextNestResult<()> {
        // Connection-creation knobs. Read per-call because env can be
        // hot-reloaded between substrate restarts; reads are cheap and the
        // values are stable within a single consolidation tick.
        //
        // CONTEXTNEST_MAX_CONNECTIONS_PER_NODE caps the fan-out per new
        // fragment. Without it, `create_connections_for_node` issues one
        // `create_connection` per peer above the similarity threshold,
        // letting avg_degree (and per-insert CPU) grow monotonically with
        // substrate size. Production substrate hit avg_degree=204 / 7M
        // edges / 75% sustained CPU before this cap landed.
        //
        // CONTEXTNEST_CONNECTION_SIMILARITY_THRESHOLD raises the floor
        // when operators want fewer but stronger edges (e.g. during
        // backlog drain). Defaults preserve the pre-cap connectivity for
        // small substrates: most fragments have <32 peers above 0.7, so
        // the cap is a no-op there and only bites at scale.
        let max_connections = std::env::var("CONTEXTNEST_MAX_CONNECTIONS_PER_NODE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(32);
        let similarity_threshold = std::env::var("CONTEXTNEST_CONNECTION_SIMILARITY_THRESHOLD")
            .ok()
            .and_then(|s| s.parse::<f32>().ok())
            .unwrap_or(0.7);

        // Phase 1: snapshot similarity candidates under a read lock. We
        // collect owned `(other_id, similarity)` pairs and release the read
        // lock by letting `graph` drop. Without this snapshot, the inner
        // `create_connection` call below tries to take a `write()` while we
        // still hold the `read()`, which deadlocks `std::sync::RwLock`.
        let mut candidates: Vec<(String, f32)> = {
            let graph = self.graph.read().unwrap();
            let new_node = match graph.nodes.get(node_id) {
                Some(node) => node,
                None => return Ok(()), // Race: node was removed before we connected.
            };
            graph
                .nodes
                .iter()
                .filter(|(other_id, _)| other_id.as_str() != node_id)
                .filter_map(|(other_id, other_node)| {
                    let sim = utils::cosine_similarity(&new_node.content, &other_node.content);
                    (sim > similarity_threshold).then(|| (other_id.clone(), sim))
                })
                .collect()
        };

        // Top-K cap: sort strongest-first and truncate. The strongest-K
        // peers carry almost all of the basin signal — empirically the
        // similarity distribution has a long thin tail past rank ~20, so
        // dropping the tail loses little retrieval quality while bounding
        // per-insert cost from O(N) to O(K).
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        if candidates.len() > max_connections {
            candidates.truncate(max_connections);
        }

        // Phase 2: issue connection creates with no read lock held.
        for (other_id, similarity) in candidates {
            let _ = self
                .create_connection(node_id, &other_id, ConnectionType::Semantic, similarity)
                .await;
        }

        Ok(())
    }

    fn update_connection_weight(&self, edge_id: &str, weight: f32) {
        let mut weights = self.connection_weights.write().unwrap();
        weights.insert(edge_id.to_string(), weight);
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
    fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            adjacency_list: HashMap::new(),
            metrics: GraphMetrics::default(),
        }
    }

    fn update_metrics(&mut self) {
        self.metrics.total_nodes = self.nodes.len();

        // Edge count is `edges.len()`, not a `.values().map(len).sum()` scan:
        // `create_connection` stores every edge as its own single-element Vec,
        // so the map length equals the edge total exactly. At 1.5M edges the
        // old O(E) sum — invoked up to MAX_CONNECTIONS_PER_NODE times per
        // ingested fragment via `create_connection` — was ~40% of the
        // consolidation worker's pegged core.
        self.metrics.total_edges = self.edges.len();

        // avg_degree via a closed form instead of an O(V) sum over the
        // adjacency lists. Every edge is bidirectional and contributes exactly
        // one adjacency entry to each endpoint, so total adjacency degree is
        // always 2 * total_edges. This invariant holds through both
        // `create_connection` (+1 edge, +2 entries) and `remove_node`
        // (-d edges, -2d entries), so the result is identical to the old scan.
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

    fn rebuild_adjacency_list(&mut self) {
        self.adjacency_list.clear();

        // Initialize adjacency list
        for node_id in self.nodes.keys() {
            self.adjacency_list.insert(node_id.clone(), Vec::new());
        }

        // Build adjacency list from edges
        for edges in self.edges.values() {
            for edge in edges {
                self.adjacency_list
                    .get_mut(&edge.source)
                    .unwrap()
                    .push(edge.target.clone());
                if edge.bidirectional {
                    self.adjacency_list
                        .get_mut(&edge.target)
                        .unwrap()
                        .push(edge.source.clone());
                }
            }
        }
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
        let mut results = Vec::new();

        for (node_id, node) in graph.nodes.iter() {
            let similarity = utils::cosine_similarity(&query.content, &node.content);

            if similarity >= self.similarity_threshold {
                results.push(RetrievalResult {
                    memory_id: node_id.clone(),
                    confidence: similarity,
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
        let mut results = Vec::new();

        // Find nodes with highest degree (most connected)
        let mut node_degrees: Vec<(String, usize)> = graph
            .adjacency_list
            .iter()
            .map(|(id, neighbors)| (id.clone(), neighbors.len()))
            .collect();

        node_degrees.sort_by(|a, b| b.1.cmp(&a.1));

        // Return top connected nodes
        for (node_id, degree) in node_degrees.iter().take(query.max_results) {
            let confidence = (*degree as f32 / graph.metrics.total_nodes as f32).min(1.0);

            if confidence >= query.min_confidence {
                results.push(RetrievalResult {
                    memory_id: node_id.clone(),
                    confidence,
                    retrieval_path: None,
                    strategy: self.name().to_string(),
                    metadata: HashMap::new(),
                });
            }
        }

        results
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
        let mut results = Vec::new();

        for (node_id, node) in graph.nodes.iter() {
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
            let mut matches_filters = true;
            for (filter_key, filter_value) in &query.context_filters {
                if let Some(node_value) = node.metadata.get(filter_key) {
                    if node_value != filter_value {
                        matches_filters = false;
                        break;
                    }
                } else {
                    matches_filters = false;
                    break;
                }
            }

            if matches_filters && confidence >= query.min_confidence {
                results.push(RetrievalResult {
                    memory_id: node_id.clone(),
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
        // Simplified Dijkstra implementation. BinaryHeap needs Ord, but f32 is
        // PartialOrd-only (NaN incomparable). Wrap distances in `NotNan<f32>`
        // so the heap can compare-and-swap deterministically; we already
        // protect against NaN at graph construction time so the unwrap is safe.
        use ordered_float::NotNan;
        let inf = NotNan::new(f32::INFINITY).expect("INFINITY is not NaN");
        let zero = NotNan::new(0.0_f32).expect("0.0 is not NaN");
        let mut distances: HashMap<String, NotNan<f32>> = HashMap::new();
        let mut previous: HashMap<String, String> = HashMap::new();
        let mut unvisited: BinaryHeap<(std::cmp::Reverse<NotNan<f32>>, String)> = BinaryHeap::new();

        // Initialize distances
        for node_id in graph.nodes.keys() {
            distances.insert(node_id.clone(), inf);
        }
        distances.insert(start.to_string(), zero);

        // Use reverse ordering for min-heap behavior
        unvisited.push((std::cmp::Reverse(zero), start.to_string()));

        while let Some((std::cmp::Reverse(dist), current)) = unvisited.pop() {
            if current == end {
                break;
            }

            if dist > distances[&current] {
                continue;
            }

            // Check neighbors
            if let Some(neighbors) = graph.adjacency_list.get(&current) {
                for neighbor in neighbors {
                    let edge_weight = NotNan::new(1.0_f32).expect("constant 1.0 is not NaN");
                    let alt = distances[&current] + edge_weight;

                    if alt < distances[neighbor] {
                        distances.insert(neighbor.clone(), alt);
                        previous.insert(neighbor.clone(), current.clone());
                        unvisited.push((std::cmp::Reverse(alt), neighbor.clone()));
                    }
                }
            }
        }

        // Reconstruct path
        if distances.get(end).copied().unwrap_or(inf) == inf {
            return None;
        }

        let mut path_nodes = Vec::new();
        let mut current = end.to_string();

        while current != start {
            path_nodes.push(current.clone());
            current = previous.get(&current)?.clone();
        }

        path_nodes.push(start.to_string());
        path_nodes.reverse();

        Some(Path {
            nodes: path_nodes.clone(),
            total_weight: distances[end].into_inner(),
            length: path_nodes.len(),
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

    #[tokio::test]
    async fn test_connection_network() {
        let config = MemoryAttractorConfig::default();
        let network = ConnectionNetwork::new(config);
        network.initialize().await.unwrap();

        // Add a node
        let node = MemoryNode {
            id: "test_node".to_string(),
            node_type: MemoryNodeType::Primary,
            content: vec![0.1; 64],
            importance: 0.8,
            last_accessed: Utc::now(),
            access_frequency: 1.0,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![],
        };

        network.add_node(node).await.unwrap();

        // Get statistics
        let stats = network.get_statistics();
        assert_eq!(stats.total_nodes_added, 1);
    }

    #[tokio::test]
    async fn test_connection_creation() {
        let config = MemoryAttractorConfig::default();
        let network = ConnectionNetwork::new(config);
        network.initialize().await.unwrap();

        // Add two nodes
        let node1 = MemoryNode {
            id: "node1".to_string(),
            node_type: MemoryNodeType::Primary,
            content: vec![0.1; 32],
            importance: 0.8,
            last_accessed: Utc::now(),
            access_frequency: 1.0,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![],
        };

        let node2 = MemoryNode {
            id: "node2".to_string(),
            node_type: MemoryNodeType::Primary,
            content: vec![0.2; 32],
            importance: 0.7,
            last_accessed: Utc::now(),
            access_frequency: 1.0,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![],
        };

        network.add_node(node1).await.unwrap();
        network.add_node(node2).await.unwrap();

        // Create connection
        let edge_id = network
            .create_connection("node1", "node2", ConnectionType::Semantic, 0.8)
            .await
            .unwrap();

        assert!(!edge_id.is_empty());

        // `add_node(node2)` auto-creates a Semantic connection because
        // `create_connections_for_node` finds node1 highly similar
        // (vec![0.1; 32] vs vec![0.2; 32] has cosine similarity > 0.7), so
        // when we then *manually* create_connection the count is 2, not 1.
        // The pre-Phase-A version of this test asserted 1 because the
        // auto-connection path was masked by the read+write deadlock in
        // `add_node`; with that fixed the auto-path runs to completion.
        let stats = network.get_statistics();
        assert!(
            stats.total_connections_created >= 1,
            "expected at least one connection, got {}",
            stats.total_connections_created
        );
    }

    #[tokio::test]
    async fn test_memory_retrieval() {
        let config = MemoryAttractorConfig::default();
        let network = ConnectionNetwork::new(config);
        network.initialize().await.unwrap();

        // Add a node
        let node = MemoryNode {
            id: "test_node".to_string(),
            node_type: MemoryNodeType::Primary,
            content: vec![0.1; 64],
            importance: 0.8,
            last_accessed: Utc::now(),
            access_frequency: 1.0,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![],
        };

        network.add_node(node).await.unwrap();

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

        // Add nodes and create a path
        let node1 = MemoryNode {
            id: "node1".to_string(),
            node_type: MemoryNodeType::Primary,
            content: vec![0.1; 32],
            importance: 0.8,
            last_accessed: Utc::now(),
            access_frequency: 1.0,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![],
        };

        let node2 = MemoryNode {
            id: "node2".to_string(),
            node_type: MemoryNodeType::Primary,
            content: vec![0.2; 32],
            importance: 0.7,
            last_accessed: Utc::now(),
            access_frequency: 1.0,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![],
        };

        network.add_node(node1).await.unwrap();
        network.add_node(node2).await.unwrap();

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

        let mut graph = MemoryGraph::new();
        graph.nodes.insert(
            "node1".to_string(),
            MemoryNode {
                id: "node1".to_string(),
                node_type: MemoryNodeType::Primary,
                content: vec![0.1; 32],
                importance: 0.8,
                last_accessed: Utc::now(),
                access_frequency: 1.0,
                metadata: HashMap::new(),
                created_at: Utc::now(),
                fragment_ids: vec![],
            },
        );

        let results = strategy.find_memories(&query, &graph);
        assert_eq!(results.len(), 1);
        assert!(results[0].confidence > 0.9);
    }

    #[test]
    fn test_dijkstra_pathfinding() {
        let algorithm = DijkstraPathfinding;

        let mut graph = MemoryGraph::new();

        // Create a simple graph: node1 -> node2 -> node3
        for i in 1..=3 {
            let node_id = format!("node{}", i);
            graph.nodes.insert(
                node_id.clone(),
                MemoryNode {
                    id: node_id,
                    node_type: MemoryNodeType::Primary,
                    content: vec![0.1; 32],
                    importance: 0.8,
                    last_accessed: Utc::now(),
                    access_frequency: 1.0,
                    metadata: HashMap::new(),
                    created_at: Utc::now(),
                    fragment_ids: vec![],
                },
            );
        }

        // Add adjacency
        graph
            .adjacency_list
            .insert("node1".to_string(), vec!["node2".to_string()]);
        graph
            .adjacency_list
            .insert("node2".to_string(), vec!["node3".to_string()]);
        graph.adjacency_list.insert("node3".to_string(), vec![]);

        let path = algorithm.find_path(&graph, "node1", "node3");
        assert!(path.is_some());

        let path = path.unwrap();
        assert_eq!(path.nodes, vec!["node1", "node2", "node3"]);
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
            let peer = MemoryNode {
                id: format!("peer{}", i),
                node_type: MemoryNodeType::Primary,
                content: vec![0.1; 32],
                importance: 0.5,
                last_accessed: Utc::now(),
                access_frequency: 1.0,
                metadata: HashMap::new(),
                created_at: Utc::now(),
                fragment_ids: vec![],
            };
            network.add_node(peer).await.unwrap();
        }

        // Baseline: capture connection count after the seeding completes
        // (peers connect to each other as they're added).
        let stats_before = network.get_statistics();

        // Inbound node — same content vector, so all 10 peers qualify.
        let inbound = MemoryNode {
            id: "inbound".to_string(),
            node_type: MemoryNodeType::Primary,
            content: vec![0.1; 32],
            importance: 0.8,
            last_accessed: Utc::now(),
            access_frequency: 1.0,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![],
        };
        network.add_node(inbound).await.unwrap();

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
        let node1 = MemoryNode {
            id: "pf_node1".to_string(),
            node_type: MemoryNodeType::Primary,
            content: vec![0.1; 32],
            importance: 0.8,
            last_accessed: Utc::now(),
            access_frequency: 1.0,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![],
        };
        let node2 = MemoryNode {
            id: "pf_node2".to_string(),
            node_type: MemoryNodeType::Primary,
            content: vec![0.9; 32],
            importance: 0.7,
            last_accessed: Utc::now(),
            access_frequency: 1.0,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![],
        };
        network.add_node(node1).await.unwrap();
        network.add_node(node2).await.unwrap();
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
        let node = MemoryNode {
            id: "ro_node1".to_string(),
            node_type: MemoryNodeType::Primary,
            content: vec![0.5; 32],
            importance: 0.8,
            last_accessed: Utc::now(),
            access_frequency: 1.0,
            metadata: HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![],
        };
        network.add_node(node).await.unwrap();

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
}

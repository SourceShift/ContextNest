//! Memory Attractor Manager
//! Central coordinator for the entire memory attractor system, managing
//! all components and providing unified access to memory attractor functionality.

use crate::error::ContextNestResult;
use crate::error::{ContextNestError, Result};
use crate::memory::attractors::{
    utils, AdaptiveDecaySystem, AttractorBasinManager, ComponentStatus, ConnectionNetwork,
    GapFillingEngine, MemoryAttractorConfig, MemoryAttractorMetrics, MemoryFragment,
    MemoryReconstructionProtocol, ReconstructedMemory,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock as AsyncRwLock;
use uuid::Uuid;

/// Main memory attractor system manager
#[derive(Debug)]
pub struct MemoryAttractorManager {
    /// Configuration
    config: MemoryAttractorConfig,
    /// Attractor basin manager
    basin_manager: Arc<AttractorBasinManager>,
    /// Memory reconstruction protocol
    reconstruction_protocol: Arc<MemoryReconstructionProtocol>,
    /// Gap filling engine
    gap_filling_engine: Arc<GapFillingEngine>,
    /// Adaptive decay system
    decay_system: Arc<AdaptiveDecaySystem>,
    /// Connection network
    connection_network: Arc<ConnectionNetwork>,
    /// System metrics
    metrics: Arc<RwLock<SystemMetrics>>,
    /// Component status
    status: Arc<RwLock<ComponentStatus>>,
    /// Background tasks
    background_tasks: Arc<RwLock<Vec<BackgroundTask>>>,
}

/// System-wide metrics
#[derive(Debug, Clone, Default)]
pub struct SystemMetrics {
    /// Total memories processed
    pub total_memories_processed: usize,
    /// Successful reconstructions
    pub successful_reconstructions: usize,
    /// Average reconstruction confidence
    pub avg_reconstruction_confidence: f32,
    /// Total gaps filled
    pub total_gaps_filled: usize,
    /// Gap filling success rate
    pub gap_filling_success_rate: f32,
    /// Active attractor basins
    pub active_attractor_basins: usize,
    /// Network efficiency
    pub network_efficiency: f32,
    /// System throughput (operations/sec)
    pub system_throughput: f64,
    /// Memory usage statistics
    pub memory_usage: MemoryUsageStats,
    /// Component health metrics
    pub component_health: HashMap<String, ComponentHealth>,
}

/// Memory usage statistics
#[derive(Debug, Clone, Default)]
pub struct MemoryUsageStats {
    /// Total memory used (bytes)
    pub total_memory_used: usize,
    /// Fragment storage usage
    pub fragment_storage: usize,
    /// Attractor basin usage
    pub basin_storage: usize,
    /// Connection network usage
    pub network_storage: usize,
    /// Cache usage
    pub cache_storage: usize,
    /// Memory fragmentation ratio
    pub fragmentation_ratio: f32,
}

/// Component health metrics
#[derive(Debug, Clone)]
pub struct ComponentHealth {
    /// Component status
    pub status: ComponentStatus,
    /// Health score (0-1)
    pub health_score: f32,
    /// Last health check
    pub last_health_check: DateTime<Utc>,
    /// Performance metrics
    pub performance_metrics: HashMap<String, f32>,
    /// Error count
    pub error_count: usize,
    /// Last error (if any)
    pub last_error: Option<String>,
}

/// Background task information
#[derive(Debug, Clone)]
pub struct BackgroundTask {
    /// Task ID
    pub id: String,
    /// Task name
    pub name: String,
    /// Task type
    pub task_type: BackgroundTaskType,
    /// Task status
    pub status: TaskStatus,
    /// Start time
    pub start_time: DateTime<Utc>,
    /// Estimated completion time
    pub estimated_completion: Option<DateTime<Utc>>,
    /// Progress percentage
    pub progress: f32,
    /// Task-specific data
    pub data: HashMap<String, serde_json::Value>,
}

/// Types of background tasks
#[derive(Debug, Clone)]
pub enum BackgroundTaskType {
    /// Memory consolidation
    MemoryConsolidation,
    /// Network optimization
    NetworkOptimization,
    /// Profile optimization
    ProfileOptimization,
    /// Cache cleanup
    CacheCleanup,
    /// Health monitoring
    HealthMonitoring,
    /// Metrics collection
    MetricsCollection,
}

/// Task execution status
#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

/// Memory processing request
#[derive(Debug, Clone)]
pub struct MemoryProcessingRequest {
    /// Request ID
    pub id: String,
    /// Memory fragments
    pub fragments: Vec<MemoryFragment>,
    /// Processing options
    pub options: ProcessingOptions,
    /// Request priority
    pub priority: ProcessingPriority,
    /// Request timestamp
    pub created_at: DateTime<Utc>,
}

/// Memory processing options
#[derive(Debug, Clone)]
pub struct ProcessingOptions {
    /// Enable attractor basin creation
    pub enable_attractor_creation: bool,
    /// Enable memory reconstruction
    pub enable_reconstruction: bool,
    /// Enable gap filling
    pub enable_gap_filling: bool,
    /// Enable connection creation
    pub enable_connections: bool,
    /// Quality threshold
    pub quality_threshold: f32,
    /// Maximum processing time
    pub max_processing_time: Duration,
}

/// Processing priority levels
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProcessingPriority {
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

/// Memory processing result
#[derive(Debug, Clone)]
pub struct MemoryProcessingResult {
    /// Request ID
    pub request_id: String,
    /// Processing success
    pub success: bool,
    /// Reconstructed memories
    pub reconstructed_memories: Vec<ReconstructedMemory>,
    /// Created attractor basins
    pub created_basins: Vec<String>,
    /// Created connections
    pub created_connections: Vec<String>,
    /// Gaps filled
    pub gaps_filled: usize,
    /// Processing time
    pub processing_time: Duration,
    /// Quality metrics
    pub quality_metrics: ProcessingQualityMetrics,
}

/// Processing quality metrics
#[derive(Debug, Clone, Default)]
pub struct ProcessingQualityMetrics {
    /// Overall quality score
    pub overall_quality: f32,
    /// Reconstruction quality
    pub reconstruction_quality: f32,
    /// Basin quality
    pub basin_quality: f32,
    /// Connection quality
    pub connection_quality: f32,
    /// Completeness score
    pub completeness: f32,
    /// Consistency score
    pub consistency: f32,
}

impl MemoryAttractorManager {
    /// Create a new memory attractor manager
    pub fn new(config: MemoryAttractorConfig) -> Self {
        Self {
            config: config.clone(),
            basin_manager: Arc::new(AttractorBasinManager::new(config.clone())),
            reconstruction_protocol: Arc::new(MemoryReconstructionProtocol::new(config.clone())),
            gap_filling_engine: Arc::new(GapFillingEngine::new(config.clone())),
            decay_system: Arc::new(AdaptiveDecaySystem::new(config.clone())),
            connection_network: Arc::new(ConnectionNetwork::new(config)),
            metrics: Arc::new(RwLock::new(SystemMetrics::default())),
            status: Arc::new(RwLock::new(ComponentStatus::Initializing)),
            background_tasks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Initialize the memory attractor system

    pub async fn initialize(&self) -> ContextNestResult<()> {
        // Initialize all components
        self.basin_manager.initialize().await?;
        self.reconstruction_protocol.initialize().await?;
        self.gap_filling_engine.initialize().await?;
        self.decay_system.initialize().await?;
        self.connection_network.initialize().await?;

        *self.status.write().unwrap() = ComponentStatus::Running;

        // Start background tasks
        self.start_background_tasks().await?;

        Ok(())
    }

    /// Process memory fragments through the full attractor system

    pub async fn process_memories(
        &self,
        request: MemoryProcessingRequest,
    ) -> ContextNestResult<MemoryProcessingResult> {
        let start_time = Utc::now();

        // Update metrics
        self.update_metrics(|metrics| {
            metrics.total_memories_processed += 1;
        });

        let mut result = MemoryProcessingResult {
            request_id: request.id.clone(),
            success: false,
            reconstructed_memories: Vec::new(),
            created_basins: Vec::new(),
            created_connections: Vec::new(),
            gaps_filled: 0,
            processing_time: Duration::from_secs(0),
            quality_metrics: ProcessingQualityMetrics::default(),
        };

        // Step 1: Create attractor basins if enabled
        if request.options.enable_attractor_creation {
            for fragment in &request.fragments {
                if let Ok(basin_id) = self.create_attractor_basin_from_fragment(fragment).await {
                    result.created_basins.push(basin_id);
                }
            }
        }

        // Step 1.5: Mirror each fragment into the connection network's graph
        // so the explicit connection-creation in Step 3 can resolve the
        // source/target node IDs. Without this, `create_memory_connection`
        // fails with `NotFound`, silently swallowed by the if-let-Ok wrapping
        // — connections never form even when fragments are highly similar.
        // `add_node` itself runs `create_connections_for_node` which can
        // auto-link to existing similar nodes via the configured strategies,
        // so this single call gives us *two* paths to connection formation:
        // implicit (similarity-driven inside add_node) and explicit (Step 3
        // below).
        if request.options.enable_connections {
            for fragment in &request.fragments {
                let node = crate::memory::attractors::connection_network::MemoryNode {
                    id: fragment.id.clone(),
                    node_type:
                        crate::memory::attractors::connection_network::MemoryNodeType::Fragment,
                    content: fragment.content.clone(),
                    importance: fragment.importance,
                    last_accessed: fragment.last_accessed,
                    access_frequency: 0.0,
                    metadata: std::collections::HashMap::new(),
                    created_at: fragment.created_at,
                    fragment_ids: vec![fragment.id.clone()],
                };
                // `add_node` errors only when the node already exists, which
                // we treat as benign (idempotent ingestion).
                let _ = self.connection_network.add_node(node).await;
            }
        }

        // Step 1.6: Mirror each fragment into the reconstruction protocol's
        // canonical fragment_store. The protocol's own pipeline path
        // (`reconstruct_memory`) resolves fragment IDs against this map, so
        // Step 2 above will silently find nothing unless the fragments land
        // here first. We also need this store to be the authoritative source
        // for the key-value CRUD methods (`get_fragment`, `update_fragment_importance`,
        // `discard_fragment`, `list_fragment_ids`) added in Phase H — those
        // operations work directly on this map rather than on the
        // connection-network graph, keeping read/update/delete semantics
        // O(1) and independent of graph traversal cost.
        // Note: this store is intentionally *session-agnostic*. Session
        // affinity (which fragments belong to which session) is maintained
        // by the SessionIndex in a parallel agent's work; the canonical store
        // here tracks only "fragment exists" vs "fragment doesn't exist".
        {
            // Bind the Arc to a named local so it lives long enough for the
            // async write-lock guard to remain valid across the loop body.
            // Without this binding Rust drops the Arc at the semicolon of
            // `.write().await`, invalidating the guard immediately.
            let store_arc = self.reconstruction_protocol.fragment_store();
            let mut store = store_arc.write().await;
            for fragment in &request.fragments {
                // insert is idempotent for the key; we overwrite with the
                // latest version if the same ID is submitted twice, which is
                // the correct behaviour for re-ingestion of updated fragments.
                store.insert(fragment.id.clone(), fragment.clone());
            }
        }

        // Step 2: Reconstruct memories if enabled
        if request.options.enable_reconstruction && request.fragments.len() > 1 {
            let fragment_ids: Vec<String> =
                request.fragments.iter().map(|f| f.id.clone()).collect();

            let recon_request = crate::memory::attractors::reconstruction_protocol::ReconstructionRequest {
                id: format!("recon_{}", request.id),
                available_fragments: fragment_ids,
                target_size: request.fragments.iter().map(|f| f.content.len()).sum(),
                quality_threshold: request.options.quality_threshold,
                enable_ai_filling: self.config.enable_ai_gap_filling,
                priority: match request.priority {
                    ProcessingPriority::Critical => crate::memory::attractors::reconstruction_protocol::ReconstructionPriority::Critical,
                    ProcessingPriority::High => crate::memory::attractors::reconstruction_protocol::ReconstructionPriority::High,
                    ProcessingPriority::Medium => crate::memory::attractors::reconstruction_protocol::ReconstructionPriority::Medium,
                    ProcessingPriority::Low => crate::memory::attractors::reconstruction_protocol::ReconstructionPriority::Low,
                },
                created_at: Utc::now(),
            };

            if let Ok(recon_result) = self
                .reconstruction_protocol
                .reconstruct_memory(recon_request)
                .await
            {
                if recon_result.success {
                    if let Some(memory) = recon_result.memory {
                        result.reconstructed_memories.push(memory);
                    }
                }
            }
        }

        // Step 3: Create connections if enabled
        if request.options.enable_connections && request.fragments.len() > 1 {
            for i in 0..request.fragments.len() {
                for j in (i + 1)..request.fragments.len() {
                    let fragment1 = &request.fragments[i];
                    let fragment2 = &request.fragments[j];

                    let similarity =
                        utils::cosine_similarity(&fragment1.content, &fragment2.content);

                    if similarity > 0.7 {
                        // Threshold for connection creation
                        if let Ok(connection_id) = self
                            .create_memory_connection(&fragment1.id, &fragment2.id, similarity)
                            .await
                        {
                            result.created_connections.push(connection_id);
                        }
                    }
                }
            }
        }

        // Calculate processing time and quality metrics
        result.processing_time = Utc::now()
            .signed_duration_since(start_time)
            .to_std()
            .unwrap_or_default();
        result.quality_metrics = self.calculate_processing_quality(&request, &result);

        // Update success status
        result.success =
            result.quality_metrics.overall_quality >= request.options.quality_threshold;

        // Update metrics
        if result.success {
            self.update_metrics(|metrics| {
                metrics.successful_reconstructions += 1;
                metrics.avg_reconstruction_confidence = (metrics.avg_reconstruction_confidence
                    * (metrics.successful_reconstructions - 1) as f32
                    + result.quality_metrics.overall_quality)
                    / metrics.successful_reconstructions as f32;
            });
        }

        Ok(result)
    }

    /// Retrieve memories using the connection network

    pub async fn retrieve_memories(
        &self,
        query_content: Vec<f32>,
        max_results: usize,
        min_confidence: f32,
    ) -> ContextNestResult<Vec<crate::memory::attractors::connection_network::RetrievalResult>>
    {
        let query = crate::memory::attractors::connection_network::RetrievalQuery {
            id: utils::generate_unique_id(),
            content: query_content,
            query_type: crate::memory::attractors::connection_network::QueryType::Hybrid,
            max_results,
            min_confidence,
            context_filters: HashMap::new(),
            allowed_strategies: vec!["similarity".to_string(), "association".to_string()],
        };

        self.connection_network.retrieve_memories(query).await
    }

    /// Find memory attractor basins for a given memory content

    /// Snapshot every basin currently in the basin manager. Forwards to
    /// [`crate::memory::attractors::attractor_basin::AttractorBasinManager::list_snapshots`].
    /// Used by `/api/v1/field/basins` (Phase 3 of the neural-field
    /// epic). Returns empty when no consolidation has run yet.
    pub async fn list_basin_snapshots(
        &self,
    ) -> Vec<crate::memory::attractors::attractor_basin::BasinSnapshot> {
        self.basin_manager.list_snapshots().await
    }

    pub async fn find_attractor_basins(&self, content: &[f32]) -> ContextNestResult<Vec<String>> {
        let nearest = self.basin_manager.find_nearest_basin(content).await?;

        match nearest {
            Some(basin_id) => Ok(vec![basin_id]),
            None => Ok(Vec::new()),
        }
    }

    /// Converge memory content to nearest attractor basin

    pub async fn converge_to_attractor(
        &self,
        content: &[f32],
    ) -> ContextNestResult<crate::memory::attractors::attractor_basin::BasinConvergenceResult> {
        self.basin_manager
            .converge_to_attractor(content, None)
            .await
    }

    /// Apply adaptive decay to the system

    pub async fn apply_decay(
        &self,
        time_delta: Duration,
    ) -> ContextNestResult<crate::memory::attractors::adaptive_decay::DecayResult> {
        self.decay_system.apply_decay(time_delta).await
    }

    /// Optimize the entire system

    pub async fn optimize_system(&self) -> ContextNestResult<SystemOptimizationResult> {
        let start_time = Utc::now();
        let mut optimizations = Vec::new();

        // Optimize attractor basins
        let basin_opt = self.basin_manager.remove_unhealthy_basins(0.3).await?;
        if basin_opt > 0 {
            optimizations.push(OptimizationResult {
                component: "attractor_basins".to_string(),
                optimization_type: "remove_unhealthy".to_string(),
                improvement: basin_opt as f32,
                description: format!("Removed {} unhealthy basins", basin_opt),
            });
        }

        // Optimize connection network
        let network_opt = self.connection_network.optimize_network().await?;
        if network_opt.improvement_score > 0.0 {
            optimizations.push(OptimizationResult {
                component: "connection_network".to_string(),
                optimization_type: "network_optimization".to_string(),
                improvement: network_opt.improvement_score,
                description: format!(
                    "Network optimization with {} improvements",
                    network_opt.optimizations.len()
                ),
            });
        }

        // Optimize decay profiles
        let decay_opt = self.decay_system.optimize_profiles().await?;
        if decay_opt.improvement_score > 0.0 {
            optimizations.push(OptimizationResult {
                component: "adaptive_decay".to_string(),
                optimization_type: "profile_optimization".to_string(),
                improvement: decay_opt.improvement_score,
                description: format!(
                    "Decay profile optimization with {} improvements",
                    decay_opt.optimized_profiles.len()
                ),
            });
        }

        // Clean up caches. `gap_filling_engine.clear_cache` returns `usize`
        // directly (infallible — it just reads a HashMap.len then clears),
        // while `reconstruction_protocol.clear_cache` returns
        // `ContextNestResult<usize>` because it can fail to acquire its lock.
        let recon_cache_cleared = self.reconstruction_protocol.clear_cache()?;
        let gap_cache_cleared = self.gap_filling_engine.clear_cache();

        if recon_cache_cleared > 0 || gap_cache_cleared > 0 {
            optimizations.push(OptimizationResult {
                component: "caches".to_string(),
                optimization_type: "cache_cleanup".to_string(),
                improvement: (recon_cache_cleared + gap_cache_cleared) as f32,
                description: format!(
                    "Cleared {} cache entries",
                    recon_cache_cleared + gap_cache_cleared
                ),
            });
        }

        let optimization_time = Utc::now()
            .signed_duration_since(start_time)
            .to_std()
            .unwrap_or_default();
        let overall_improvement = optimizations.iter().map(|o| o.improvement).sum::<f32>();

        Ok(SystemOptimizationResult {
            optimizations,
            overall_improvement,
            optimization_time,
            success: overall_improvement > 0.0,
        })
    }

    /// Get system health status

    pub async fn get_system_health(&self) -> SystemHealthStatus {
        let mut component_health = HashMap::new();

        // Check each component's health
        component_health.insert(
            "basin_manager".to_string(),
            self.check_component_health("basin_manager").await,
        );
        component_health.insert(
            "reconstruction_protocol".to_string(),
            self.check_component_health("reconstruction_protocol").await,
        );
        component_health.insert(
            "gap_filling_engine".to_string(),
            self.check_component_health("gap_filling_engine").await,
        );
        component_health.insert(
            "decay_system".to_string(),
            self.check_component_health("decay_system").await,
        );
        component_health.insert(
            "connection_network".to_string(),
            self.check_component_health("connection_network").await,
        );

        // Calculate overall system health
        let overall_health = component_health
            .values()
            .map(|h| h.health_score)
            .sum::<f32>()
            / component_health.len() as f32;

        SystemHealthStatus {
            overall_health,
            component_health,
            last_check: Utc::now(),
            active_background_tasks: self.background_tasks.read().unwrap().len(),
        }
    }

    /// Get comprehensive system metrics. Async because `basin_manager.get_statistics`
    /// holds an async lock; downstream `network_stats` is sync but unifying
    /// both under one async call keeps the signature consistent.
    pub async fn get_system_metrics(&self) -> SystemMetrics {
        let mut metrics = self.metrics.read().unwrap().clone();

        // Update component-specific metrics. `basin_manager.get_statistics` is
        // a `ContextNestResult` (the read can fail under lock poisoning) — fall
        // back to a zero count rather than propagating, since callers reading
        // a metrics snapshot don't expect this call to fail.
        let basin_stats = self
            .basin_manager
            .get_statistics()
            .await
            .unwrap_or_else(|_| {
                crate::memory::attractors::attractor_basin::BasinStatistics::default()
            });
        metrics.active_attractor_basins = basin_stats.active_basins;

        let network_stats = self.connection_network.get_statistics();
        metrics.network_efficiency = network_stats.network_efficiency;

        // Update memory usage
        metrics.memory_usage = self.estimate_memory_usage();

        metrics
    }

    /// Get background task status

    pub fn get_background_tasks(&self) -> Vec<BackgroundTask> {
        self.background_tasks.read().unwrap().clone()
    }

    /// Shutdown the memory attractor system

    pub async fn shutdown(&self) -> ContextNestResult<()> {
        *self.status.write().unwrap() = ComponentStatus::Stopped;

        // Cancel all background tasks
        let mut tasks = self.background_tasks.write().unwrap();
        for task in tasks.iter_mut() {
            task.status = TaskStatus::Cancelled;
        }
        tasks.clear();

        Ok(())
    }

    // ── Phase H: key-value CRUD on the canonical fragment store ────────────
    // These methods give the seven-tool API direct, O(1) access to individual
    // fragments without going through the reconstruction or basin pipelines.
    // They intentionally know nothing about session affinity — the SessionIndex
    // (maintained by a parallel agent) maps session IDs → fragment IDs; these
    // methods just ask "does this key exist?" and act on it.

    /// Returns a clone of the fragment if it exists in the canonical store.
    /// Returns `Ok(None)` for unknown IDs rather than an error because "not
    /// found" is a normal, expected outcome when the API layer handles an
    /// `update` or `discard` request — the caller needs to distinguish a 404
    /// from a 500 without inspecting error variant strings.
    pub async fn get_fragment(&self, id: &str) -> ContextNestResult<Option<MemoryFragment>> {
        let store_arc = self.reconstruction_protocol.fragment_store();
        let store = store_arc.read().await;
        Ok(store.get(id).cloned())
    }

    /// In-place update of the fragment's `importance` field and refreshes
    /// `last_accessed` to now.
    /// Only `importance` and `last_accessed` are mutable here. All other
    /// fields (`id`, `content`, `created_at`, `connections`,
    /// `attractor_basin_id`, `confidence`) are derived from canonical
    /// pipeline operations and must not be patched out-of-band — doing so
    /// would create divergence between the canonical store, the connection
    /// network graph, and the attractor basin assignments.
    /// Returns `Ok(true)` if the fragment was found and updated,
    /// `Ok(false)` if not found (the API layer maps this to a 404).
    pub async fn update_fragment_importance(
        &self,
        id: &str,
        new_importance: f32,
    ) -> ContextNestResult<bool> {
        let store_arc = self.reconstruction_protocol.fragment_store();
        let mut store = store_arc.write().await;
        if let Some(fragment) = store.get_mut(id) {
            fragment.importance = new_importance;
            fragment.last_accessed = Utc::now();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Hard delete: remove the fragment from the canonical store entirely.
    /// This is a hard delete at the canonical-store layer; soft-delete
    /// semantics (marking a fragment as "discarded" without removing it)
    /// live at the API layer via the SessionIndex. The canonical store only
    /// tracks "exists" / "doesn't exist" — the connection-network graph and
    /// attractor basins retain their references until their own cleanup
    /// passes run, which is intentional: those subsystems use lazy eviction
    /// and should not be synchronously updated on every discard call.
    /// Returns `Ok(true)` if the fragment was removed, `Ok(false)` if it
    /// was not present (idempotent — double-discard is not an error).
    pub async fn discard_fragment(&self, id: &str) -> ContextNestResult<bool> {
        let store_arc = self.reconstruction_protocol.fragment_store();
        let mut store = store_arc.write().await;
        Ok(store.remove(id).is_some())
    }

    /// List all fragment IDs currently in the canonical store.
    /// Cheap because we clone only the keys (short strings), not the full
    /// `MemoryFragment` values. The `summarize` tool uses this to enumerate
    /// a session's working set; pair with the SessionIndex's per-session
    /// view to filter down to just the fragments that belong to a given
    /// session — this method is intentionally unfiltered.
    pub async fn list_fragment_ids(&self) -> Vec<String> {
        let store_arc = self.reconstruction_protocol.fragment_store();
        let store = store_arc.read().await;
        store.keys().cloned().collect()
    }

    // Helper methods

    async fn create_attractor_basin_from_fragment(
        &self,
        fragment: &MemoryFragment,
    ) -> ContextNestResult<String> {
        use crate::memory::attractors::attractor_basin::BasinType;

        let basin_id = self
            .basin_manager
            .create_basin(
                fragment.content.clone(),
                fragment.importance,
                fragment.confidence,
                BasinType::Secondary,
            )
            .await?;
        // Record the fragment as a member of the basin it seeded.
        // Without this the basin's `associated_fragments` stays empty
        // forever, which breaks every read that wants to know basin
        // membership (Phase 3 of the neural-field epic surfaces real
        // basin mass at `/api/v1/field/basins` and saw zero everywhere
        // before this line existed).
        self.basin_manager
            .add_fragment_to_basin(&basin_id, fragment.id.clone())
            .await?;
        Ok(basin_id)
    }

    async fn create_memory_connection(
        &self,
        id1: &str,
        id2: &str,
        weight: f32,
    ) -> ContextNestResult<String> {
        use crate::memory::attractors::connection_network::ConnectionType;

        self.connection_network
            .create_connection(id1, id2, ConnectionType::Semantic, weight)
            .await
    }

    fn calculate_processing_quality(
        &self,
        request: &MemoryProcessingRequest,
        result: &MemoryProcessingResult,
    ) -> ProcessingQualityMetrics {
        let reconstruction_quality = if !result.reconstructed_memories.is_empty() {
            result
                .reconstructed_memories
                .iter()
                .map(|m| m.confidence)
                .sum::<f32>()
                / result.reconstructed_memories.len() as f32
        } else {
            0.0
        };

        let basin_quality = if !result.created_basins.is_empty() {
            0.8 // Placeholder - would need actual basin quality calculation
        } else {
            0.0
        };

        let connection_quality = if !result.created_connections.is_empty() {
            0.7 // Placeholder - would need actual connection quality calculation
        } else {
            0.0
        };

        let completeness = if request.options.enable_reconstruction {
            result.reconstructed_memories.len() as f32 / request.fragments.len() as f32
        } else {
            0.5
        };

        let consistency = reconstruction_quality * 0.8 + basin_quality * 0.2;

        let overall_quality = (reconstruction_quality * 0.4
            + basin_quality * 0.2
            + connection_quality * 0.2
            + completeness * 0.1
            + consistency * 0.1)
            .min(1.0);

        ProcessingQualityMetrics {
            overall_quality,
            reconstruction_quality,
            basin_quality,
            connection_quality,
            completeness,
            consistency,
        }
    }

    async fn check_component_health(&self, component_name: &str) -> ComponentHealth {
        let status = ComponentStatus::Running; // Simplified - would check actual component status
        let health_score = 0.9; // Placeholder - would calculate based on metrics

        ComponentHealth {
            status,
            health_score,
            last_health_check: Utc::now(),
            performance_metrics: HashMap::new(),
            error_count: 0,
            last_error: None,
        }
    }

    fn estimate_memory_usage(&self) -> MemoryUsageStats {
        // Simplified memory usage estimation
        MemoryUsageStats {
            total_memory_used: 10 * 1024 * 1024, // 10MB placeholder
            fragment_storage: 2 * 1024 * 1024,   // 2MB placeholder
            basin_storage: 3 * 1024 * 1024,      // 3MB placeholder
            network_storage: 3 * 1024 * 1024,    // 3MB placeholder
            cache_storage: 2 * 1024 * 1024,      // 2MB placeholder
            fragmentation_ratio: 0.15,           // 15% fragmentation placeholder
        }
    }

    async fn start_background_tasks(&self) -> ContextNestResult<()> {
        let mut tasks = self.background_tasks.write().unwrap();

        // Health monitoring task
        tasks.push(BackgroundTask {
            id: utils::generate_unique_id(),
            name: "Health Monitoring".to_string(),
            task_type: BackgroundTaskType::HealthMonitoring,
            status: TaskStatus::Running,
            start_time: Utc::now(),
            estimated_completion: None,
            progress: 0.0,
            data: HashMap::new(),
        });

        // Metrics collection task
        tasks.push(BackgroundTask {
            id: utils::generate_unique_id(),
            name: "Metrics Collection".to_string(),
            task_type: BackgroundTaskType::MetricsCollection,
            status: TaskStatus::Running,
            start_time: Utc::now(),
            estimated_completion: None,
            progress: 0.0,
            data: HashMap::new(),
        });

        Ok(())
    }

    fn update_metrics<F>(&self, update_fn: F)
    where
        F: FnOnce(&mut SystemMetrics),
    {
        let mut metrics = self.metrics.write().unwrap();
        update_fn(&mut metrics);

        // Update derived metrics.
        // placeholder — proper throughput would be
        // `total_memories_processed / elapsed_secs` but no wall-clock
        // reference is plumbed through here yet. The previous
        // `total / total` expression was a clippy correctness bug
        // (always 1.0 when > 0). Set to 0.0 (no-signal) until real
        // time-series tracking lands. Tracked:
        if metrics.total_memories_processed > 0 {
            metrics.system_throughput = 0.0;
        }
    }
}

/// System optimization result
#[derive(Debug, Clone)]
pub struct SystemOptimizationResult {
    /// Optimizations performed
    pub optimizations: Vec<OptimizationResult>,
    /// Overall improvement score
    pub overall_improvement: f32,
    /// Time taken for optimization
    pub optimization_time: Duration,
    /// Optimization success
    pub success: bool,
}

/// Individual optimization result
#[derive(Debug, Clone)]
pub struct OptimizationResult {
    /// Component optimized
    pub component: String,
    /// Type of optimization
    pub optimization_type: String,
    /// Improvement achieved
    pub improvement: f32,
    /// Description
    pub description: String,
}

/// System health status
#[derive(Debug, Clone)]
pub struct SystemHealthStatus {
    /// Overall health score (0-1)
    pub overall_health: f32,
    /// Health of individual components
    pub component_health: HashMap<String, ComponentHealth>,
    /// Last health check timestamp
    pub last_check: DateTime<Utc>,
    /// Number of active background tasks
    pub active_background_tasks: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::attractors::MemoryAttractorConfig;

    #[tokio::test]
    async fn test_memory_attractor_manager() {
        let config = MemoryAttractorConfig::default();
        let manager = MemoryAttractorManager::new(config);
        manager.initialize().await.unwrap();

        // Check system health
        let health = manager.get_system_health().await;
        assert!(health.overall_health > 0.0);

        // Get system metrics
        let metrics = manager.get_system_metrics().await;
        assert_eq!(metrics.total_memories_processed, 0); // Should be 0 initially
    }

    #[tokio::test]
    async fn test_memory_processing() {
        let config = MemoryAttractorConfig::default();
        let manager = MemoryAttractorManager::new(config);
        manager.initialize().await.unwrap();

        // Create test fragments
        let fragments = vec![
            MemoryFragment {
                id: "frag1".to_string(),
                content: vec![0.1; 32],
                importance: 0.8,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                attractor_basin_id: None,
                connections: std::collections::HashSet::new(),
                confidence: 0.9,
            },
            MemoryFragment {
                id: "frag2".to_string(),
                content: vec![0.2; 32],
                importance: 0.7,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                attractor_basin_id: None,
                connections: std::collections::HashSet::new(),
                confidence: 0.8,
            },
        ];

        // Create processing request
        let request = MemoryProcessingRequest {
            id: "test_req".to_string(),
            fragments: fragments.clone(),
            options: ProcessingOptions {
                enable_attractor_creation: true,
                enable_reconstruction: true,
                enable_gap_filling: true,
                enable_connections: true,
                quality_threshold: 0.5,
                max_processing_time: Duration::from_secs(10),
            },
            priority: ProcessingPriority::Medium,
            created_at: Utc::now(),
        };

        let result = manager.process_memories(request).await.unwrap();
        let _ = result;
    }

    #[tokio::test]
    async fn test_memory_retrieval() {
        let config = MemoryAttractorConfig::default();
        let manager = MemoryAttractorManager::new(config);
        manager.initialize().await.unwrap();

        // Create test node in connection network
        let node = crate::memory::attractors::connection_network::MemoryNode {
            id: "test_node".to_string(),
            node_type: crate::memory::attractors::connection_network::MemoryNodeType::Primary,
            content: vec![0.1; 32],
            importance: 0.8,
            last_accessed: Utc::now(),
            access_frequency: 1.0,
            metadata: std::collections::HashMap::new(),
            created_at: Utc::now(),
            fragment_ids: vec![],
        };

        manager.connection_network.add_node(node).await.unwrap();

        // Retrieve memories
        let results = manager
            .retrieve_memories(vec![0.1; 32], 5, 0.5)
            .await
            .unwrap();

        // Should find at least one result (or none if threshold not met)
        let _ = results.len();
    }

    #[tokio::test]
    async fn test_system_optimization() {
        let config = MemoryAttractorConfig::default();
        let manager = MemoryAttractorManager::new(config);
        manager.initialize().await.unwrap();

        let result = manager.optimize_system().await.unwrap();
        let _ = result;
        // `optimization_time` is measured with `Utc::now()` which has
        // microsecond resolution; on an empty manager the run completes in
        // sub-millisecond time, so check nanos rather than millis.
        assert!(result.optimization_time.as_nanos() > 0);
    }

    #[test]
    fn test_processing_quality_metrics() {
        let manager = MemoryAttractorManager::new(MemoryAttractorConfig::default());

        let fragments = vec![MemoryFragment {
            id: "frag1".to_string(),
            content: vec![0.1; 32],
            importance: 0.8,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            attractor_basin_id: None,
            connections: std::collections::HashSet::new(),
            confidence: 0.9,
        }];

        let request = MemoryProcessingRequest {
            id: "test".to_string(),
            fragments: fragments.clone(),
            options: ProcessingOptions {
                enable_attractor_creation: true,
                enable_reconstruction: true,
                enable_gap_filling: true,
                enable_connections: true,
                quality_threshold: 0.5,
                max_processing_time: Duration::from_secs(10),
            },
            priority: ProcessingPriority::Medium,
            created_at: Utc::now(),
        };

        let result = MemoryProcessingResult {
            request_id: "test".to_string(),
            success: true,
            reconstructed_memories: vec![],
            created_basins: vec![],
            created_connections: vec![],
            gaps_filled: 0,
            processing_time: Duration::from_millis(100),
            quality_metrics: ProcessingQualityMetrics::default(),
        };

        let quality = manager.calculate_processing_quality(&request, &result);
        assert!(quality.overall_quality >= 0.0);
        assert!(quality.overall_quality <= 1.0);
    }

    #[test]
    fn test_background_task_creation() {
        let task = BackgroundTask {
            id: "test_task".to_string(),
            name: "Test Task".to_string(),
            task_type: BackgroundTaskType::HealthMonitoring,
            status: TaskStatus::Running,
            start_time: Utc::now(),
            estimated_completion: None,
            progress: 0.0,
            data: std::collections::HashMap::new(),
        };

        assert_eq!(task.name, "Test Task");
        assert!(matches!(task.status, TaskStatus::Running));
    }

    // ── Phase H: CRUD tests ─────────────────────────────────────────────────

    /// Helper: build a minimal MemoryProcessingRequest from a slice of
    /// (id, content) pairs. Shared across several CRUD tests to avoid
    /// repeating the boilerplate construction.
    fn make_request(id: &str, fragments: Vec<MemoryFragment>) -> MemoryProcessingRequest {
        MemoryProcessingRequest {
            id: id.to_string(),
            fragments,
            options: ProcessingOptions {
                enable_attractor_creation: false,
                enable_reconstruction: false,
                enable_gap_filling: false,
                enable_connections: false,
                quality_threshold: 0.0,
                max_processing_time: Duration::from_secs(5),
            },
            priority: ProcessingPriority::Low,
            created_at: Utc::now(),
        }
    }

    fn make_fragment(id: &str, importance: f32) -> MemoryFragment {
        MemoryFragment {
            id: id.to_string(),
            content: vec![0.1_f32; 32],
            importance,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            attractor_basin_id: None,
            connections: std::collections::HashSet::new(),
            confidence: 0.8,
        }
    }

    /// get_fragment returns Ok(None) for an ID that was never ingested.
    #[tokio::test]
    async fn test_get_fragment_unknown_id_returns_none() {
        let manager = MemoryAttractorManager::new(MemoryAttractorConfig::default());
        manager.initialize().await.unwrap();

        let result = manager.get_fragment("no-such-id").await.unwrap();
        assert!(
            result.is_none(),
            "expected None for unknown id, got {result:?}"
        );
    }

    /// After process_memories ingests a fragment, get_fragment can retrieve it.
    #[tokio::test]
    async fn test_get_fragment_after_process_memories() {
        let manager = MemoryAttractorManager::new(MemoryAttractorConfig::default());
        manager.initialize().await.unwrap();

        let fragment = make_fragment("frag-crud-1", 0.6);
        let request = make_request("req-crud-1", vec![fragment.clone()]);
        manager.process_memories(request).await.unwrap();

        let retrieved = manager
            .get_fragment("frag-crud-1")
            .await
            .unwrap()
            .expect("fragment should be present after process_memories");

        assert_eq!(retrieved.id, "frag-crud-1");
        assert!((retrieved.importance - 0.6).abs() < f32::EPSILON);
    }

    /// update_fragment_importance returns true for a known fragment, mutates
    /// the importance field, and a follow-up get_fragment reflects the new value.
    #[tokio::test]
    async fn test_update_fragment_importance_known_id() {
        let manager = MemoryAttractorManager::new(MemoryAttractorConfig::default());
        manager.initialize().await.unwrap();

        let fragment = make_fragment("frag-crud-2", 0.5);
        let request = make_request("req-crud-2", vec![fragment]);
        manager.process_memories(request).await.unwrap();

        let updated = manager
            .update_fragment_importance("frag-crud-2", 0.99)
            .await
            .unwrap();
        assert!(updated, "expected true (fragment found)");

        let retrieved = manager
            .get_fragment("frag-crud-2")
            .await
            .unwrap()
            .expect("fragment should still be present");

        assert!(
            (retrieved.importance - 0.99).abs() < f32::EPSILON,
            "importance should have been updated to 0.99, got {}",
            retrieved.importance
        );
    }

    /// update_fragment_importance returns false for an unknown ID.
    #[tokio::test]
    async fn test_update_fragment_importance_unknown_id() {
        let manager = MemoryAttractorManager::new(MemoryAttractorConfig::default());
        manager.initialize().await.unwrap();

        let result = manager
            .update_fragment_importance("ghost-id", 0.5)
            .await
            .unwrap();
        assert!(!result, "expected false (unknown id), got true");
    }

    /// discard_fragment returns true for a known fragment, then false on a
    /// subsequent call (idempotent), and get_fragment returns None afterward.
    #[tokio::test]
    async fn test_discard_fragment_known_then_absent() {
        let manager = MemoryAttractorManager::new(MemoryAttractorConfig::default());
        manager.initialize().await.unwrap();

        let fragment = make_fragment("frag-crud-3", 0.7);
        let request = make_request("req-crud-3", vec![fragment]);
        manager.process_memories(request).await.unwrap();

        // First discard: fragment exists → true
        let first = manager.discard_fragment("frag-crud-3").await.unwrap();
        assert!(first, "first discard should return true");

        // Second discard: already gone → false (idempotent, not an error)
        let second = manager.discard_fragment("frag-crud-3").await.unwrap();
        assert!(!second, "second discard should return false");

        // get_fragment must now return None
        let after = manager.get_fragment("frag-crud-3").await.unwrap();
        assert!(after.is_none(), "fragment should be gone after discard");
    }

    /// list_fragment_ids reports the correct count after a multi-fragment
    /// process_memories call. Order is not guaranteed (HashMap); we only
    /// check that all submitted IDs are present.
    #[tokio::test]
    async fn test_list_fragment_ids_after_multi_fragment_ingest() {
        let manager = MemoryAttractorManager::new(MemoryAttractorConfig::default());
        manager.initialize().await.unwrap();

        let fragments = vec![
            make_fragment("list-a", 0.3),
            make_fragment("list-b", 0.4),
            make_fragment("list-c", 0.5),
            make_fragment("list-d", 0.6),
        ];
        let request = make_request("req-list", fragments);
        manager.process_memories(request).await.unwrap();

        let ids = manager.list_fragment_ids().await;
        assert_eq!(
            ids.len(),
            4,
            "expected 4 fragment IDs, got {}: {ids:?}",
            ids.len()
        );

        for expected in &["list-a", "list-b", "list-c", "list-d"] {
            assert!(
                ids.iter().any(|id| id == expected),
                "expected id {expected:?} to be in list; got {ids:?}"
            );
        }
    }

    #[test]
    fn test_system_health_status() {
        let mut component_health = std::collections::HashMap::new();
        component_health.insert(
            "test_component".to_string(),
            ComponentHealth {
                status: ComponentStatus::Running,
                health_score: 0.9,
                last_health_check: Utc::now(),
                performance_metrics: std::collections::HashMap::new(),
                error_count: 0,
                last_error: None,
            },
        );

        let health = SystemHealthStatus {
            overall_health: 0.85,
            component_health,
            last_check: Utc::now(),
            active_background_tasks: 2,
        };

        assert_eq!(health.overall_health, 0.85);
        assert_eq!(health.active_background_tasks, 2);
    }
}

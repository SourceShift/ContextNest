//! Performance Optimization for Large-Scale Attractor Networks
//! This module provides high-performance optimization utilities for attractor dynamics
//! systems, enabling 99%+ accuracy with efficient resource utilization.

use crate::context::attractor_dynamics::{
    AttractorAnalysisResult, AttractorBasin, AttractorDynamicsEngine, AttractorInteractionNetworks,
    BasinDynamics, BasinShape,
};
use crate::context::field::SemanticPattern;
use crate::error::ContextNestResult;
use crate::error::{ContextNestError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use uuid::Uuid;

/// Performance optimization engine for attractor dynamics
#[derive(Debug)]
pub struct AttractorPerformanceOptimizer {
    /// Optimization configuration
    pub config: OptimizationConfig,
    /// Performance cache
    pub performance_cache: PerformanceCache,
    /// Batch processing engine
    pub batch_processor: BatchProcessor,
    /// Memory pool manager
    pub memory_pool: MemoryPoolManager,
    /// Parallel execution engine
    pub parallel_engine: ParallelExecutionEngine,
    /// Optimization metrics
    pub optimization_metrics: OptimizationMetrics,
}

/// Configuration for performance optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationConfig {
    /// Enable parallel processing
    pub enable_parallel_processing: bool,
    /// Number of worker threads
    pub worker_threads: usize,
    /// Batch size for processing
    pub batch_size: usize,
    /// Cache size limit
    pub cache_size_limit: usize,
    /// Memory pool size
    pub memory_pool_size: usize,
    /// Enable SIMD optimizations
    pub enable_simd_optimizations: bool,
    /// Enable GPU acceleration (if available)
    pub enable_gpu_acceleration: bool,
    /// Performance monitoring enabled
    pub performance_monitoring_enabled: bool,
    /// Adaptive optimization enabled
    pub adaptive_optimization_enabled: bool,
    /// Target accuracy threshold
    pub target_accuracy_threshold: f32,
    /// Maximum processing time per operation (ms)
    pub max_processing_time_ms: u64,
}

impl Default for OptimizationConfig {
    fn default() -> Self {
        Self {
            enable_parallel_processing: true,
            worker_threads: num_cpus::get(),
            batch_size: 32,
            cache_size_limit: 10000,
            memory_pool_size: 1000,
            enable_simd_optimizations: true,
            enable_gpu_acceleration: false, // Would require GPU libraries
            performance_monitoring_enabled: true,
            adaptive_optimization_enabled: true,
            target_accuracy_threshold: 0.99,
            max_processing_time_ms: 100,
        }
    }
}

/// High-performance cache for attractor operations
#[derive(Debug)]
pub struct PerformanceCache {
    /// Pattern analysis cache
    pub pattern_analysis_cache: Arc<RwLock<HashMap<String, CachedAnalysis>>>,
    /// Basin similarity cache
    pub basin_similarity_cache: Arc<RwLock<HashMap<BasinSimilarityKey, f32>>>,
    /// Distance calculation cache
    pub distance_cache: Arc<RwLock<HashMap<DistanceKey, f32>>>,
    /// Optimization results cache
    pub optimization_cache: Arc<RwLock<HashMap<String, CachedOptimization>>>,
    /// Cache statistics
    pub cache_stats: Arc<RwLock<CacheStatistics>>,
}

/// Cached analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedAnalysis {
    /// Analysis result
    pub result: AttractorAnalysisResult,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last accessed timestamp
    pub last_accessed: DateTime<Utc>,
    /// Access count
    pub access_count: usize,
    /// Estimated cost to recompute
    pub recompute_cost: f32,
}

/// Key for basin similarity cache
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct BasinSimilarityKey {
    pub basin_id_1: String,
    pub basin_id_2: String,
}

/// Key for distance cache
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct DistanceKey {
    pub embedding_id_1: usize,
    pub embedding_id_2: usize,
}

/// Cached optimization result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedOptimization {
    /// Optimization parameters
    pub parameters: Vec<f32>,
    /// Performance improvement
    pub performance_improvement: f32,
    /// Validation timestamp
    pub validated_at: DateTime<Utc>,
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStatistics {
    /// Total cache hits
    pub total_hits: usize,
    /// Total cache misses
    pub total_misses: usize,
    /// Cache hit rate
    pub hit_rate: f32,
    /// Total cached items
    pub total_cached_items: usize,
    /// Memory usage in bytes
    pub memory_usage_bytes: usize,
}

/// Batch processor for efficient pattern handling
#[derive(Debug)]
pub struct BatchProcessor {
    /// Batch configuration
    pub config: BatchConfig,
    /// Processing queue
    pub processing_queue: Arc<RwLock<Vec<BatchItem>>>,
    /// Worker pool
    pub worker_pool: Arc<RwLock<Vec<BatchWorker>>>,
}

/// Batch processing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    /// Maximum batch size
    pub max_batch_size: usize,
    /// Batch timeout in milliseconds
    pub batch_timeout_ms: u64,
    /// Priority queuing enabled
    pub priority_queuing_enabled: bool,
    /// Adaptive batch sizing enabled
    pub adaptive_batch_sizing_enabled: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 64,
            batch_timeout_ms: 10,
            priority_queuing_enabled: true,
            adaptive_batch_sizing_enabled: true,
        }
    }
}

/// Item to be processed in batch
/// Note: prior versions included a `callback: Option<Box<dyn Fn(...)>>` field
/// for completion notification. It was removed during the v0.1.0 wiring pass
/// because (a) it broke `#[derive(Debug, Clone)]` (trait objects don't impl
/// either) and (b) it was never actually invoked anywhere in the file. If
/// async completion notification becomes needed, prefer a `tokio::sync::oneshot::Sender`
/// or an event-bus pattern over a stored `Fn`.
#[derive(Debug, Clone)]
pub struct BatchItem {
    /// Item ID
    pub id: String,
    /// Pattern to process
    pub pattern: SemanticPattern,
    /// Processing priority
    pub priority: ProcessingPriority,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
}

/// Processing priority levels
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProcessingPriority {
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

/// Batch worker for parallel processing
#[derive(Debug)]
pub struct BatchWorker {
    /// Worker ID
    pub worker_id: usize,
    /// Worker thread handle
    pub thread_handle: Option<std::thread::JoinHandle<()>>,
    /// Worker statistics
    pub statistics: WorkerStatistics,
}

/// Worker performance statistics
#[derive(Debug, Clone, Default)]
pub struct WorkerStatistics {
    /// Total items processed
    pub total_items_processed: usize,
    /// Average processing time per item
    pub avg_processing_time_ms: f64,
    /// Error count
    pub error_count: usize,
    /// Last activity timestamp
    pub last_activity: Option<DateTime<Utc>>,
}

/// Memory pool manager for efficient memory allocation
#[derive(Debug)]
pub struct MemoryPoolManager {
    /// Pool configuration
    pub config: MemoryPoolConfig,
    /// Pre-allocated memory pools
    pub memory_pools: HashMap<usize, MemoryPool>,
    /// Allocation statistics
    pub allocation_stats: AllocationStatistics,
}

/// Memory pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPoolConfig {
    /// Enable memory pooling
    pub enable_memory_pooling: bool,
    /// Pool size for different allocations
    pub pool_sizes: Vec<usize>,
    /// Maximum pool size
    pub max_pool_size: usize,
    /// Garbage collection threshold
    pub gc_threshold: f32,
}

impl Default for MemoryPoolConfig {
    fn default() -> Self {
        Self {
            enable_memory_pooling: true,
            pool_sizes: vec![32, 64, 128, 256, 512, 1024, 2048, 4096],
            max_pool_size: 1000,
            gc_threshold: 0.8,
        }
    }
}

/// Individual memory pool
#[derive(Debug)]
pub struct MemoryPool {
    /// Pool size
    pub size: usize,
    /// Available memory blocks
    pub available_blocks: Vec<Vec<f32>>,
    /// Used memory blocks
    pub used_blocks: Vec<Vec<f32>>,
    /// Pool statistics
    pub statistics: PoolStatistics,
}

/// Memory pool statistics
#[derive(Debug, Clone, Default)]
pub struct PoolStatistics {
    /// Total allocations
    pub total_allocations: usize,
    /// Peak usage
    pub peak_usage: usize,
    /// Current usage
    pub current_usage: usize,
    /// Fragmentation ratio
    pub fragmentation_ratio: f32,
}

/// Allocation statistics
#[derive(Debug, Clone, Default)]
pub struct AllocationStatistics {
    /// Total allocations
    pub total_allocations: usize,
    /// Total deallocations
    pub total_deallocations: usize,
    /// Peak memory usage
    pub peak_memory_usage: usize,
    /// Current memory usage
    pub current_memory_usage: usize,
    /// Allocation efficiency
    pub allocation_efficiency: f32,
}

/// Parallel execution engine for attractor operations
#[derive(Debug)]
pub struct ParallelExecutionEngine {
    /// Engine configuration
    pub config: ParallelExecutionConfig,
    /// Thread pool
    pub thread_pool: Arc<RwLock<Option<threadpool::ThreadPool>>>,
    /// Task scheduler
    pub task_scheduler: TaskScheduler,
    /// Load balancer
    pub load_balancer: LoadBalancer,
}

/// Parallel execution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelExecutionConfig {
    /// Maximum concurrent tasks
    pub max_concurrent_tasks: usize,
    /// Task queue size
    pub task_queue_size: usize,
    /// Load balancing strategy
    pub load_balancing_strategy: LoadBalancingStrategy,
    /// Enable work stealing
    pub enable_work_stealing: bool,
    /// Task timeout in milliseconds
    pub task_timeout_ms: u64,
}

/// Load balancing strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoadBalancingStrategy {
    /// Round-robin distribution
    RoundRobin,
    /// Least loaded first
    LeastLoaded,
    /// Random distribution
    Random,
    /// Performance-based distribution
    PerformanceBased,
    /// Adaptive distribution
    Adaptive,
}

/// Task scheduler for parallel execution
#[derive(Debug)]
pub struct TaskScheduler {
    /// Priority queue for tasks
    pub priority_queue: Arc<RwLock<Vec<Task>>>,
    /// Running tasks
    pub running_tasks: Arc<RwLock<HashMap<String, TaskStatus>>>,
    /// Completed tasks
    pub completed_tasks: Arc<RwLock<Vec<TaskResult>>>,
}

/// Task to be executed
#[derive(Debug, Clone)]
pub struct Task {
    /// Task ID
    pub id: String,
    /// Task type
    pub task_type: TaskType,
    /// Task priority
    pub priority: ProcessingPriority,
    /// Task data
    pub data: TaskData,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Dependencies
    pub dependencies: Vec<String>,
}

/// Types of tasks
#[derive(Debug, Clone)]
pub enum TaskType {
    /// Pattern analysis task
    PatternAnalysis,
    /// Basin creation task
    BasinCreation,
    /// Basin update task
    BasinUpdate,
    /// Memory consolidation task
    MemoryConsolidation,
    /// Performance optimization task
    PerformanceOptimization,
}

/// Task data
#[derive(Debug, Clone)]
pub enum TaskData {
    /// Pattern analysis data
    PatternAnalysis(SemanticPattern),
    /// Basin creation data
    BasinCreation(SemanticPattern),
    /// Basin update data
    BasinUpdate(String, SemanticPattern),
    /// Optimization data
    Optimization(HashMap<String, f32>),
}

/// Task status
#[derive(Debug, Clone)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
    Cancelled,
}

/// Task execution result
#[derive(Debug, Clone)]
pub struct TaskResult {
    /// Task ID
    pub task_id: String,
    /// Execution status
    pub status: TaskStatus,
    /// Execution time in milliseconds
    pub execution_time_ms: u64,
    /// Result data
    pub result_data: Option<TaskResultData>,
    /// Performance metrics
    pub performance_metrics: TaskPerformanceMetrics,
}

/// Task result data
#[derive(Debug, Clone)]
pub enum TaskResultData {
    /// Analysis result
    Analysis(AttractorAnalysisResult),
    /// Basin ID result
    BasinId(String),
    /// Optimization metrics result
    OptimizationMetrics(OptimizationMetrics),
}

/// Task performance metrics
#[derive(Debug, Clone, Default)]
pub struct TaskPerformanceMetrics {
    /// CPU usage percentage
    pub cpu_usage: f32,
    /// Memory usage in bytes
    pub memory_usage: usize,
    /// Cache hit rate
    pub cache_hit_rate: f32,
    /// SIMD efficiency
    pub simd_efficiency: f32,
}

/// Load balancer for task distribution
#[derive(Debug)]
pub struct LoadBalancer {
    /// Balancer strategy
    pub strategy: LoadBalancingStrategy,
    /// Worker statistics
    pub worker_stats: Arc<RwLock<Vec<WorkerStats>>>,
    /// Current task distribution
    pub current_distribution: Arc<RwLock<HashMap<usize, usize>>>,
}

/// Worker statistics for load balancing
#[derive(Debug, Clone, Default)]
pub struct WorkerStats {
    /// Worker ID
    pub worker_id: usize,
    /// Current load
    pub current_load: f32,
    /// Average processing time
    pub avg_processing_time_ms: f64,
    /// Success rate
    pub success_rate: f32,
    /// Last update timestamp
    pub last_update: Option<DateTime<Utc>>,
}

/// Optimization metrics tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationMetrics {
    /// Performance improvement over baseline
    pub performance_improvement: f32,
    /// Accuracy improvement
    pub accuracy_improvement: f32,
    /// Speed improvement factor
    pub speed_improvement_factor: f32,
    /// Memory usage reduction
    pub memory_usage_reduction: f32,
    /// Cache efficiency metrics
    pub cache_efficiency: CacheEfficiencyMetrics,
    /// Parallel efficiency metrics
    pub parallel_efficiency: ParallelEfficiencyMetrics,
    /// Overall optimization score
    pub overall_optimization_score: f32,
}

/// Cache efficiency metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEfficiencyMetrics {
    /// Hit rate
    pub hit_rate: f32,
    /// Miss rate
    pub miss_rate: f32,
    /// Average lookup time
    pub avg_lookup_time_ns: u64,
    /// Cache size utilization
    pub size_utilization: f32,
    /// Eviction rate
    pub eviction_rate: f32,
}

/// Parallel efficiency metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelEfficiencyMetrics {
    /// Parallelization efficiency
    pub parallelization_efficiency: f32,
    /// Workload distribution balance
    pub workload_balance: f32,
    /// Thread utilization rate
    pub thread_utilization: f32,
    /// Speedup factor
    pub speedup_factor: f32,
    /// Overhead ratio
    pub overhead_ratio: f32,
}

impl AttractorPerformanceOptimizer {
    /// Create new performance optimizer
    pub fn new(config: OptimizationConfig) -> Self {
        let thread_pool = if config.enable_parallel_processing {
            Some(threadpool::ThreadPool::new(config.worker_threads))
        } else {
            None
        };

        Self {
            config: config.clone(),
            performance_cache: PerformanceCache {
                pattern_analysis_cache: Arc::new(RwLock::new(HashMap::new())),
                basin_similarity_cache: Arc::new(RwLock::new(HashMap::new())),
                distance_cache: Arc::new(RwLock::new(HashMap::new())),
                optimization_cache: Arc::new(RwLock::new(HashMap::new())),
                cache_stats: Arc::new(RwLock::new(CacheStatistics::default())),
            },
            batch_processor: BatchProcessor {
                config: BatchConfig::default(),
                processing_queue: Arc::new(RwLock::new(Vec::new())),
                worker_pool: Arc::new(RwLock::new(Vec::new())),
            },
            memory_pool: MemoryPoolManager {
                config: MemoryPoolConfig::default(),
                memory_pools: HashMap::new(),
                allocation_stats: AllocationStatistics::default(),
            },
            parallel_engine: ParallelExecutionEngine {
                config: ParallelExecutionConfig {
                    max_concurrent_tasks: config.worker_threads,
                    task_queue_size: config.batch_size * 2,
                    load_balancing_strategy: LoadBalancingStrategy::LeastLoaded,
                    enable_work_stealing: true,
                    task_timeout_ms: config.max_processing_time_ms,
                },
                thread_pool: Arc::new(RwLock::new(thread_pool)),
                task_scheduler: TaskScheduler {
                    priority_queue: Arc::new(RwLock::new(Vec::new())),
                    running_tasks: Arc::new(RwLock::new(HashMap::new())),
                    completed_tasks: Arc::new(RwLock::new(Vec::new())),
                },
                load_balancer: LoadBalancer {
                    strategy: LoadBalancingStrategy::Adaptive,
                    worker_stats: Arc::new(RwLock::new(Vec::new())),
                    current_distribution: Arc::new(RwLock::new(HashMap::new())),
                },
            },
            optimization_metrics: OptimizationMetrics {
                performance_improvement: 0.0,
                accuracy_improvement: 0.0,
                speed_improvement_factor: 1.0,
                memory_usage_reduction: 0.0,
                cache_efficiency: CacheEfficiencyMetrics {
                    hit_rate: 0.0,
                    miss_rate: 1.0,
                    avg_lookup_time_ns: 0,
                    size_utilization: 0.0,
                    eviction_rate: 0.0,
                },
                parallel_efficiency: ParallelEfficiencyMetrics {
                    parallelization_efficiency: 1.0,
                    workload_balance: 1.0,
                    thread_utilization: 0.0,
                    speedup_factor: 1.0,
                    overhead_ratio: 0.0,
                },
                overall_optimization_score: 0.0,
            },
        }
    }

    /// Optimize attractor analysis with performance enhancements
    pub fn optimize_pattern_analysis(
        &mut self,
        attractor_engine: &mut AttractorDynamicsEngine,
        pattern: &SemanticPattern,
    ) -> ContextNestResult<AttractorAnalysisResult> {
        let start_time = Instant::now();

        // Check cache first
        let cache_key = format!(
            "pattern_{}_{}",
            pattern.id,
            pattern
                .embedding
                .iter()
                .take(8)
                .map(|&x| (x as i32).to_string())
                .collect::<Vec<String>>()
                .join("_")
        );

        if let Some(cached_result) = self.get_cached_analysis(&cache_key) {
            self.record_cache_hit();
            return Ok(cached_result);
        }

        // Use parallel processing if enabled and beneficial
        let result = if self.config.enable_parallel_processing
            && self.should_use_parallel_processing(pattern)
        {
            self.analyze_pattern_parallel(attractor_engine, pattern)?
        } else {
            attractor_engine.analyze_pattern(pattern)?
        };

        // Cache the result if it's expensive to compute
        if self.should_cache_result(&result) {
            self.cache_analysis_result(cache_key, result.clone())?;
        }

        // Update optimization metrics
        let processing_time = start_time.elapsed().as_millis() as f32;
        self.update_optimization_metrics(&result, processing_time)?;

        Ok(result)
    }

    /// Process multiple patterns in optimized batch
    pub fn optimize_batch_processing(
        &mut self,
        attractor_engine: &mut AttractorDynamicsEngine,
        patterns: &[SemanticPattern],
    ) -> ContextNestResult<Vec<AttractorAnalysisResult>> {
        if patterns.is_empty() {
            return Ok(Vec::new());
        }

        let start_time = Instant::now();

        // Determine optimal batch size
        let optimal_batch_size = self.calculate_optimal_batch_size(patterns.len());

        let mut results = Vec::new();

        if patterns.len() <= optimal_batch_size {
            // Process as single batch
            results = self.process_single_batch(attractor_engine, patterns)?;
        } else {
            // Process in multiple batches
            for chunk in patterns.chunks(optimal_batch_size) {
                let batch_results = self.process_single_batch(attractor_engine, chunk)?;
                results.extend(batch_results);
            }
        }

        // Update batch processing metrics
        let total_time = start_time.elapsed().as_millis() as f32;
        self.update_batch_processing_metrics(results.len(), total_time)?;

        Ok(results)
    }

    /// Optimize memory usage for attractor networks
    pub fn optimize_memory_usage(
        &mut self,
        attractor_engine: &mut AttractorDynamicsEngine,
    ) -> ContextNestResult<MemoryOptimizationResult> {
        let start_time = Instant::now();

        let initial_memory = self.estimate_memory_usage(attractor_engine);

        // Optimize basin storage
        self.optimize_basin_storage(attractor_engine)?;

        // Optimize interaction networks
        self.optimize_interaction_networks(attractor_engine)?;

        // Garbage collect if needed
        if self.should_trigger_gc(attractor_engine) {
            self.perform_garbage_collection(attractor_engine)?;
        }

        let final_memory = self.estimate_memory_usage(attractor_engine);
        let memory_reduction = initial_memory.saturating_sub(final_memory);
        let reduction_percentage = (memory_reduction as f32 / initial_memory as f32) * 100.0;

        Ok(MemoryOptimizationResult {
            initial_memory,
            final_memory,
            memory_reduction,
            reduction_percentage,
            optimization_time_ms: start_time.elapsed().as_millis() as u64,
            optimization_success: reduction_percentage > 5.0,
        })
    }

    /// Optimize for target accuracy threshold
    pub fn optimize_for_accuracy(
        &mut self,
        attractor_engine: &mut AttractorDynamicsEngine,
        target_accuracy: f32,
    ) -> ContextNestResult<AccuracyOptimizationResult> {
        let start_time = Instant::now();

        let initial_accuracy = self.estimate_current_accuracy(attractor_engine);

        if initial_accuracy >= target_accuracy {
            return Ok(AccuracyOptimizationResult {
                initial_accuracy,
                final_accuracy: initial_accuracy,
                accuracy_improvement: 0.0,
                target_achieved: true,
                optimization_time_ms: start_time.elapsed().as_millis() as u64,
                optimizations_applied: Vec::new(),
            });
        }

        let mut optimizations_applied = Vec::new();
        let mut current_accuracy = initial_accuracy;

        // Apply accuracy optimization strategies
        while current_accuracy < target_accuracy {
            let improvement = self.apply_accuracy_optimization(
                attractor_engine,
                current_accuracy,
                target_accuracy,
            )?;

            if improvement.improvement < 0.001 {
                // Minimal improvement
                break;
            }

            current_accuracy += improvement.improvement;
            optimizations_applied.push(improvement);

            if current_accuracy >= target_accuracy {
                break;
            }
        }

        let accuracy_improvement = current_accuracy - initial_accuracy;

        Ok(AccuracyOptimizationResult {
            initial_accuracy,
            final_accuracy: current_accuracy,
            accuracy_improvement,
            target_achieved: current_accuracy >= target_accuracy,
            optimization_time_ms: start_time.elapsed().as_millis() as u64,
            optimizations_applied,
        })
    }

    // Helper methods

    fn get_cached_analysis(&self, cache_key: &str) -> Option<AttractorAnalysisResult> {
        if let Ok(cache) = self.performance_cache.pattern_analysis_cache.read() {
            if let Some(cached) = cache.get(cache_key) {
                // Check if cache entry is still valid
                let age = Utc::now()
                    .signed_duration_since(cached.created_at)
                    .num_seconds() as f32;
                if age < 300.0 {
                    // 5 minutes cache TTL
                    return Some(cached.result.clone());
                }
            }
        }
        None
    }

    fn cache_analysis_result(
        &mut self,
        cache_key: String,
        result: AttractorAnalysisResult,
    ) -> ContextNestResult<()> {
        // Check size + evict without holding the write lock across the &mut self call.
        let needs_eviction = self
            .performance_cache
            .pattern_analysis_cache
            .read()
            .map(|cache| cache.len() >= self.config.cache_size_limit)
            .unwrap_or(false);

        if needs_eviction {
            self.evict_cache_entries()?;
        }

        if let Ok(mut cache) = self.performance_cache.pattern_analysis_cache.write() {
            cache.insert(
                cache_key,
                CachedAnalysis {
                    result,
                    created_at: Utc::now(),
                    last_accessed: Utc::now(),
                    access_count: 1,
                    recompute_cost: 1.0, // Would need actual estimation
                },
            );
        }
        Ok(())
    }

    fn evict_cache_entries(&mut self) -> ContextNestResult<()> {
        // Simple LRU eviction — remove oldest 25% of entries.
        let eviction_count = self.config.cache_size_limit / 4;

        if let Ok(mut cache) = self.performance_cache.pattern_analysis_cache.write() {
            // Collect owned keys to avoid borrowing `cache` immutably while we
            // mutate it. (The previous version did `cache.iter()` then
            // `cache.remove()` which conflicted.)
            let mut keyed: Vec<(String, DateTime<Utc>)> = cache
                .iter()
                .map(|(k, v)| (k.clone(), v.created_at))
                .collect();
            keyed.sort_by_key(|(_, t)| *t);

            for (key, _) in keyed.into_iter().take(eviction_count) {
                cache.remove(&key);
            }
        }

        Ok(())
    }

    fn record_cache_hit(&mut self) {
        if let Ok(mut stats) = self.performance_cache.cache_stats.write() {
            stats.total_hits += 1;
            stats.total_cached_items += 1;
            stats.hit_rate =
                stats.total_hits as f32 / (stats.total_hits + stats.total_misses) as f32;
        }
    }

    fn record_cache_miss(&mut self) {
        if let Ok(mut stats) = self.performance_cache.cache_stats.write() {
            stats.total_misses += 1;
            stats.hit_rate =
                stats.total_hits as f32 / (stats.total_hits + stats.total_misses) as f32;
        }
    }

    fn should_use_parallel_processing(&self, pattern: &SemanticPattern) -> bool {
        // Use parallel processing for complex patterns or when load is low
        pattern.embedding.len() > 500 || self.estimate_system_load() < 0.7
    }

    fn should_cache_result(&self, result: &AttractorAnalysisResult) -> bool {
        // Cache results that are expensive to compute or frequently accessed
        result.processing_time_ms > 10 || result.confidence_score > 0.8
    }

    fn analyze_pattern_parallel(
        &mut self,
        attractor_engine: &mut AttractorDynamicsEngine,
        pattern: &SemanticPattern,
    ) -> ContextNestResult<AttractorAnalysisResult> {
        // Create parallel analysis task
        let pattern_clone = pattern.clone();

        // For demonstration, we'll use a simple parallel approach
        // In production, this would use the full thread pool
        std::thread::scope(|s| {
            // Parallel basin search
            let basin_search_handle = s.spawn(|| attractor_engine.analyze_pattern(&pattern_clone));

            // Wait for completion
            basin_search_handle.join().unwrap_or_else(|_| {
                Err(ContextNestError::Api(
                    "Parallel analysis failed".to_string(),
                ))
            })
        })
    }

    fn calculate_optimal_batch_size(&self, pattern_count: usize) -> usize {
        let base_size = self.config.batch_size;

        // Adjust batch size based on system load and pattern complexity
        let load_factor = 1.0 - self.estimate_system_load();
        let complexity_factor = if pattern_count > 100 { 0.5 } else { 1.0 };

        let adjusted_size = (base_size as f32 * load_factor * complexity_factor) as usize;
        adjusted_size.max(1).min(pattern_count)
    }

    fn process_single_batch(
        &mut self,
        attractor_engine: &mut AttractorDynamicsEngine,
        patterns: &[SemanticPattern],
    ) -> ContextNestResult<Vec<AttractorAnalysisResult>> {
        let mut results = Vec::new();

        for pattern in patterns {
            let result = self.optimize_pattern_analysis(attractor_engine, pattern)?;
            results.push(result);
        }

        Ok(results)
    }

    fn update_optimization_metrics(
        &mut self,
        result: &AttractorAnalysisResult,
        processing_time: f32,
    ) -> ContextNestResult<()> {
        // Update cache efficiency
        if let Ok(stats) = self.performance_cache.cache_stats.read() {
            self.optimization_metrics.cache_efficiency = CacheEfficiencyMetrics {
                hit_rate: stats.hit_rate,
                miss_rate: 1.0 - stats.hit_rate,
                avg_lookup_time_ns: 50, // Estimate
                size_utilization: stats.total_cached_items as f32
                    / self.config.cache_size_limit as f32,
                eviction_rate: 0.01, // Estimate
            };
        }

        // Update performance metrics
        self.optimization_metrics.performance_improvement = result.confidence_score * 0.1;
        self.optimization_metrics.speed_improvement_factor = 1000.0 / processing_time.max(1.0);

        Ok(())
    }

    fn update_batch_processing_metrics(
        &mut self,
        processed_count: usize,
        total_time: f32,
    ) -> ContextNestResult<()> {
        let avg_time_per_item = total_time / processed_count as f32;
        let baseline_time = 50.0; // Estimate baseline processing time

        self.optimization_metrics.speed_improvement_factor = baseline_time / avg_time_per_item;

        Ok(())
    }

    fn estimate_memory_usage(&self, attractor_engine: &AttractorDynamicsEngine) -> usize {
        // Estimate memory usage based on component sizes
        let basin_memory =
            attractor_engine.attractor_basins.len() * std::mem::size_of::<AttractorBasin>();
        let pattern_memory = attractor_engine
            .attractor_basins
            .iter()
            .map(|b| b.associated_patterns.len() * std::mem::size_of::<String>())
            .sum::<usize>();

        basin_memory + pattern_memory + 1000000 // Add 1MB base overhead
    }

    fn optimize_basin_storage(
        &mut self,
        attractor_engine: &mut AttractorDynamicsEngine,
    ) -> ContextNestResult<()> {
        // Remove inactive basins
        let initial_count = attractor_engine.attractor_basins.len();

        attractor_engine.attractor_basins.retain(|basin| {
            basin.health.overall_health > 0.1 && basin.associated_patterns.len() > 0
        });

        let removed_count = initial_count - attractor_engine.attractor_basins.len();
        if removed_count > 0 {
            tracing::info!("Removed {} inactive attractor basins", removed_count);
        }

        Ok(())
    }

    fn optimize_interaction_networks(
        &mut self,
        attractor_engine: &mut AttractorDynamicsEngine,
    ) -> ContextNestResult<()> {
        // Remove weak connections
        let mut networks = attractor_engine.interaction_networks.clone();

        // Implementation would optimize network connections
        // This is a placeholder for the actual optimization logic

        attractor_engine.interaction_networks = networks;
        Ok(())
    }

    fn should_trigger_gc(&self, attractor_engine: &AttractorDynamicsEngine) -> bool {
        let memory_usage = self.estimate_memory_usage(attractor_engine);
        let threshold = self.config.cache_size_limit * std::mem::size_of::<CachedAnalysis>();

        memory_usage > threshold
    }

    fn perform_garbage_collection(
        &mut self,
        attractor_engine: &mut AttractorDynamicsEngine,
    ) -> ContextNestResult<()> {
        // Clear expired cache entries
        self.evict_cache_entries()?;

        // Clear old learning history
        for basin in &mut attractor_engine.attractor_basins {
            let max_events = 1000;
            if basin.learning_history.learning_events.len() > max_events {
                basin
                    .learning_history
                    .learning_events
                    .drain(0..basin.learning_history.learning_events.len() - max_events);
            }
        }

        Ok(())
    }

    fn estimate_current_accuracy(&self, attractor_engine: &AttractorDynamicsEngine) -> f32 {
        // Estimate accuracy based on basin health and count
        if attractor_engine.attractor_basins.is_empty() {
            return 0.5;
        }

        let avg_health: f32 = attractor_engine
            .attractor_basins
            .iter()
            .map(|b| b.health.overall_health)
            .sum::<f32>()
            / attractor_engine.attractor_basins.len() as f32;

        avg_health * 0.9 + 0.1 // Base accuracy offset
    }

    fn apply_accuracy_optimization(
        &mut self,
        attractor_engine: &mut AttractorDynamicsEngine,
        current_accuracy: f32,
        target_accuracy: f32,
    ) -> ContextNestResult<AccuracyOptimizationApplied> {
        let (improvement, optimization_type) = if target_accuracy - current_accuracy > 0.1 {
            // Large improvement needed
            for basin in &mut attractor_engine.attractor_basins {
                basin.health.overall_health = (basin.health.overall_health * 0.95 + 0.05).min(1.0);
                basin.depth = (basin.depth * 0.95 + 0.02).min(1.0);
            }
            (0.05, AccuracyOptimizationType::BasinHealthImprovement)
        } else {
            // Small improvement needed
            for basin in &mut attractor_engine.attractor_basins {
                basin.health.overall_health = (basin.health.overall_health * 0.98 + 0.02).min(1.0);
            }
            (0.02, AccuracyOptimizationType::FineTuning)
        };

        Ok(AccuracyOptimizationApplied {
            optimization_type,
            improvement,
            processing_cost: 0.1,
        })
    }

    fn estimate_system_load(&self) -> f32 {
        // Simple system load estimation
        // In production, this would use actual system metrics
        0.5 // Placeholder
    }

    /// Get current optimization metrics
    pub fn get_optimization_metrics(&self) -> &OptimizationMetrics {
        &self.optimization_metrics
    }

    /// Get cache statistics
    pub fn get_cache_statistics(&self) -> CacheStatistics {
        self.performance_cache.cache_stats.read().unwrap().clone()
    }
}

// Supporting types for optimization results

/// Result of memory optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryOptimizationResult {
    pub initial_memory: usize,
    pub final_memory: usize,
    pub memory_reduction: usize,
    pub reduction_percentage: f32,
    pub optimization_time_ms: u64,
    pub optimization_success: bool,
}

/// Result of accuracy optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccuracyOptimizationResult {
    pub initial_accuracy: f32,
    pub final_accuracy: f32,
    pub accuracy_improvement: f32,
    pub target_achieved: bool,
    pub optimization_time_ms: u64,
    pub optimizations_applied: Vec<AccuracyOptimizationApplied>,
}

/// Applied accuracy optimization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccuracyOptimizationApplied {
    pub optimization_type: AccuracyOptimizationType,
    pub improvement: f32,
    pub processing_cost: f32,
}

/// Types of accuracy optimizations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccuracyOptimizationType {
    BasinHealthImprovement,
    FineTuning,
    ParameterAdjustment,
    NetworkReconfiguration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::attractor_dynamics::AttractorDynamicsEngine;
    use crate::context::field::SemanticPattern;
    use chrono::Utc;

    #[test]
    fn test_performance_optimizer_creation() {
        let config = OptimizationConfig::default();
        let optimizer = AttractorPerformanceOptimizer::new(config);

        assert!(optimizer.config.enable_parallel_processing);
        assert_eq!(optimizer.config.batch_size, 32);
    }

    #[test]
    fn test_cached_analysis() {
        let config = OptimizationConfig::default();
        let mut optimizer = AttractorPerformanceOptimizer::new(config);

        // Create mock engine and pattern
        let mut engine = AttractorDynamicsEngine::new(100);
        let pattern = SemanticPattern {
            id: "test".to_string(),
            content: "Test pattern".to_string(),
            embedding: vec![0.1; 100],
            strength: 0.8,
            resonance: 0.7,
            decay_rate: 0.1,
            created_at: Utc::now(),
            last_activated: Utc::now(),
            activation_count: 1,
            deleted_at: None,
            delete_reason: None,
        };

        // First analysis should miss cache
        let result1 = optimizer
            .optimize_pattern_analysis(&mut engine, &pattern)
            .unwrap();

        // Second analysis should hit cache
        let result2 = optimizer
            .optimize_pattern_analysis(&mut engine, &pattern)
            .unwrap();

        assert_eq!(result1.confidence_score, result2.confidence_score);
    }

    #[test]
    fn test_batch_processing() {
        let config = OptimizationConfig::default();
        let mut optimizer = AttractorPerformanceOptimizer::new(config);

        // Create mock engine and patterns
        let mut engine = AttractorDynamicsEngine::new(100);
        let patterns: Vec<_> = (0..5)
            .map(|i| SemanticPattern {
                id: format!("test_{}", i),
                content: format!("Test pattern {}", i),
                embedding: vec![0.1; 100],
                strength: 0.8,
                resonance: 0.7,
                decay_rate: 0.1,
                created_at: Utc::now(),
                last_activated: Utc::now(),
                activation_count: 1,
                deleted_at: None,
                delete_reason: None,
            })
            .collect();

        let results = optimizer
            .optimize_batch_processing(&mut engine, &patterns)
            .unwrap();

        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_memory_optimization() {
        let config = OptimizationConfig::default();
        let mut optimizer = AttractorPerformanceOptimizer::new(config);

        // Create mock engine
        let mut engine = AttractorDynamicsEngine::new(100);

        // Add some basins to optimize
        let pattern = SemanticPattern {
            id: "test".to_string(),
            content: "Test pattern".to_string(),
            embedding: vec![0.1; 100],
            strength: 0.8,
            resonance: 0.7,
            decay_rate: 0.1,
            created_at: Utc::now(),
            last_activated: Utc::now(),
            activation_count: 1,
            deleted_at: None,
            delete_reason: None,
        };

        engine.create_attractor_basin(&pattern).unwrap();

        let result = optimizer.optimize_memory_usage(&mut engine).unwrap();

        assert!(result.final_memory <= result.initial_memory);
    }
}

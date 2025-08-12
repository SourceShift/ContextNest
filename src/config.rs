//! Configuration management for ContextNest
//! Provides centralized configuration for all ContextNest components including
//! context management, neural fields, memory systems, protocols, and services.

use crate::error::ContextNestResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// Re-export service configuration types

/// Main configuration for ContextNest system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Core context management configuration
    pub context: ContextConfig,

    /// Neural field configuration
    pub neural_field: NeuralFieldConfig,

    /// Memory system configuration
    pub memory: MemoryConfig,

    /// Protocol system configuration
    pub protocols: ProtocolConfig,

    /// Service configurations
    pub services: ServicesConfig,

    /// API configuration
    pub api: ApiConfig,

    /// Logging and monitoring configuration
    pub monitoring: MonitoringConfig,

    /// Performance and resource configuration
    pub performance: PerformanceConfig,

    /// Database configuration
    pub database: DatabaseConfig,

    /// Enhanced security configuration
    pub security: SecurityConfig,

    /// Real-time synchronization configuration
    pub synchronization: SynchronizationConfig,

    /// Plugin system configuration
    pub plugins: PluginConfig,
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Neo4j connection URI
    pub neo4j_uri: String,
    /// Neo4j username
    pub neo4j_username: String,
    /// Neo4j password
    pub neo4j_password: String,
    /// Neo4j database name
    pub neo4j_database: String,
    /// Use mock database for development
    pub use_mock: bool,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            neo4j_uri: "neo4j://localhost:7687".to_string(),
            neo4j_username: "neo4j".to_string(),
            neo4j_password: "password".to_string(),
            neo4j_database: "neo4j".to_string(),
            use_mock: true, // Default to mock mode for development
        }
    }
}

/// Context management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Maximum token budget for context
    pub max_token_budget: usize,

    /// Default context level
    pub default_level: ContextLevel,

    /// Auto-enhancement settings
    pub auto_enhancement: AutoEnhancementConfig,

    /// Context persistence settings
    pub persistence: ContextPersistenceConfig,

    /// Context validation settings
    pub validation: ContextValidationConfig,
}

/// Context levels in hierarchical order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextLevel {
    Atomic,
    Molecular,
    Cellular,
    Organic,
    Field,
    Programmatic,
    ProtocolBased,
}

/// Auto-enhancement configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoEnhancementConfig {
    /// Enable automatic context enhancement
    pub enabled: bool,

    /// Token budget threshold for enhancement
    pub enhancement_threshold: f32,

    /// Maximum enhancement iterations
    pub max_iterations: u32,

    /// Enhancement strategy
    pub strategy: EnhancementStrategy,
}

/// Enhancement strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EnhancementStrategy {
    Conservative,
    Balanced,
    Aggressive,
    Adaptive,
}

/// Context persistence configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPersistenceConfig {
    /// Enable context persistence
    pub enabled: bool,

    /// Persistence storage path
    pub storage_path: PathBuf,

    /// Persistence interval in seconds
    pub save_interval_seconds: u64,

    /// Maximum stored context sessions
    pub max_stored_sessions: u32,

    /// Compression settings
    pub compression: CompressionConfig,
}

/// Compression configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    /// Enable compression
    pub enabled: bool,

    /// Compression algorithm
    pub algorithm: CompressionAlgorithm,

    /// Compression level (1-9)
    pub level: u8,
}

/// Compression algorithms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    Gzip,
    Zstd,
    Lz4,
    Brotli,
}

/// Context validation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextValidationConfig {
    /// Enable context validation
    pub enabled: bool,

    /// Validation strictness level
    pub strictness: ValidationStrictness,

    /// Maximum validation time in milliseconds
    pub max_validation_time_ms: u64,

    /// Validation rules
    pub rules: Vec<ValidationRule>,
}

/// Validation strictness levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationStrictness {
    Permissive,
    Moderate,
    Strict,
    Paranoid,
}

/// Validation rules
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationRule {
    TokenBudgetLimit,
    SemanticCoherence,
    StructuralIntegrity,
    SecurityConstraints,
    PerformanceThresholds,
}

/// Neural field configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralFieldConfig {
    /// Default embedding dimensions
    pub embedding_dim: usize,

    /// Field dynamics configuration
    pub dynamics: FieldDynamicsConfig,

    /// Pattern configuration
    pub patterns: PatternConfig,

    /// Resonance configuration
    pub resonance: ResonanceConfig,

    /// Coherence configuration
    pub coherence: CoherenceConfig,
}

/// Field dynamics configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDynamicsConfig {
    /// Default decay rate for patterns
    pub default_decay_rate: f32,

    /// Minimum pattern strength threshold
    pub min_strength_threshold: f32,

    /// Maximum field capacity
    pub max_field_capacity: usize,

    /// Update frequency in milliseconds
    pub update_frequency_ms: u64,

    /// Enable adaptive dynamics
    pub adaptive_dynamics: bool,
}

/// Pattern configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternConfig {
    /// Default pattern strength
    pub default_strength: f32,

    /// Pattern similarity threshold
    pub similarity_threshold: f32,

    /// Maximum patterns per field
    pub max_patterns_per_field: usize,

    /// Pattern clustering settings
    pub clustering: ClusteringConfig,
}

/// Clustering configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusteringConfig {
    /// Enable pattern clustering
    pub enabled: bool,

    /// Clustering algorithm
    pub algorithm: ClusteringAlgorithm,

    /// Minimum cluster size
    pub min_cluster_size: usize,

    /// Maximum distance for clustering
    pub max_cluster_distance: f32,
}

/// Clustering algorithms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClusteringAlgorithm {
    KMeans,
    DBSCAN,
    Hierarchical,
    Adaptive,
}

/// Resonance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResonanceConfig {
    /// Default amplification factor
    pub default_amplification: f32,

    /// Resonance frequency
    pub frequency_hz: f32,

    /// Resonance damping factor
    pub damping_factor: f32,

    /// Enable harmonic resonance
    pub enable_harmonics: bool,

    /// Scaffolding parameters
    pub scaffolding: ScaffoldingConfig,
}

/// Scaffolding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaffoldingConfig {
    /// Enable resonance scaffolding
    pub enabled: bool,

    /// Scaffolding strength
    pub strength: f32,

    /// Scaffolding duration in seconds
    pub duration_seconds: u64,

    /// Auto-scaffolding triggers
    pub auto_triggers: Vec<ScaffoldingTrigger>,
}

/// Scaffolding triggers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScaffoldingTrigger {
    LowCoherence,
    PatternDegradation,
    ResonanceLoss,
    ExternalStimulus,
}

/// Coherence configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoherenceConfig {
    /// Minimum coherence threshold
    pub min_coherence_threshold: f32,

    /// Coherence measurement interval in milliseconds
    pub measurement_interval_ms: u64,

    /// Auto-repair settings
    pub auto_repair: AutoRepairConfig,

    /// Coherence monitoring
    pub monitoring: CoherenceMonitoringConfig,
}

/// Auto-repair configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoRepairConfig {
    /// Enable automatic field repair
    pub enabled: bool,

    /// Repair strategy
    pub strategy: RepairStrategy,

    /// Maximum repair attempts
    pub max_attempts: u32,

    /// Repair cooldown in seconds
    pub cooldown_seconds: u64,
}

/// Repair strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RepairStrategy {
    Conservative,
    Balanced,
    Aggressive,
    Adaptive,
}

/// Coherence monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoherenceMonitoringConfig {
    /// Enable coherence monitoring
    pub enabled: bool,

    /// Alert threshold for low coherence
    pub alert_threshold: f32,

    /// Monitoring history size
    pub history_size: usize,

    /// Trend analysis settings
    pub trend_analysis: TrendAnalysisConfig,
}

/// Trend analysis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendAnalysisConfig {
    /// Enable trend analysis
    pub enabled: bool,

    /// Analysis window size
    pub window_size: usize,

    /// Trend detection sensitivity
    pub sensitivity: f32,

    /// Prediction horizon in measurements
    pub prediction_horizon: usize,
}

/// Memory system configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Attractor configuration
    pub attractors: AttractorConfig,

    /// Memory persistence settings
    pub persistence: MemoryPersistenceConfig,

    /// Memory optimization settings
    pub optimization: MemoryOptimizationConfig,

    /// Memory metrics configuration
    pub metrics: MemoryMetricsConfig,
}

/// Attractor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttractorConfig {
    /// Default attractor strength
    pub default_strength: f32,

    /// Default importance weight
    pub default_importance: f32,

    /// Maximum attractors per field
    pub max_attractors_per_field: usize,

    /// Attractor decay settings
    pub decay: AttractorDecayConfig,

    /// Connection settings
    pub connections: ConnectionConfig,
}

/// Attractor decay configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttractorDecayConfig {
    /// Base decay rate
    pub base_decay_rate: f32,

    /// Usage-based decay adjustment
    pub usage_factor: f32,

    /// Importance-based decay adjustment
    pub importance_factor: f32,

    /// Connection-based decay adjustment
    pub connection_factor: f32,
}

/// Connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    /// Maximum connections per attractor
    pub max_connections_per_attractor: usize,

    /// Connection strength threshold
    pub connection_threshold: f32,

    /// Auto-connection settings
    pub auto_connection: AutoConnectionConfig,
}

/// Auto-connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoConnectionConfig {
    /// Enable automatic connection formation
    pub enabled: bool,

    /// Similarity threshold for auto-connection
    pub similarity_threshold: f32,

    /// Maximum auto-connections per update
    pub max_per_update: usize,
}

/// Memory persistence configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPersistenceConfig {
    /// Enable memory persistence
    pub enabled: bool,

    /// Storage backend
    pub backend: StorageBackend,

    /// Persistence strategy
    pub strategy: PersistenceStrategy,

    /// Backup configuration
    pub backup: BackupConfig,
}

/// Storage backends
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageBackend {
    FileSystem { path: PathBuf },
    Database { connection_string: String },
    Memory,
    Custom { config: HashMap<String, String> },
}

/// Persistence strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PersistenceStrategy {
    Immediate,
    Batched { batch_size: usize },
    Scheduled { interval_seconds: u64 },
    OnDemand,
}

/// Backup configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    /// Enable automatic backups
    pub enabled: bool,

    /// Backup interval in hours
    pub interval_hours: u64,

    /// Maximum backup files to keep
    pub max_backups: u32,

    /// Backup compression
    pub compression: CompressionConfig,
}

/// Memory optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryOptimizationConfig {
    /// Enable automatic optimization
    pub enabled: bool,

    /// Optimization interval in minutes
    pub interval_minutes: u64,

    /// Optimization strategies
    pub strategies: Vec<OptimizationStrategy>,

    /// Performance thresholds
    pub thresholds: OptimizationThresholds,
}

/// Optimization strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptimizationStrategy {
    RemoveWeakAttractors,
    ConsolidateSimilarAttractors,
    PruneUnusedConnections,
    RebalanceStrengths,
    CompactStorage,
}

/// Optimization thresholds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationThresholds {
    /// Memory usage threshold (percentage)
    pub memory_usage_threshold: f32,

    /// Fragmentation threshold
    pub fragmentation_threshold: f32,

    /// Efficiency threshold
    pub efficiency_threshold: f32,
}

/// Memory metrics configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMetricsConfig {
    /// Enable metrics collection
    pub enabled: bool,

    /// Collection interval in seconds
    pub collection_interval_seconds: u64,

    /// Metrics retention period in days
    pub retention_days: u32,

    /// Metrics to collect
    pub collected_metrics: Vec<MemoryMetric>,
}

/// Memory metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryMetric {
    UtilizationRate,
    FragmentationLevel,
    AttractorCount,
    ConnectionDensity,
    AccessFrequency,
    DecayRates,
}

/// Protocol system configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolConfig {
    /// Protocol registry settings
    pub registry: ProtocolRegistryConfig,

    /// Execution settings
    pub execution: ProtocolExecutionConfig,

    /// Audit and lineage settings
    pub audit: ProtocolAuditConfig,

    /// Security settings
    pub security: ProtocolSecurityConfig,
}

/// Protocol registry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolRegistryConfig {
    /// Enable protocol registry
    pub enabled: bool,

    /// Registry storage path
    pub storage_path: PathBuf,

    /// Auto-discovery settings
    pub auto_discovery: AutoDiscoveryConfig,

    /// Protocol validation
    pub validation: ProtocolValidationConfig,
}

/// Auto-discovery configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoDiscoveryConfig {
    /// Enable auto-discovery
    pub enabled: bool,

    /// Discovery paths
    pub discovery_paths: Vec<PathBuf>,

    /// Discovery interval in minutes
    pub interval_minutes: u64,
}

/// Protocol validation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolValidationConfig {
    /// Enable protocol validation
    pub enabled: bool,

    /// Validation strictness
    pub strictness: ValidationStrictness,

    /// Required metadata fields
    pub required_metadata: Vec<String>,
}

/// Protocol execution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolExecutionConfig {
    /// Default execution timeout in seconds
    pub default_timeout_seconds: u64,

    /// Maximum concurrent executions
    pub max_concurrent_executions: usize,

    /// Execution retry settings
    pub retry: RetryConfig,

    /// Resource limits
    pub resource_limits: ResourceLimitsConfig,
}

/// Retry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Enable automatic retries
    pub enabled: bool,

    /// Maximum retry attempts
    pub max_attempts: u32,

    /// Retry delay strategy
    pub delay_strategy: DelayStrategy,

    /// Backoff multiplier
    pub backoff_multiplier: f32,
}

/// Delay strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DelayStrategy {
    Fixed { delay_ms: u64 },
    Linear { initial_ms: u64, increment_ms: u64 },
    Exponential { initial_ms: u64 },
    Adaptive,
}

/// Resource limits configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimitsConfig {
    /// Maximum memory usage in MB
    pub max_memory_mb: usize,

    /// Maximum CPU usage percentage
    pub max_cpu_percent: f32,

    /// Maximum execution time in seconds
    pub max_execution_seconds: u64,

    /// Maximum disk usage in MB
    pub max_disk_mb: usize,
}

/// Protocol audit configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolAuditConfig {
    /// Enable audit logging
    pub enabled: bool,

    /// Audit log path
    pub log_path: PathBuf,

    /// Audit level
    pub audit_level: AuditLevel,

    /// Lineage tracking
    pub lineage: LineageConfig,
}

/// Audit levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditLevel {
    Basic,
    Detailed,
    Comprehensive,
    Debug,
}

/// Lineage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageConfig {
    /// Enable lineage tracking
    pub enabled: bool,

    /// Lineage storage backend
    pub storage: StorageBackend,

    /// Lineage retention period in days
    pub retention_days: u32,
}

/// Protocol security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolSecurityConfig {
    /// Enable security checks
    pub enabled: bool,

    /// Security level
    pub security_level: SecurityLevel,

    /// Allowed operations
    pub allowed_operations: Vec<String>,

    /// Sandboxing settings
    pub sandboxing: SandboxConfig,
}

/// Security levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityLevel {
    Permissive,
    Moderate,
    Strict,
    Paranoid,
}

/// Sandbox configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Enable sandboxing
    pub enabled: bool,

    /// Sandbox type
    pub sandbox_type: SandboxType,

    /// Resource restrictions
    pub restrictions: ResourceRestrictionsConfig,
}

/// Sandbox types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SandboxType {
    Process,
    Container,
    VirtualMachine,
    None,
}

/// Resource restrictions configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRestrictionsConfig {
    /// Network access allowed
    pub network_access: bool,

    /// File system access paths
    pub filesystem_access: Vec<PathBuf>,

    /// Environment variables access
    pub env_access: bool,

    /// System calls allowed
    pub syscalls_allowed: Vec<String>,
}

/// Services configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicesConfig {
    /// Graph services configuration
    pub graph: Option<GraphServicesConfig>,

    /// Embedding services configuration
    pub embedding: Option<EmbeddingServicesConfig>,

    /// Analysis services configuration
    pub analysis: Option<AnalysisServicesConfig>,
}

/// Graph services configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphServicesConfig {
    /// Enable graph services
    pub enabled: bool,

    /// Graph storage backend
    pub storage: GraphStorageConfig,

    /// Graph algorithms configuration
    pub algorithms: GraphAlgorithmsConfig,

    /// Performance settings
    pub performance: GraphPerformanceConfig,
}

/// Graph storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStorageConfig {
    /// Storage backend type
    pub backend_type: GraphStorageBackend,

    /// Connection settings
    pub connection: GraphConnectionConfig,

    /// Indexing settings
    pub indexing: GraphIndexConfig,
}

/// Graph storage backends
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphStorageBackend {
    InMemory,
    Neo4j { url: String, database: String },
    ArangoDB { url: String, database: String },
    TigerGraph { url: String },
    Custom { config: HashMap<String, String> },
}

/// Graph connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphConnectionConfig {
    /// Connection timeout in seconds
    pub timeout_seconds: u64,

    /// Maximum connections in pool
    pub max_connections: usize,

    /// Connection retry settings
    pub retry: RetryConfig,
    // Authentication removed - all endpoints are now public
}

// Authentication configuration removed - all endpoints are now public

/// Graph index configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphIndexConfig {
    /// Enable indexing
    pub enabled: bool,

    /// Index types to create
    pub index_types: Vec<IndexType>,

    /// Index update strategy
    pub update_strategy: IndexUpdateStrategy,
}

/// Index types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndexType {
    Node,
    Relationship,
    Property,
    FullText,
    Spatial,
}

/// Index update strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IndexUpdateStrategy {
    Immediate,
    Batched,
    Scheduled,
    OnDemand,
}

/// Graph algorithms configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphAlgorithmsConfig {
    /// Enable graph algorithms
    pub enabled: bool,

    /// Available algorithms
    pub algorithms: Vec<GraphAlgorithm>,

    /// Algorithm performance settings
    pub performance: AlgorithmPerformanceConfig,
}

/// Graph algorithms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphAlgorithm {
    ShortestPath,
    PageRank,
    BetweennessCentrality,
    ClusteringCoefficient,
    CommunityDetection,
    SimilarityScoring,
}

/// Algorithm performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlgorithmPerformanceConfig {
    /// Maximum execution time in seconds
    pub max_execution_seconds: u64,

    /// Memory limit in MB
    pub memory_limit_mb: usize,

    /// Parallel execution settings
    pub parallel: ParallelConfig,
}

/// Parallel execution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelConfig {
    /// Enable parallel execution
    pub enabled: bool,

    /// Number of worker threads
    pub worker_threads: usize,

    /// Batch size for parallel processing
    pub batch_size: usize,
}

/// Graph performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphPerformanceConfig {
    /// Cache settings
    pub cache: CacheConfig,

    /// Connection pooling
    pub pooling: PoolingConfig,

    /// Query optimization
    pub optimization: QueryOptimizationConfig,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Enable caching
    pub enabled: bool,

    /// Cache size in MB
    pub size_mb: usize,

    /// Cache TTL in seconds
    pub ttl_seconds: u64,

    /// Cache eviction policy
    pub eviction_policy: EvictionPolicy,
}

/// Cache eviction policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvictionPolicy {
    LRU,
    LFU,
    FIFO,
    Random,
    TTL,
}

/// Connection pooling configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolingConfig {
    /// Enable connection pooling
    pub enabled: bool,

    /// Minimum pool size
    pub min_size: usize,

    /// Maximum pool size
    pub max_size: usize,

    /// Pool timeout in seconds
    pub timeout_seconds: u64,
}

/// Query optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryOptimizationConfig {
    /// Enable query optimization
    pub enabled: bool,

    /// Query caching
    pub query_caching: bool,

    /// Query planning
    pub query_planning: bool,

    /// Index hints
    pub index_hints: bool,
}

/// Embedding services configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingServicesConfig {
    /// Enable embedding services
    pub enabled: bool,

    /// Default embedding model
    pub default_model: String,

    /// Provider type (openai, local, etc.)
    pub provider: String,

    /// API key for external providers
    pub api_key: Option<String>,

    /// Model name/identifier
    pub model: String,

    /// Model configurations
    pub models: HashMap<String, EmbeddingModelConfig>,

    /// Embedding cache configuration
    pub cache: EmbeddingCacheConfig,

    /// Performance settings
    pub performance: EmbeddingPerformanceConfig,

    /// Embedding dimensions
    pub dimensions: usize,
}

/// Embedding model configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingModelConfig {
    /// Model type
    pub model_type: EmbeddingModelType,

    /// Model parameters
    pub parameters: HashMap<String, String>,

    /// Embedding dimensions
    pub dimensions: usize,

    /// Model-specific settings
    pub settings: EmbeddingModelSettings,
}

/// Embedding model types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmbeddingModelType {
    OpenAI,
    HuggingFace,
    SentenceTransformers,
    Local,
    Custom,
}

/// Embedding model settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingModelSettings {
    /// Batch size for embedding generation
    pub batch_size: usize,

    /// Maximum input length
    pub max_input_length: usize,

    /// Normalization settings
    pub normalization: bool,

    /// Pooling strategy
    pub pooling_strategy: PoolingStrategy,
}

impl Default for EmbeddingModelSettings {
    fn default() -> Self {
        Self {
            batch_size: 32,
            max_input_length: 512,
            normalization: true,
            pooling_strategy: PoolingStrategy::Mean,
        }
    }
}

/// Pooling strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PoolingStrategy {
    Mean,
    Max,
    CLS,
    Sum,
    WeightedMean,
}

/// Embedding cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingCacheConfig {
    /// Enable embedding caching
    pub enabled: bool,

    /// Cache backend
    pub backend: CacheBackend,

    /// Cache settings
    pub settings: CacheSettings,
}

/// Cache backends
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CacheBackend {
    Memory,
    Disk { path: PathBuf },
    Redis { url: String },
    Custom { config: HashMap<String, String> },
}

/// Cache settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSettings {
    /// Maximum cache size in MB
    pub max_size_mb: usize,

    /// Cache TTL in hours
    pub ttl_hours: u64,

    /// Eviction policy
    pub eviction_policy: EvictionPolicy,

    /// Compression settings
    pub compression: CompressionConfig,
}

/// Embedding performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingPerformanceConfig {
    /// Parallel processing settings
    pub parallel: ParallelConfig,

    /// GPU acceleration settings
    pub gpu: GpuConfig,

    /// Memory management
    pub memory: MemoryManagementConfig,
}

/// GPU configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuConfig {
    /// Enable GPU acceleration
    pub enabled: bool,

    /// GPU device ID
    pub device_id: Option<u32>,

    /// GPU memory limit in MB
    pub memory_limit_mb: Option<usize>,

    /// CUDA settings
    pub cuda: CudaConfig,
}

/// CUDA configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CudaConfig {
    /// Enable CUDA
    pub enabled: bool,

    /// CUDA version
    pub version: Option<String>,

    /// CUDA compute capability
    pub compute_capability: Option<String>,
}

/// Memory management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryManagementConfig {
    /// Memory limit in MB
    pub limit_mb: usize,

    /// Memory monitoring
    pub monitoring: bool,

    /// Garbage collection settings
    pub gc: GcConfig,
}

/// Garbage collection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GcConfig {
    /// Enable automatic garbage collection
    pub enabled: bool,

    /// GC interval in seconds
    pub interval_seconds: u64,

    /// Memory threshold for GC trigger
    pub memory_threshold: f32,
}

/// Analysis services configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisServicesConfig {
    /// Enable analysis services
    pub enabled: bool,

    /// Analysis engines
    pub engines: Vec<AnalysisEngine>,

    /// Analysis performance settings
    pub performance: AnalysisPerformanceConfig,

    /// Result storage settings
    pub storage: AnalysisStorageConfig,
}

/// Analysis engines
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnalysisEngine {
    Semantic,
    Syntactic,
    Performance,
    Security,
    Quality,
    Custom { name: String },
}

/// Analysis performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisPerformanceConfig {
    /// Parallel analysis settings
    pub parallel: ParallelConfig,

    /// Analysis timeout in seconds
    pub timeout_seconds: u64,

    /// Memory limit for analysis
    pub memory_limit_mb: usize,
}

/// Analysis storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisStorageConfig {
    /// Storage backend
    pub backend: StorageBackend,

    /// Result retention period in days
    pub retention_days: u32,

    /// Compression settings
    pub compression: CompressionConfig,
}

/// API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// REST API configuration
    pub rest: RestApiConfig,

    /// GraphQL API configuration
    pub graphql: GraphqlApiConfig,

    /// WebSocket configuration
    pub websocket: WebSocketConfig,

    /// API security settings
    pub security: ApiSecurityConfig,
}

/// REST API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestApiConfig {
    /// Enable REST API
    pub enabled: bool,

    /// Bind address
    pub bind_address: String,

    /// Port number
    pub port: u16,

    /// API version
    pub version: String,
}

/// GraphQL API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphqlApiConfig {
    /// Enable GraphQL API
    pub enabled: bool,

    /// GraphQL endpoint path
    pub endpoint: String,

    /// GraphQL playground
    pub playground: bool,

    /// Schema settings
    pub schema: GraphqlSchemaConfig,
}

/// GraphQL schema configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphqlSchemaConfig {
    /// Enable introspection
    pub introspection: bool,

    /// Schema validation
    pub validation: bool,

    /// Query complexity analysis
    pub complexity_analysis: bool,

    /// Maximum query depth
    pub max_query_depth: u32,
}

/// WebSocket configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketConfig {
    /// Enable WebSocket support
    pub enabled: bool,

    /// WebSocket endpoint path
    pub endpoint: String,

    /// Connection settings
    pub connection: WebSocketConnectionConfig,

    /// Message settings
    pub message: WebSocketMessageConfig,
}

/// Reconnection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectionConfig {
    /// Enable automatic reconnection
    pub enabled: bool,

    /// Maximum reconnection attempts
    pub max_attempts: u32,

    /// Initial delay in milliseconds
    pub initial_delay_ms: u64,

    /// Backoff multiplier
    pub backoff_multiplier: f32,
}

/// WebSocket connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketConnectionConfig {
    /// Maximum connections
    pub max_connections: usize,

    /// Connection timeout in seconds
    pub timeout_seconds: u64,

    /// Heartbeat interval in seconds
    pub heartbeat_interval_seconds: u64,

    /// Reconnection settings
    pub reconnection: ReconnectionConfig,
}

/// Message validation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageValidationConfig {
    /// Enable message validation
    pub enabled: bool,

    /// Validation rules
    pub rules: Vec<ValidationRule>,

    /// Strict validation
    pub strict: bool,
}

/// WebSocket message configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketMessageConfig {
    /// Maximum message size in bytes
    pub max_message_size: usize,

    /// Enable compression
    pub compression: bool,

    /// Message batching
    pub batching: MessageBatchingConfig,

    /// Message validation
    pub validation: MessageValidationConfig,
}

/// Message batching configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBatchingConfig {
    /// Enable message batching
    pub enabled: bool,

    /// Batch size
    pub batch_size: usize,

    /// Batch timeout in milliseconds
    pub batch_timeout_ms: u64,
}

/// API security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSecurityConfig {
    /// Enable API security
    pub enabled: bool,

    // Authentication removed - all endpoints are now public
    /// TLS settings
    pub tls: TlsConfig,

    /// CORS settings
    pub cors: CorsConfig,
}

// API authentication configuration removed - all endpoints are now public

/// API key configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyConfig {
    /// API key header name
    pub header_name: String,

    /// Valid API keys
    pub valid_keys: Vec<String>,

    /// Key rotation settings
    pub rotation: KeyRotationConfig,
}

/// Key rotation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRotationConfig {
    /// Enable key rotation
    pub enabled: bool,

    /// Rotation interval in days
    pub interval_days: u32,

    /// Grace period in hours
    pub grace_period_hours: u64,
}

// API authorization configuration removed - all endpoints are now public

// Role and authorization configuration removed - all endpoints are now public

/// TLS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    /// Enable TLS
    pub enabled: bool,

    /// Certificate file path
    pub cert_file: PathBuf,

    /// Private key file path
    pub key_file: PathBuf,

    /// TLS version
    pub version: TlsVersion,

    /// Cipher suites
    pub cipher_suites: Vec<String>,
}

/// TLS versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TlsVersion {
    V1_2,
    V1_3,
}

/// CORS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    /// Enable CORS
    pub enabled: bool,

    /// Allowed origins
    pub allowed_origins: Vec<String>,

    /// Allowed methods
    pub allowed_methods: Vec<String>,

    /// Allowed headers
    pub allowed_headers: Vec<String>,

    /// Allow credentials
    pub allow_credentials: bool,

    /// Max age in seconds
    pub max_age_seconds: u64,
}

/// Monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    /// Logging configuration
    pub logging: LoggingConfig,

    /// Metrics configuration
    pub metrics: MetricsConfig,

    /// Tracing configuration
    pub tracing: TracingConfig,

    /// Health check configuration
    pub health: HealthConfig,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Enable logging
    pub enabled: bool,

    /// Log level
    pub level: LogLevel,

    /// Log format
    pub format: LogFormat,

    /// Log outputs
    pub outputs: Vec<LogOutput>,

    /// Log rotation
    pub rotation: LogRotationConfig,
}

/// Log levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Log formats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogFormat {
    Text,
    Json,
    Structured,
}

/// Log outputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogOutput {
    Console,
    File { path: PathBuf },
    Syslog,
    Network { url: String },
}

/// Log rotation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRotationConfig {
    /// Enable log rotation
    pub enabled: bool,

    /// Rotation strategy
    pub strategy: RotationStrategy,

    /// Maximum file size in MB
    pub max_file_size_mb: usize,

    /// Maximum number of files
    pub max_files: u32,
}

/// Rotation strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RotationStrategy {
    Size,
    Time,
    Daily,
    Weekly,
    Monthly,
}

/// Metrics configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Enable metrics collection
    pub enabled: bool,

    /// Metrics format
    pub format: MetricsFormat,

    /// Collection interval in seconds
    pub collection_interval_seconds: u64,

    /// Metrics exporters
    pub exporters: Vec<MetricsExporter>,

    /// Custom metrics
    pub custom_metrics: Vec<CustomMetricConfig>,
}

/// Metrics formats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricsFormat {
    Prometheus,
    StatsD,
    InfluxDB,
    OpenTelemetry,
}

/// Metrics exporters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricsExporter {
    Console,
    File { path: PathBuf },
    Http { endpoint: String },
    Push { gateway: String },
}

/// Custom metric configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomMetricConfig {
    /// Metric name
    pub name: String,

    /// Metric type
    pub metric_type: MetricType,

    /// Metric description
    pub description: String,

    /// Metric labels
    pub labels: Vec<String>,
}

/// Metric types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
    Summary,
}

/// Tracing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracingConfig {
    /// Enable tracing
    pub enabled: bool,

    /// Tracing service name
    pub service_name: String,

    /// Tracing exporters
    pub exporters: Vec<TracingExporter>,

    /// Sampling configuration
    pub sampling: SamplingConfig,
}

/// Tracing exporters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TracingExporter {
    Console,
    Jaeger { endpoint: String },
    Zipkin { endpoint: String },
    OTLP { endpoint: String },
}

/// Sampling configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingConfig {
    /// Sampling strategy
    pub strategy: SamplingStrategy,

    /// Sampling rate (0.0 to 1.0)
    pub rate: f32,
}

/// Sampling strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SamplingStrategy {
    Always,
    Never,
    Probabilistic,
    RateLimited,
    Adaptive,
}

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthConfig {
    /// Enable health checks
    pub enabled: bool,

    /// Health check endpoint
    pub endpoint: String,

    /// Check interval in seconds
    pub interval_seconds: u64,

    /// Health check components
    pub components: Vec<HealthCheckComponent>,
}

/// Health check components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthCheckComponent {
    Database,
    Cache,
    ExternalService { name: String },
    FileSystem,
    Memory,
    CPU,
}

/// Performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Thread pool configuration
    pub thread_pool: ThreadPoolConfig,

    /// Memory management
    pub memory: MemoryManagementConfig,

    /// I/O settings
    pub io: IoConfig,

    /// Optimization settings
    pub optimization: PerformanceOptimizationConfig,
}

/// Thread pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadPoolConfig {
    /// Core thread count
    pub core_threads: usize,

    /// Maximum thread count
    pub max_threads: usize,

    /// Thread keep-alive time in seconds
    pub keep_alive_seconds: u64,

    /// Queue size
    pub queue_size: usize,
}

/// I/O configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoConfig {
    /// I/O buffer size in bytes
    pub buffer_size: usize,

    /// Async I/O settings
    pub async_io: AsyncIoConfig,

    /// File I/O settings
    pub file_io: FileIoConfig,
}

/// Async I/O configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncIoConfig {
    /// Enable async I/O
    pub enabled: bool,

    /// Async runtime
    pub runtime: AsyncRuntime,

    /// Worker threads
    pub worker_threads: usize,
}

/// Async runtimes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AsyncRuntime {
    Tokio,
    AsyncStd,
    Smol,
}

/// File I/O configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileIoConfig {
    /// Enable memory mapping
    pub memory_mapping: bool,

    /// File system cache
    pub fs_cache: bool,

    /// Read-ahead size in bytes
    pub read_ahead_bytes: usize,
}

/// Performance optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceOptimizationConfig {
    /// Enable JIT compilation
    pub jit_compilation: bool,

    /// CPU optimization
    pub cpu_optimization: CpuOptimizationConfig,

    /// Memory optimization
    pub memory_optimization: MemoryOptimizationSettings,
}

/// CPU optimization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuOptimizationConfig {
    /// Enable SIMD instructions
    pub simd: bool,

    /// Target CPU features
    pub target_features: Vec<String>,

    /// Optimization level
    pub optimization_level: OptimizationLevel,
}

/// Optimization levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OptimizationLevel {
    Debug,
    Release,
    Aggressive,
}

/// Memory optimization settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryOptimizationSettings {
    /// Enable memory pooling
    pub pooling: bool,

    /// Arena allocation
    pub arena_allocation: bool,

    /// Memory alignment
    pub alignment: usize,
}

/// Enhanced security configuration for ContextNest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Sandbox security configuration
    pub sandbox: SandboxSecurityConfig,

    /// Plugin verification configuration
    pub plugin_verification: PluginVerificationConfig,

    /// Resource monitoring configuration
    pub resource_monitoring: ResourceMonitoringConfig,

    /// Audit logging configuration
    pub audit_logging: AuditLoggingConfig,

    /// Threat detection configuration
    pub threat_detection: ThreatDetectionConfig,
}

/// Sandbox security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxSecurityConfig {
    /// Enable sandbox isolation
    pub enabled: bool,

    /// Default isolation type
    pub default_isolation_type: SandboxIsolationType,

    /// Resource limits for sandboxes
    pub resource_limits: SandboxResourceLimits,

    /// Network access control
    pub network_access: NetworkAccessConfig,

    /// File system access control
    pub filesystem_access: FilesystemAccessConfig,

    /// Process isolation settings
    pub process_isolation: ProcessIsolationConfig,
}

/// Sandbox isolation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SandboxIsolationType {
    /// Process-level isolation
    Process,
    /// Container-level isolation
    Container,
    /// Virtual machine isolation
    VirtualMachine,
    /// WebAssembly isolation
    WebAssembly,
    /// No isolation (development only)
    None,
}

/// Resource limits for sandboxes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResourceLimits {
    /// Maximum memory in MB
    pub max_memory_mb: usize,

    /// Maximum CPU percentage
    pub max_cpu_percent: f32,

    /// Maximum execution time in seconds
    pub max_execution_seconds: u64,

    /// Maximum disk usage in MB
    pub max_disk_mb: usize,

    /// Maximum network connections
    pub max_network_connections: usize,
}

/// Network access configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkAccessConfig {
    /// Allow network access
    pub allowed: bool,

    /// Allowed endpoints
    pub allowed_endpoints: Vec<String>,

    /// Blocked endpoints
    pub blocked_endpoints: Vec<String>,

    /// Allow DNS resolution
    pub allow_dns: bool,

    /// Proxy configuration
    pub proxy: Option<ProxyConfig>,
}

/// Proxy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Proxy URL
    pub url: String,

    /// Proxy authentication
    pub auth: Option<ProxyAuth>,
}

/// Proxy authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyAuth {
    /// Username
    pub username: String,

    /// Password
    pub password: String,
}

/// Filesystem access configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemAccessConfig {
    /// Enable filesystem access
    pub enabled: bool,

    /// Read-only paths
    pub read_only_paths: Vec<PathBuf>,

    /// Read-write paths
    pub read_write_paths: Vec<PathBuf>,

    /// Temporary directory
    pub temp_directory: Option<PathBuf>,

    /// Allow device access
    pub allow_devices: bool,
}

/// Process isolation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessIsolationConfig {
    /// Dedicated user for sandbox
    pub sandbox_user: Option<String>,

    /// Dedicated group for sandbox
    pub sandbox_group: Option<String>,

    /// Namespace isolation
    pub enable_namespaces: bool,

    /// Capabilities to drop
    pub drop_capabilities: Vec<String>,

    /// Seccomp filter
    pub enable_seccomp: bool,
}

/// Plugin verification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginVerificationConfig {
    /// Enable plugin verification
    pub enabled: bool,

    /// Require digital signatures
    pub require_signatures: bool,

    /// Trusted public keys
    pub trusted_keys: Vec<String>,

    /// Verification cache TTL in hours
    pub cache_ttl_hours: u64,

    /// Allow unsigned plugins in development
    pub allow_unsigned_dev: bool,
}

/// Resource monitoring configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMonitoringConfig {
    /// Enable resource monitoring
    pub enabled: bool,

    /// Monitoring interval in seconds
    pub interval_seconds: u64,

    /// Metrics retention in hours
    pub retention_hours: u64,

    /// Alert thresholds
    pub alert_thresholds: ResourceAlertThresholds,

    /// Enable automatic cleanup
    pub enable_cleanup: bool,
}

/// Resource alert thresholds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAlertThresholds {
    /// Memory usage threshold (percentage)
    pub memory_threshold: f32,

    /// CPU usage threshold (percentage)
    pub cpu_threshold: f32,

    /// Disk usage threshold (percentage)
    pub disk_threshold: f32,

    /// Network usage threshold (percentage)
    pub network_threshold: f32,
}

/// Audit logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLoggingConfig {
    /// Enable audit logging
    pub enabled: bool,

    /// Log file path
    pub log_file: PathBuf,

    /// Log rotation settings
    pub rotation: LogRotationConfig,

    /// Event types to log
    pub event_types: Vec<AuditEventType>,

    /// Log format
    pub format: AuditLogFormat,
}

/// Audit event types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditEventType {
    /// Security events
    Security,
    /// Plugin loading/unloading
    PluginManagement,
    /// Resource violations
    ResourceViolation,
    /// Access control events
    AccessControl,
    /// System events
    System,
    /// All events
    All,
}

/// Audit log formats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditLogFormat {
    /// JSON format
    Json,
    /// Structured text format
    Structured,
    /// Syslog format
    Syslog,
}

/// Threat detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatDetectionConfig {
    /// Enable threat detection
    pub enabled: bool,

    /// Detection rules
    pub rules: Vec<ThreatDetectionRule>,

    /// Response actions
    pub response_actions: Vec<ThreatResponseAction>,

    /// False positive tolerance
    pub false_positive_tolerance: f32,
}

/// Threat detection rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatDetectionRule {
    /// Rule name
    pub name: String,

    /// Rule pattern
    pub pattern: String,

    /// Severity level
    pub severity: ThreatSeverity,

    /// Rule enabled
    pub enabled: bool,
}

/// Threat severity levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThreatSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Threat response action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatResponseAction {
    /// Action name
    pub name: String,

    /// Action type
    pub action_type: ThreatActionType,

    /// Action parameters
    pub parameters: HashMap<String, String>,
}

/// Threat action types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThreatActionType {
    /// Block the action
    Block,
    /// Quarantine the plugin
    Quarantine,
    /// Alert administrators
    Alert,
    /// Log the event
    Log,
    /// Terminate the process
    Terminate,
}

/// Real-time synchronization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynchronizationConfig {
    /// Enable real-time synchronization
    pub enabled: bool,

    /// WebSocket configuration
    pub websocket: WebSocketSyncConfig,

    /// Conflict resolution strategy
    pub conflict_resolution: ConflictResolutionConfig,

    /// Consistency settings
    pub consistency: ConsistencyConfig,

    /// Performance settings
    pub performance: SyncPerformanceConfig,
}

/// WebSocket synchronization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketSyncConfig {
    /// WebSocket endpoint
    pub endpoint: String,

    /// Connection settings
    pub connection: WebSocketConnectionConfig,

    /// Message settings
    pub message: WebSocketMessageConfig,

    /// Security settings
    pub security: WebSocketSecurityConfig,
}

/// WebSocket security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketSecurityConfig {
    /// Enable authentication
    pub authentication: bool,

    /// Enable authorization
    pub authorization: bool,

    /// TLS configuration
    pub tls: Option<TlsConfig>,
}

/// Conflict resolution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictResolutionConfig {
    /// Default resolution strategy
    pub default_strategy: ConflictResolutionStrategy,

    /// Type-specific strategies
    pub type_strategies: HashMap<String, ConflictResolutionStrategy>,

    /// Manual resolution settings
    pub manual_resolution: ManualResolutionConfig,
}

/// Conflict resolution strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConflictResolutionStrategy {
    /// Last write wins
    LastWriteWins,
    /// First write wins
    FirstWriteWins,
    /// Merge conflicts
    Merge,
    /// Manual resolution required
    Manual,
    /// Timestamp-based resolution
    TimestampBased,
    /// User priority-based resolution
    PriorityBased,
}

/// Manual resolution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualResolutionConfig {
    /// Enable manual resolution
    pub enabled: bool,

    /// Resolution timeout in minutes
    pub timeout_minutes: u64,

    /// Default resolution if timeout
    pub default_resolution: ConflictResolutionStrategy,
}

/// Consistency configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsistencyConfig {
    /// Consistency level
    pub level: ConsistencyLevel,

    /// Validation settings
    pub validation: ConsistencyValidationConfig,

    /// Healing settings
    pub healing: ConsistencyHealingConfig,
}

/// Consistency levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsistencyLevel {
    /// Strong consistency
    Strong,
    /// Eventual consistency
    Eventual,
    /// Weak consistency
    Weak,
    /// Causal consistency
    Causal,
}

/// Consistency validation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsistencyValidationConfig {
    /// Enable validation
    pub enabled: bool,

    /// Validation interval in seconds
    pub interval_seconds: u64,

    /// Validation rules
    pub rules: Vec<ConsistencyRule>,
}

/// Consistency rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsistencyRule {
    /// Rule name
    pub name: String,

    /// Rule pattern
    pub pattern: String,

    /// Action on violation
    pub action: ConsistencyAction,
}

/// Consistency actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsistencyAction {
    /// Log the violation
    Log,
    /// Attempt automatic healing
    Heal,
    /// Alert administrators
    Alert,
    /// Quarantine the data
    Quarantine,
}

/// Consistency healing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsistencyHealingConfig {
    /// Enable automatic healing
    pub enabled: bool,

    /// Healing strategies
    pub strategies: Vec<HealingStrategy>,

    /// Healing timeout in minutes
    pub timeout_minutes: u64,
}

/// Healing strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealingStrategy {
    /// Strategy name
    pub name: String,

    /// Strategy type
    pub strategy_type: HealingStrategyType,

    /// Strategy parameters
    pub parameters: HashMap<String, String>,
}

/// Healing strategy types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealingStrategyType {
    /// Data synchronization
    Synchronize,
    /// Data repair
    Repair,
    /// Data restoration
    Restore,
    /// Data reconstruction
    Reconstruct,
}

/// Synchronization performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPerformanceConfig {
    /// Batch size for synchronization
    pub batch_size: usize,

    /// Parallel synchronization workers
    pub parallel_workers: usize,

    /// Compression settings
    pub compression: CompressionConfig,

    /// Caching settings
    pub caching: SyncCacheConfig,
}

/// Synchronization cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncCacheConfig {
    /// Enable caching
    pub enabled: bool,

    /// Cache size in MB
    pub size_mb: usize,

    /// Cache TTL in minutes
    pub ttl_minutes: u64,

    /// Cache eviction policy
    pub eviction_policy: EvictionPolicy,
}

/// Plugin system configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    /// Enable plugin system
    pub enabled: bool,

    /// Plugin directories
    pub plugin_directories: Vec<PathBuf>,

    /// Auto-discovery settings
    pub auto_discovery: PluginAutoDiscoveryConfig,

    /// Hot reloading settings
    pub hot_reload: PluginHotReloadConfig,

    /// Plugin sandbox configuration
    pub sandbox: PluginSandboxConfig,

    /// Plugin lifecycle configuration
    pub lifecycle: PluginLifecycleConfig,
}

/// Plugin auto-discovery configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginAutoDiscoveryConfig {
    /// Enable auto-discovery
    pub enabled: bool,

    /// Discovery interval in minutes
    pub interval_minutes: u64,

    /// Scan subdirectories
    pub scan_subdirectories: bool,

    /// File patterns to match
    pub file_patterns: Vec<String>,
}

/// Plugin hot reload configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHotReloadConfig {
    /// Enable hot reloading
    pub enabled: bool,

    /// Reload on file changes
    pub watch_files: bool,

    /// Graceful reload timeout in seconds
    pub timeout_seconds: u64,

    /// Preserve state during reload
    pub preserve_state: bool,

    /// Directories to watch for changes
    pub watch_directories: Vec<PathBuf>,

    /// File patterns to watch (e.g., ["*.rs", "*.json", "*.toml"])
    pub watch_patterns: Vec<String>,

    /// Debounce time in milliseconds (to prevent rapid reloads)
    pub debounce_ms: u64,

    /// Enable automatic reloading
    pub auto_reload: bool,

    /// Maximum number of reload attempts
    pub max_reload_attempts: u32,

    /// Delay between reload attempts in milliseconds
    pub reload_delay_ms: u64,

    /// Enable rollback on failure
    pub enable_rollback: bool,
}

/// Plugin sandbox configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSandboxConfig {
    /// Enable sandboxing for all plugins
    pub enabled: bool,

    /// Default sandbox configuration
    pub default_config: SandboxSecurityConfig,

    /// Per-plugin sandbox overrides
    pub plugin_overrides: HashMap<String, SandboxSecurityConfig>,
}

/// Plugin lifecycle configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginLifecycleConfig {
    /// Initialization timeout in seconds
    pub init_timeout_seconds: u64,

    /// Shutdown timeout in seconds
    pub shutdown_timeout_seconds: u64,

    /// Health check interval in seconds
    pub health_check_interval_seconds: u64,

    /// Maximum restart attempts
    pub max_restart_attempts: u32,

    /// Restart delay in seconds
    pub restart_delay_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            context: ContextConfig::default(),
            neural_field: NeuralFieldConfig::default(),
            memory: MemoryConfig::default(),
            protocols: ProtocolConfig::default(),
            services: ServicesConfig::default(),
            api: ApiConfig::default(),
            monitoring: MonitoringConfig::default(),
            performance: PerformanceConfig::default(),
            database: DatabaseConfig::default(),
            security: SecurityConfig::default(),
            synchronization: SynchronizationConfig::default(),
            plugins: PluginConfig::default(),
        }
    }
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_token_budget: std::env::var("CONTEXTNEST_TOKEN_BUDGET")
                .ok()
                .and_then(|budget| budget.parse().ok())
                .unwrap_or(4096),
            default_level: ContextLevel::Atomic,
            auto_enhancement: AutoEnhancementConfig::default(),
            persistence: ContextPersistenceConfig::default(),
            validation: ContextValidationConfig::default(),
        }
    }
}

impl Default for AutoEnhancementConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            enhancement_threshold: 0.8,
            max_iterations: 5,
            strategy: EnhancementStrategy::Balanced,
        }
    }
}

impl Default for ContextPersistenceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            storage_path: PathBuf::from("./data/context"),
            save_interval_seconds: 300, // 5 minutes
            max_stored_sessions: 1000,
            compression: CompressionConfig::default(),
        }
    }
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            algorithm: CompressionAlgorithm::Zstd,
            level: 3,
        }
    }
}

impl Default for ContextValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strictness: ValidationStrictness::Moderate,
            max_validation_time_ms: 1000,
            rules: vec![
                ValidationRule::TokenBudgetLimit,
                ValidationRule::SemanticCoherence,
                ValidationRule::StructuralIntegrity,
            ],
        }
    }
}

impl Default for NeuralFieldConfig {
    fn default() -> Self {
        Self {
            embedding_dim: 1536, // Default OpenAI embedding dimension
            dynamics: FieldDynamicsConfig::default(),
            patterns: PatternConfig::default(),
            resonance: ResonanceConfig::default(),
            coherence: CoherenceConfig::default(),
        }
    }
}

impl Default for FieldDynamicsConfig {
    fn default() -> Self {
        Self {
            default_decay_rate: 0.01,
            min_strength_threshold: 0.1,
            max_field_capacity: 10000,
            update_frequency_ms: 100,
            adaptive_dynamics: true,
        }
    }
}

impl Default for PatternConfig {
    fn default() -> Self {
        Self {
            default_strength: 1.0,
            similarity_threshold: 0.8,
            max_patterns_per_field: 1000,
            clustering: ClusteringConfig::default(),
        }
    }
}

impl Default for ClusteringConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            algorithm: ClusteringAlgorithm::Adaptive,
            min_cluster_size: 3,
            max_cluster_distance: 0.5,
        }
    }
}

impl Default for ResonanceConfig {
    fn default() -> Self {
        Self {
            default_amplification: 1.2,
            frequency_hz: 10.0,
            damping_factor: 0.1,
            enable_harmonics: true,
            scaffolding: ScaffoldingConfig::default(),
        }
    }
}

impl Default for ScaffoldingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strength: 1.5,
            duration_seconds: 60,
            auto_triggers: vec![
                ScaffoldingTrigger::LowCoherence,
                ScaffoldingTrigger::PatternDegradation,
            ],
        }
    }
}

impl Default for CoherenceConfig {
    fn default() -> Self {
        Self {
            min_coherence_threshold: 0.6,
            measurement_interval_ms: 1000,
            auto_repair: AutoRepairConfig::default(),
            monitoring: CoherenceMonitoringConfig::default(),
        }
    }
}

impl Default for AutoRepairConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: RepairStrategy::Balanced,
            max_attempts: 3,
            cooldown_seconds: 30,
        }
    }
}

impl Default for CoherenceMonitoringConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            alert_threshold: 0.5,
            history_size: 1000,
            trend_analysis: TrendAnalysisConfig::default(),
        }
    }
}

impl Default for TrendAnalysisConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            window_size: 100,
            sensitivity: 0.1,
            prediction_horizon: 10,
        }
    }
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            attractors: AttractorConfig::default(),
            persistence: MemoryPersistenceConfig::default(),
            optimization: MemoryOptimizationConfig::default(),
            metrics: MemoryMetricsConfig::default(),
        }
    }
}

impl Default for AttractorConfig {
    fn default() -> Self {
        Self {
            default_strength: 1.0,
            default_importance: 0.5,
            max_attractors_per_field: 100,
            decay: AttractorDecayConfig::default(),
            connections: ConnectionConfig::default(),
        }
    }
}

impl Default for AttractorDecayConfig {
    fn default() -> Self {
        Self {
            base_decay_rate: 0.001,
            usage_factor: 0.1,
            importance_factor: 0.2,
            connection_factor: 0.05,
        }
    }
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            max_connections_per_attractor: 10,
            connection_threshold: 0.7,
            auto_connection: AutoConnectionConfig::default(),
        }
    }
}

impl Default for AutoConnectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            similarity_threshold: 0.8,
            max_per_update: 5,
        }
    }
}

impl Default for MemoryPersistenceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            backend: StorageBackend::FileSystem {
                path: PathBuf::from("./data/memory"),
            },
            strategy: PersistenceStrategy::Batched { batch_size: 100 },
            backup: BackupConfig::default(),
        }
    }
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_hours: 24,
            max_backups: 7,
            compression: CompressionConfig::default(),
        }
    }
}

impl Default for MemoryOptimizationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_minutes: 60,
            strategies: vec![
                OptimizationStrategy::RemoveWeakAttractors,
                OptimizationStrategy::PruneUnusedConnections,
            ],
            thresholds: OptimizationThresholds::default(),
        }
    }
}

impl Default for OptimizationThresholds {
    fn default() -> Self {
        Self {
            memory_usage_threshold: 0.8,
            fragmentation_threshold: 0.3,
            efficiency_threshold: 0.6,
        }
    }
}

impl Default for MemoryMetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            collection_interval_seconds: 60,
            retention_days: 30,
            collected_metrics: vec![
                MemoryMetric::UtilizationRate,
                MemoryMetric::FragmentationLevel,
                MemoryMetric::AttractorCount,
            ],
        }
    }
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            registry: ProtocolRegistryConfig::default(),
            execution: ProtocolExecutionConfig::default(),
            audit: ProtocolAuditConfig::default(),
            security: ProtocolSecurityConfig::default(),
        }
    }
}

impl Default for ProtocolRegistryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            storage_path: PathBuf::from("./data/protocols"),
            auto_discovery: AutoDiscoveryConfig::default(),
            validation: ProtocolValidationConfig::default(),
        }
    }
}

impl Default for AutoDiscoveryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            discovery_paths: vec![PathBuf::from("./protocols")],
            interval_minutes: 60,
        }
    }
}

impl Default for ProtocolValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strictness: ValidationStrictness::Moderate,
            required_metadata: vec![
                "name".to_string(),
                "version".to_string(),
                "description".to_string(),
            ],
        }
    }
}

impl Default for ProtocolExecutionConfig {
    fn default() -> Self {
        Self {
            default_timeout_seconds: 300, // 5 minutes
            max_concurrent_executions: 10,
            retry: RetryConfig::default(),
            resource_limits: ResourceLimitsConfig::default(),
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_attempts: 3,
            delay_strategy: DelayStrategy::Exponential { initial_ms: 1000 },
            backoff_multiplier: 2.0,
        }
    }
}

impl Default for ResourceLimitsConfig {
    fn default() -> Self {
        Self {
            max_memory_mb: 1024, // 1GB
            max_cpu_percent: 50.0,
            max_execution_seconds: 600, // 10 minutes
            max_disk_mb: 512,           // 512MB
        }
    }
}

impl Default for ProtocolAuditConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            log_path: PathBuf::from("./logs/protocol_audit.log"),
            audit_level: AuditLevel::Detailed,
            lineage: LineageConfig::default(),
        }
    }
}

impl Default for LineageConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            storage: StorageBackend::FileSystem {
                path: PathBuf::from("./data/lineage"),
            },
            retention_days: 90,
        }
    }
}

impl Default for ProtocolSecurityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            security_level: SecurityLevel::Moderate,
            allowed_operations: vec![
                "field.resonance_scaffold".to_string(),
                "field.self_repair".to_string(),
                "memory.persistence_attractor".to_string(),
            ],
            sandboxing: SandboxConfig::default(),
        }
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sandbox_type: SandboxType::Process,
            restrictions: ResourceRestrictionsConfig::default(),
        }
    }
}

impl Default for ResourceRestrictionsConfig {
    fn default() -> Self {
        Self {
            network_access: false,
            filesystem_access: vec![PathBuf::from("./data"), PathBuf::from("./temp")],
            env_access: false,
            syscalls_allowed: vec!["read".to_string(), "write".to_string(), "open".to_string()],
        }
    }
}

impl Default for ServicesConfig {
    fn default() -> Self {
        Self {
            graph: Some(GraphServicesConfig::default()),
            embedding: Some(EmbeddingServicesConfig::default()),
            analysis: Some(AnalysisServicesConfig::default()),
        }
    }
}

impl Default for GraphServicesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            storage: GraphStorageConfig::default(),
            algorithms: GraphAlgorithmsConfig::default(),
            performance: GraphPerformanceConfig::default(),
        }
    }
}

impl Default for GraphStorageConfig {
    fn default() -> Self {
        Self {
            backend_type: GraphStorageBackend::InMemory,
            connection: GraphConnectionConfig::default(),
            indexing: GraphIndexConfig::default(),
        }
    }
}

impl Default for GraphConnectionConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 30,
            max_connections: 10,
            retry: RetryConfig::default(),
        }
    }
}

impl Default for GraphIndexConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            index_types: vec![IndexType::Node, IndexType::Relationship],
            update_strategy: IndexUpdateStrategy::Immediate,
        }
    }
}

impl Default for GraphAlgorithmsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            algorithms: vec![
                GraphAlgorithm::ShortestPath,
                GraphAlgorithm::SimilarityScoring,
            ],
            performance: AlgorithmPerformanceConfig::default(),
        }
    }
}

impl Default for AlgorithmPerformanceConfig {
    fn default() -> Self {
        Self {
            max_execution_seconds: 60,
            memory_limit_mb: 512,
            parallel: ParallelConfig::default(),
        }
    }
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            worker_threads: num_cpus::get(),
            batch_size: 1000,
        }
    }
}

impl Default for GraphPerformanceConfig {
    fn default() -> Self {
        Self {
            cache: CacheConfig::default(),
            pooling: PoolingConfig::default(),
            optimization: QueryOptimizationConfig::default(),
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            size_mb: 256,
            ttl_seconds: 3600, // 1 hour
            eviction_policy: EvictionPolicy::LRU,
        }
    }
}

impl Default for PoolingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_size: 2,
            max_size: 10,
            timeout_seconds: 30,
        }
    }
}

impl Default for QueryOptimizationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            query_caching: true,
            query_planning: true,
            index_hints: true,
        }
    }
}

impl Default for EmbeddingServicesConfig {
    fn default() -> Self {
        // v0.1.0 default: a self-contained `local` model so tests + mock-mode
        // callers can call `generate_embedding` without an API key. Production
        // deployments that want OpenAI can override `default_model` + supply
        // an `api_key` via env/config. The local model is TF-IDF-flavored
        // semantic feature extraction (see `EmbeddingService::generate_local_embedding`).
        let mut models = HashMap::new();
        models.insert(
            "local".to_string(),
            EmbeddingModelConfig {
                model_type: EmbeddingModelType::Local,
                parameters: HashMap::new(),
                dimensions: 256,
                settings: EmbeddingModelSettings::default(),
            },
        );

        Self {
            enabled: true,
            default_model: "local".to_string(),
            provider: "local".to_string(),
            api_key: None,
            model: "local".to_string(),
            models,
            cache: EmbeddingCacheConfig::default(),
            performance: EmbeddingPerformanceConfig::default(),
            dimensions: 256,
        }
    }
}

impl Default for EmbeddingCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            backend: CacheBackend::Memory,
            settings: CacheSettings::default(),
        }
    }
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self {
            max_size_mb: 512,
            ttl_hours: 24,
            eviction_policy: EvictionPolicy::LRU,
            compression: CompressionConfig::default(),
        }
    }
}

impl Default for EmbeddingPerformanceConfig {
    fn default() -> Self {
        Self {
            parallel: ParallelConfig::default(),
            gpu: GpuConfig::default(),
            memory: MemoryManagementConfig::default(),
        }
    }
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Default to CPU
            device_id: None,
            memory_limit_mb: None,
            cuda: CudaConfig::default(),
        }
    }
}

impl Default for CudaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            version: None,
            compute_capability: None,
        }
    }
}

impl Default for MemoryManagementConfig {
    fn default() -> Self {
        Self {
            limit_mb: 2048, // 2GB
            monitoring: true,
            gc: GcConfig::default(),
        }
    }
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_seconds: 300, // 5 minutes
            memory_threshold: 0.8,
        }
    }
}

impl Default for AnalysisServicesConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            engines: vec![AnalysisEngine::Semantic, AnalysisEngine::Performance],
            performance: AnalysisPerformanceConfig::default(),
            storage: AnalysisStorageConfig::default(),
        }
    }
}

impl Default for AnalysisPerformanceConfig {
    fn default() -> Self {
        Self {
            parallel: ParallelConfig::default(),
            timeout_seconds: 300,  // 5 minutes
            memory_limit_mb: 1024, // 1GB
        }
    }
}

impl Default for AnalysisStorageConfig {
    fn default() -> Self {
        Self {
            backend: StorageBackend::FileSystem {
                path: PathBuf::from("./data/analysis"),
            },
            retention_days: 30,
            compression: CompressionConfig::default(),
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            rest: RestApiConfig::default(),
            graphql: GraphqlApiConfig::default(),
            websocket: WebSocketConfig::default(),
            security: ApiSecurityConfig::default(),
        }
    }
}

impl Default for RestApiConfig {
    fn default() -> Self {
        Self {
            enabled: std::env::var("CONTEXTNEST_REST_API_ENABLED")
                .map(|v| v.parse().unwrap_or(true))
                .unwrap_or(true),
            bind_address: std::env::var("CONTEXTNEST_SERVER_HOST")
                .unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: std::env::var("CONTEXTNEST_SERVER_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8080),
            version: std::env::var("CONTEXTNEST_REST_API_VERSION")
                .unwrap_or_else(|_| "v1".to_string()),
        }
    }
}

impl Default for GraphqlApiConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            endpoint: "/graphql".to_string(),
            playground: true,
            schema: GraphqlSchemaConfig::default(),
        }
    }
}

impl Default for GraphqlSchemaConfig {
    fn default() -> Self {
        Self {
            introspection: true,
            validation: true,
            complexity_analysis: true,
            max_query_depth: 10,
        }
    }
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            endpoint: "/ws".to_string(),
            connection: WebSocketConnectionConfig::default(),
            message: WebSocketMessageConfig::default(),
        }
    }
}

impl Default for WebSocketConnectionConfig {
    fn default() -> Self {
        Self {
            max_connections: 1000,
            timeout_seconds: 300, // 5 minutes
            heartbeat_interval_seconds: 30,
            reconnection: ReconnectionConfig::default(),
        }
    }
}

impl Default for WebSocketMessageConfig {
    fn default() -> Self {
        Self {
            max_message_size: 1024 * 1024, // 1MB
            compression: true,
            batching: MessageBatchingConfig::default(),
            validation: MessageValidationConfig::default(),
        }
    }
}

impl Default for MessageBatchingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            batch_size: 100,
            batch_timeout_ms: 100,
        }
    }
}

impl Default for ApiSecurityConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            // Authentication removed - all endpoints are now public
            tls: TlsConfig::default(),
            cors: CorsConfig::default(),
        }
    }
}

// Authentication and authorization Default implementations removed - all endpoints are now public

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Default to HTTP for development
            cert_file: PathBuf::from("./certs/server.crt"),
            key_file: PathBuf::from("./certs/server.key"),
            version: TlsVersion::V1_3,
            cipher_suites: vec![],
        }
    }
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allowed_origins: vec!["*".to_string()], // Permissive for development
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "OPTIONS".to_string(),
            ],
            allowed_headers: vec!["*".to_string()],
            allow_credentials: true,
            max_age_seconds: 3600,
        }
    }
}

impl Default for MonitoringConfig {
    fn default() -> Self {
        Self {
            logging: LoggingConfig::default(),
            metrics: MetricsConfig::default(),
            tracing: TracingConfig::default(),
            health: HealthConfig::default(),
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        let level = std::env::var("CONTEXTNEST_SERVER_LOG_LEVEL")
            .ok()
            .and_then(|level| match level.to_lowercase().as_str() {
                "trace" => Some(LogLevel::Trace),
                "debug" => Some(LogLevel::Debug),
                "info" => Some(LogLevel::Info),
                "warn" => Some(LogLevel::Warn),
                "error" => Some(LogLevel::Error),
                _ => None,
            })
            .unwrap_or(LogLevel::Info);

        Self {
            enabled: true,
            level,
            format: LogFormat::Structured,
            outputs: vec![
                LogOutput::Console,
                LogOutput::File {
                    path: PathBuf::from("./logs/contextnest.log"),
                },
            ],
            rotation: LogRotationConfig::default(),
        }
    }
}

impl Default for LogRotationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: RotationStrategy::Daily,
            max_file_size_mb: 100,
            max_files: 7,
        }
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            format: MetricsFormat::Prometheus,
            collection_interval_seconds: 30,
            exporters: vec![MetricsExporter::Http {
                endpoint: "http://localhost:9090/metrics".to_string(),
            }],
            custom_metrics: vec![],
        }
    }
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            service_name: "ContextNest".to_string(),
            exporters: vec![TracingExporter::Console],
            sampling: SamplingConfig::default(),
        }
    }
}

impl Default for SamplingConfig {
    fn default() -> Self {
        Self {
            strategy: SamplingStrategy::Probabilistic,
            rate: 0.1, // 10% sampling
        }
    }
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            endpoint: "/health".to_string(),
            interval_seconds: 30,
            components: vec![
                HealthCheckComponent::Memory,
                HealthCheckComponent::CPU,
                HealthCheckComponent::FileSystem,
            ],
        }
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            thread_pool: ThreadPoolConfig::default(),
            memory: MemoryManagementConfig::default(),
            io: IoConfig::default(),
            optimization: PerformanceOptimizationConfig::default(),
        }
    }
}

impl Default for ThreadPoolConfig {
    fn default() -> Self {
        Self {
            core_threads: num_cpus::get(),
            max_threads: num_cpus::get() * 2,
            keep_alive_seconds: 60,
            queue_size: 1000,
        }
    }
}

impl Default for IoConfig {
    fn default() -> Self {
        Self {
            buffer_size: 8192, // 8KB
            async_io: AsyncIoConfig::default(),
            file_io: FileIoConfig::default(),
        }
    }
}

impl Default for AsyncIoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            runtime: AsyncRuntime::Tokio,
            worker_threads: num_cpus::get(),
        }
    }
}

impl Default for FileIoConfig {
    fn default() -> Self {
        Self {
            memory_mapping: true,
            fs_cache: true,
            read_ahead_bytes: 65536, // 64KB
        }
    }
}

impl Default for PerformanceOptimizationConfig {
    fn default() -> Self {
        Self {
            jit_compilation: false, // Conservative default
            cpu_optimization: CpuOptimizationConfig::default(),
            memory_optimization: MemoryOptimizationSettings::default(),
        }
    }
}

impl Default for CpuOptimizationConfig {
    fn default() -> Self {
        Self {
            simd: true,
            target_features: vec!["sse4.2".to_string(), "avx2".to_string()],
            optimization_level: OptimizationLevel::Release,
        }
    }
}

impl Default for MemoryOptimizationSettings {
    fn default() -> Self {
        Self {
            pooling: true,
            arena_allocation: false,
            alignment: 64, // Cache line aligned
        }
    }
}

// Default implementations for new security and configuration types

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            sandbox: SandboxSecurityConfig::default(),
            plugin_verification: PluginVerificationConfig::default(),
            resource_monitoring: ResourceMonitoringConfig::default(),
            audit_logging: AuditLoggingConfig::default(),
            threat_detection: ThreatDetectionConfig::default(),
        }
    }
}

impl Default for SandboxSecurityConfig {
    fn default() -> Self {
        Self {
            enabled: std::env::var("CONTEXTNEST_SANDBOX_ENABLED")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true), // Enable by default for security
            default_isolation_type: SandboxIsolationType::Process,
            resource_limits: SandboxResourceLimits::default(),
            network_access: NetworkAccessConfig::default(),
            filesystem_access: FilesystemAccessConfig::default(),
            process_isolation: ProcessIsolationConfig::default(),
        }
    }
}

impl Default for SandboxResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_mb: 512, // 512MB per sandbox
            max_cpu_percent: 50.0,
            max_execution_seconds: 300, // 5 minutes
            max_disk_mb: 100,           // 100MB temporary storage
            max_network_connections: 10,
        }
    }
}

impl Default for NetworkAccessConfig {
    fn default() -> Self {
        Self {
            allowed: false, // Most secure default
            allowed_endpoints: vec![],
            blocked_endpoints: vec![],
            allow_dns: false,
            proxy: None,
        }
    }
}

impl Default for FilesystemAccessConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            read_only_paths: vec![
                PathBuf::from("/usr/lib"),
                PathBuf::from("/lib"),
                PathBuf::from("/lib64"),
            ],
            read_write_paths: vec![PathBuf::from("/tmp"), PathBuf::from("./data/temp")],
            temp_directory: Some(PathBuf::from("/tmp/contextnest")),
            allow_devices: false,
        }
    }
}

impl Default for ProcessIsolationConfig {
    fn default() -> Self {
        Self {
            sandbox_user: Some("nobody".to_string()),
            sandbox_group: Some("nogroup".to_string()),
            enable_namespaces: true,
            drop_capabilities: vec![
                "CAP_SYS_ADMIN".to_string(),
                "CAP_SYS_PTRACE".to_string(),
                "CAP_SYS_MODULE".to_string(),
            ],
            enable_seccomp: true,
        }
    }
}

impl Default for PluginVerificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            require_signatures: std::env::var("CONTEXTNEST_REQUIRE_SIGNATURES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false), // Disabled by default for development
            trusted_keys: vec![],
            cache_ttl_hours: 24,
            allow_unsigned_dev: cfg!(debug_assertions),
        }
    }
}

impl Default for ResourceMonitoringConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_seconds: 30,
            retention_hours: 168, // 7 days
            alert_thresholds: ResourceAlertThresholds::default(),
            enable_cleanup: true,
        }
    }
}

impl Default for ResourceAlertThresholds {
    fn default() -> Self {
        Self {
            memory_threshold: 80.0,
            cpu_threshold: 80.0,
            disk_threshold: 90.0,
            network_threshold: 70.0,
        }
    }
}

impl Default for AuditLoggingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            log_file: PathBuf::from("./logs/security_audit.log"),
            rotation: LogRotationConfig::default(),
            event_types: vec![
                AuditEventType::Security,
                AuditEventType::PluginManagement,
                AuditEventType::ResourceViolation,
            ],
            format: AuditLogFormat::Json,
        }
    }
}

impl Default for ThreatDetectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rules: vec![
                ThreatDetectionRule {
                    name: "Memory Exhaustion".to_string(),
                    pattern: "memory_usage > 95%".to_string(),
                    severity: ThreatSeverity::High,
                    enabled: true,
                },
                ThreatDetectionRule {
                    name: "CPU Hogging".to_string(),
                    pattern: "cpu_usage > 90% for > 60s".to_string(),
                    severity: ThreatSeverity::Medium,
                    enabled: true,
                },
            ],
            response_actions: vec![
                ThreatResponseAction {
                    name: "Block Action".to_string(),
                    action_type: ThreatActionType::Block,
                    parameters: HashMap::new(),
                },
                ThreatResponseAction {
                    name: "Alert Admin".to_string(),
                    action_type: ThreatActionType::Alert,
                    parameters: HashMap::new(),
                },
            ],
            false_positive_tolerance: 0.1,
        }
    }
}

impl Default for SynchronizationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            websocket: WebSocketSyncConfig::default(),
            conflict_resolution: ConflictResolutionConfig::default(),
            consistency: ConsistencyConfig::default(),
            performance: SyncPerformanceConfig::default(),
        }
    }
}

impl Default for WebSocketSyncConfig {
    fn default() -> Self {
        Self {
            endpoint: "/ws/sync".to_string(),
            connection: WebSocketConnectionConfig::default(),
            message: WebSocketMessageConfig::default(),
            security: WebSocketSecurityConfig::default(),
        }
    }
}

// Duplicate Default implementation removed - using the one at line ~3595 instead
/*
impl Default for WebSocketConnectionConfig {
    fn default() -> Self {
        Self {
            max_connections: 1000,
            timeout_seconds: 300,
            heartbeat_interval_seconds: 30,
            reconnection: ReconnectionConfig::default(),
        }
    }
}
*/

impl Default for ReconnectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_attempts: 5,
            initial_delay_ms: 1000,
            backoff_multiplier: 2.0,
        }
    }
}

// Duplicate Default implementation removed - using the one at line ~3606 instead
/*
impl Default for WebSocketMessageConfig {
    fn default() -> Self {
        Self {
            max_message_size: 1024 * 1024, // 1MB
            compression: true,
            batching: MessageBatchingConfig::default(),
            validation: MessageValidationConfig::default(),
        }
    }
}
*/

impl Default for MessageValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rules: vec![
                ValidationRule::StructuralIntegrity,
                ValidationRule::SecurityConstraints,
            ],
            strict: false,
        }
    }
}

impl Default for WebSocketSecurityConfig {
    fn default() -> Self {
        Self {
            authentication: true,
            authorization: true,
            tls: None, // Let the main TLS config handle this
        }
    }
}

impl Default for ConflictResolutionConfig {
    fn default() -> Self {
        Self {
            default_strategy: ConflictResolutionStrategy::LastWriteWins,
            type_strategies: HashMap::new(),
            manual_resolution: ManualResolutionConfig::default(),
        }
    }
}

impl Default for ManualResolutionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            timeout_minutes: 60,
            default_resolution: ConflictResolutionStrategy::LastWriteWins,
        }
    }
}

impl Default for ConsistencyConfig {
    fn default() -> Self {
        Self {
            level: ConsistencyLevel::Eventual,
            validation: ConsistencyValidationConfig::default(),
            healing: ConsistencyHealingConfig::default(),
        }
    }
}

impl Default for ConsistencyValidationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_seconds: 300, // 5 minutes
            rules: vec![ConsistencyRule {
                name: "Data Integrity".to_string(),
                pattern: "checksum_mismatch".to_string(),
                action: ConsistencyAction::Heal,
            }],
        }
    }
}

impl Default for ConsistencyHealingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategies: vec![HealingStrategy {
                name: "Synchronization".to_string(),
                strategy_type: HealingStrategyType::Synchronize,
                parameters: HashMap::new(),
            }],
            timeout_minutes: 30,
        }
    }
}

impl Default for SyncPerformanceConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            parallel_workers: num_cpus::get(),
            compression: CompressionConfig::default(),
            caching: SyncCacheConfig::default(),
        }
    }
}

impl Default for SyncCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            size_mb: 256,
            ttl_minutes: 60,
            eviction_policy: EvictionPolicy::LRU,
        }
    }
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            plugin_directories: vec![
                PathBuf::from("./plugins"),
                PathBuf::from("./examples/domains"),
            ],
            auto_discovery: PluginAutoDiscoveryConfig::default(),
            hot_reload: PluginHotReloadConfig::default(),
            sandbox: PluginSandboxConfig::default(),
            lifecycle: PluginLifecycleConfig::default(),
        }
    }
}

impl Default for PluginAutoDiscoveryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_minutes: 5,
            scan_subdirectories: true,
            file_patterns: vec![
                "*.so".to_string(),
                "*.dll".to_string(),
                "*.dylib".to_string(),
                "plugin.json".to_string(),
            ],
        }
    }
}

impl Default for PluginHotReloadConfig {
    fn default() -> Self {
        Self {
            enabled: cfg!(debug_assertions), // Only in debug mode
            watch_files: true,
            timeout_seconds: 30,
            preserve_state: true,
            watch_directories: vec![
                PathBuf::from("./plugins"),
                PathBuf::from("./domains"),
                PathBuf::from("./src/domains"),
            ],
            watch_patterns: vec![
                "*.rs".to_string(),
                "*.json".to_string(),
                "*.toml".to_string(),
                "*.yaml".to_string(),
                "*.yml".to_string(),
            ],
            debounce_ms: 500,
            auto_reload: true,
            max_reload_attempts: 3,
            reload_delay_ms: 1000,
            enable_rollback: true,
        }
    }
}

impl Default for PluginSandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_config: SandboxSecurityConfig::default(),
            plugin_overrides: HashMap::new(),
        }
    }
}

impl Default for PluginLifecycleConfig {
    fn default() -> Self {
        Self {
            init_timeout_seconds: 30,
            shutdown_timeout_seconds: 15,
            health_check_interval_seconds: 60,
            max_restart_attempts: 3,
            restart_delay_seconds: 5,
        }
    }
}

impl Config {
    /// Load configuration from file
    pub fn from_file(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to file
    pub fn to_file(&self, path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        // This would be implemented to load from environment variables
        // For now, return default configuration
        Ok(Self::default())
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Validate token budget
        if self.context.max_token_budget == 0 {
            return Err("Context max_token_budget must be greater than 0".into());
        }

        // Validate embedding dimensions
        if self.neural_field.embedding_dim == 0 {
            return Err("Neural field embedding_dim must be greater than 0".into());
        }

        // Validate API configuration
        if self.api.rest.enabled && self.api.rest.port == 0 {
            return Err("REST API port must be specified when enabled".into());
        }

        // Validate thread pool configuration
        if self.performance.thread_pool.core_threads == 0 {
            return Err("Thread pool core_threads must be greater than 0".into());
        }

        Ok(())
    }
}
/// Parser configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParserConfig {
    /// Enable parser
    pub enabled: bool,

    /// Parser timeout in seconds
    pub timeout_seconds: u64,

    /// Maximum file size in bytes
    pub max_file_size: usize,

    /// Supported file extensions
    pub supported_extensions: Vec<String>,

    /// Parse concurrency limit
    pub concurrency_limit: usize,

    /// Dart analyzer executable path
    pub dart_analyzer_path: String,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_seconds: 30,
            max_file_size: 10 * 1024 * 1024, // 10MB
            supported_extensions: vec![
                "dart".to_string(),
                "rs".to_string(),
                "js".to_string(),
                "ts".to_string(),
                "py".to_string(),
            ],
            concurrency_limit: 4,
            dart_analyzer_path: "dart".to_string(),
        }
    }
}

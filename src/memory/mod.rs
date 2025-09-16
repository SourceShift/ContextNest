//! ContextNest memory subsystem — canonical attractor-based memory primitives.
//! This is the **memory substrate** half of the v0.1.0 surface. The other half
//! lives in `crate::context` (neural field + reconstructive memory). Together
//! they implement the canon described in `resources/Context-Engineering/`
//! modules 05 (memory systems) and 08 (field-theory integration).
//! ## Public API surface
//! - [`AttractorBasin`] + [`AttractorBasinManager`]: stability/convergence
//!   dynamics for memory patterns (canon: `40_reference/attractor_dynamics.md`)
//! - [`AdaptiveDecaySystem`] + [`DecayProfile`]: importance-weighted persistence
//!   (canon: `00_foundations/09_persistence_and_resonance.md`) — the §1.4
//!   differentiator
//! - [`ConnectionNetwork`] + [`MemoryGraph`]: associative retrieval over
//!   connected memory fragments (canon: `00_COURSE/05_memory_systems/00_memory_architectures.md`)
//! - [`GapFillingEngine`] + [`AIGenerationService`]: reconstructive gap-fill
//!   (canon: `00_COURSE/05_memory_systems/04_reconstructive_memory.md`)
//! - [`MemoryAttractorManager`]: top-level coordinator that orchestrates the
//!   four primitives above
//! - [`MemoryReconstructionProtocol`]: protocol-level memory reconstruction
//!   (paired with `crate::context::memory_reconstruction`)

pub mod attractors;

// Curated public re-exports. Authoritative IP surface for the memory substrate.
pub use attractors::{
    AIGenerationMetrics,
    AIGenerationResult,
    AIGenerationService,
    AIModelConfig,
    AccessPattern,
    // Adaptive decay (importance-weighted persistence)
    AdaptiveDecaySystem,
    AssociationType,

    // Attractor basin dynamics
    AttractorBasin,
    AttractorBasinManager,
    BackgroundTask,
    BackgroundTaskType,
    BasinDynamics,
    BasinHealth,
    BasinInteractionNetwork,

    BasinShape,
    BasinType,
    ComponentHealth,
    ComponentStatus,
    ConnectionEdge,
    // Connection network (associative retrieval)
    ConnectionNetwork,
    ConnectionType,
    ContextAssociation,
    DecayProfile,
    GapFillSource,
    // Gap filling for reconstruction
    GapFillingEngine,
    GapFillingMethod,
    GapInfo,
    GraphMetrics,
    ImportanceEvent,
    ImportanceRecord,
    ImportanceSnapshot,
    ImportanceTracker,
    MemoryAttractorComponent,
    MemoryAttractorConfig,
    // Top-level coordinator + configuration
    MemoryAttractorManager,
    MemoryAttractorMetrics,
    // Shared types
    MemoryFragment,
    MemoryGraph,
    MemoryNode,
    MemoryNodeType,
    MemoryPattern,

    MemoryProcessingRequest,
    // Reconstruction protocol
    MemoryReconstructionProtocol,
    MemoryUsageStats,
    PatternBasedFiller,
    PatternDatabase,
    PatternMatchingEngine,
    PatternTemplate,

    ProcessingOptions,
    ProcessingPriority,

    ReconstructedMemory,
    ReconstructionPriority,
    ReconstructionRequest,
    ReconstructionResult,
    RetrievalOptimizer,
    RetrievalQuery,

    RetrievalStrategy,
    ShapeType,
    SpatialIndex,
    SystemMetrics,
    TaskStatus,
};

//! ContextNest prelude — single import for the v0.1.0 substrate surface.
//! ```ignore
//! use contextnest::prelude::*;
//! ```
//! Exposes the seven-tool memory API request/response types, the substrate's
//! service container, the neural-field core, the canonical reconstructive-
//! memory primitives, and the memory-attractor sub-engines.
//! For lower-level access (each module's full type surface), import directly
//! from `contextnest::{context, memory, api}`.

// =============================================================================
// Service container + health
// =============================================================================

pub use crate::services::{
    ContextManagerService, ContextNestServices, EmbeddingService, EnhancedGraphService,
    GraphService, HealthStatus, ParserService,
};

// =============================================================================
// Core context types
// =============================================================================

pub use crate::context::{
    ContextLevel, ContextManager, ContextMetrics, Example, MemoryCell, MemoryStrategy,
};

// =============================================================================
// Neural field
// =============================================================================

pub use crate::context::field::{CoherenceAnalysis, FieldHealth, NeuralField, SemanticPattern};

// =============================================================================
// Memory substrate (canonical attractor-based primitives)
// =============================================================================

pub use crate::context::memory::{
    AttractorField, MemoryAttractor, MemoryOrchestrator, PersistenceParameters,
};

// Canonical attractor sub-engines per Module 05 of the canon
// (`00_COURSE/05_memory_systems/`). The fix-up PR (~80 latent bugs) is
// done; these are now part of the public substrate surface.
pub use crate::memory::attractors::adaptive_decay::{AdaptiveDecaySystem, DecayProfile};
pub use crate::memory::attractors::attractor_basin::{
    AttractorBasinManager, BasinDynamics, BasinHealth, BasinType,
};
pub use crate::memory::attractors::connection_network::{
    ConnectionNetwork, MemoryGraph, MemoryNode, QueryType, RetrievalQuery,
};
pub use crate::memory::attractors::gap_filling_engine::GapFillingEngine;
pub use crate::memory::attractors::reconstruction_protocol::MemoryReconstructionProtocol;
pub use crate::memory::attractors::{
    ComponentStatus, GapFillSource, GapFillingMethod, GapInfo, MemoryAttractorConfig,
    MemoryAttractorManager, MemoryFragment, ReconstructedMemory,
};

// Phase C: bridges between the canonical fragment and the context-side
// reconstruction modules. Use the `From` impls when piping fragments
// canonical → context-side; use the free helpers `canonical_from_*` for
// the reverse direction (text content is discarded — see module docs).
pub use crate::context::fragment_bridge::{
    canonical_from_reconstruction, canonical_from_resonance,
};

// =============================================================================
// Reconstructive memory (canonical chain)
// =============================================================================

pub use crate::context::{
    GapIdentifier,
    MemoryReconstructionProtocolCoordinator, // top-level pipeline coordinator
    MemoryReconstructor, // memory_reconstruction::MemoryReconstructionCoordinator
    ResonanceActivator,
};

// =============================================================================
// Field operators (canonical Context-Engineering field primitives)
// =============================================================================

pub use crate::context::{
    EmergenceDetectionSystem, EmergenceDetector, EmergenceEvent, EmergenceType, FieldProjector,
    ProjectionMethod, SelfOrganizingEmergence,
};

// =============================================================================
// Attractor performance optimization (opt-in parallel/cache/batch layer)
// =============================================================================

pub use crate::context::{
    AccuracyOptimizationResult, AttractorPerformanceOptimizer, MemoryOptimizationResult,
    OptimizationConfig, OptimizationMetrics,
};

// =============================================================================
// Cognition + autonomy (the "continual-learning + agentic substrate" pitch)
// =============================================================================

pub use crate::context::{
    Action, ActionType, AutonomyLevel, MetaRecursiveEngine, RecursiveLearner,
};

// =============================================================================
// HTTP API (seven-tool memory API)
// =============================================================================

pub use crate::api::create_simple_app;
pub use crate::api::tools::{
    DiscardRequest,
    DiscardResponse,
    ReconstructRequest,
    ReconstructResponse,
    ResonateActivation,
    ResonateRequest,
    ResonateResponse,
    // Sub-types
    RetrieveHit,
    RetrieveRequest,
    RetrieveResponse,
    // Request types
    StoreRequest,
    // Response types
    StoreResponse,
    SummarizeRequest,
    SummarizeResponse,
    UpdateRequest,
    UpdateResponse,
};

// =============================================================================
// Error types
// =============================================================================

pub use crate::error::{ContextNestError, ContextNestResult, Result};

// =============================================================================
// Config
// =============================================================================

pub use crate::Config;

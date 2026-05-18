//! # ContextNest
//! Continual-learning memory substrate for LLM agents — neural-field
//! attractor consolidation.
//! ## Quick start
//! ```ignore
//! use contextnest::prelude::*;
//! let services = ContextNestServices::new_default().await?;
//! let app = create_simple_app(services).await?;
//! axum::serve(listener, app).await?;
//! ```
//! Then call the seven-tool memory API at `/api/v1/tools/{store|retrieve|
//! update|summarize|discard|reconstruct|resonate}`.
//! ## Crate layout
//! - [`prelude`] — recommended single-import entry point for application code
//! - [`api`] — HTTP server + seven-tool memory API
//! - [`context`] — neural field, attractor dynamics, reconstructive memory,
//!   meta-cognition, agency (the substrate's primitives)
//! - [`memory`] — canonical attractor-based memory sub-engines (adaptive decay,
//!   connection network, gap filling, basin dynamics)
//! - [`services`] — service container glue (embedding, graph, parser)
//! - [`protocols`] — Pareto-Lang protocol types
//! - [`config`], [`error`] — config + error types
//! See `resources/Context-Engineering/` for the canonical theory this
//! substrate implements.

pub mod api;
pub mod cli;
pub mod config;
pub mod context;
pub mod domains;
pub mod error;
pub mod memory; // Canonical attractor-based memory sub-engines.
pub mod models;
pub mod patterns;
pub mod prelude;
pub mod protocols;
pub mod security;
pub mod services;

// Re-export service container + core services for convenience. Most users
// should `use contextnest::prelude::*` instead.
pub use services::{
    ContextManagerService, ContextNestServices, EmbeddingService, EnhancedGraphService,
    GraphService, HealthStatus, ParserService,
};

pub use crate::config::Config;
pub use error::{ContextNestError, ContextNestResult, Result};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{
        field::NeuralField, memory::MemoryOrchestrator, ContextManager, MemoryStrategy,
    };
    use crate::protocols::ProtocolRegistry;

    #[test]
    fn test_context_manager_creation() {
        let _context_manager = ContextManager::new(1024);
    }

    #[test]
    fn test_neural_field_creation() {
        let field = NeuralField::new();
        assert_eq!(field.patterns.len(), 0);
    }

    #[test]
    fn test_memory_orchestrator_creation() {
        let _memory = MemoryOrchestrator::new(MemoryStrategy::PriorityPruning { max_tokens: 1024 });
    }

    #[test]
    fn test_protocol_registry_creation() {
        let registry = ProtocolRegistry::new();
        let stats = registry.get_registry_stats();
        assert_eq!(stats.total_protocols, 0);
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        // Check that config has reasonable defaults
        assert!(config.context.max_token_budget > 0);
    }

    #[test]
    fn test_error_creation() {
        let error = ContextNestError::Configuration("test".to_string());
        match error {
            ContextNestError::Configuration(msg) => assert_eq!(msg, "test"),
            _ => panic!("Wrong error type"),
        }
    }
}

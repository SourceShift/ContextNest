use crate::error::ContextNestResult;
use axum::Router;
use std::sync::Arc;

// Re-export the canonical ContextNestServices from the services module
pub use crate::services::ContextNestServices;
pub use crate::services::HealthStatus;

// Domain-specific API modules moved to examples/domains/
// Use route_registry for dynamic plugin-based routing

pub mod cc_hooks;
pub mod coord;
pub mod field;
pub mod inbox;
pub mod llm_proxy;
pub mod middleware;
pub mod prompt_context;
pub mod server;
pub mod services;
pub mod sessions;
pub mod simple;
pub mod stats;
pub mod substrate;
pub mod tools;

#[cfg(test)]
pub mod tests;

pub use cc_hooks::{create_cc_hooks_router, HookPayload, SessionTracker};
pub use field::create_field_router;
pub use inbox::create_inbox_router;
pub use middleware::{apply_middleware, build_middleware_stack, MiddlewareStack};
pub use prompt_context::create_prompt_context_router;
pub use server::{ApiRequest, ApiResponse, ContextNestApiServer, ProcessedApiRequest, UserContext};
pub use sessions::create_sessions_router;
pub use simple::create_simple_app;
pub use stats::create_stats_router;
pub use substrate::create_substrate_router;
pub use tools::create_tools_router;

/// Create the main application router with comprehensive middleware
pub async fn create_app(
    services: crate::services::ContextNestServices,
) -> crate::error::ContextNestResult<Router> {
    let router = create_simple_app(services).await?;
    Ok(apply_middleware(router))
}

/// Create a comprehensive API server instance
pub fn create_api_server(services: crate::services::ContextNestServices) -> ContextNestApiServer {
    ContextNestApiServer::new(services)
}

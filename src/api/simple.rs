use axum::{extract::State, http::StatusCode, response::Json, routing::get, Extension, Router};
use serde::Serialize;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;

use crate::api::cc_hooks::{self, SessionTracker};
use crate::api::coord;
use crate::api::field;
use crate::api::inbox;
use crate::api::llm_proxy;
use crate::api::prompt_context;
use crate::api::services::ServiceContainer;
use crate::api::sessions;
use crate::api::stats;
use crate::api::substrate;
use crate::api::tools;
use crate::services::ContextNestServices;

/// Create a simple working application router
/// This is now a domain-agnostic core API.
/// Domain-specific routes should be registered via the plugin system.
/// See examples/domains/ for domain implementations.
pub async fn create_simple_app(services: ContextNestServices) -> crate::Result<Router> {
    // Create service container for dependency injection
    let service_container = ServiceContainer::new().await?;

    // Seven-tool memory API per OPEN_SOURCE_PLAN §4.2.
    let tools_router = tools::create_tools_router();

    // Claude Code real-time hook receiver. Shares the same state type
    // as the tools router; the per-session byte-offset tracker rides as
    // an Extension so handlers can access it without changing the
    // router's State<S> signature.
    let cc_hooks_router = cc_hooks::create_cc_hooks_router();
    let coord_router = coord::create_coord_router();
    let sessions_router = sessions::create_sessions_router();
    let inbox_router = inbox::create_inbox_router();
    let stats_router = stats::create_stats_router();
    let substrate_router = substrate::create_substrate_router();
    let field_router = field::create_field_router();
    let prompt_context_router = prompt_context::create_prompt_context_router();
    let llm_proxy_router = llm_proxy::create_llm_proxy_router();
    let session_tracker = Arc::new(SessionTracker::new());

    // Defence-in-depth sweeper: re-tail every tracked session's
    // transcript on a 30s cadence so a dropped hook (Claude killed
    // mid-curl, substrate restart longer than the curl retry window,
    // session abandoned before Stop fires) cannot strand a session's
    // recent z-insight blocks out of the inbox. The hook path remains
    // the primary delivery channel; this is the "pull" backstop.
    cc_hooks::spawn_sweeper(
        services.clone(),
        session_tracker.clone(),
        std::time::Duration::from_secs(30),
    );

    let base_router = Router::new()
        .route("/api/health", get(health_check))
        .route("/api/status", get(status_check))
        .merge(tools_router)
        .merge(cc_hooks_router)
        .merge(coord_router)
        .merge(sessions_router)
        .merge(inbox_router)
        .merge(stats_router)
        .merge(substrate_router)
        .merge(field_router)
        .merge(prompt_context_router)
        .merge(llm_proxy_router)
        .layer(Extension(session_tracker))
        .with_state(services);

    info!("Core API initialized (domain-agnostic)");
    info!("Delete endpoints registered");

    Ok(base_router.layer(
        ServiceBuilder::new()
            .layer(TraceLayer::new_for_http())
            .layer(CorsLayer::permissive()),
    ))
}

/// Simple health check endpoint
async fn health_check(
    State(services): State<ContextNestServices>,
) -> std::result::Result<Json<HealthResponse>, StatusCode> {
    match services.health_check().await {
        Ok(status) => Ok(Json(HealthResponse {
            status: "ok".to_string(),
            healthy: status.overall,
        })),
        Err(_) => Err(StatusCode::SERVICE_UNAVAILABLE),
    }
}

/// Simple status endpoint
async fn status_check() -> Json<StatusResponse> {
    Json(StatusResponse {
        version: "0.1.0".to_string(), // This should be updated with Cargo.toml
        name: "contextnest".to_string(), // This should be updated with Cargo.toml
        description: "ContextNest — neural-field attractor memory substrate".to_string(),
    })
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: String,
    healthy: bool,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    version: String,
    name: String,
    description: String,
}

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Extension, Router};
use serde::Serialize;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;

use crate::api::services::ServiceContainer;
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

    let base_router = Router::new()
        .route("/api/health", get(health_check))
        .route("/api/status", get(status_check))
        .merge(tools_router)
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

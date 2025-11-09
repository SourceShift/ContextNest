//! Service container and dependency injection for API handlers.
//! Provides centralized service management for all API endpoints in the
//! ContextNest system. The container is kept domain-agnostic and acts as a
//! stable extension point so future plugin/integration systems can add fields
//! here without breaking external callers (`src/api/simple.rs` uses
//! `ServiceContainer::new()` + `ServiceContainer::health_check`).

use crate::error::ContextNestResult;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Domain-agnostic service container. Add a new `Arc<RwLock<T>>` field here
/// when a future plugin or integration system needs a wired hook.
#[derive(Clone, Default)]
pub struct DomainServices;

impl DomainServices {
    /// Create new domain services container.
    pub fn new() -> Self {
        Self
    }
}

/// Central service container for dependency injection.
#[derive(Clone)]
pub struct ServiceContainer {
    /// Domain services extension point.
    pub domain_services: Arc<RwLock<DomainServices>>,
}

impl ServiceContainer {
    /// Create a new domain-agnostic service container.
    pub async fn new() -> ContextNestResult<Self> {
        let domain_services = Arc::new(RwLock::new(DomainServices::new()));

        info!("ServiceContainer initialized (domain-agnostic)");

        Ok(Self { domain_services })
    }

    /// Get domain services.
    pub fn get_domain_services(&self) -> Arc<RwLock<DomainServices>> {
        self.domain_services.clone()
    }

    /// Health check for core services.
    /// the `plugin_count` field of `ServiceHealthStatus`
    /// is retained for wire-compatibility with v0.0 health-probe clients
    /// but is always `0` in v0.1.0 (no plugin system). The `loaded_domains`
    /// vector is similarly always empty.
    pub async fn health_check(&self) -> ServiceHealthStatus {
        let _domain_services = self.domain_services.read().await;
        ServiceHealthStatus {
            core_services: "healthy".to_string(),
            plugin_count: 0,
            loaded_domains: vec![],
            timestamp: chrono::Utc::now(),
        }
    }
}

/// Health status for all services (domain-agnostic).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceHealthStatus {
    pub core_services: String,
    pub plugin_count: usize,
    pub loaded_domains: Vec<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Health check response (domain-agnostic).
#[derive(Debug, Serialize)]
pub struct HealthCheckResponse {
    pub status: String,
    pub timestamp: String,
    pub services: ServiceHealthStatus,
}

//! Minimal Domain trait stub.
//! Reserved for future per-agent context profiles. Not wired into any code path
//! in v0.1.0. Kept so external code can begin building toward this contract
//! without waiting for v0.5+ to commit to the surface.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::Result;

/// Domain identifier — opaque string in v0.1.0. Future versions may add
/// well-known variants.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DomainId(pub String);

impl DomainId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for DomainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Per-domain configuration. Free-form for now.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomainConfig {
    pub domain_id: Option<DomainId>,
    pub enabled: bool,
    pub settings: HashMap<String, serde_json::Value>,
}

/// Per-user, per-domain runtime context.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomainContext {
    pub domain_id: Option<DomainId>,
    pub user_id: Option<String>,
    pub data: HashMap<String, serde_json::Value>,
}

/// Future contract for per-agent / per-tenant context profiles. Not implemented
/// in v0.1.0.
#[async_trait]
pub trait Domain: Send + Sync {
    fn id(&self) -> DomainId;
    fn name(&self) -> &str;
    fn description(&self) -> &str {
        ""
    }
    async fn validate_config(&self, _config: &DomainConfig) -> Result<()> {
        Ok(())
    }
    async fn cleanup(&self) -> Result<()> {
        Ok(())
    }
}

/// Companion processor trait (also a stub).
#[async_trait]
pub trait DomainProcessor: Send + Sync {
    async fn process(&self, input: serde_json::Value) -> Result<serde_json::Value>;
}

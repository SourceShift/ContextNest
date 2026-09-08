use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const PIPELINE_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TenantPolicy {
    pub version: u32,
    pub pipeline_version: u32,
    /// Empty selects the configured provider space on first registration.
    pub embedding_space: String,
    pub retention_days: u32,
    pub max_sessions: usize,
    pub max_records_per_session: usize,
    pub max_pending: usize,
    pub max_content_bytes: usize,
    pub kinds: Vec<String>,
    pub max_connections: usize,
    pub connection_threshold: f32,
    pub basin_attach_threshold: f32,
}
impl Default for TenantPolicy {
    fn default() -> Self {
        Self {
            version: 1,
            pipeline_version: PIPELINE_VERSION,
            embedding_space: String::new(),
            retention_days: 90,
            max_sessions: 1000,
            max_records_per_session: 2000,
            max_pending: 4096,
            max_content_bytes: 16_384,
            kinds: vec!["conversation-turn".into()],
            max_connections: 32,
            connection_threshold: 0.7,
            basin_attach_threshold: 0.4,
        }
    }
}
impl TenantPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if self.pipeline_version != PIPELINE_VERSION
            || self.version == 0
            || !(1..=3650).contains(&self.retention_days)
            || !(1..=10_000).contains(&self.max_sessions)
            || !(1..=10_000).contains(&self.max_records_per_session)
            || !(1..=100_000).contains(&self.max_pending)
            || !(1..=65_536).contains(&self.max_content_bytes)
            || self.max_connections > 64
            || self.kinds.is_empty()
            || !self.connection_threshold.is_finite()
            || !(0.0..=1.0).contains(&self.connection_threshold)
            || !self.basin_attach_threshold.is_finite()
            || !(0.0..=2.0).contains(&self.basin_attach_threshold)
        {
            return Err("invalid tenant policy limits".into());
        }
        Ok(())
    }
}

/// Constructed only after a verified capability and current session generation.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemoryScope {
    pub tenant_id: String,
    pub session_id: String,
    pub generation: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoreRequest {
    pub source_event_id: String,
    #[serde(default = "one")]
    pub revision: i64,
    pub content: String,
    #[serde(default = "importance")]
    pub importance: f32,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
}
fn one() -> i64 {
    1
}
fn importance() -> f32 {
    0.5
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredRecord {
    pub fragment_id: String,
    pub source_event_id: String,
    pub revision: i64,
    pub content: String,
    pub importance: f32,
    pub metadata: BTreeMap<String, Value>,
    pub created_at: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Acceptance {
    pub fragment_id: String,
    pub revision: i64,
    pub indexing_status: String,
    pub duplicate: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScopeStats {
    pub collected_at: String,
    pub ready: usize,
    pub pending: usize,
    pub processing: usize,
    pub failed: usize,
    pub basins: usize,
    pub edges: usize,
    pub candidates_scored: u64,
    pub processing_ms: u64,
    pub queue_wait_ms: u64,
    pub embedding_ms: u64,
    pub generation: i64,
}

pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-.:".contains(&c))
}

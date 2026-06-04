//! `DELETE /llm/v1/cache/entries/:fingerprint` — v0.3 Phase 3 slice 3.3.
//!
//! Hard-deletes one bucket of cached chat-completion responses keyed
//! by `ExactKeyPrefix::fingerprint()` (a 32-byte SHA-256 over project
//! + model + temperature + system_prompt_hash). The fingerprint is
//! passed as a 64-char lowercase hex string in the path.
//!
//! Mechanics (full chain documented on
//! [`crate::services::llm_cache::LlmCacheService::discard_prefix`]):
//!
//! 1. Decode the path segment to a 32-byte fingerprint.
//! 2. Reconstruct the [`ExactKeyPrefix`] by scanning the in-memory
//!    cache for a matching fingerprint. The endpoint can't recover
//!    the original (project_id, model, system_prompt) tuple from the
//!    hash alone — those fields go through SHA-256 in `fingerprint()`.
//!    So we do a linear scan over the bucket keys and pick whichever
//!    one matches. Cheap because the cache typically has < few
//!    thousand buckets.
//! 3. Hand off to `discard_prefix(prefix, reason)`, which drops the
//!    bucket from memory + appends a WAL tombstone + emits an audit
//!    log line.
//!
//! ## Auth posture
//!
//! v0.3 has no auth model — the proxy assumes the operator owns the
//! host. Slice 3.4 (SECURITY.md) revisits this; multi-tenant
//! per-project auth lands with v0.2's tenancy work. For now: anyone
//! who can reach the substrate's HTTP port can hard-delete cache
//! entries. The audit log captures every deletion so unauthorized
//! intent can be reconstructed even without auth.
//!
//! ## Response shape
//!
//! ```json
//! {
//!   "deleted": true,
//!   "removed_rows": 3,
//!   "fingerprint": "<64-char hex>"
//! }
//! ```
//!
//! `deleted` is `false` and `removed_rows: 0` when the fingerprint
//! doesn't match any in-memory bucket (no-op). The tombstone is
//! still NOT written in that case — there's nothing to tombstone,
//! and writing an empty tombstone would just pollute the WAL.
//!
//! ## Error responses
//!
//! - `400 Bad Request` — fingerprint isn't 64 hex chars.
//! - `503 Service Unavailable` — never; the cache is always reachable
//!   in-process. Network/auth errors aren't possible against the
//!   in-memory store.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};

use crate::services::llm_cache::{hex_decode_32, ExactKeyPrefix};
use crate::services::ContextNestServices;

#[derive(Debug, Deserialize)]
pub struct DiscardQuery {
    /// Free-form audit reason. Persisted in the WAL tombstone +
    /// audit log line so the deletion is traceable. Optional.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiscardResponse {
    /// `true` when one or more in-memory entries were removed and a
    /// tombstone was written. `false` when the fingerprint matched
    /// no in-memory bucket (no-op; no tombstone written).
    pub deleted: bool,
    /// Count of in-memory entries within the bucket that were
    /// removed. A bucket can hold multiple entries (same project +
    /// model + temperature + system_prompt with different user
    /// prompts). Zero means no-op.
    pub removed_rows: usize,
    /// Echo of the requested fingerprint, hex-encoded. Lets the
    /// caller confirm parsing succeeded without inspecting the path
    /// param again.
    pub fingerprint: String,
}

/// Handler for `DELETE /llm/v1/cache/entries/:fingerprint`.
pub async fn discard_entry(
    State(services): State<ContextNestServices>,
    Path(fingerprint): Path<String>,
    Query(params): Query<DiscardQuery>,
) -> Result<Json<DiscardResponse>, StatusCode> {
    let Some(fp_bytes) = hex_decode_32(&fingerprint) else {
        return Err(StatusCode::BAD_REQUEST);
    };

    // Walk the cache's bucket keys to find which ExactKeyPrefix
    // matches this fingerprint. There's no reverse-mapping table
    // (fingerprint is a one-way hash); the linear scan is O(num
    // buckets), typically dozens-to-low-thousands. The alternative —
    // requiring the caller to pass project/model/system explicitly —
    // pushes the SHA-256 computation to the caller and complicates
    // the URL. The scan approach lets the caller hand the
    // fingerprint back verbatim from the audit log.
    let matching_prefix: Option<ExactKeyPrefix> =
        services.llm_cache.find_prefix_by_fingerprint(&fp_bytes);

    let Some(prefix) = matching_prefix else {
        return Ok(Json(DiscardResponse {
            deleted: false,
            removed_rows: 0,
            fingerprint,
        }));
    };

    let removed = services.llm_cache.discard_prefix(&prefix, params.reason);

    Ok(Json(DiscardResponse {
        deleted: removed > 0,
        removed_rows: removed,
        fingerprint,
    }))
}

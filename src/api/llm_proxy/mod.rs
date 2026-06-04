//! OpenAI-compatible LLM proxy surface.
//!
//! v0.3 LLM proxy milestone (`docs/roadmap/v0.3-llm-proxy.md`).
//!
//! Phase 1 (OpenAI surface):
//! - Slice 1.1 — wire-format types (`openai_shapes`)
//! - Slice 1.2 — `POST /llm/v1/chat/completions` forwarder (`handler`)
//! - Slice 1.3a — `GET /llm/v1/models` (`models`)
//! - Slice 1.3b — `POST /llm/v1/embeddings` forwarder (`embeddings`)
//! - Slice 1.3c — multi-provider routing (in `LlmService::complete_chat`)
//! - Slice 1.4 — SDK fixture-parity tests (`tests/llm_proxy_sdk_parity.rs`)
//!
//! Phase 2 (cache layer):
//! - Slice 2.1 — cache-key derivation (`services::llm_cache`)
//! - Slice 2.2 — in-memory cache store (`services::llm_cache::LlmCacheService`)
//! - Slice 2.3 — cache wired into the chat-completions handler
//! - Slice 2.4 — `GET /llm/v1/cache/stats` (`cache_stats`)
//!
//! Remaining in Phase 2: substrate-backed semantic match (2.5).
//!
//! The shapes are intentionally `serde::Deserialize` + `serde::Serialize`
//! over `serde_json::Value` for the open-ended fields (tool parameters,
//! logit_bias, response_format JSON schemas) so the proxy can passthrough
//! provider-specific extensions without owning the full spec surface.
//! When usage proves a field needs typed validation, narrow it from
//! `Value` to a concrete type in a follow-up PR.

pub mod cache_delete;
pub mod cache_stats;
pub mod embeddings;
pub mod handler;
pub mod models;
pub mod openai_shapes;

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::services::ContextNestServices;

/// Build the LLM proxy router. Exposes chat-completions (POST),
/// models (GET), embeddings (POST), and cache stats (GET).
pub fn create_llm_proxy_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/llm/v1/chat/completions", post(handler::chat_completions))
        .route("/llm/v1/models", get(models::list_models))
        .route("/llm/v1/embeddings", post(embeddings::embeddings))
        .route("/llm/v1/cache/stats", get(cache_stats::cache_stats))
        .route(
            "/llm/v1/cache/entries/:fingerprint",
            delete(cache_delete::discard_entry),
        )
}

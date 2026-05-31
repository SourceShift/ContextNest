//! OpenAI-compatible LLM proxy surface.
//!
//! Phase 1 of the v0.3 LLM proxy milestone
//! (`docs/roadmap/v0.3-llm-proxy.md`). Shipped so far:
//!
//! - Slice 1.1 — wire-format types (`openai_shapes`)
//! - Slice 1.2 — `POST /llm/v1/chat/completions` forwarder (`handler`)
//! - Slice 1.3a — `GET /llm/v1/models` (`models`)
//!
//! Remaining in Phase 1: `/embeddings` (1.3b), multi-provider routing
//! (1.3c), SDK fixture-parity tests (1.4).
//!
//! The shapes are intentionally `serde::Deserialize` + `serde::Serialize`
//! over `serde_json::Value` for the open-ended fields (tool parameters,
//! logit_bias, response_format JSON schemas) so the proxy can passthrough
//! provider-specific extensions without owning the full spec surface.
//! When usage proves a field needs typed validation, narrow it from
//! `Value` to a concrete type in a follow-up PR.

pub mod handler;
pub mod models;
pub mod openai_shapes;

use axum::{
    routing::{get, post},
    Router,
};

use crate::services::ContextNestServices;

/// Build the LLM proxy router. Exposes chat-completions (POST) and
/// models (GET); embeddings (1.3b) and multi-provider routing (1.3c)
/// remain in Phase 1's queue.
pub fn create_llm_proxy_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/llm/v1/chat/completions", post(handler::chat_completions))
        .route("/llm/v1/models", get(models::list_models))
}

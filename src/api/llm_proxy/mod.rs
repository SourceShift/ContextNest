//! OpenAI-compatible LLM proxy surface.
//!
//! Phase 1 of the v0.3 LLM proxy milestone
//! (`docs/roadmap/v0.3-llm-proxy.md`). Slice 1.1 shipped the wire-format
//! types (`openai_shapes`); slice 1.2 (this) ships the
//! `POST /llm/v1/chat/completions` handler forwarding to the configured
//! `LlmService`. Provider routing keyed off the request `model` lands in
//! slice 1.3; fixture-parity tests in 1.4.
//!
//! The shapes are intentionally `serde::Deserialize` + `serde::Serialize`
//! over `serde_json::Value` for the open-ended fields (tool parameters,
//! logit_bias, response_format JSON schemas) so the proxy can passthrough
//! provider-specific extensions without owning the full spec surface.
//! When usage proves a field needs typed validation, narrow it from
//! `Value` to a concrete type in a follow-up PR.

pub mod handler;
pub mod openai_shapes;

use axum::{routing::post, Router};

use crate::services::ContextNestServices;

/// Build the LLM proxy router. Currently exposes only the chat-completions
/// endpoint; slice 1.3 adds `/embeddings` + `/models`.
pub fn create_llm_proxy_router() -> Router<ContextNestServices> {
    Router::new().route("/llm/v1/chat/completions", post(handler::chat_completions))
}

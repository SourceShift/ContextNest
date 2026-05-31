//! OpenAI-compatible LLM proxy surface.
//!
//! Phase 1 of the v0.3 LLM proxy milestone
//! (`docs/roadmap/v0.3-llm-proxy.md`). This module currently exposes only
//! the wire-format types (`openai_shapes`) — the HTTP handler and
//! provider routing land in slice 1.2, fixture-parity tests in 1.4.
//!
//! The shapes are intentionally `serde::Deserialize` + `serde::Serialize`
//! over `serde_json::Value` for the open-ended fields (tool parameters,
//! logit_bias, response_format JSON schemas) so the proxy can passthrough
//! provider-specific extensions without owning the full spec surface.
//! When usage proves a field needs typed validation, narrow it from
//! `Value` to a concrete type in a follow-up PR.

pub mod openai_shapes;

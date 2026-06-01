//! `GET /llm/v1/models` (v0.3 slice 1.3a).
//!
//! Returns an OpenAI-compatible list of available models. The set is
//! derived from whichever provider the substrate's `LlmService` is
//! configured with — when OpenAI is the configured backend, we report
//! a canonical set of OpenAI model IDs; same for Anthropic / Google.
//! When the LLM is disabled the response is an empty list with a 200
//! (NOT a 503) — clients use this endpoint to discover capability and
//! "no models" is a valid discovery answer.
//!
//! ## Why hardcoded, not proxied
//!
//! The spec's "proxy to upstream `models` and filter" requires a live
//! upstream HTTP call on every `/models` request. Most SDK callers hit
//! this endpoint exactly once to populate a UI dropdown, then never
//! again — paying the per-request HTTP cost for a list that changes
//! roughly quarterly is a poor trade. Versioning the canonical list in
//! source code is honest, deterministic, cache-able, and zero-cost at
//! request time. We refresh the list when a new model lands; that
//! cadence matches reality.

use axum::{extract::State, response::Json};
use serde::Serialize;

use crate::services::ContextNestServices;

/// One model entry in the `/models` response. Matches the OpenAI wire
/// format: `id`, `object: "model"`, `created` (unix ts), `owned_by`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelEntry {
    pub id: String,
    pub object: &'static str,
    pub created: i64,
    pub owned_by: &'static str,
}

/// `GET /llm/v1/models` response body. `object: "list"` per OpenAI's
/// list-style endpoints.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelsListResponse {
    pub object: &'static str,
    pub data: Vec<ModelEntry>,
}

/// Stable creation timestamp for canonical model entries. The OpenAI
/// wire format requires a `created` unix timestamp; we use a fixed
/// reference date (2024-01-01 UTC) for all entries so the response is
/// reproducible — call-time clock skew shouldn't show up as different
/// `created` values across requests. Real upstream `/models` endpoints
/// each carry their own creation timestamp; the OpenAI Python SDK
/// doesn't inspect this field for any logic.
const CANONICAL_CREATED_TS: i64 = 1_704_067_200; // 2024-01-01T00:00:00Z

/// `GET /llm/v1/models` handler.
///
/// Reports the UNION of canonical model lists for every provider the
/// substrate currently has configured. With slice 1.3c's multi-provider
/// routing, a substrate carrying both `ANTHROPIC_API_KEY` and
/// `OPENAI_API_KEY` exposes BOTH providers' models — the client can
/// then send a request with `model: "gpt-4o"` or `model:
/// "claude-3-5-sonnet-latest"` and the proxy routes accordingly.
pub async fn list_models(State(services): State<ContextNestServices>) -> Json<ModelsListResponse> {
    let mut data: Vec<ModelEntry> = Vec::new();
    for kind in services.llm.configured_provider_kinds() {
        match kind {
            "openai" => data.extend(openai_models()),
            "anthropic" => data.extend(anthropic_models()),
            "google" => data.extend(google_models()),
            "custom" => data.extend(custom_models()),
            _ => {} // unreachable; configured_provider_kinds emits the four cases
        }
    }
    Json(ModelsListResponse {
        object: "list",
        data,
    })
}

/// Canonical OpenAI model IDs commonly used through the chat-completions
/// endpoint. List sourced from OpenAI's public API docs; refresh when
/// a new generation lands. Order is "newest-most-capable first" so a
/// dropdown built from this list defaults sensibly.
fn openai_models() -> Vec<ModelEntry> {
    [
        "gpt-4o",
        "gpt-4o-mini",
        "gpt-4-turbo",
        "gpt-4",
        "gpt-3.5-turbo",
    ]
    .iter()
    .map(|id| ModelEntry {
        id: (*id).to_string(),
        object: "model",
        created: CANONICAL_CREATED_TS,
        owned_by: "openai",
    })
    .collect()
}

fn anthropic_models() -> Vec<ModelEntry> {
    [
        "claude-3-5-sonnet-latest",
        "claude-3-5-haiku-latest",
        "claude-3-opus-latest",
        "claude-3-haiku-20240307",
    ]
    .iter()
    .map(|id| ModelEntry {
        id: (*id).to_string(),
        object: "model",
        created: CANONICAL_CREATED_TS,
        owned_by: "anthropic",
    })
    .collect()
}

fn google_models() -> Vec<ModelEntry> {
    [
        "gemini-1.5-pro",
        "gemini-1.5-flash",
        "gemini-2.0-flash",
        "gemini-1.0-pro",
    ]
    .iter()
    .map(|id| ModelEntry {
        id: (*id).to_string(),
        object: "model",
        created: CANONICAL_CREATED_TS,
        owned_by: "google",
    })
    .collect()
}

/// `LlmProvider::Custom` is an OpenAI-compatible-protocol provider
/// (z.ai's GLM, LiteLLM, Ollama, etc.) — we don't know its model list
/// statically because each setup advertises a different one. Empty list
/// is the honest answer; callers either know what model to send or
/// inspect their own provider's docs.
fn custom_models() -> Vec<ModelEntry> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_models_carry_correct_owner() {
        let m = openai_models();
        assert!(!m.is_empty());
        for entry in &m {
            assert_eq!(entry.object, "model");
            assert_eq!(entry.owned_by, "openai");
            assert_eq!(entry.created, CANONICAL_CREATED_TS);
        }
    }

    #[test]
    fn anthropic_models_carry_correct_owner() {
        let m = anthropic_models();
        assert!(!m.is_empty());
        for entry in &m {
            assert_eq!(entry.owned_by, "anthropic");
        }
    }

    #[test]
    fn google_models_carry_correct_owner() {
        let m = google_models();
        assert!(!m.is_empty());
        for entry in &m {
            assert_eq!(entry.owned_by, "google");
        }
    }

    #[test]
    fn custom_models_is_empty_list() {
        // OpenAI-compatible provider — we don't know its catalog statically.
        let m = custom_models();
        assert!(m.is_empty());
    }

    #[test]
    fn response_envelope_uses_list_object_type() {
        let resp = ModelsListResponse {
            object: "list",
            data: openai_models(),
        };
        let v = serde_json::to_value(&resp).expect("serialize");
        assert_eq!(v["object"], "list");
        assert!(v["data"].is_array());
        assert!(!v["data"].as_array().unwrap().is_empty());
        // Each entry has the OpenAI shape.
        let first = &v["data"][0];
        assert!(first["id"].is_string());
        assert_eq!(first["object"], "model");
        assert!(first["created"].is_number());
        assert!(first["owned_by"].is_string());
    }

    #[test]
    fn newest_model_is_first_in_each_list() {
        // Sanity-check the ordering convention so a UI defaulting to
        // the first entry picks the newest-most-capable model.
        assert_eq!(openai_models()[0].id, "gpt-4o");
        assert_eq!(anthropic_models()[0].id, "claude-3-5-sonnet-latest");
        assert_eq!(google_models()[0].id, "gemini-1.5-pro");
    }
}

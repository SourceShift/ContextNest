//! `POST /llm/v1/embeddings` handler (v0.3 slice 1.3b).
//!
//! Maps OpenAI's embeddings wire format to the substrate's
//! [`crate::services::EmbeddingService`], returning OpenAI-shaped
//! responses. **The response's `model` field reports the substrate's
//! actual configured embedder, NOT the model the client requested.**
//! When the two differ, a `warn!` log line surfaces the substitution
//! so operators can audit. The response carries the actual model so
//! the caller can't be misled about what produced their vector.
//!
//! ## Why reuse `EmbeddingService` rather than build a parallel proxy
//!
//! The substrate already has an `EmbeddingService` with provider
//! routing, an exact-text cache (matching open-question #2's resolution
//! — exact-match cache only), retry handling, and configured API key
//! resolution from `config.toml` / env. Building a parallel HTTP client
//! to OpenAI's `/v1/embeddings` would duplicate ~200 LOC of
//! infrastructure that already works. The honest tradeoff is the
//! response model field carrying the substrate's actual model rather
//! than the requested one — documented above and surfaced in the
//! response itself.
//!
//! ## Slice 1.3b scope vs spec
//!
//! - Single-string and string-array `input` shapes: supported.
//! - Token-ID array `input` shapes (`Vec<u32>` / `Vec<Vec<u32>>`):
//!   400 — `EmbeddingService` operates on text, not pre-tokenized IDs.
//! - `encoding_format: "base64"`: 501 — float-only in this slice.
//! - `dimensions` override: parsed and ignored (logged warn).
//! - Caching: handled inside `EmbeddingService` via its `cache` field;
//!   no separate cache layer needed at the proxy level.
//!
//! ## Errors
//!
//! | Condition                                   | HTTP | type                    |
//! |---------------------------------------------|------|-------------------------|
//! | empty `input.Text("")` / empty `Texts([])`  | 400  | `invalid_request_error` |
//! | token-id input shape                        | 400  | `invalid_request_error` |
//! | `encoding_format: "base64"`                 | 501  | `unsupported_param`     |
//! | embedder call failed                        | 502  | `upstream_error`        |

use axum::{
    extract::{Json, State},
    http::StatusCode,
};
use serde_json::json;
use tracing::warn;

use super::openai_shapes::{
    EmbeddingEntry, EmbeddingsInput, EmbeddingsRequest, EmbeddingsResponse, EmbeddingsUsage,
};
use crate::services::ContextNestServices;

/// `POST /llm/v1/embeddings` handler.
pub async fn embeddings(
    State(services): State<ContextNestServices>,
    Json(req): Json<EmbeddingsRequest>,
) -> Result<Json<EmbeddingsResponse>, (StatusCode, Json<serde_json::Value>)> {
    // --- Validation --------------------------------------------------------
    if let Some(fmt) = req.encoding_format.as_deref() {
        if fmt != "float" {
            return Err(error_body(
                StatusCode::NOT_IMPLEMENTED,
                "unsupported_param",
                "`encoding_format: \"base64\"` is not supported in v0.3 slice 1.3b; only \"float\" is accepted",
            ));
        }
    }

    let inputs: Vec<String> = match req.input {
        EmbeddingsInput::Text(s) => {
            if s.is_empty() {
                return Err(error_body(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    "`input` must not be an empty string",
                ));
            }
            vec![s]
        }
        EmbeddingsInput::Texts(v) => {
            if v.is_empty() {
                return Err(error_body(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    "`input` array must contain at least one entry",
                ));
            }
            if v.iter().any(String::is_empty) {
                return Err(error_body(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    "`input` array entries must not be empty strings",
                ));
            }
            v
        }
        EmbeddingsInput::TokenIds(_) | EmbeddingsInput::TokenIdsBatch(_) => {
            return Err(error_body(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "token-id `input` shapes are not supported in v0.3 slice 1.3b; pass text strings instead",
            ));
        }
    };

    if req.dimensions.is_some() {
        // Forward intent in the log, ignore the value — the substrate's
        // embedder dimension is fixed per its configuration.
        warn!(
            requested_dimensions = ?req.dimensions,
            substrate_model = services.embedding.configured_model_name(),
            "embeddings: dimensions override is ignored in slice 1.3b — the substrate's embedder dimension is fixed"
        );
    }

    let substrate_model = services.embedding.configured_model_name().to_string();
    if req.model != substrate_model {
        warn!(
            client_requested = %req.model,
            substrate_actual = %substrate_model,
            "embeddings: client requested a different model than the substrate's configured embedder; response `model` field will report the substrate's actual model"
        );
    }

    // --- Dispatch ----------------------------------------------------------
    let mut data: Vec<EmbeddingEntry> = Vec::with_capacity(inputs.len());
    for (i, text) in inputs.iter().enumerate() {
        let embedding = match services.embedding.generate_embedding(text).await {
            Ok(e) => e,
            Err(e) => {
                return Err(error_body(
                    StatusCode::BAD_GATEWAY,
                    "upstream_error",
                    &format!("embedding generation failed for input[{i}]: {e}"),
                ));
            }
        };
        data.push(EmbeddingEntry {
            object: "embedding".to_string(),
            embedding,
            index: i as u32,
        });
    }

    // Token counts are not exposed by EmbeddingService today; report 0.
    // OpenAI's spec calls these "approximations" anyway. Future work
    // could integrate a tokenizer for accurate counts.
    let usage = EmbeddingsUsage {
        prompt_tokens: 0,
        total_tokens: 0,
    };

    Ok(Json(EmbeddingsResponse {
        object: "list".to_string(),
        data,
        model: substrate_model,
        usage,
    }))
}

/// Build an OpenAI-shaped error body — same shape as the chat-completions
/// handler's `error_body` helper, kept local to this module to avoid
/// cross-module coupling between two otherwise-independent handlers.
fn error_body(
    status: StatusCode,
    err_type: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(json!({
            "error": {
                "message": message,
                "type": err_type,
                "code": null,
                "param": null,
            }
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embeddings_input_deserializes_string_form() {
        let fixture = serde_json::json!({
            "model": "text-embedding-3-small",
            "input": "hello world"
        });
        let req: EmbeddingsRequest = serde_json::from_value(fixture).expect("must parse");
        match req.input {
            EmbeddingsInput::Text(s) => assert_eq!(s, "hello world"),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn embeddings_input_deserializes_array_form() {
        let fixture = serde_json::json!({
            "model": "text-embedding-3-small",
            "input": ["a", "b", "c"]
        });
        let req: EmbeddingsRequest = serde_json::from_value(fixture).expect("must parse");
        match req.input {
            EmbeddingsInput::Texts(v) => assert_eq!(v.len(), 3),
            _ => panic!("expected Texts variant"),
        }
    }

    #[test]
    fn embeddings_input_token_array_disambiguates() {
        // `[1, 2, 3]` should land in TokenIds, not Texts — the untagged
        // enum picks the first variant that parses. Texts requires
        // strings, so a numeric array goes to TokenIds.
        let fixture = serde_json::json!({
            "model": "text-embedding-3-small",
            "input": [1, 2, 3]
        });
        let req: EmbeddingsRequest = serde_json::from_value(fixture).expect("must parse");
        assert!(matches!(req.input, EmbeddingsInput::TokenIds(_)));
    }

    #[test]
    fn embeddings_response_envelope_uses_list_object() {
        let resp = EmbeddingsResponse {
            object: "list".to_string(),
            data: vec![EmbeddingEntry {
                object: "embedding".to_string(),
                embedding: vec![0.1, 0.2, 0.3],
                index: 0,
            }],
            model: "text-embedding-3-small".to_string(),
            usage: EmbeddingsUsage {
                prompt_tokens: 5,
                total_tokens: 5,
            },
        };
        let v = serde_json::to_value(&resp).expect("serialize");
        assert_eq!(v["object"], "list");
        assert_eq!(v["data"][0]["object"], "embedding");
        assert_eq!(v["data"][0]["index"], 0);
        assert_eq!(v["data"][0]["embedding"].as_array().unwrap().len(), 3);
        assert_eq!(v["usage"]["prompt_tokens"], 5);
    }

    #[test]
    fn error_body_matches_openai_envelope() {
        let (status, body) = error_body(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "test message",
        );
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let v = body.0;
        let err = &v["error"];
        assert_eq!(err["message"], "test message");
        assert_eq!(err["type"], "invalid_request_error");
        assert!(err["code"].is_null());
        assert!(err["param"].is_null());
    }
}

//! `POST /llm/v1/chat/completions` handler.
//!
//! Maps OpenAI's chat/completions wire format (from `openai_shapes`) to
//! [`crate::services::llm::LlmService::complete_chat`], then maps the
//! result back to the OpenAI response shape.
//!
//! ## Slice history
//!
//! - 1.2 plain forwarder
//! - 1.3c multi-provider routing
//! - **2.3 (this commit) caching:** consult `LlmCacheService` before
//!   dispatching upstream; insert on miss. Per-request
//!   `x-cn-cache-max-age: <seconds>` header overrides the default TTL;
//!   `max-age: 0` forces a cache bypass. Cache failures (embedder
//!   unavailable, empty embedding, etc.) are best-effort — they never
//!   fail the upstream call.
//!
//! ## Error surface
//!
//! | Error                                | HTTP | Why                                  |
//! |--------------------------------------|------|--------------------------------------|
//! | empty `messages`                     | 400  | client mistake; surface at parse-time |
//! | unsupported role / multimodal content | 400 | not in 1.2's scope; explicit fail    |
//! | `stream: true`                       | 501  | v0.3 doesn't cache streams (open-q #1)|
//! | LLM provider unconfigured            | 503  | `LlmService::is_enabled() == false`  |
//! | upstream provider failure            | 502  | generic upstream error               |
//!
//! ## What's NOT in this slice (deferred to later PRs per #90)
//!
//! - Multi-provider routing keyed off the request's `model` field (1.3).
//! - Tool / function-call handling (deferred to v0.3.1 per open-q #4).
//! - Multimodal `image_url` parts — text-only chat in 1.2; multimodal
//!   pass-through requires the underlying SDK's multimodal Part variants
//!   wired through, which is its own slice.
//! - Streaming SSE responses (out of scope for v0.3 per open-q #1).
//! - Caching (Phase 2).

use std::time::Duration;

use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::json;

use super::openai_shapes::{
    ChatCompletionsRequest, ChatCompletionsResponse, Choice, ContentPart, Message, MessageContent,
    Role, Usage,
};
use crate::services::llm::{ChatCompletionOpts, ChatMessage, ChatRole};
use crate::services::llm_cache::{
    derive_cache_key, extract_system_prompt_text, extract_user_prompt_text, CacheKey,
    DEFAULT_PROJECT_ID,
};
use crate::services::ContextNestServices;

/// Header name (lowercase as required by HTTP/2) for per-request TTL override.
/// Mirrors OpenAI's `x-*` extension convention.
pub const CACHE_MAX_AGE_HEADER: &str = "x-cn-cache-max-age";

/// `POST /llm/v1/chat/completions` — OpenAI-compatible plain proxy.
///
/// See module docs for error surface. On success returns a single-choice
/// response with the model's text + token usage.
pub async fn chat_completions(
    State(services): State<ContextNestServices>,
    headers: HeaderMap,
    Json(req): Json<ChatCompletionsRequest>,
) -> Result<Json<ChatCompletionsResponse>, (StatusCode, Json<serde_json::Value>)> {
    // --- Validation ---------------------------------------------------------
    if req.messages.is_empty() {
        return Err(error_body(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "`messages` must contain at least one entry",
        ));
    }
    if req.stream.unwrap_or(false) {
        // v0.3's open-question #1 resolution: streaming is not in scope
        // for this milestone. Returning 501 is more honest than silently
        // ignoring the flag and sending a non-streamed body the client
        // can't parse with its SSE consumer.
        return Err(error_body(
            StatusCode::NOT_IMPLEMENTED,
            "unsupported_param",
            "`stream: true` is not supported in v0.3; omit the flag or set it to false",
        ));
    }

    // --- Translate OpenAI request → ChatCompletionOpts ----------------------
    let mut internal_messages: Vec<ChatMessage> = Vec::with_capacity(req.messages.len());
    for (i, m) in req.messages.iter().enumerate() {
        let text = match flatten_message_content(m) {
            Ok(t) => t,
            Err(e) => {
                return Err(error_body(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    &format!("message[{i}]: {e}"),
                ));
            }
        };
        internal_messages.push(ChatMessage {
            role: map_role(&m.role),
            content: text,
        });
    }

    // OpenAI's `max_completion_tokens` is the newer name; `max_tokens` the
    // legacy one. When both are sent, OpenAI's own rule is
    // `max_completion_tokens` wins; we mirror that.
    let effective_max_tokens = req.max_completion_tokens.or(req.max_tokens);

    // --- Cache lookup -------------------------------------------------------
    //
    // Best-effort cache attempt BEFORE the upstream-availability check.
    // A cached entry is valid even when the upstream provider is offline —
    // that's the whole point of the cache. Embedder failures, empty
    // embeddings, or missing cache configuration all fall through to the
    // upstream path; no cache error fails the request.
    let max_age = parse_max_age_header(&headers);
    let cache_attempt = try_build_cache_key(&services, &req).await;
    if let Some(ref key) = cache_attempt {
        if let Some(hit) = services.llm_cache.lookup(key, max_age) {
            // Cache hit. Re-stamp `id` and `created` so distinct hits
            // from one cached entry carry distinct trace identifiers
            // (per-request traceability is preserved even on hit) and
            // overwrite the response's `model` field with the originally
            // requested model so the client sees what it asked for.
            let mut hit = hit;
            hit.id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
            hit.created = chrono::Utc::now().timestamp();
            hit.model = req.model.clone();
            tracing::debug!(
                model = %req.model,
                "llm_proxy cache hit"
            );
            return Ok(Json(hit));
        }
    }

    let opts = ChatCompletionOpts {
        model: req.model.clone(),
        messages: internal_messages,
        temperature: req.temperature,
        top_p: req.top_p,
        max_tokens: effective_max_tokens,
        seed: req.seed,
        presence_penalty: req.presence_penalty,
        frequency_penalty: req.frequency_penalty,
    };

    // --- Dispatch -----------------------------------------------------------
    if !services.llm.is_enabled() {
        return Err(error_body(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "LLM provider is not configured on this substrate",
        ));
    }
    let result = match services.llm.complete_chat(opts).await {
        Ok(r) => r,
        Err(e) => {
            return Err(error_body(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                &format!("LLM provider call failed: {e}"),
            ));
        }
    };

    // --- Translate ChatCompletionResult → OpenAI response -------------------
    let response = ChatCompletionsResponse {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp(),
        model: req.model,
        choices: vec![Choice {
            index: 0,
            message: Message {
                role: Role::Assistant,
                content: Some(MessageContent::Text(result.text)),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            // The underlying SDK doesn't expose finish_reason via
            // ModelUsage; "stop" is the safe default for a complete
            // (non-streamed, non-truncated) generation. Slice 1.3 may
            // refine when the provider adapters surface this.
            finish_reason: Some("stop".to_string()),
            logprobs: None,
        }],
        usage: Some(Usage {
            prompt_tokens: result.input_tokens,
            completion_tokens: result.output_tokens,
            total_tokens: result.input_tokens + result.output_tokens,
            prompt_tokens_details: None,
            completion_tokens_details: None,
        }),
        system_fingerprint: None,
    };

    // --- Cache insert (best-effort) -----------------------------------------
    //
    // Only when we successfully derived a cache key on the lookup pass. If
    // the embedder was unavailable then we can't key the entry and there's
    // nothing to store; the upstream response still returns normally.
    // `max_age = 0` (bypass) is honored on the lookup but NOT on insert —
    // a bypass request still warms the cache for subsequent callers,
    // matching `Cache-Control: no-cache` HTTP semantics ("don't serve me
    // a cached response, but you may cache mine").
    if let Some(key) = cache_attempt {
        services.llm_cache.insert(key, response.clone());
        tracing::debug!(model = %response.model, "llm_proxy cache insert");
    }

    Ok(Json(response))
}

/// Best-effort `CacheKey` derivation. Returns `None` when the embedder
/// can't produce a vector for the user prompt — in that case the caller
/// proceeds without cache lookup or insert. Errors are logged at
/// `debug!` because they're a normal degradation path, not a problem.
async fn try_build_cache_key(
    services: &ContextNestServices,
    req: &ChatCompletionsRequest,
) -> Option<CacheKey> {
    let user_text = extract_user_prompt_text(&req.messages);
    if user_text.is_empty() {
        // No user content to embed — can happen for assistant-only
        // requests (rare). Skip cache for these.
        return None;
    }
    let system_text = extract_system_prompt_text(&req.messages);
    let embedding = match services.embedding.generate_embedding(&user_text).await {
        Ok(v) if !v.is_empty() => v,
        Ok(_) => {
            tracing::debug!("llm_proxy: empty embedding — skipping cache");
            return None;
        }
        Err(e) => {
            tracing::debug!(error = %e, "llm_proxy: embedder failed — skipping cache");
            return None;
        }
    };
    Some(derive_cache_key(
        DEFAULT_PROJECT_ID,
        &req.model,
        req.temperature,
        &system_text,
        embedding,
    ))
}

/// Parse the per-request TTL override header. Returns `Some(Duration)`
/// when present and valid; `None` falls back to the cache's default TTL.
/// `Some(Duration::ZERO)` forces a cache bypass.
pub fn parse_max_age_header(headers: &HeaderMap) -> Option<Duration> {
    let value = headers.get(CACHE_MAX_AGE_HEADER)?.to_str().ok()?;
    let secs: u64 = value.trim().parse().ok()?;
    Some(Duration::from_secs(secs))
}

/// Reduce a [`Message`]'s content (string OR array of parts) into a single
/// text blob. Returns an error string for unsupported part types so the
/// caller can surface them as 400s instead of silently dropping them.
fn flatten_message_content(m: &Message) -> Result<String, String> {
    match &m.content {
        // Assistant messages can be content-less when they only emit
        // tool_calls. Tool messages must carry content. The proxy in 1.2
        // treats no-content assistant messages as empty strings; tool
        // messages without content are a client error.
        None => match m.role {
            Role::Assistant => Ok(String::new()),
            _ => Err("missing required `content` field".to_string()),
        },
        Some(MessageContent::Text(s)) => Ok(s.clone()),
        Some(MessageContent::Parts(parts)) => {
            let mut out = String::new();
            for part in parts {
                match part {
                    ContentPart::Text { text } => out.push_str(text),
                    ContentPart::ImageUrl { .. } => {
                        return Err(
                            "multimodal `image_url` content is not supported in v0.3 slice 1.2"
                                .to_string(),
                        );
                    }
                    ContentPart::Other => {
                        return Err(
                            "unsupported content part `type` — only `text` is accepted in this slice"
                                .to_string(),
                        );
                    }
                }
            }
            Ok(out)
        }
    }
}

fn map_role(role: &Role) -> ChatRole {
    match role {
        Role::System => ChatRole::System,
        Role::User => ChatRole::User,
        Role::Assistant => ChatRole::Assistant,
        Role::Tool => ChatRole::Tool,
    }
}

/// Build an OpenAI-shaped error body (`{ "error": { "message", "type",
/// "code" } }`) at a chosen HTTP status. Matches OpenAI's wire format so
/// SDK error parsers (`openai.APIError`, etc.) can deserialize directly.
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

    fn user_msg(s: &str) -> Message {
        Message {
            role: Role::User,
            content: Some(MessageContent::Text(s.to_string())),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    #[test]
    fn flatten_string_content_is_passthrough() {
        let m = user_msg("hello world");
        assert_eq!(flatten_message_content(&m).unwrap(), "hello world");
    }

    #[test]
    fn flatten_parts_concatenates_text_parts() {
        let m = Message {
            role: Role::User,
            content: Some(MessageContent::Parts(vec![
                ContentPart::Text {
                    text: "hello ".into(),
                },
                ContentPart::Text {
                    text: "world".into(),
                },
            ])),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        };
        assert_eq!(flatten_message_content(&m).unwrap(), "hello world");
    }

    #[test]
    fn flatten_image_url_part_returns_error_for_1_2() {
        let m = Message {
            role: Role::User,
            content: Some(MessageContent::Parts(vec![
                ContentPart::Text {
                    text: "describe: ".into(),
                },
                ContentPart::ImageUrl {
                    image_url: serde_json::json!({"url": "https://example.com/x.png"}),
                },
            ])),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        };
        let err = flatten_message_content(&m).expect_err("must reject image_url");
        assert!(err.contains("image_url"));
        assert!(err.contains("not supported in v0.3 slice 1.2"));
    }

    #[test]
    fn flatten_missing_content_on_assistant_is_empty_string() {
        // Assistant messages can omit content when they only emit
        // tool_calls — empty-string content here is correct, not an error.
        let m = Message {
            role: Role::Assistant,
            content: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        };
        assert_eq!(flatten_message_content(&m).unwrap(), "");
    }

    #[test]
    fn flatten_missing_content_on_user_is_an_error() {
        let m = Message {
            role: Role::User,
            content: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        };
        let err = flatten_message_content(&m).expect_err("must reject");
        assert!(err.contains("missing required `content` field"));
    }

    #[test]
    fn role_mapping_is_one_to_one() {
        assert_eq!(map_role(&Role::System), ChatRole::System);
        assert_eq!(map_role(&Role::User), ChatRole::User);
        assert_eq!(map_role(&Role::Assistant), ChatRole::Assistant);
        assert_eq!(map_role(&Role::Tool), ChatRole::Tool);
    }

    #[test]
    fn error_body_matches_openai_envelope() {
        let (status, body) = error_body(StatusCode::BAD_REQUEST, "invalid_request_error", "boom");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let v = body.0;
        let err = v.get("error").expect("error envelope present");
        assert_eq!(err["message"], "boom");
        assert_eq!(err["type"], "invalid_request_error");
        assert!(err["code"].is_null());
        assert!(err["param"].is_null());
    }

    // =========================================================================
    // Cache integration tests (slice 2.3)
    // =========================================================================
    //
    // These cover the cache header parsing + the bypass / TTL-override
    // semantics. Full end-to-end hit/miss against the live handler is
    // covered by direct `LlmCacheService` tests in `services::llm_cache`
    // (slice 2.2) since spinning up `ContextNestServices` in a unit
    // test would couple this handler test to mock-mode database wiring.

    use axum::http::HeaderValue;

    #[test]
    fn parse_max_age_header_returns_none_when_missing() {
        let h = HeaderMap::new();
        assert_eq!(parse_max_age_header(&h), None);
    }

    #[test]
    fn parse_max_age_header_returns_seconds_when_present() {
        let mut h = HeaderMap::new();
        h.insert(CACHE_MAX_AGE_HEADER, HeaderValue::from_static("3600"));
        assert_eq!(parse_max_age_header(&h), Some(Duration::from_secs(3600)));
    }

    #[test]
    fn parse_max_age_header_zero_is_explicit_bypass() {
        // The bypass path. `Duration::ZERO` reaches `LlmCacheService::lookup`
        // which short-circuits to miss. Documents the wire contract:
        // `x-cn-cache-max-age: 0` means "don't serve me a cached response".
        let mut h = HeaderMap::new();
        h.insert(CACHE_MAX_AGE_HEADER, HeaderValue::from_static("0"));
        assert_eq!(parse_max_age_header(&h), Some(Duration::ZERO));
    }

    #[test]
    fn parse_max_age_header_tolerates_whitespace() {
        let mut h = HeaderMap::new();
        h.insert(CACHE_MAX_AGE_HEADER, HeaderValue::from_static("  60  "));
        assert_eq!(parse_max_age_header(&h), Some(Duration::from_secs(60)));
    }

    #[test]
    fn parse_max_age_header_rejects_non_numeric() {
        // Garbled value → None (fall back to default TTL). Not a 400 —
        // mirroring how OpenAI ignores unknown `x-*` headers.
        let mut h = HeaderMap::new();
        h.insert(CACHE_MAX_AGE_HEADER, HeaderValue::from_static("forever"));
        assert_eq!(parse_max_age_header(&h), None);
    }

    #[test]
    fn parse_max_age_header_rejects_negative_value() {
        // u64 parse rejects "-1" — None falls back to default TTL.
        let mut h = HeaderMap::new();
        h.insert(CACHE_MAX_AGE_HEADER, HeaderValue::from_static("-1"));
        assert_eq!(parse_max_age_header(&h), None);
    }

    #[test]
    fn header_name_constant_is_lowercase_for_http2_compat() {
        // HTTP/2 requires lowercase header names; if the constant ever
        // gains an uppercase character, axum will mis-route the header
        // lookup. Pin the contract.
        assert_eq!(CACHE_MAX_AGE_HEADER, CACHE_MAX_AGE_HEADER.to_lowercase());
    }
}

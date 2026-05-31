//! OpenAI chat/completions wire-format types.
//!
//! `serde::Deserialize` + `serde::Serialize` shapes that match the request
//! and response bodies the `openai-python` SDK, `@anthropic-ai/sdk`
//! (compatibility mode), and `@google/generative-ai` (when pointed at the
//! OpenAI-compat shim) emit and parse. Designed to round-trip recorded
//! SDK fixtures verbatim — the proxy's job in slice 1.2 is to accept
//! these unchanged and translate to provider-specific calls.
//!
//! ## Design decisions
//!
//! - **`content` is `String | Vec<ContentPart>` (untagged enum).** OpenAI
//!   accepts both forms; the common case is string, the multimodal case
//!   is the array. Modeling both means the proxy never has to fail on
//!   real-world SDK requests.
//! - **Open-ended fields stay as `serde_json::Value`.** `tools[].function.parameters`
//!   is a JSON schema; `logit_bias`, `response_format.json_schema`, and
//!   `tool_calls[].function.arguments` (stringified JSON) all carry
//!   payloads whose shape is provider-specific. Modeling them concretely
//!   would require pinning to a spec version we'd then have to chase.
//! - **Optional fields skip-serialize-if-none.** Requests round-trip with
//!   only the keys the caller actually set, so an SDK fixture deserialized
//!   then re-serialized produces byte-equivalent output (modulo key
//!   ordering, which serde_json doesn't preserve across maps anyway).
//! - **Unknown fields preserved on response side; rejected on request side.**
//!   `#[serde(deny_unknown_fields)]` on request types catches client typos
//!   (`temprature` → 400 INVALID_PARAMS at parse time). Response types
//!   stay open so upstream providers can add fields without breaking the
//!   proxy's pass-through.
//!
//! ## What's NOT in this PR
//!
//! - Streaming (SSE) response types — v0.3 doesn't cache streaming;
//!   resolution carried forward from the spec's open-questions.
//! - Embeddings request/response — added in slice 1.3.
//! - The `models` list response — also slice 1.3.
//! - Error envelope shape — added when slice 1.2 wires the handler
//!   (error mapping needs the dispatch context).

use serde::{Deserialize, Serialize};
use serde_json::Value;

// =============================================================================
// Request
// =============================================================================

/// `POST /v1/chat/completions` request body. Mirrors the OpenAI spec
/// surface: model + messages plus the per-call generation knobs.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ChatCompletionsRequest {
    /// Provider-specific model identifier (`gpt-4o-mini`, `claude-3-5-sonnet-latest`,
    /// `gemini-1.5-flash`, ...). The proxy routes based on this string.
    pub model: String,
    /// Conversation history including the latest user turn. Order matters.
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Number of completions to return. Defaults to 1 if omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    /// Max tokens to generate. Newer OpenAI clients send `max_completion_tokens`
    /// instead; we accept the legacy `max_tokens` name on request because
    /// the SDKs we target still emit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Newer OpenAI field. When both are present `max_completion_tokens`
    /// wins per OpenAI's own resolution rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u32>,
    /// `"stop"` can be a single string or an array of up to 4 strings in
    /// OpenAI's spec; untagged enum accepts both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<StringOrVec>,
    /// SSE streaming. The handler in slice 1.2 will read this; the cache
    /// layer in Phase 2 deliberately does not cache streamed responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    /// Token-id → bias map. Open-ended payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<Value>,
    /// Reproducibility seed (best-effort; providers don't guarantee identical
    /// output even with identical seeds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// `text` (default), `json_object`, or `json_schema` with a schema
    /// payload. Kept as `Value` because the schema sub-payload is
    /// provider-specific.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    /// Function/tool definitions the model may call. Open-ended `parameters`
    /// payload — slice 1.2 forwards verbatim, the cache key in Phase 2
    /// deliberately does NOT include tools (per spec's open-question #4
    /// resolution).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDef>>,
    /// `"auto"` / `"none"` / `"required"` / `{type:"function", function:{name}}`.
    /// Kept as `Value` because of the polymorphism.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    /// Identifier passed through for upstream rate-limit / abuse tracking.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Return log-probs per token. Triggers a different upstream call path
    /// at some providers; passed verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_logprobs: Option<u32>,
}

/// Either a string or an array of strings. Used for the `stop` field which
/// OpenAI accepts in both shapes.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum StringOrVec {
    String(String),
    Vec(Vec<String>),
}

// =============================================================================
// Message
// =============================================================================

/// One turn in the conversation history. `role` discriminates how the
/// content is interpreted; tool messages also carry `tool_call_id`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Message {
    pub role: Role,
    /// `String` for the common case, `Vec<ContentPart>` for multimodal /
    /// structured content. Untagged so SDK fixtures deserialize verbatim.
    /// Optional because an assistant message MAY have only `tool_calls`
    /// and no textual content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<MessageContent>,
    /// Speaker tag for system / function-name disambiguation. Rarely used
    /// outside of legacy function-calling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Set on `role=tool` messages — references the originating tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Set on `role=assistant` messages when the model invoked tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// `system` | `user` | `assistant` | `tool`. The legacy `function` role is
/// intentionally omitted — providers map old function calls into the
/// `tool` role under the modern spec.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// Content can be a plain string OR an array of typed parts (text / image /
/// audio / file). Untagged enum lets SDK fixtures deserialize either form.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

/// One typed content fragment in a multimodal message. `text` parts carry
/// the visible string; `image_url` and other provider-specific parts stay
/// as a tagged `Value` so the proxy can forward them without owning every
/// modality's schema.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl {
        /// `{ "url": "data:image/png;base64,...", "detail": "auto" }` etc.
        /// Kept as `Value` because providers diverge on the inner shape.
        image_url: Value,
    },
    /// Catch-all for `audio` / `file` / future content-part kinds. The
    /// proxy forwards these verbatim; providers either accept or reject.
    #[serde(other, skip_serializing)]
    Other,
}

// =============================================================================
// Tools
// =============================================================================

/// Tool definition exposed to the model. The OpenAI spec only defines
/// `type: "function"`; we represent that explicitly while keeping the
/// inner `parameters` open-ended (JSON Schema).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub kind: ToolKind,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    Function,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema describing the function's parameters. Kept as `Value`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
    /// OpenAI's `strict: true` opt-in to strict-schema validation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}

/// Tool call emitted by an assistant message. `arguments` is stringified
/// JSON per OpenAI's wire format — the caller parses it.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: ToolKind,
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ToolCallFunction {
    pub name: String,
    /// Stringified JSON. OpenAI's wire format quirk — keep as `String`,
    /// parse at the consumer.
    pub arguments: String,
}

// =============================================================================
// Response
// =============================================================================

/// `POST /v1/chat/completions` response body. Response types do NOT use
/// `deny_unknown_fields` so upstream providers can add new fields without
/// breaking the proxy's pass-through.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ChatCompletionsResponse {
    pub id: String,
    /// Always `"chat.completion"` per spec.
    pub object: String,
    /// Unix timestamp in seconds.
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    /// `stop` / `length` / `tool_calls` / `content_filter` / `function_call`
    /// (legacy). Kept as String because providers occasionally emit
    /// variants outside the spec.
    pub finish_reason: Option<String>,
    /// Logprobs payload — open-ended `Value`. Set when the request opted
    /// in via `logprobs: true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    /// Newer breakdown fields (cached_tokens, audio_tokens, reasoning_tokens,
    /// ...). Open-ended; providers add these incrementally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<Value>,
}

// =============================================================================
// Embeddings request / response
// =============================================================================

/// `POST /v1/embeddings` request body. `input` is the polymorphic field
/// the OpenAI SDK accepts in four shapes: a single string, an array of
/// strings, a single token-id array, or an array of token-id arrays.
/// Slice 1.3b accepts the two text-string shapes; token-id arrays
/// return 400 (the underlying `EmbeddingService` operates on text).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingsRequest {
    /// Model identifier from the caller. The proxy forwards to the
    /// substrate's configured embedder regardless; the response's
    /// `model` field reports the substrate's actual model so the
    /// caller sees no fidelity lie.
    pub model: String,
    /// String, string-array, token-id-array, or array-of-token-id-arrays
    /// per OpenAI's spec. The handler validates which variant landed.
    pub input: EmbeddingsInput,
    /// `"float"` (default — Vec<f32> in response) or `"base64"`. v0.3
    /// slice 1.3b ships float only; base64 returns 501.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<String>,
    /// Provider-specific override for the embedding dimensionality
    /// (some embedder models support trimmed dimensions). Forwarded to
    /// the substrate which currently ignores it — slice 1.3c will
    /// surface this through to the EmbeddingService if multi-model
    /// support lands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// Polymorphic `input` field. Untagged enum accepts the four shapes
/// OpenAI's SDK emits.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum EmbeddingsInput {
    Text(String),
    Texts(Vec<String>),
    TokenIds(Vec<u32>),
    TokenIdsBatch(Vec<Vec<u32>>),
}

/// `POST /v1/embeddings` response body.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct EmbeddingsResponse {
    /// Always `"list"`.
    pub object: String,
    pub data: Vec<EmbeddingEntry>,
    /// The model that actually ran. May differ from
    /// `EmbeddingsRequest.model` when the proxy substitutes the
    /// substrate's configured embedder — by design, so callers can
    /// audit fidelity from the response itself.
    pub model: String,
    pub usage: EmbeddingsUsage,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct EmbeddingEntry {
    /// Always `"embedding"`.
    pub object: String,
    pub embedding: Vec<f32>,
    pub index: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct EmbeddingsUsage {
    pub prompt_tokens: u32,
    pub total_tokens: u32,
}

// =============================================================================
// Tests — round-trip fixtures from real SDK request/response bodies
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Minimal request from `openai-python` v1.x — the most common
    /// real-world shape.
    #[test]
    fn deserialize_minimal_openai_request() {
        let fixture = json!({
            "model": "gpt-4o-mini",
            "messages": [
                {"role": "user", "content": "Hello, world!"}
            ]
        });
        let req: ChatCompletionsRequest = serde_json::from_value(fixture).expect("must parse");
        assert_eq!(req.model, "gpt-4o-mini");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, Role::User);
        match &req.messages[0].content {
            Some(MessageContent::Text(t)) => assert_eq!(t, "Hello, world!"),
            _ => panic!("expected string content"),
        }
        assert!(req.temperature.is_none());
        assert!(req.tools.is_none());
    }

    /// Full-feature request exercising every commonly-used field.
    /// Catches missed defaults / wrong types.
    #[test]
    fn deserialize_full_feature_openai_request() {
        let fixture = json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "Be terse."},
                {"role": "user", "content": "What is 2+2?"},
                {"role": "assistant", "content": "4."},
                {"role": "user", "content": "Why?"}
            ],
            "temperature": 0.7,
            "top_p": 0.95,
            "max_tokens": 256,
            "stop": ["\n\n", "END"],
            "stream": false,
            "presence_penalty": 0.1,
            "frequency_penalty": 0.0,
            "seed": 42,
            "user": "test-user-1",
            "response_format": {"type": "json_object"},
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Look up the weather",
                    "parameters": {
                        "type": "object",
                        "properties": {"city": {"type": "string"}}
                    }
                }
            }],
            "tool_choice": "auto"
        });
        let req: ChatCompletionsRequest = serde_json::from_value(fixture).expect("must parse");
        assert_eq!(req.model, "gpt-4o");
        assert_eq!(req.messages.len(), 4);
        assert_eq!(req.temperature, Some(0.7));
        assert_eq!(req.seed, Some(42));
        assert_eq!(req.max_tokens, Some(256));
        match req.stop.as_ref().unwrap() {
            StringOrVec::Vec(v) => assert_eq!(v.len(), 2),
            StringOrVec::String(_) => panic!("expected array"),
        }
        let tools = req.tools.as_ref().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.name, "get_weather");
    }

    /// Multimodal `content: [...]` array — the form sent when callers
    /// attach images alongside text.
    #[test]
    fn deserialize_multimodal_content_array() {
        let fixture = json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Describe this image"},
                    {"type": "image_url", "image_url": {"url": "https://example.com/a.png"}}
                ]
            }]
        });
        let req: ChatCompletionsRequest = serde_json::from_value(fixture).expect("must parse");
        match req.messages[0].content.as_ref().unwrap() {
            MessageContent::Parts(parts) => {
                assert_eq!(parts.len(), 2);
                match &parts[0] {
                    ContentPart::Text { text } => assert_eq!(text, "Describe this image"),
                    _ => panic!("expected text part"),
                }
                match &parts[1] {
                    ContentPart::ImageUrl { image_url } => {
                        assert_eq!(image_url["url"], "https://example.com/a.png");
                    }
                    _ => panic!("expected image_url part"),
                }
            }
            _ => panic!("expected parts array"),
        }
    }

    /// `stop` accepts a single string OR an array. The OpenAI spec
    /// supports both — must not fail on either.
    #[test]
    fn stop_field_accepts_string_or_array() {
        let s1 = json!({"model": "x", "messages": [], "stop": "END"});
        let s2 = json!({"model": "x", "messages": [], "stop": ["A", "B"]});
        let r1: ChatCompletionsRequest = serde_json::from_value(s1).expect("string stop");
        let r2: ChatCompletionsRequest = serde_json::from_value(s2).expect("array stop");
        match r1.stop.unwrap() {
            StringOrVec::String(s) => assert_eq!(s, "END"),
            StringOrVec::Vec(_) => panic!("expected string variant"),
        }
        match r2.stop.unwrap() {
            StringOrVec::Vec(v) => assert_eq!(v, vec!["A".to_string(), "B".to_string()]),
            StringOrVec::String(_) => panic!("expected array variant"),
        }
    }

    /// Unknown request field → rejected. Catches client typos at parse time
    /// rather than letting them silently pass through to the upstream
    /// (which would then reject them with a less-readable error).
    #[test]
    fn unknown_request_field_is_rejected() {
        let fixture = json!({
            "model": "gpt-4o",
            "messages": [],
            "temprature": 0.7  // typo
        });
        let err =
            serde_json::from_value::<ChatCompletionsRequest>(fixture).expect_err("must reject");
        assert!(
            err.to_string().contains("temprature") || err.to_string().contains("unknown field"),
            "expected unknown-field error mentioning the typo, got: {err}"
        );
    }

    /// Minimal response — what a non-streaming call returns. Verifies the
    /// happy path of the deserialize direction.
    #[test]
    fn deserialize_minimal_response() {
        let fixture = json!({
            "id": "chatcmpl-abc123",
            "object": "chat.completion",
            "created": 1716120000,
            "model": "gpt-4o-mini",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "4"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 1, "total_tokens": 11}
        });
        let resp: ChatCompletionsResponse = serde_json::from_value(fixture).expect("must parse");
        assert_eq!(resp.id, "chatcmpl-abc123");
        assert_eq!(resp.object, "chat.completion");
        assert_eq!(resp.choices.len(), 1);
        let choice = &resp.choices[0];
        assert_eq!(choice.index, 0);
        assert_eq!(choice.message.role, Role::Assistant);
        assert_eq!(choice.finish_reason.as_deref(), Some("stop"));
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 1);
        assert_eq!(usage.total_tokens, 11);
    }

    /// Response with tool_calls (assistant invokes a function). Verifies
    /// the `tool_calls` shape, the `arguments` stringified-JSON quirk,
    /// and that `content` can be absent.
    #[test]
    fn deserialize_response_with_tool_calls() {
        let fixture = json!({
            "id": "chatcmpl-tc1",
            "object": "chat.completion",
            "created": 1716120000,
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_xyz",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Berlin\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });
        let resp: ChatCompletionsResponse = serde_json::from_value(fixture).expect("must parse");
        let msg = &resp.choices[0].message;
        assert!(
            msg.content.is_none(),
            "tool-call assistant has no text content"
        );
        let tc = msg.tool_calls.as_ref().expect("tool_calls present");
        assert_eq!(tc.len(), 1);
        assert_eq!(tc[0].id, "call_xyz");
        assert_eq!(tc[0].function.name, "get_weather");
        // The wire format keeps arguments as a stringified JSON blob.
        assert_eq!(tc[0].function.arguments, r#"{"city":"Berlin"}"#);
    }

    /// Round-trip: deserialize → re-serialize produces equivalent JSON
    /// modulo key ordering. Catches accidental field drops or rename
    /// drift in `#[serde(rename = "...")]`.
    #[test]
    fn round_trip_request_preserves_set_fields() {
        let original = json!({
            "model": "gpt-4o-mini",
            "messages": [{"role": "user", "content": "hi"}],
            "temperature": 0.5,
            "max_tokens": 100
        });
        let req: ChatCompletionsRequest = serde_json::from_value(original.clone()).expect("parse");
        let back = serde_json::to_value(&req).expect("serialize");
        // skip_serializing_if = "Option::is_none" should mean only the
        // four set fields appear in the output.
        let obj = back.as_object().expect("object");
        assert_eq!(obj.len(), 4, "got {obj:?}");
        assert_eq!(obj["model"], original["model"]);
        assert_eq!(obj["temperature"], original["temperature"]);
        assert_eq!(obj["max_tokens"], original["max_tokens"]);
    }

    /// Anthropic's compat mode sends `max_completion_tokens` instead of
    /// `max_tokens`. We must accept both. (Verified via Anthropic's
    /// OpenAI-compatibility shim docs.)
    #[test]
    fn accepts_max_completion_tokens_alias() {
        let fixture = json!({
            "model": "claude-3-5-sonnet-latest",
            "messages": [{"role": "user", "content": "hi"}],
            "max_completion_tokens": 512
        });
        let req: ChatCompletionsRequest = serde_json::from_value(fixture).expect("must parse");
        assert!(req.max_tokens.is_none());
        assert_eq!(req.max_completion_tokens, Some(512));
    }

    /// `system_fingerprint` and `prompt_tokens_details` are optional and
    /// must round-trip when set (newer responses carry them).
    #[test]
    fn deserialize_response_with_newer_fields() {
        let fixture = json!({
            "id": "chatcmpl-2",
            "object": "chat.completion",
            "created": 1716120000,
            "model": "gpt-4o-2024-08-06",
            "system_fingerprint": "fp_abc",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 1,
                "total_tokens": 6,
                "prompt_tokens_details": {"cached_tokens": 0}
            }
        });
        let resp: ChatCompletionsResponse = serde_json::from_value(fixture).expect("must parse");
        assert_eq!(resp.system_fingerprint.as_deref(), Some("fp_abc"));
        let usage = resp.usage.unwrap();
        assert_eq!(usage.prompt_tokens_details.unwrap()["cached_tokens"], 0);
    }

    /// Tool message (user-supplied tool result) — has `role: "tool"`,
    /// `tool_call_id`, and a content payload.
    #[test]
    fn deserialize_tool_role_message() {
        let fixture = json!({
            "role": "tool",
            "tool_call_id": "call_xyz",
            "content": "{\"temp_c\": 18}"
        });
        let msg: Message = serde_json::from_value(fixture).expect("must parse");
        assert_eq!(msg.role, Role::Tool);
        assert_eq!(msg.tool_call_id.as_deref(), Some("call_xyz"));
        match msg.content.unwrap() {
            MessageContent::Text(s) => assert!(s.contains("temp_c")),
            _ => panic!("expected text content"),
        }
    }

    /// Response with `n > 1` returns multiple choices. The default `n` is
    /// 1 but callers can request more.
    #[test]
    fn deserialize_response_with_multiple_choices() {
        let fixture = json!({
            "id": "x",
            "object": "chat.completion",
            "created": 0,
            "model": "gpt-4o-mini",
            "choices": [
                {"index": 0, "message": {"role": "assistant", "content": "a"}, "finish_reason": "stop"},
                {"index": 1, "message": {"role": "assistant", "content": "b"}, "finish_reason": "stop"},
                {"index": 2, "message": {"role": "assistant", "content": "c"}, "finish_reason": "length"}
            ]
        });
        let resp: ChatCompletionsResponse = serde_json::from_value(fixture).expect("must parse");
        assert_eq!(resp.choices.len(), 3);
        assert_eq!(resp.choices[2].finish_reason.as_deref(), Some("length"));
    }
}

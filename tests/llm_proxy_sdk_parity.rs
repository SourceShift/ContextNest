//! v0.3 LLM proxy — Phase 1 exit-gate SDK fixture-parity tests.
//!
//! Each fixture under `tests/fixtures/llm_proxy_sdk/` is a recorded request
//! or response body emitted by one of the three target SDKs:
//!
//! - `openai-python` 1.x — chat/completions + embeddings
//! - `@anthropic-ai/sdk` via the OpenAI-compatibility base URL
//! - `@google/generative-ai` via its OpenAI-shim client
//!
//! These tests prove that:
//!
//! 1. Each request fixture deserialises cleanly into the proxy's
//!    `ChatCompletionsRequest` / `EmbeddingsRequest` shape — no
//!    `deny_unknown_fields` violations.
//! 2. Each response fixture deserialises cleanly into the proxy's
//!    `ChatCompletionsResponse` / `EmbeddingsResponse` shape.
//! 3. The load-bearing fields each SDK depends on (model, role,
//!    content, finish_reason, tool_calls, usage) are extracted at the
//!    right types — not silently lost to a catch-all `Value`.
//! 4. Each shape round-trips: `T -> JSON -> T'` produces an equal
//!    value, so the proxy can re-encode requests to upstream providers
//!    without lossy translation.
//!
//! This is the Phase 1 exit gate per `docs/roadmap/v0.3-llm-proxy.md`:
//! "Recorded-fixture tests pass for chat + embeddings; `openai-python`,
//! `@anthropic-ai/sdk`, and Google's `generative-ai` Node SDK all work
//! against the proxy with only a `base_url` change."
//!
//! If a future SDK release breaks one of these tests, the fix is to
//! update both the fixture (so `git log` records the wire-format
//! change) AND the shape in `src/api/llm_proxy/openai_shapes.rs`
//! together. See `tests/fixtures/llm_proxy_sdk/README.md` for the
//! re-recording protocol.

use contextnest::api::llm_proxy::openai_shapes::{
    ChatCompletionsRequest, ChatCompletionsResponse, ContentPart, EmbeddingsInput,
    EmbeddingsRequest, EmbeddingsResponse, MessageContent, Role, StringOrVec, ToolKind,
};

/// Resolve a fixture path relative to the test file at compile time.
fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/llm_proxy_sdk/{}",
        env!("CARGO_MANIFEST_DIR"),
        name
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {path}: {e}"))
}

/// Deserialise a request fixture and assert it round-trips by value.
fn parse_chat_request(name: &str) -> ChatCompletionsRequest {
    let raw = fixture(name);
    let parsed: ChatCompletionsRequest = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("parse {name}: {e}\n--- body ---\n{raw}"));
    let reserialised = serde_json::to_string(&parsed).expect("reserialise");
    let parsed2: ChatCompletionsRequest = serde_json::from_str(&reserialised).expect("re-parse");
    assert_eq!(
        parsed, parsed2,
        "round-trip changed value for {name}: T -> JSON -> T' must be equal"
    );
    parsed
}

fn parse_embeddings_request(name: &str) -> EmbeddingsRequest {
    let raw = fixture(name);
    let parsed: EmbeddingsRequest = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("parse {name}: {e}\n--- body ---\n{raw}"));
    let reserialised = serde_json::to_string(&parsed).expect("reserialise");
    let parsed2: EmbeddingsRequest = serde_json::from_str(&reserialised).expect("re-parse");
    assert_eq!(parsed, parsed2, "round-trip changed value for {name}");
    parsed
}

// =============================================================================
// Chat completions — requests
// =============================================================================

#[test]
fn openai_python_simple_chat_request_deserialises() {
    let req = parse_chat_request("openai_python_chat_simple_request.json");
    assert_eq!(req.model, "gpt-4o-mini");
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, Role::System);
    assert_eq!(req.messages[1].role, Role::User);
    match req.messages[1].content.as_ref().expect("user content") {
        MessageContent::Text(s) => assert_eq!(s, "What is 2 + 2?"),
        MessageContent::Parts(_) => panic!("expected Text variant for plain string content"),
    }
    assert_eq!(req.temperature, Some(0.7));
}

#[test]
fn openai_python_multimodal_chat_request_deserialises() {
    let req = parse_chat_request("openai_python_chat_multimodal_request.json");
    assert_eq!(req.model, "gpt-4o");
    assert_eq!(req.messages.len(), 1);
    let parts = match req.messages[0]
        .content
        .as_ref()
        .expect("multimodal content")
    {
        MessageContent::Parts(p) => p,
        MessageContent::Text(_) => panic!("expected Parts variant for multimodal array"),
    };
    assert_eq!(parts.len(), 2, "expected text + image_url parts");
    match &parts[0] {
        ContentPart::Text { text } => assert_eq!(text, "What is in this image?"),
        _ => panic!("first part must deserialise as Text"),
    }
    match &parts[1] {
        ContentPart::ImageUrl { image_url } => {
            assert!(
                image_url.get("url").is_some(),
                "image_url payload preserved"
            );
        }
        _ => panic!("second part must deserialise as ImageUrl"),
    }
    assert_eq!(req.max_tokens, Some(300));
}

#[test]
fn openai_python_tool_use_request_deserialises() {
    let req = parse_chat_request("openai_python_chat_tool_use_request.json");
    let tools = req.tools.as_ref().expect("tools array present");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].kind, ToolKind::Function);
    assert_eq!(tools[0].function.name, "get_weather");
    assert_eq!(tools[0].function.strict, Some(true));
    assert!(
        tools[0].function.parameters.is_some(),
        "parameters JSON schema preserved as Value"
    );
    assert!(
        req.tool_choice.is_some(),
        "tool_choice forwarded as Value (polymorphic shape)"
    );
}

#[test]
fn anthropic_sdk_compat_request_deserialises() {
    let req = parse_chat_request("anthropic_sdk_compat_chat_request.json");
    assert_eq!(req.model, "claude-3-5-sonnet-latest");
    assert_eq!(req.messages.len(), 2);
    assert_eq!(req.messages[0].role, Role::System);
    assert_eq!(req.max_tokens, Some(512));
    assert_eq!(req.temperature, Some(1.0));
}

#[test]
fn google_genai_shim_request_deserialises() {
    let req = parse_chat_request("google_genai_openai_shim_request.json");
    assert_eq!(req.model, "gemini-1.5-flash");
    assert_eq!(req.n, Some(1));
    assert_eq!(req.stream, Some(false));
    assert_eq!(req.top_p, Some(0.95));
}

// =============================================================================
// Chat completions — responses
// =============================================================================

#[test]
fn openai_python_chat_response_deserialises() {
    let raw = fixture("openai_python_chat_response.json");
    let resp: ChatCompletionsResponse = serde_json::from_str(&raw).expect("parse response");
    assert_eq!(resp.object, "chat.completion");
    assert_eq!(resp.choices.len(), 1);
    let choice = &resp.choices[0];
    assert_eq!(choice.index, 0);
    assert_eq!(choice.finish_reason.as_deref(), Some("stop"));
    assert_eq!(choice.message.role, Role::Assistant);
    match choice.message.content.as_ref().expect("assistant content") {
        MessageContent::Text(s) => assert!(s.contains("4"), "answer string preserved"),
        _ => panic!("assistant content must deserialise as Text"),
    }
    let usage = resp.usage.as_ref().expect("usage present");
    assert_eq!(usage.total_tokens, 29);
    assert!(
        usage.prompt_tokens_details.is_some(),
        "newer breakdown fields tolerated as open Value (response is not deny_unknown_fields)"
    );
}

#[test]
fn openai_python_tool_use_response_deserialises() {
    let raw = fixture("openai_python_chat_tool_use_response.json");
    let resp: ChatCompletionsResponse = serde_json::from_str(&raw).expect("parse response");
    let choice = &resp.choices[0];
    assert_eq!(choice.finish_reason.as_deref(), Some("tool_calls"));
    assert!(
        choice.message.content.is_none(),
        "tool-call response carries no textual content"
    );
    let calls = choice
        .message
        .tool_calls
        .as_ref()
        .expect("tool_calls array");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].function.name, "get_weather");
    // OpenAI's wire format: arguments is stringified JSON, not parsed.
    assert!(
        calls[0].function.arguments.starts_with('{'),
        "arguments field stays as raw JSON string per OpenAI's wire spec"
    );
}

// =============================================================================
// Embeddings — requests + response
// =============================================================================

#[test]
fn openai_python_embeddings_string_request_deserialises() {
    let req = parse_embeddings_request("openai_python_embeddings_string_request.json");
    assert_eq!(req.model, "text-embedding-3-small");
    match &req.input {
        EmbeddingsInput::Text(s) => assert!(s.contains("fox")),
        _ => panic!(
            "untagged enum must pick Text variant for string input — \
             a wrong pick means the proxy would dispatch to the array path"
        ),
    }
}

#[test]
fn openai_python_embeddings_array_request_deserialises() {
    let req = parse_embeddings_request("openai_python_embeddings_array_request.json");
    match &req.input {
        EmbeddingsInput::Texts(v) => assert_eq!(v.len(), 3),
        _ => panic!("untagged enum must pick Texts variant for string array"),
    }
    assert_eq!(req.encoding_format.as_deref(), Some("float"));
    assert_eq!(req.user.as_deref(), Some("user-1234"));
}

#[test]
fn openai_python_embeddings_response_deserialises() {
    let raw = fixture("openai_python_embeddings_response.json");
    let resp: EmbeddingsResponse = serde_json::from_str(&raw).expect("parse response");
    assert_eq!(resp.object, "list");
    assert_eq!(resp.data.len(), 2);
    assert_eq!(resp.data[0].object, "embedding");
    assert_eq!(resp.data[0].index, 0);
    assert_eq!(resp.data[0].embedding.len(), 5);
    assert_eq!(resp.data[1].index, 1);
    assert_eq!(resp.usage.total_tokens, 8);
}

// =============================================================================
// Cross-cutting — load-bearing semantics
// =============================================================================

#[test]
fn untagged_string_or_vec_disambiguates_stop_field() {
    // OpenAI's `stop` field accepts both forms. Untagged enum must pick the
    // right variant from JSON shape alone.
    let one: StringOrVec = serde_json::from_str("\"END\"").expect("string stop");
    assert!(matches!(one, StringOrVec::String(_)));
    let many: StringOrVec =
        serde_json::from_str("[\"END\", \"STOP\", \"DONE\"]").expect("array stop");
    assert!(matches!(many, StringOrVec::Vec(_)));
}

#[test]
fn deny_unknown_fields_catches_typos_on_request() {
    // Defensive: if a client sends `temprature` (typo), the proxy must
    // 400 at parse-time, not silently drop the field and run a default-
    // temperature completion.
    let body = r#"{
        "model": "gpt-4o-mini",
        "messages": [{"role":"user","content":"hi"}],
        "temprature": 0.7
    }"#;
    let err = serde_json::from_str::<ChatCompletionsRequest>(body).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("temprature") || msg.contains("unknown field"),
        "deny_unknown_fields must surface the typo: got {msg}"
    );
}

#[test]
fn response_tolerates_unknown_fields_for_upstream_evolution() {
    // The proxy must NOT break when upstream providers add new response
    // fields (e.g. OpenAI adding `service_tier`, `prompt_logprobs`, etc).
    // Response shapes deliberately do NOT use deny_unknown_fields.
    let body = r#"{
        "id": "x",
        "object": "chat.completion",
        "created": 1,
        "model": "gpt-4o-mini",
        "choices": [],
        "service_tier": "default",
        "prompt_logprobs": null,
        "some_future_field_2027": "tolerated"
    }"#;
    let resp: ChatCompletionsResponse = serde_json::from_str(body)
        .expect("response must tolerate unknown fields for forward-compat");
    assert_eq!(resp.id, "x");
}

//! Integration tests for v0.3 Phase 3 slice 3.2 — PII redactor on the
//! LLM proxy cache insert path.
//!
//! Pins the end-to-end behaviour that the unit tests in
//! `src/services/llm_cache_redactor.rs` can't prove:
//!
//! 1. After `LlmCacheService::insert`, the in-memory entry's
//!    response choices contain `[REDACTED:EMAIL]` placeholders, not
//!    the original email addresses.
//! 2. Disabling the redactor (via `Redactor::disabled()`) preserves
//!    the original content verbatim — confirms the opt-out path
//!    works without surprises.
//! 3. The /api/v1/substrate/config endpoint surfaces the redactor's
//!    enabled state and rule count for operator inspection.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::api::llm_proxy::openai_shapes::{
    ChatCompletionsResponse, Choice, Message, MessageContent, Role,
};
use contextnest::services::llm_cache::{
    derive_cache_key, CacheKey, ExactKeyPrefix, LlmCacheService,
};
use contextnest::services::llm_cache_redactor::Redactor;
use contextnest::services::ContextNestServices;
use serde_json::Value;

fn make_response_with_email(text: &str) -> ChatCompletionsResponse {
    ChatCompletionsResponse {
        id: "chatcmpl-test-1".into(),
        object: "chat.completion".into(),
        created: 0,
        model: "gpt-test".into(),
        choices: vec![Choice {
            index: 0,
            message: Message {
                role: Role::Assistant,
                content: Some(MessageContent::Text(text.into())),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
            logprobs: None,
        }],
        usage: None,
        system_fingerprint: None,
    }
}

fn make_key(suffix: u8) -> CacheKey {
    CacheKey {
        exact: ExactKeyPrefix {
            project_id: "test".into(),
            model: "gpt-test".into(),
            temperature_bucket: 0,
            system_prompt_hash: [suffix; 8],
        },
        semantic_embedding: vec![0.1f32; 8],
    }
}

#[tokio::test]
async fn redactor_scrubs_email_from_response_on_insert() {
    let cache = LlmCacheService::new().with_redactor(Redactor::defaults_only());
    let response =
        make_response_with_email("Sure — I'll email john.doe@example.com about the bug.");
    let key = make_key(0xAA);

    cache.insert(key.clone(), response);

    // Look up the entry we just inserted and inspect the in-memory
    // representation.
    let hit = cache
        .lookup(&key, Some(std::time::Duration::from_secs(3600)))
        .expect("entry must be retrievable after insert");
    let content = hit
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref())
        .expect("response has assistant content");
    let text = match content {
        MessageContent::Text(s) => s,
        _ => panic!("expected text content"),
    };
    assert!(
        text.contains("[REDACTED:EMAIL]"),
        "redactor failed to replace email: {text}"
    );
    assert!(
        !text.contains("john.doe@example.com"),
        "raw email leaked into cache: {text}"
    );
}

#[tokio::test]
async fn disabled_redactor_preserves_response_verbatim() {
    let cache = LlmCacheService::new().with_redactor(Redactor::disabled());
    let original = "Sure — I'll email john.doe@example.com about the bug.";
    let response = make_response_with_email(original);
    let key = make_key(0xBB);

    cache.insert(key.clone(), response);

    let hit = cache
        .lookup(&key, Some(std::time::Duration::from_secs(3600)))
        .expect("entry must be retrievable after insert");
    let content = hit
        .choices
        .first()
        .and_then(|c| c.message.content.as_ref())
        .expect("response has assistant content");
    let text = match content {
        MessageContent::Text(s) => s,
        _ => panic!("expected text content"),
    };
    assert_eq!(
        text, original,
        "disabled redactor must be a pure passthrough"
    );
}

#[tokio::test]
async fn config_endpoint_surfaces_redactor_state() {
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init");
    let app = create_simple_app(services.clone())
        .await
        .expect("app should build");
    let server = TestServer::new(app).expect("test server should start");

    let res = server.get("/api/v1/substrate/config").await;
    res.assert_status_ok();
    let body: Value = res.json();

    // Wire contract: redactor_enabled + redactor_rule_count present
    // on llm_cache view.
    assert!(
        body["llm_cache"]["redactor_enabled"].is_boolean(),
        "redactor_enabled must be a bool"
    );
    assert!(
        body["llm_cache"]["redactor_rule_count"].is_number(),
        "redactor_rule_count must be a number"
    );

    // The default-services construction uses Redactor::from_env(),
    // which in turn defaults to enabled with 3 rules (EMAIL +
    // PHONE + CC). Assert that as the wire-contract baseline so a
    // future change to defaults forces a documented test update.
    let enabled = body["llm_cache"]["redactor_enabled"].as_bool().unwrap();
    let count = body["llm_cache"]["redactor_rule_count"].as_u64().unwrap();
    if enabled {
        assert_eq!(
            count, 3,
            "default redactor must compile 3 rules (EMAIL + PHONE + CC)"
        );
    } else {
        // Host env explicitly disabled it (CI runner with
        // CONTEXTNEST_LLM_CACHE_REDACTOR_ENABLED=false). Rule count
        // is 0 by contract.
        assert_eq!(count, 0);
    }
}

#[tokio::test]
async fn redactor_handles_multi_part_content() {
    use contextnest::api::llm_proxy::openai_shapes::ContentPart;
    let cache = LlmCacheService::new().with_redactor(Redactor::defaults_only());

    let parts = vec![
        ContentPart::Text {
            text: "First reach me at a@b.co".into(),
        },
        ContentPart::Text {
            text: "Or use +1 415-867-5309 anytime".into(),
        },
    ];
    let response = ChatCompletionsResponse {
        id: "chatcmpl-multipart".into(),
        object: "chat.completion".into(),
        created: 0,
        model: "gpt-test".into(),
        choices: vec![Choice {
            index: 0,
            message: Message {
                role: Role::Assistant,
                content: Some(MessageContent::Parts(parts)),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            finish_reason: Some("stop".into()),
            logprobs: None,
        }],
        usage: None,
        system_fingerprint: None,
    };

    let key = make_key(0xCC);
    cache.insert(key.clone(), response);
    let hit = cache
        .lookup(&key, Some(std::time::Duration::from_secs(3600)))
        .expect("entry retrievable");
    let parts = match hit.choices.first().and_then(|c| c.message.content.as_ref()) {
        Some(MessageContent::Parts(p)) => p.clone(),
        _ => panic!("expected multi-part content"),
    };
    let joined: String = parts
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        joined.contains("[REDACTED:EMAIL]"),
        "email part should be redacted: {joined}"
    );
    assert!(
        joined.contains("[REDACTED:PHONE]"),
        "phone part should be redacted: {joined}"
    );
    assert!(!joined.contains("a@b.co"), "email leaked: {joined}");
    assert!(!joined.contains("415-867-5309"), "phone leaked: {joined}");
}

// Confirm via the derive_cache_key path that the lookup we depend
// on in the other tests actually finds the entry (sanity check —
// not a redactor-specific assertion but eliminates a class of
// "redactor passed because lookup failed silently" false positives).
#[tokio::test]
async fn cache_lookup_finds_entry_after_insert() {
    let cache = LlmCacheService::new().with_redactor(Redactor::disabled());
    let response = make_response_with_email("plain response");
    let key = make_key(0xDD);
    cache.insert(key.clone(), response.clone());
    let _ = derive_cache_key; // referenced to silence unused-import diagnostics
    let hit = cache.lookup(&key, Some(std::time::Duration::from_secs(3600)));
    let _ = response; // moved into cache above; silence "unused after move" lint
    assert!(hit.is_some(), "lookup should hit immediately after insert");
}

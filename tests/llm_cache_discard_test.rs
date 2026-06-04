//! Integration tests for v0.3 Phase 3 slice 3.3 — hard-delete
//! propagation on the LLM proxy cache.
//!
//! Pins the four properties that make the DELETE endpoint useful:
//!
//! 1. Inserting then DELETE'ing a bucket removes it from in-memory
//!    lookups (subsequent same-key lookup returns None).
//! 2. The response carries the correct `removed_rows` count.
//! 3. Hitting DELETE for an unknown fingerprint is a no-op (returns
//!    `deleted: false`, `removed_rows: 0`, HTTP 200).
//! 4. Malformed fingerprint (not 64 hex chars) returns HTTP 400.
//!
//! Tombstone-replay semantics (the WAL-level "insert + discard,
//! restart, insert should NOT come back") are unit-tested in
//! `services::llm_cache::tests` because they need direct WAL record
//! injection and don't need an HTTP server.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::api::llm_proxy::openai_shapes::{
    ChatCompletionsResponse, Choice, Message, MessageContent, Role,
};
use contextnest::services::llm_cache::{CacheKey, ExactKeyPrefix};
use contextnest::services::ContextNestServices;
use serde_json::Value;

async fn make_setup() -> (ContextNestServices, TestServer) {
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init");
    let app = create_simple_app(services.clone())
        .await
        .expect("seven-tool app should build");
    let server = TestServer::new(app).expect("test server should start");
    (services, server)
}

fn fake_response(body: &str) -> ChatCompletionsResponse {
    ChatCompletionsResponse {
        id: "chatcmpl-discard-test".into(),
        object: "chat.completion".into(),
        created: 0,
        model: "gpt-test".into(),
        choices: vec![Choice {
            index: 0,
            message: Message {
                role: Role::Assistant,
                content: Some(MessageContent::Text(body.into())),
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

fn key_with_suffix(suffix: u8) -> CacheKey {
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

fn fingerprint_hex(key: &CacheKey) -> String {
    let fp = key.exact.fingerprint();
    let mut s = String::with_capacity(64);
    for b in fp.iter() {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[tokio::test]
async fn discard_removes_bucket_from_in_memory_cache() {
    let (services, server) = make_setup().await;
    let key = key_with_suffix(0xAA);

    // Insert directly through the wired-up cache so the HTTP layer
    // sees the same Inner map. Plain text — no PII triggers, so the
    // env-default redactor passes it through unmodified.
    services
        .llm_cache
        .insert(key.clone(), fake_response("hello world"));
    assert!(
        services
            .llm_cache
            .lookup(&key, Some(std::time::Duration::from_secs(3600)))
            .is_some(),
        "entry must be retrievable before discard"
    );

    let fp = fingerprint_hex(&key);
    let res = server
        .delete(&format!("/llm/v1/cache/entries/{fp}?reason=test-cleanup"))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["deleted"], true);
    assert_eq!(body["removed_rows"].as_u64().unwrap(), 1);
    assert_eq!(body["fingerprint"], fp);

    // The same Inner map the HTTP layer mutated must now miss.
    assert!(
        services
            .llm_cache
            .lookup(&key, Some(std::time::Duration::from_secs(3600)))
            .is_none(),
        "entry must be gone after DELETE"
    );
}

#[tokio::test]
async fn discard_unknown_fingerprint_is_noop() {
    let (_services, server) = make_setup().await;
    // 64 hex chars but not matching any bucket — valid input, no
    // matching bucket, returns 200 + deleted=false.
    let bogus = "0".repeat(64);
    let res = server
        .delete(&format!("/llm/v1/cache/entries/{bogus}"))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["deleted"], false);
    assert_eq!(body["removed_rows"], 0);
}

#[tokio::test]
async fn discard_malformed_fingerprint_returns_400() {
    let (_services, server) = make_setup().await;
    // Not 64 hex chars.
    let res = server
        .delete("/llm/v1/cache/entries/not-a-hex-fingerprint")
        .await;
    assert_eq!(res.status_code(), 400);
}

#[tokio::test]
async fn discard_returns_zero_when_fingerprint_truly_unknown() {
    // Distinct from the noop test above: this one verifies the
    // wire-shape (response carries the input fingerprint echoed
    // back even on no-op).
    let (_services, server) = make_setup().await;
    let echo = "abcdef0123456789".repeat(4); // 64 chars
    let res = server
        .delete(&format!("/llm/v1/cache/entries/{echo}"))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["fingerprint"], echo);
    assert_eq!(body["deleted"], false);
}

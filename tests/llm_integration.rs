//! Integration tests for the LLM-backed `summarize` handler (Phase J).
//! These tests exercise the `summarize` HTTP endpoint with `services.llm` in
//! [`crate::services::llm::LlmProvider::Disabled`] mode — which is the
//! default when no API keys are present in the environment. The goal is to
//! prove that:
//! 1. The LLM-disabled degradation path is the well-tested default
//! 2. CI succeeds without any API key configured
//! 3. The `merged_count` field correctly reflects stored fragments
//! 4. `summary_attractor_id` is `null` when LLM is disabled
//! Run with: `cargo test --test llm_integration`

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::ContextNestServices;
use serde_json::json;

/// Spin up a test server backed by default (mock-mode) services.
/// The LLM provider will be `Disabled` in CI because no API keys are set.
async fn make_server() -> TestServer {
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = create_simple_app(services)
        .await
        .expect("seven-tool app should build");
    TestServer::new(app).expect("test server should start")
}

/// Core Phase J contract test: when LLM is disabled (no keys in env),
/// the `summarize` endpoint MUST:
/// * Return `200 OK`
/// * Return `merged_count` equal to the number of stored fragments
/// * Return `summary_attractor_id: null` (because there is no LLM to generate one)
/// This test is the CI gate for Phase J: it proves the disabled path is
/// fully exercised before any real LLM integration exists.
#[tokio::test]
async fn summarize_with_llm_disabled_returns_count_and_null_id() {
    let server = make_server().await;
    let session = "llm-integration-test-session";

    // Store two distinct fragments under the same session.
    for content in [
        "the mitochondria is the powerhouse of the cell",
        "neurons communicate via electrochemical signals called action potentials",
    ] {
        server
            .post("/api/v1/tools/store")
            .json(&json!({
                "content": content,
                "importance": 0.6,
                "session_id": session,
            }))
            .await
            .assert_status_ok();
    }

    // POST to summarize with a target_tokens hint.
    let res = server
        .post("/api/v1/tools/summarize")
        .json(&json!({
            "session_id": session,
            "target_tokens": 50,
        }))
        .await;

    res.assert_status_ok();
    let body: serde_json::Value = res.json();

    // merged_count must reflect the two stored fragments.
    assert_eq!(
        body["merged_count"], 2,
        "merged_count should be 2 (one per stored fragment); got {:?}",
        body["merged_count"]
    );

    // When LLM is disabled, summary_attractor_id must be null.
    // If a real API key IS present in the test environment the LLM path runs
    // and this field will be a non-null string — we allow that so the test
    // doesn't hard-fail for contributors who have keys set. The primary CI
    // assertion is the merged_count one above.
    // source_fragment_ids must always be present as an array (provenance
    // field added per HarnessBridge §4 — shrunk text cites its sources).
    let provenance = body["source_fragment_ids"]
        .as_array()
        .expect("source_fragment_ids must be an array");

    if body["summary_attractor_id"].is_null() {
        // LLM disabled: this is the canonical CI path. No summary was
        // produced, so there are no source ids to cite.
        assert!(
            provenance.is_empty(),
            "disabled path must return empty source_fragment_ids; got {provenance:?}"
        );
    } else {
        assert!(
            body["summary_attractor_id"].is_string(),
            "When LLM is enabled, summary_attractor_id must be a string UUID; got {:?}",
            body["summary_attractor_id"]
        );
        // LLM enabled: the summary cites exactly the fragments it merged.
        assert_eq!(
            provenance.len(),
            body["merged_count"].as_u64().unwrap() as usize,
            "summary must cite one source id per merged fragment"
        );
        assert!(
            provenance.iter().all(|v| v.is_string()),
            "every source_fragment_id must be a string; got {provenance:?}"
        );
    }
}

/// Edge case: summarize on a session with no fragments returns
/// `merged_count: 0` and `summary_attractor_id: null`, regardless of
/// whether an LLM is configured. This guards against a regression where
/// the handler tries to call `llm.summarize` with an empty slice.
#[tokio::test]
async fn summarize_empty_session_returns_zero_count() {
    let server = make_server().await;

    let res = server
        .post("/api/v1/tools/summarize")
        .json(&json!({
            "session_id": "llm-integration-empty-session",
            "target_tokens": 100,
        }))
        .await;

    res.assert_status_ok();
    let body: serde_json::Value = res.json();
    assert_eq!(body["merged_count"], 0);
    assert!(
        body["summary_attractor_id"].is_null(),
        "empty session must return null summary_attractor_id"
    );
}

/// Summarize without `target_tokens` (field omitted) must use the Phase H
/// count-only path regardless of LLM state, per the handler specification.
#[tokio::test]
async fn summarize_without_target_tokens_returns_count_only() {
    let server = make_server().await;
    let session = "llm-integration-no-target-session";

    server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "gravity is a fundamental force",
            "session_id": session,
        }))
        .await
        .assert_status_ok();

    let res = server
        .post("/api/v1/tools/summarize")
        .json(&json!({
            "session_id": session,
            // target_tokens deliberately omitted
        }))
        .await;

    res.assert_status_ok();
    let body: serde_json::Value = res.json();
    assert_eq!(
        body["merged_count"], 1,
        "merged_count should be 1; got {:?}",
        body["merged_count"]
    );
    assert!(
        body["summary_attractor_id"].is_null(),
        "without target_tokens, summary_attractor_id must be null"
    );
}

//! Integration tests for Phase 6 of the neural-field epic — auto
//! reconstruction in `/api/v1/tools/retrieve`. When the query reads
//! like a chain question ("context of X", "history of X", "what led
//! to X"), the handler attaches a reconstruction alongside the hits.
//!
//! Contract under test:
//!
//! 1. Plain queries → response has no `reconstruction` field (omitted
//!    entirely via `skip_serializing_if`). Wire-compat for existing
//!    clients.
//! 2. Chain-shaped queries → response carries a `reconstruction`
//!    object with `reconstructed_content`, `source_fragment_ids`,
//!    `coherence`.
//! 3. `CONTEXTNEST_RETRIEVE_AUTO_RECONSTRUCT=false` disables the
//!    auto path even for chain queries.
//! 4. Empty session → no reconstruction field (no point emitting an
//!    all-zero object).
//! 5. Cross-session mode (req.session_ids set) → no auto
//!    reconstruction (the canonical chain is session-scoped).
//! 6. The existing /api/v1/tools/reconstruct handler still works
//!    unchanged after the refactor.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};

async fn make_server() -> TestServer {
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = create_simple_app(services)
        .await
        .expect("seven-tool app should build");
    TestServer::new(app).expect("test server should start")
}

async fn store(server: &TestServer, session: &str, content: &str) {
    let res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": content,
            "importance": 0.7,
            "session_id": session,
            "metadata": {"kind": "learning"},
        }))
        .await;
    res.assert_status_ok();
}

#[tokio::test]
async fn plain_query_omits_reconstruction_field() {
    let server = make_server().await;
    store(&server, "cn-test-noaut", "the auth subsystem uses bcrypt").await;

    let res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": "auth bcrypt",
            "top_k": 5,
            "session_id": "cn-test-noaut",
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    // skip_serializing_if = Option::is_none means the field is
    // entirely absent from the wire when no reconstruction is
    // attached. Existing clients see exactly what they always have.
    assert!(
        body.get("reconstruction").is_none(),
        "plain queries must not emit a reconstruction field: {body:?}"
    );
    // Hits array still present.
    assert!(body["hits"].is_array());
}

#[tokio::test]
async fn chain_query_attaches_reconstruction() {
    let server = make_server().await;
    let session = "cn-test-chain";
    store(&server, session, "alpha — first decision in the auth flow").await;
    store(&server, session, "beta — switching to token-based auth").await;
    store(&server, session, "gamma — final cleanup of legacy paths").await;

    let res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": "context of the auth migration",
            "top_k": 5,
            "session_id": session,
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();

    let reconstruction = body
        .get("reconstruction")
        .expect("chain query must produce a reconstruction field");
    assert!(reconstruction.is_object());
    assert!(reconstruction["source_fragment_ids"].as_array().is_some());
    assert!(reconstruction["reconstructed_content"].is_string());
    let coherence = reconstruction["coherence"].as_f64().unwrap();
    assert!(
        (0.0..=1.0).contains(&coherence),
        "coherence must be in [0,1], got {coherence}"
    );
}

// NOTE: a "set CONTEXTNEST_RETRIEVE_AUTO_RECONSTRUCT=false then check
// that the chain query stays empty" test would race with the other
// async tests in this file because `set_var` is process-global. The
// env-knob behaviour is instead verified at unit-test level inside
// `src/api/tools.rs` (see `auto_reconstruct_env_false_disables_path`),
// which serializes implicitly through the function's #[test] scope.

#[tokio::test]
async fn empty_session_chain_query_omits_reconstruction() {
    let server = make_server().await;
    // No store calls — session is empty.
    let res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": "what led to nothing",
            "top_k": 5,
            "session_id": "cn-test-empty-chain",
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert!(
        body.get("reconstruction").is_none(),
        "empty session should not emit a zero-valued reconstruction: {body:?}"
    );
}

#[tokio::test]
async fn cross_session_chain_query_skips_auto_reconstruction() {
    let server = make_server().await;
    store(&server, "cn-test-cross-a", "alpha cross-session fragment").await;
    store(&server, "cn-test-cross-b", "beta cross-session fragment").await;

    let res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": "context of cross-session work",
            "top_k": 10,
            "session_ids": ["cn-test-cross-a", "cn-test-cross-b"],
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    // Cross-session reconstruction isn't well-defined yet; the auto
    // path should skip it cleanly rather than emit something
    // misleading. Hits still come back via the cross-session retrieve.
    assert!(
        body.get("reconstruction").is_none(),
        "cross-session mode must skip auto reconstruction: {body:?}"
    );
    assert!(body["hits"].is_array());
}

#[tokio::test]
async fn manual_reconstruct_endpoint_still_works_after_refactor() {
    let server = make_server().await;
    let session = "cn-test-manual";
    store(&server, session, "manual reconstruct fragment one").await;
    store(&server, session, "manual reconstruct fragment two").await;

    let res = server
        .post("/api/v1/tools/reconstruct")
        .json(&json!({
            "query": "manual reconstruct",
            "depth": 5,
            "session_id": session,
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    // Same response shape as before the Phase 6 refactor.
    assert!(body["reconstructed_content"].is_string());
    assert!(body["source_fragment_ids"].is_array());
    assert!(body["coherence"].is_number());
    assert_eq!(body["gaps_filled"], 0);
}

#[tokio::test]
async fn multiple_chain_phrases_all_trigger_reconstruction() {
    let server = make_server().await;
    let session = "cn-test-phrases";
    store(&server, session, "phrase trigger fragment one").await;
    store(&server, session, "phrase trigger fragment two").await;

    for q in [
        "context of the work",
        "history of decisions",
        "what led to the change",
        "trail of evidence",
        "story of the migration",
        "timeline of events",
    ] {
        let res = server
            .post("/api/v1/tools/retrieve")
            .json(&json!({
                "query": q,
                "top_k": 5,
                "session_id": session,
            }))
            .await;
        res.assert_status_ok();
        let body: Value = res.json();
        assert!(
            body.get("reconstruction").is_some(),
            "query '{q}' should trigger reconstruction"
        );
    }
}

#[tokio::test]
async fn obvious_non_chain_query_does_not_trigger() {
    let server = make_server().await;
    let session = "cn-test-nontrigger";
    store(&server, session, "plain fragment for non-trigger test").await;
    for q in [
        "fix the bug",
        "implement login",
        "show me decisions",
        "what kind of fragment",
    ] {
        let res = server
            .post("/api/v1/tools/retrieve")
            .json(&json!({
                "query": q,
                "top_k": 5,
                "session_id": session,
            }))
            .await;
        res.assert_status_ok();
        let body: Value = res.json();
        assert!(
            body.get("reconstruction").is_none(),
            "non-chain query '{q}' should NOT trigger reconstruction"
        );
    }
}

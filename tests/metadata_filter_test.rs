//! Integration tests for `metadata_filter` on the `retrieve` HTTP handler.
//!
//! Exercises the round-trip contract added in this PR:
//!
//! 1. `store` accepts and persists arbitrary `metadata: HashMap<String, Value>`
//! 2. `retrieve` without `metadata_filter` returns all matching fragments
//!    (backward-compatible — pre-existing tests still pass)
//! 3. `retrieve` with `metadata_filter` only returns fragments whose stored
//!    metadata contains every (key, value) pair from the filter
//! 4. Fragments stored without metadata are filtered out by any non-empty
//!    filter (the conservative semantics: "no metadata at all" never
//!    matches a positive constraint)
//! 5. The `RetrieveHit` response carries the fragment's full metadata so
//!    consumers can see what they matched on without a second request

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

async fn store(server: &TestServer, session: &str, content: &str, metadata: Value) -> String {
    let res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": content,
            "importance": 0.7,
            "session_id": session,
            "metadata": metadata,
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["stored"], true);
    body["attractor_id"]
        .as_str()
        .expect("attractor_id present")
        .to_string()
}

async fn retrieve(server: &TestServer, session: &str, query: &str, filter: Option<Value>) -> Value {
    let mut body = json!({
        "query": query,
        "top_k": 50,
        "session_id": session,
    });
    if let Some(f) = filter {
        body["metadata_filter"] = f;
    }
    let res = server.post("/api/v1/tools/retrieve").json(&body).await;
    res.assert_status_ok();
    res.json()
}

#[tokio::test]
async fn retrieve_without_filter_returns_every_matching_fragment() {
    let server = make_server().await;
    let session = "test-no-filter";

    store(
        &server,
        session,
        "the user wants to fix the mermaid parse error",
        json!({"kind": "decision"}),
    )
    .await;
    store(
        &server,
        session,
        "another mermaid fix note",
        json!({"kind": "learning"}),
    )
    .await;

    let body = retrieve(&server, session, "mermaid", None).await;
    let hits = body["hits"].as_array().expect("hits array");
    // Both fragments come back when no filter is set — backward-compat.
    assert_eq!(hits.len(), 2, "no filter → both fragments returned");
}

#[tokio::test]
async fn retrieve_with_filter_only_returns_matching_fragments() {
    let server = make_server().await;
    let session = "test-filter-kind";

    store(
        &server,
        session,
        "deciding whether to use embeddings or token overlap",
        json!({"kind": "decision", "urgency": "now"}),
    )
    .await;
    store(
        &server,
        session,
        "learned that mermaid sequenceDiagram does not support br tags",
        json!({"kind": "learning"}),
    )
    .await;
    store(
        &server,
        session,
        "shipped the schema extension",
        json!({"kind": "accomplishment"}),
    )
    .await;

    // Filter by kind=decision → exactly one match
    let body = retrieve(
        &server,
        session,
        "what should we do next?",
        Some(json!({"kind": "decision"})),
    )
    .await;
    let hits = body["hits"].as_array().expect("hits array");
    assert_eq!(hits.len(), 1, "exactly one decision in this session");
    assert!(hits[0]["content"]
        .as_str()
        .unwrap()
        .contains("deciding whether"));
    // Response carries the matched metadata back
    assert_eq!(hits[0]["metadata"]["kind"], "decision");
    assert_eq!(hits[0]["metadata"]["urgency"], "now");
}

#[tokio::test]
async fn filter_requires_all_keys_to_match() {
    let server = make_server().await;
    let session = "test-multi-key-filter";

    // Two decisions, one urgent, one not
    store(
        &server,
        session,
        "urgent thing to decide",
        json!({"kind": "decision", "urgency": "now"}),
    )
    .await;
    store(
        &server,
        session,
        "later thing to decide",
        json!({"kind": "decision", "urgency": "later"}),
    )
    .await;

    // Filter on kind alone → both
    let body = retrieve(
        &server,
        session,
        "what should we decide?",
        Some(json!({"kind": "decision"})),
    )
    .await;
    assert_eq!(body["hits"].as_array().unwrap().len(), 2);

    // Filter on kind + urgency → only the urgent one
    let body = retrieve(
        &server,
        session,
        "what should we decide?",
        Some(json!({"kind": "decision", "urgency": "now"})),
    )
    .await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["metadata"]["urgency"], "now");
}

#[tokio::test]
async fn fragments_without_metadata_dont_match_a_positive_filter() {
    let server = make_server().await;
    let session = "test-missing-meta";

    // One with metadata, one without
    store(
        &server,
        session,
        "has metadata",
        json!({"kind": "learning"}),
    )
    .await;
    let bare = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "no metadata at all",
            "importance": 0.7,
            "session_id": session,
            // metadata omitted entirely
        }))
        .await;
    bare.assert_status_ok();

    // Without filter → both come back
    let body = retrieve(&server, session, "metadata", None).await;
    assert_eq!(body["hits"].as_array().unwrap().len(), 2);

    // With filter requiring kind=learning → only the one with metadata
    let body = retrieve(
        &server,
        session,
        "metadata",
        Some(json!({"kind": "learning"})),
    )
    .await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0]["content"]
        .as_str()
        .unwrap()
        .contains("has metadata"));
}

#[tokio::test]
async fn empty_filter_is_a_no_op_not_a_no_match() {
    let server = make_server().await;
    let session = "test-empty-filter";

    store(&server, session, "thing A", json!({"kind": "thing"})).await;
    store(&server, session, "thing B", json!({})).await;

    // `metadata_filter: {}` should match every fragment (vacuous filter)
    let body = retrieve(&server, session, "thing", Some(json!({}))).await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 2, "empty filter matches everything");
}

#[tokio::test]
async fn nonmatching_filter_returns_empty_hits_not_an_error() {
    let server = make_server().await;
    let session = "test-no-match";

    store(
        &server,
        session,
        "only fragment",
        json!({"kind": "learning"}),
    )
    .await;

    let body = retrieve(
        &server,
        session,
        "anything",
        Some(json!({"kind": "decision"})),
    )
    .await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 0, "no kind=decision in this session");
}

#[tokio::test]
async fn nested_value_filter_works_for_simple_types() {
    let server = make_server().await;
    let session = "test-nested-values";

    // Boolean values + numeric step are common in user_action metadata.
    store(
        &server,
        session,
        "awaiting decision text",
        json!({
            "kind": "decision",
            "awaiting_decision": true,
            "step": 1,
        }),
    )
    .await;
    store(
        &server,
        session,
        "settled decision text",
        json!({
            "kind": "decision",
            "awaiting_decision": false,
            "step": 1,
        }),
    )
    .await;

    // Filter by boolean → only awaiting one
    let body = retrieve(
        &server,
        session,
        "decision",
        Some(json!({"awaiting_decision": true})),
    )
    .await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0]["content"].as_str().unwrap().contains("awaiting"));
}

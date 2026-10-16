//! Regression tests for `retrieve`'s unknown-session signal, and for the
//! global-search owner tagging that shared its code path.
//!
//! Before this, naming a session that does not exist returned a 200 with an
//! empty `hits` array in ~1.4 ms — byte-identical to a real session with
//! nothing matching, and silent. A mock fixture id (`cc-7f3a2e91`, from
//! `web/src/lib/mock-data.ts`) reaching a real request URL blanked a whole
//! dashboard page while every other signal said the backend was healthy.
//!
//! The contract asserted here: the empty result stays a 200 with `hits: []`
//! (wire compatibility for cc_hooks / MCP / the dashboard), but it now carries
//! `session_known: false` when — and only when — the named sessions do not
//! exist at all. A known session that merely has nothing matching must still
//! omit the field entirely, so that clients predating it cannot mistake a real
//! empty answer for an error.
//!
//! Global search (neither `session_id` nor `session_ids`) is the same code path
//! and must never carry the flag: an empty result there is a fact about the
//! substrate, not about the question.

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
            "metadata": { "kind": "learning" },
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["stored"], true);
}

async fn retrieve(server: &TestServer, body: Value) -> Value {
    let res = server.post("/api/v1/tools/retrieve").json(&body).await;
    res.assert_status_ok();
    res.json()
}

/// The reported failure, verbatim in shape: a mock fixture id that was never a
/// session. It must stay a 200 (callers parse it) but stop lying about why the
/// result is empty.
#[tokio::test]
async fn unknown_session_id_is_flagged_not_silently_empty() {
    let server = make_server().await;
    let body = retrieve(
        &server,
        json!({
            "query": "back-pressure policy",
            "top_k": 10,
            "session_id": "cc-7f3a2e91",
        }),
    )
    .await;

    assert_eq!(
        body["hits"].as_array().expect("hits is an array").len(),
        0,
        "an unknown session still returns an empty hit set"
    );
    assert_eq!(
        body["session_known"],
        json!(false),
        "the response must point at the session id, not at the memories"
    );
}

/// The plural field takes the same path and gets the same signal.
#[tokio::test]
async fn unknown_ids_in_the_plural_field_are_flagged_too() {
    let server = make_server().await;
    let body = retrieve(
        &server,
        json!({
            "query": "back-pressure policy",
            "top_k": 10,
            "session_ids": ["cc-7f3a2e91", "cc-deadbeef"],
        }),
    )
    .await;

    assert_eq!(body["hits"].as_array().expect("hits is an array").len(), 0);
    assert_eq!(body["session_known"], json!(false));
}

/// A real session must not carry the field at all — absence is what tells a
/// caller "the session question never came up".
#[tokio::test]
async fn known_session_omits_the_flag() {
    let server = make_server().await;
    store(
        &server,
        "sess-known",
        "back-pressure policy is set by the queue",
    )
    .await;

    let body = retrieve(
        &server,
        json!({
            "query": "back-pressure policy",
            "top_k": 10,
            "session_id": "sess-known",
        }),
    )
    .await;

    assert!(
        !body["hits"]
            .as_array()
            .expect("hits is an array")
            .is_empty(),
        "the stored fragment must come back"
    );
    assert!(
        body.get("session_known").is_none(),
        "a known session must omit `session_known` entirely, got {:?}",
        body.get("session_known")
    );
}

/// Global search resolves each hit's owning session from the index's reverse
/// lookup rather than from a whole-substrate map. The tag must survive that:
/// cross-session callers and the session rollup both read it.
#[tokio::test]
async fn global_search_tags_hits_with_their_owning_session() {
    let server = make_server().await;
    store(&server, "sess-a", "back-pressure is enforced at the queue").await;
    store(
        &server,
        "sess-b",
        "the queue applies back-pressure under load",
    )
    .await;

    let body = retrieve(&server, json!({ "query": "back-pressure", "top_k": 10 })).await;

    let hits = body["hits"].as_array().expect("hits is an array");
    assert_eq!(
        hits.len(),
        2,
        "both sessions' fragments are in scope: {hits:?}"
    );
    let mut owners: Vec<&str> = hits
        .iter()
        .map(|h| {
            h.get("session_id")
                .and_then(|v| v.as_str())
                .expect("every global hit carries its owning session")
        })
        .collect();
    owners.sort_unstable();
    assert_eq!(owners, vec!["sess-a", "sess-b"]);

    assert!(
        body.get("session_known").is_none(),
        "global search never asks whether a session exists"
    );
}

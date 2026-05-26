//! Integration tests for `GET /api/v1/inbox`.
//!
//! Locks down the dashboard contract the endpoint replaces (the old
//! frontend fan-out in `web/src/hooks/useInbox.ts`): user_action always
//! surfaces, decision only when `awaiting_decision: true`, soft-deleted
//! fragments stay hidden, multi-session items come back in one response.

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
            "importance": 0.6,
            "session_id": session,
            "metadata": metadata,
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    body["attractor_id"]
        .as_str()
        .expect("attractor_id present")
        .to_string()
}

async fn inbox(server: &TestServer) -> Value {
    let res = server.get("/api/v1/inbox").await;
    res.assert_status_ok();
    res.json()
}

#[tokio::test]
async fn empty_substrate_returns_empty_items() {
    let server = make_server().await;
    let body = inbox(&server).await;
    assert!(body["items"].as_array().expect("items array").is_empty());
}

#[tokio::test]
async fn todo_fragments_surface_in_inbox() {
    // Regression: researcher autopilot ingest emits kind=todo for
    // user-actionable items (e.g. "User picks: ship / iterate / suspend").
    // The pre-broadening filter excluded these and left the inbox empty
    // even when the substrate held thousands of relevant todos.
    let server = make_server().await;
    store(
        &server,
        "sess-todo",
        "User picks: ship the feature or keep iterating",
        json!({"kind": "todo", "ts": "2026-05-20T17:00:00Z"}),
    )
    .await;

    let body = inbox(&server).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["metadata"]["kind"], "todo");
}

#[tokio::test]
async fn user_actions_surface_in_inbox() {
    let server = make_server().await;
    store(
        &server,
        "sess-aaaa",
        "run the migration before EOD",
        json!({"kind": "user_action", "urgency": "now", "ts": "2026-05-20T18:00:00Z"}),
    )
    .await;

    let body = inbox(&server).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["session_id"], "sess-aaaa");
    assert_eq!(items[0]["metadata"]["kind"], "user_action");
}

#[tokio::test]
async fn awaiting_decisions_surface_other_decisions_do_not() {
    let server = make_server().await;
    store(
        &server,
        "sess-bbbb",
        "should we adopt embeddings or token overlap?",
        json!({
            "kind": "decision",
            "awaiting_decision": true,
            "ts": "2026-05-20T18:05:00Z",
        }),
    )
    .await;
    store(
        &server,
        "sess-bbbb",
        "we resolved to use embeddings",
        json!({
            "kind": "decision",
            "awaiting_decision": false,
            "ts": "2026-05-19T10:00:00Z",
        }),
    )
    .await;

    let body = inbox(&server).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "only the awaiting decision is in the inbox");
    assert_eq!(items[0]["metadata"]["awaiting_decision"], true);
}

#[tokio::test]
async fn non_inbox_kinds_are_filtered_out() {
    let server = make_server().await;
    store(
        &server,
        "sess-cccc",
        "learned a new mermaid quirk",
        json!({"kind": "learning"}),
    )
    .await;
    store(
        &server,
        "sess-cccc",
        "shipped the schema extension",
        json!({"kind": "accomplishment"}),
    )
    .await;
    store(
        &server,
        "sess-cccc",
        "stop polling so aggressively",
        json!({"kind": "user_action", "urgency": "soon"}),
    )
    .await;

    let body = inbox(&server).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "only the user_action surfaces");
    assert_eq!(items[0]["metadata"]["kind"], "user_action");
}

#[tokio::test]
async fn cross_session_items_are_returned_in_one_response() {
    let server = make_server().await;
    store(
        &server,
        "sess1",
        "session 1 action",
        json!({"kind": "user_action", "ts": "2026-05-20T10:00:00Z"}),
    )
    .await;
    store(
        &server,
        "sess2",
        "session 2 action",
        json!({"kind": "user_action", "ts": "2026-05-20T11:00:00Z"}),
    )
    .await;
    store(
        &server,
        "sess3",
        "session 3 awaiting decision",
        json!({
            "kind": "decision",
            "awaiting_decision": true,
            "ts": "2026-05-20T12:00:00Z",
        }),
    )
    .await;

    let body = inbox(&server).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 3);

    let sessions: std::collections::HashSet<&str> = items
        .iter()
        .map(|i| i["session_id"].as_str().unwrap())
        .collect();
    assert!(sessions.contains("sess1"));
    assert!(sessions.contains("sess2"));
    assert!(sessions.contains("sess3"));
}

#[tokio::test]
async fn items_are_sorted_by_ts_descending() {
    let server = make_server().await;
    store(
        &server,
        "sess-sort",
        "oldest",
        json!({"kind": "user_action", "ts": "2026-05-18T10:00:00Z"}),
    )
    .await;
    store(
        &server,
        "sess-sort",
        "newest",
        json!({"kind": "user_action", "ts": "2026-05-20T10:00:00Z"}),
    )
    .await;
    store(
        &server,
        "sess-sort",
        "middle",
        json!({"kind": "user_action", "ts": "2026-05-19T10:00:00Z"}),
    )
    .await;

    let body = inbox(&server).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["content"], "newest");
    assert_eq!(items[1]["content"], "middle");
    assert_eq!(items[2]["content"], "oldest");
}

#[tokio::test]
async fn soft_deleted_fragments_do_not_appear() {
    let server = make_server().await;
    let keep_id = store(
        &server,
        "sess-soft",
        "still active",
        json!({"kind": "user_action", "ts": "2026-05-20T10:00:00Z"}),
    )
    .await;
    let drop_id = store(
        &server,
        "sess-soft",
        "about to be discarded",
        json!({"kind": "user_action", "ts": "2026-05-20T11:00:00Z"}),
    )
    .await;

    let res = server
        .post("/api/v1/tools/discard")
        .json(&json!({
            "attractor_id": drop_id,
            "soft_delete": true,
            "session_id": "sess-soft",
        }))
        .await;
    res.assert_status_ok();

    let body = inbox(&server).await;
    let items = body["items"].as_array().expect("items array");
    assert_eq!(items.len(), 1, "soft-deleted fragment must be hidden");
    assert_eq!(items[0]["id"], keep_id);
}

#[tokio::test]
async fn duplicate_emissions_collapse_to_newest_per_session_kind_content() {
    // Regression: long-running agents (researcher autopilot etc.) re-emit
    // the same requires_user_action / todo across many turns while
    // waiting for the user. Pre-fix, every emission produced a new
    // fragment AND a new inbox row — 72% of the user's inbox was
    // duplicate rows for the same actionable item.
    //
    // View-layer fix: group by (session_id, kind, content); keep the
    // newest by ts.
    let server = make_server().await;

    // Same content emitted across three turns (ts ascending). Should
    // collapse to ONE inbox row carrying the newest ts.
    for ts in [
        "2026-05-21T10:00:00Z",
        "2026-05-21T11:00:00Z",
        "2026-05-21T12:00:00Z",
    ] {
        store(
            &server,
            "sess-dup",
            "Pick A/B/C for reload strategy",
            json!({"kind": "user_action", "urgency": "now", "ts": ts}),
        )
        .await;
    }

    let body = inbox(&server).await;
    let items = body["items"].as_array().expect("items array");
    let matching: Vec<_> = items
        .iter()
        .filter(|i| i["content"] == "Pick A/B/C for reload strategy")
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "three identical emissions must collapse to one row"
    );
    assert_eq!(
        matching[0]["metadata"]["ts"], "2026-05-21T12:00:00Z",
        "the kept row must be the newest emission"
    );
}

#[tokio::test]
async fn dedup_keeps_distinct_content_in_same_session() {
    // Sibling to the dedup test: ensure we DON'T overcollapse — two
    // genuinely different items in the same session must both surface.
    let server = make_server().await;
    store(
        &server,
        "sess-distinct",
        "First user action",
        json!({"kind": "user_action", "ts": "2026-05-21T10:00:00Z"}),
    )
    .await;
    store(
        &server,
        "sess-distinct",
        "Second user action",
        json!({"kind": "user_action", "ts": "2026-05-21T11:00:00Z"}),
    )
    .await;

    let body = inbox(&server).await;
    let items = body["items"].as_array().expect("items array");
    let count_in_session = items
        .iter()
        .filter(|i| i["session_id"] == "sess-distinct")
        .count();
    assert_eq!(count_in_session, 2, "distinct content stays distinct");
}

#[tokio::test]
async fn dedup_preserves_cross_session_duplicates() {
    // Two sessions both blocked on "Run wrangler login" is a separate
    // signal from one session being blocked on it twice. Dedup must
    // be per-session, not global.
    let server = make_server().await;
    store(
        &server,
        "sessA",
        "Run wrangler login",
        json!({"kind": "user_action", "ts": "2026-05-21T10:00:00Z"}),
    )
    .await;
    store(
        &server,
        "sessB",
        "Run wrangler login",
        json!({"kind": "user_action", "ts": "2026-05-21T11:00:00Z"}),
    )
    .await;

    let body = inbox(&server).await;
    let items = body["items"].as_array().expect("items array");
    let matches: Vec<_> = items
        .iter()
        .filter(|i| i["content"] == "Run wrangler login")
        .collect();
    assert_eq!(
        matches.len(),
        2,
        "cross-session occurrences of the same content must NOT collapse"
    );
}

#[tokio::test]
async fn fragments_without_metadata_do_not_appear() {
    let server = make_server().await;
    // Store with empty metadata — no `kind` field at all.
    let res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "metadata-less fragment",
            "importance": 0.5,
            "session_id": "sess-nometa",
        }))
        .await;
    res.assert_status_ok();

    let body = inbox(&server).await;
    let items = body["items"].as_array().expect("items array");
    assert!(items.is_empty());
}

//! Integration tests for `GET /api/v1/stats`.
//!
//! Locks down the substrate-wide kind-count aggregator the dashboard's
//! Substrate page uses. Tests stick to the public HTTP surface so the
//! contract is validated end-to-end (no in-process shortcuts).

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};

async fn make_server() -> TestServer {
    let services = ContextNestServices::new_default().await.unwrap();
    let app = create_simple_app(services).await.unwrap();
    TestServer::new(app).unwrap()
}

async fn store(server: &TestServer, session: &str, content: &str, kind: &str) {
    let res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": content,
            "session_id": session,
            "metadata": {"kind": kind},
        }))
        .await;
    res.assert_status_ok();
}

#[tokio::test]
async fn empty_substrate_reports_zeros() {
    let server = make_server().await;
    let body: Value = server.get("/api/v1/stats").await.json();
    assert_eq!(body["total_fragments"], 0);
    assert_eq!(body["total_sessions"], 0);
    assert_eq!(body["by_kind"], json!({}));
}

#[tokio::test]
async fn kind_counts_are_aggregated_across_sessions() {
    let server = make_server().await;
    store(&server, "cc-a", "first", "learning").await;
    store(&server, "cc-a", "second", "learning").await;
    store(&server, "cc-a", "third", "user_action").await;
    store(&server, "cc-b", "fourth", "learning").await;
    store(&server, "cc-b", "fifth", "todo").await;

    let body: Value = server.get("/api/v1/stats").await.json();
    assert_eq!(body["total_fragments"], 5);
    assert_eq!(body["total_sessions"], 2);
    assert_eq!(body["by_kind"]["learning"], 3);
    assert_eq!(body["by_kind"]["user_action"], 1);
    assert_eq!(body["by_kind"]["todo"], 1);
}

#[tokio::test]
async fn fragments_without_kind_bucket_under_unknown() {
    let server = make_server().await;
    let res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "no-meta fragment",
            "session_id": "cc-x",
        }))
        .await;
    res.assert_status_ok();

    let body: Value = server.get("/api/v1/stats").await.json();
    assert_eq!(body["total_fragments"], 1);
    assert_eq!(body["by_kind"]["unknown"], 1);
}

#[tokio::test]
async fn soft_deleted_fragments_excluded_from_stats() {
    let server = make_server().await;
    let res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "to be discarded",
            "session_id": "cc-soft",
            "metadata": {"kind": "learning"},
        }))
        .await;
    let frag_id = res.json::<Value>()["attractor_id"]
        .as_str()
        .unwrap()
        .to_string();

    server
        .post("/api/v1/tools/discard")
        .json(&json!({
            "attractor_id": frag_id,
            "soft_delete": true,
            "session_id": "cc-soft",
        }))
        .await
        .assert_status_ok();

    let body: Value = server.get("/api/v1/stats").await.json();
    assert_eq!(body["total_fragments"], 0);
    assert_eq!(body["by_kind"]["learning"].as_u64().unwrap_or(0), 0);
}

#[tokio::test]
async fn sessions_endpoint_includes_by_kind_per_session() {
    let server = make_server().await;
    store(&server, "cc-counts", "a", "learning").await;
    store(&server, "cc-counts", "b", "learning").await;
    store(&server, "cc-counts", "c", "decision").await;

    let body: Value = server.get("/api/v1/sessions").await.json();
    let sessions = body["sessions"].as_array().unwrap();
    let row = sessions
        .iter()
        .find(|s| s["id"] == "cc-counts")
        .expect("session entry");
    assert_eq!(row["by_kind"]["learning"], 2);
    assert_eq!(row["by_kind"]["decision"], 1);
    assert_eq!(row["fragment_count"], 3);
}

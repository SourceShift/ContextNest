//! Integration tests for the graph-view routes:
//! `GET /api/v1/graph/neighbors` and `GET /api/v1/graph/path`.
//!
//! All four cases below run against a fresh `new_default()` substrate,
//! which has no nodes and no edges. That cold state is the point: an
//! empty graph must answer `200` with an empty neighbour list, never a
//! 5xx, and a path query against absent nodes must answer `404` rather
//! than collapsing into the "found: false" arm.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::ContextNestServices;
use serde_json::Value;

async fn make_setup() -> (ContextNestServices, TestServer) {
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = create_simple_app(services.clone())
        .await
        .expect("seven-tool app should build");
    let server = TestServer::new(app).expect("test server should start");
    (services, server)
}

#[tokio::test]
async fn neighbors_of_unknown_node_is_empty_not_error() {
    let (_services, server) = make_setup().await;

    let res = server
        .get("/api/v1/graph/neighbors?node_id=does-not-exist")
        .await;
    res.assert_status_ok();
    let body: Value = res.json();

    assert_eq!(body["node_id"], "does-not-exist");
    assert_eq!(
        body["neighbors"].as_array().map(Vec::len),
        Some(0),
        "an unknown node has no incident edges, not an error"
    );
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn path_between_unknown_nodes_is_not_found() {
    let (_services, server) = make_setup().await;

    let res = server
        .get("/api/v1/graph/path?from=missing&to=also-missing")
        .await;
    res.assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn zero_limit_is_rejected() {
    let (_services, server) = make_setup().await;

    let res = server
        .get("/api/v1/graph/neighbors?node_id=x&limit=0")
        .await;
    res.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn omitted_limit_uses_default_and_answers_well_formed() {
    let (_services, server) = make_setup().await;

    let res = server.get("/api/v1/graph/neighbors?node_id=x").await;
    res.assert_status_ok();
    let body: Value = res.json();

    assert!(body["node_id"].is_string());
    assert!(
        body["neighbors"].is_array(),
        "neighbors must always be an array, even when empty"
    );
    assert!(
        body["total"].is_number(),
        "total must always be a number so clients can paginate"
    );
}

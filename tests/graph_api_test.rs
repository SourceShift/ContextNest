//! Integration tests for the graph-view routes:
//! `GET /api/v1/graph/neighbors` and `GET /api/v1/graph/path`, plus the
//! Neo4j-projection routes (`upsert`, `credit-chain`, `siblings`,
//! `traverse`).
//!
//! The first four cases run against a fresh `new_default()` substrate,
//! which has no nodes and no edges. That cold state is the point: an
//! empty graph must answer `200` with an empty neighbour list, never a
//! 5xx, and a path query against absent nodes must answer `404` rather
//! than collapsing into the "found: false" arm.
//!
//! The projection cases run against the same default build, which has no
//! Neo4j driver linked and no database reachable. They pin the
//! degradation contract: a write answers `200 {skipped:true}` and a read
//! answers `404`, because the capability is simply not present.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};

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

// =============================================================================
// Neo4j projection routes on a default build
// =============================================================================
//
// No driver is linked and no database is running. The contract under test
// is that this changes *nothing* for the caller: a write is accepted and
// reported as skipped, reads report the capability as absent.

const UPSERT_PATH: &str = "/api/v1/graph/upsert";

fn one_trace_one_gradient_target() -> Value {
    json!({
        "nodes": [
            {"id": "trace:abc", "label": "Trace", "props": {"status": "failure"}}
        ],
        "edges": [
            {"from": "trace:abc", "from_label": "Trace", "type": "LINKED_TO",
             "to": "grad:def", "to_label": "GradientTarget", "props": {}}
        ],
        "source": "graph-api-test"
    })
}

#[tokio::test]
async fn upsert_without_a_backend_is_accepted_and_skipped() {
    let (_services, server) = make_setup().await;

    let res = server
        .post(UPSERT_PATH)
        .json(&one_trace_one_gradient_target())
        .await;
    res.assert_status_ok();
    let body: Value = res.json();

    assert_eq!(
        body["accepted"], 2,
        "accepted counts the submitted nodes + edges regardless of the backend"
    );
    assert_eq!(
        body["skipped"], true,
        "a projection that cannot happen must be reported, not failed"
    );
    assert_eq!(body["nodes_upserted"], 0);
    assert_eq!(body["edges_upserted"], 0);
}

#[tokio::test]
async fn upsert_with_unknown_node_label_is_rejected() {
    let (_services, server) = make_setup().await;

    let payload = json!({
        "nodes": [{"id": "widget:1", "label": "Widget", "props": {}}],
        "edges": [],
        "source": "graph-api-test"
    });
    let res = server.post(UPSERT_PATH).json(&payload).await;

    res.assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upsert_with_unknown_edge_type_is_rejected() {
    let (_services, server) = make_setup().await;

    let payload = json!({
        "nodes": [],
        "edges": [
            {"from": "a", "from_label": "Run", "type": "LINKED_TO",
             "to": "b", "to_label": "Trace", "props": {}}
        ],
        "source": "graph-api-test"
    });
    let res = server.post(UPSERT_PATH).json(&payload).await;

    assert_eq!(
        res.status_code(),
        axum::http::StatusCode::BAD_REQUEST,
        "LINKED_TO connects Trace -> GradientTarget, not Run -> Trace"
    );
}

#[tokio::test]
async fn upsert_over_the_item_cap_is_payload_too_large() {
    let (_services, server) = make_setup().await;

    let nodes: Vec<Value> = (0..2001)
        .map(|i| json!({"id": format!("run:{i}"), "label": "Run", "props": {}}))
        .collect();
    let payload = json!({"nodes": nodes, "edges": [], "source": "graph-api-test"});

    let res = server.post(UPSERT_PATH).json(&payload).await;
    res.assert_status(axum::http::StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn malformed_upsert_json_is_a_client_error() {
    let (_services, server) = make_setup().await;

    // axum distinguishes a syntax error (400) from a shape error (422);
    // pinning either exact code makes the gate brittle for no gain.
    let res = server
        .post(UPSERT_PATH)
        .content_type("application/json")
        .text("{ this is not json")
        .await;

    assert!(
        res.status_code().is_client_error(),
        "expected a 4xx for malformed JSON, got {}",
        res.status_code()
    );
}

#[tokio::test]
async fn projection_reads_are_not_found_without_a_backend() {
    let (_services, server) = make_setup().await;

    for path in [
        "/api/v1/graph/credit-chain?trace_id=trace:abc",
        "/api/v1/graph/siblings?target=grad:def",
        "/api/v1/graph/traverse?from=trace:abc&to=grad:def",
    ] {
        let res = server.get(path).await;
        assert_eq!(
            res.status_code(),
            axum::http::StatusCode::NOT_FOUND,
            "{path} must report the capability as absent, not fail"
        );
    }
}

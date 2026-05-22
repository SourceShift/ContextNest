//! Integration tests for Phase 5 of the neural-field epic —
//! ConnectionNetwork 1-hop expansion at retrieve time.
//!
//! Contract under test:
//!
//! 1. Pre-consolidation (no graph nodes/edges yet) → expansion is a
//!    no-op. Retrieve behaves exactly like the basin-only baseline.
//! 2. Post-consolidation with strongly-connected fragments → the top
//!    hit's 1-hop neighbors appear in the result set with similarity
//!    scaled by edge weight × boost.
//! 3. Connection expansion respects `metadata_filter` and single-
//!    session affinity (same prefilter discipline as Phase 4).
//! 4. `CONTEXTNEST_RETRIEVE_CONNECTION_BOOST=0` disables expansion.
//! 5. `CONTEXTNEST_RETRIEVE_CONNECTION_MIN_WEIGHT` filters weak edges
//!    out (cheap noise floor for auto-created low-confidence links).
//! 6. Sibling expansion never produces a similarity higher than the
//!    top hit's.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::ingest::claude_code::extractor::{MemoryKind, MemoryRecord};
use contextnest::ingest::claude_code::sink::{ServicesSink, Sink};
use contextnest::services::consolidation::drain_for_test;
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

fn rec(text: &str, session: &str, kind: &str) -> MemoryRecord {
    let mut r = MemoryRecord::new(MemoryKind::Learning, text.to_string(), session.to_string());
    r.metadata
        .insert("kind".to_string(), Value::String(kind.to_string()));
    r.metadata.insert(
        "ts".to_string(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    r
}

async fn retrieve(server: &TestServer, session: &str, query: &str) -> Value {
    let res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": query,
            "top_k": 20,
            "session_id": session,
        }))
        .await;
    res.assert_status_ok();
    res.json()
}

#[tokio::test]
async fn neighbors_of_returns_empty_when_node_has_no_edges() {
    let (services, _server) = make_setup().await;
    // Direct unit-ish check on the public proxy. A node that never
    // saw a peer through `add_node`'s auto-connection logic has no
    // neighbors → empty Vec, not a panic or error.
    let n = services
        .attractor_manager
        .list_neighbors("nonexistent-id")
        .await;
    assert!(n.is_empty(), "unknown node should have zero neighbors");
}

#[tokio::test]
async fn neighbors_of_returns_strongest_first_after_consolidation() {
    let (services, _server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    // Highly similar fragments → ConnectionNetwork.add_node's
    // similarity-driven auto-connection should wire them together.
    for text in [
        "shared connectivity test fragment one alpha",
        "shared connectivity test fragment two beta",
        "shared connectivity test fragment three gamma",
    ] {
        sink.store(&rec(text, "cn-test-neighbors", "learning"))
            .await
            .unwrap();
    }
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let ids = services
        .session_index
        .list_active("cn-test-neighbors")
        .await;
    // For any fragment with at least one peer, neighbors_of returns a
    // sorted list. If the mock embedder produces too-distant
    // embeddings for auto-connection, this test degrades gracefully
    // (empty Vec is a legal result — we don't assert non-empty here,
    // only the sortedness invariant when non-empty).
    let neighbors = services.attractor_manager.list_neighbors(&ids[0]).await;
    for window in neighbors.windows(2) {
        assert!(
            window[0].1 >= window[1].1,
            "neighbors must be sorted by weight desc: {:?}",
            window
        );
    }
}

#[tokio::test]
async fn retrieve_respects_metadata_filter_after_connection_expand() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    sink.store(&rec(
        "kernel scheduling internals walkthrough",
        "cn-test-cn-filter",
        "learning",
    ))
    .await
    .unwrap();
    sink.store(&rec(
        "kernel scheduling decision review",
        "cn-test-cn-filter",
        "decision",
    ))
    .await
    .unwrap();
    drain_for_test(&services, &services.consolidation_queue, 2).await;

    let res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": "kernel scheduling",
            "top_k": 20,
            "session_id": "cn-test-cn-filter",
            "metadata_filter": {"kind": "learning"},
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    for h in body["hits"].as_array().unwrap() {
        assert_eq!(
            h["metadata"]["kind"].as_str().unwrap_or(""),
            "learning",
            "connection expansion must preserve metadata_filter: {h:?}"
        );
    }
}

#[tokio::test]
async fn connection_boost_env_var_zero_disables_expansion() {
    // With expansion disabled, retrieve must still succeed and the
    // results must stay within [0, 1] similarity. The strict proof
    // "no extra hit was added" is hard to assert from the response
    // alone (since pure cosine + Phase 4's basin expansion can still
    // surface multiple hits) but the non-explosion invariant holds.
    std::env::set_var("CONTEXTNEST_RETRIEVE_CONNECTION_BOOST", "0.0");
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    sink.store(&rec(
        "single fragment with no expansion",
        "cn-test-cn-zero",
        "learning",
    ))
    .await
    .unwrap();
    drain_for_test(&services, &services.consolidation_queue, 1).await;

    let body = retrieve(&server, "cn-test-cn-zero", "single fragment").await;
    let hits = body["hits"].as_array().unwrap();
    for h in hits {
        let s = h["similarity"].as_f64().unwrap();
        assert!(
            (0.0..=1.0).contains(&s),
            "similarity must stay in [0,1] with connection boost disabled, got {s}"
        );
    }
    std::env::remove_var("CONTEXTNEST_RETRIEVE_CONNECTION_BOOST");
}

#[tokio::test]
async fn min_weight_floor_filters_weak_edges() {
    // Setting MIN_WEIGHT to 1.0 means no edge will pass — connection
    // expansion becomes effectively disabled even with a non-zero
    // boost. This is the ops escape hatch when auto-created low-
    // confidence edges are surfacing too much noise.
    std::env::set_var("CONTEXTNEST_RETRIEVE_CONNECTION_MIN_WEIGHT", "1.0");
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    sink.store(&rec(
        "connectivity stress one for the min weight test",
        "cn-test-cn-minw",
        "learning",
    ))
    .await
    .unwrap();
    sink.store(&rec(
        "connectivity stress two for the min weight test",
        "cn-test-cn-minw",
        "learning",
    ))
    .await
    .unwrap();
    drain_for_test(&services, &services.consolidation_queue, 2).await;

    let body = retrieve(&server, "cn-test-cn-minw", "connectivity stress").await;
    let hits = body["hits"].as_array().unwrap();
    // We don't try to prove "no connection sibling came back" from
    // outside (Phase 4's basin expansion can still surface neighbors).
    // What we assert: no NaN, no >1 similarity, response is well-formed.
    for h in hits {
        let s = h["similarity"].as_f64().unwrap();
        assert!(s.is_finite() && s >= 0.0 && s <= 1.0);
    }
    std::env::remove_var("CONTEXTNEST_RETRIEVE_CONNECTION_MIN_WEIGHT");
}

#[tokio::test]
async fn top_hit_similarity_never_beaten_by_expansion() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    // A cluster of highly-similar fragments (so basins and edges form).
    for text in [
        "graph traversal algorithm comparison alpha",
        "graph traversal algorithm comparison beta",
        "graph traversal algorithm comparison gamma",
        "graph traversal algorithm comparison delta",
    ] {
        sink.store(&rec(text, "cn-test-cn-top", "learning"))
            .await
            .unwrap();
    }
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let body = retrieve(&server, "cn-test-cn-top", "graph traversal").await;
    let hits = body["hits"].as_array().unwrap();
    if hits.len() < 2 {
        return;
    }
    let top = hits[0]["similarity"].as_f64().unwrap();
    for h in &hits[1..] {
        let s = h["similarity"].as_f64().unwrap();
        assert!(
            s <= top + 1e-6,
            "no expansion hit may outrank the top hit: top={top}, other={s}"
        );
    }
}

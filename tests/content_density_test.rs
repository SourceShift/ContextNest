//! Integration tests for Option A — per-fragment content_density
//! (computed at consolidation time, multiplied into retrieve score).
//!
//! Pins the end-to-end behaviour that the unit tests can't prove:
//!
//! 1. After consolidation, each fragment's metadata sidecar carries
//!    `_cn_content_density` as an f64 in `[0, 1]`.
//! 2. Different texts get *different* densities — the formula isn't
//!    short-circuiting to a constant.
//! 3. Retrieve scoring honors the density multiplier so a
//!    terminology-bearing fragment ranks BELOW a content-bearing
//!    fragment on a query that mentions the shared topic, even when
//!    base similarity is comparable.

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

fn rec(text: &str, session: &str) -> MemoryRecord {
    let mut r = MemoryRecord::new(MemoryKind::Learning, text.to_string(), session.to_string());
    r.metadata
        .insert("kind".to_string(), Value::String("learning".to_string()));
    r.metadata.insert(
        "ts".to_string(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    r
}

#[tokio::test]
async fn consolidation_writes_content_density_to_metadata() {
    let (services, _server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    sink.store(&rec(
        "arxiv research trending techniques digest",
        "cn-density-1",
    ))
    .await
    .unwrap();
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let ids = services.session_index.list_active("cn-density-1").await;
    assert_eq!(ids.len(), 1, "expected one fragment for session");
    let id = &ids[0];

    let meta = services.fragment_metadata.read().await;
    let entry = meta
        .get(id)
        .expect("metadata should exist after consolidation");
    let density = entry
        .get("_cn_content_density")
        .and_then(|v| v.as_f64())
        .expect("_cn_content_density must be populated after consolidation");
    assert!(
        (0.0..=1.0).contains(&density),
        "density {density} must land in [0, 1]"
    );
}

#[tokio::test]
async fn different_texts_get_different_densities() {
    let (services, _server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());

    // Terminology-bearing: bare id, near-zero wordishness.
    sink.store(&rec("arxiv:2603.16131", "cn-density-terminology"))
        .await
        .unwrap();
    // Content-bearing: actual prose about the same topic.
    sink.store(&rec(
        "Pulled last 200 May 2026 AI papers from PG and ranked by jina cross-encoder",
        "cn-density-prose",
    ))
    .await
    .unwrap();
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let term_id = services
        .session_index
        .list_active("cn-density-terminology")
        .await
        .into_iter()
        .next()
        .unwrap();
    let prose_id = services
        .session_index
        .list_active("cn-density-prose")
        .await
        .into_iter()
        .next()
        .unwrap();

    let meta = services.fragment_metadata.read().await;
    let term_density = meta
        .get(&term_id)
        .and_then(|m| m.get("_cn_content_density"))
        .and_then(|v| v.as_f64())
        .expect("terminology fragment has density");
    let prose_density = meta
        .get(&prose_id)
        .and_then(|m| m.get("_cn_content_density"))
        .and_then(|v| v.as_f64())
        .expect("prose fragment has density");

    // The discriminating property: prose at least 0.3 above terminology.
    // This is the gap that translates into a ~3× retrieve-score lift,
    // which is what makes the search useful.
    assert!(
        prose_density > term_density + 0.3,
        "expected discrimination: prose={prose_density:.3} term={term_density:.3}"
    );
}

#[tokio::test]
async fn retrieve_ranks_prose_above_terminology_on_shared_topic_query() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());

    // Both fragments mention arxiv — but only one is *about* it.
    // Without density, the bare-id fragment would often beat the
    // prose because the query "arxiv research papers" shares tokens
    // with both. With density, the prose should dominate.
    sink.store(&rec("arxiv:2603.16131", "cn-density-rank-test"))
        .await
        .unwrap();
    sink.store(&rec(
        "Pulled last 200 May 2026 arxiv papers and ranked them by relevance",
        "cn-density-rank-test",
    ))
    .await
    .unwrap();
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let body = json!({
        "query": "arxiv papers research",
        "session_id": "cn-density-rank-test",
        "top_k": 10
    });
    let res = server.post("/api/v1/tools/retrieve").json(&body).await;
    res.assert_status_ok();
    let body: Value = res.json();
    let hits = body["hits"].as_array().expect("hits is an array");
    assert!(
        hits.len() >= 2,
        "expected at least 2 hits, got {}",
        hits.len()
    );

    // The top hit's content should be the prose fragment, not the
    // bare-id fragment. Content matching keeps the assertion robust
    // against fragment-id reshuffling.
    let top = hits[0]["content"].as_str().unwrap_or("");
    assert!(
        top.contains("Pulled") || top.contains("ranked"),
        "top hit was the wrong fragment (density didn't demote terminology): {top:?}"
    );
}

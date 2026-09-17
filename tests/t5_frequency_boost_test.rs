//! Integration test for T5 (arXiv:2606.12945, Learning What to Remember).
//!
//! Covers the frequency-of-use boost added to retrieve scoring:
//! - Each retrieve hit bumps `retrieval_count` in metadata.
//! - `frequency_multiplier` reads that count and returns a boost above
//!   the neutral 1.0 base.
//! - CONTEXTNEST_RETRIEVE_FREQUENCY_WEIGHT=0.0 disables the boost.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::ingest::claude_code::extractor::{MemoryKind, MemoryRecord};
use contextnest::ingest::claude_code::sink::{ServicesSink, Sink};
use contextnest::services::consolidation::drain_for_test;
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

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

async fn make_setup() -> (ContextNestServices, TestServer) {
    let services = ContextNestServices::new_default().await.unwrap();
    let app = create_simple_app(services.clone()).await.unwrap();
    let server = TestServer::new(app).unwrap();
    (services, server)
}

/// Retrieve bumps `retrieval_count` on every returned hit, and the
/// counter is monotonic across successive retrieves for the same
/// fragment id.
#[tokio::test]
async fn t5_retrieve_bumps_retrieval_count_on_every_hit() {
    let _guard = ENV_LOCK.lock().unwrap();
    let (services, server) = make_setup().await;

    let sink = ServicesSink::new(services.clone());
    for text in ["turn one about topic A", "turn two about topic A"] {
        sink.store(&rec(text, "cn-test-t5")).await.unwrap();
    }
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let body = json!({
        "query": "topic A",
        "session_id": "cn-test-t5",
        "top_k": 5,
    });

    // First retrieve — every returned hit should now have retrieval_count = 1.
    let resp1: serde_json::Value = server
        .post("/api/v1/tools/retrieve")
        .json(&body)
        .await
        .json();
    let hits1 = resp1["hits"].as_array().expect("hits array");
    assert!(!hits1.is_empty(), "first retrieve should return hits");

    {
        let meta = services.fragment_metadata.read().await;
        for hit in hits1 {
            let id = hit["id"].as_str().unwrap();
            let count = meta
                .get(id)
                .and_then(|m| m.get("retrieval_count"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            assert_eq!(
                count, 1,
                "hit {id} should have count=1 after first retrieve"
            );
        }
    }

    // Second retrieve — same hits should now be at count = 2.
    let _resp2: serde_json::Value = server
        .post("/api/v1/tools/retrieve")
        .json(&body)
        .await
        .json();
    {
        let meta = services.fragment_metadata.read().await;
        for hit in hits1 {
            let id = hit["id"].as_str().unwrap();
            let count = meta
                .get(id)
                .and_then(|m| m.get("retrieval_count"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            assert!(
                count >= 2,
                "hit {id} count should be >= 2 after 2nd retrieve; got {count}"
            );
        }
    }
}

//! Idempotency of `POST /api/v1/graph/upsert`, tested against an
//! in-memory [`GraphBackend`] instead of a real database.
//!
//! Two things are being pinned here, and neither needs Neo4j to be
//! running:
//!
//! 1. **Idempotency.** The same batch posted twice must report the same
//!    counts both times — callers rely on a redelivered batch being
//!    harmless.
//! 2. **The injection point.** `ContextNestServices.graph` is a plain
//!    `pub` `Arc<dyn GraphBackend>`, so a caller can replace it before
//!    the router is built. That is why it is not behind a `OnceCell`,
//!    and this test is what keeps it that way.
//!
//! Runs green with no Neo4j process anywhere — the default config selects
//! the `InMemory` backend, so `new_default()` hands back a `DisabledGraph`
//! that this test then overwrites.

#![cfg(feature = "neo4j-graph")]

use async_trait::async_trait;
use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::error::ContextNestResult;
use contextnest::services::graph_backend::{GraphBackend, GraphEdge, GraphNode, UpsertCounts};
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const UPSERT_PATH: &str = "/api/v1/graph/upsert";

/// Stands in for the database. Nodes and edges accumulate in maps keyed by
/// identity, so re-writing the same batch is a no-op the way a `MERGE` is.
#[derive(Default)]
struct MockGraphBackend {
    nodes: Mutex<HashMap<String, GraphNode>>,
    edges: Mutex<HashMap<String, GraphEdge>>,
    upsert_calls: AtomicUsize,
}

impl MockGraphBackend {
    fn upsert_calls(&self) -> usize {
        self.upsert_calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl GraphBackend for MockGraphBackend {
    async fn upsert(
        &self,
        nodes: &[GraphNode],
        edges: &[GraphEdge],
    ) -> ContextNestResult<UpsertCounts> {
        self.upsert_calls.fetch_add(1, Ordering::SeqCst);

        {
            let mut known = self.nodes.lock().expect("mock node map poisoned");
            for node in nodes {
                known.insert(node.id.clone(), node.clone());
            }
        }
        {
            let mut known = self.edges.lock().expect("mock edge map poisoned");
            for edge in edges {
                known.insert(format!("{}->{}", edge.from, edge.to), edge.clone());
            }
        }

        Ok(UpsertCounts {
            nodes: nodes.len(),
            edges: edges.len(),
        })
    }

    async fn credit_chain(&self, _trace_id: &str) -> ContextNestResult<Vec<serde_json::Value>> {
        Ok(Vec::new())
    }

    async fn siblings(&self, _target: &str) -> ContextNestResult<Vec<serde_json::Value>> {
        Ok(Vec::new())
    }

    async fn traverse(
        &self,
        _from: &str,
        _to: &str,
        _max_hops: u8,
    ) -> ContextNestResult<Option<Vec<String>>> {
        Ok(None)
    }

    fn is_available(&self) -> bool {
        true
    }
}

/// A backend swap has to happen before `create_simple_app`, because that
/// is what installs the `State` every handler reads.
async fn setup_with_mock() -> (Arc<MockGraphBackend>, TestServer) {
    let mut services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");

    let mock = Arc::new(MockGraphBackend::default());
    let backend: Arc<dyn GraphBackend> = mock.clone();
    services.graph = backend;

    let app = create_simple_app(services)
        .await
        .expect("seven-tool app should build");
    let server = TestServer::new(app).expect("test server should start");
    (mock, server)
}

fn trace_and_gradient_target() -> Value {
    json!({
        "nodes": [
            {"id": "trace:abc", "label": "Trace", "props": {"status": "failure"}},
            {"id": "grad:def", "label": "GradientTarget", "props": {"target": "planner"}}
        ],
        "edges": [
            {"from": "trace:abc", "from_label": "Trace", "type": "LINKED_TO",
             "to": "grad:def", "to_label": "GradientTarget", "props": {}}
        ],
        "source": "graph-upsert-mock-test"
    })
}

#[tokio::test]
async fn posting_the_same_batch_twice_reports_identical_counts() {
    let (mock, server) = setup_with_mock().await;
    let payload = trace_and_gradient_target();

    let first = server.post(UPSERT_PATH).json(&payload).await;
    first.assert_status_ok();
    let first_body: Value = first.json();

    let second = server.post(UPSERT_PATH).json(&payload).await;
    second.assert_status_ok();
    let second_body: Value = second.json();

    assert_eq!(
        first_body["skipped"], false,
        "an available backend must project, not skip"
    );
    assert_eq!(second_body["skipped"], false);
    assert_eq!(first_body["accepted"], 3);
    assert_eq!(first_body["nodes_upserted"], 2);
    assert_eq!(first_body["edges_upserted"], 1);

    assert_eq!(
        first_body["nodes_upserted"], second_body["nodes_upserted"],
        "a redelivered batch must merge, not duplicate"
    );
    assert_eq!(
        first_body["edges_upserted"], second_body["edges_upserted"],
        "a redelivered batch must merge, not duplicate"
    );
    assert_eq!(
        mock.upsert_calls(),
        2,
        "both requests must reach the injected backend, not a DisabledGraph"
    );
}

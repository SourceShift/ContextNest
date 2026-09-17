//! Integration tests for the Sprint 1 arxiv-2026-driven improvements
//! landed as one bundle (see `docs/roadmap/epics/2026-arxiv-improvements.md`).
//!
//! Covers:
//! - **T1 (RecMem recurrence gate, arXiv:2605.16045)** — fragments with
//!   no session peers over the cosine floor get deferred to the
//!   subconscious store instead of consuming basin + graph work.
//! - **T3 (PROJECTMEM replay endpoint, arXiv:2606.12329)** — the
//!   `GET /api/v1/substrate/replay` diagnostic returns WAL contents
//!   accurately and, on `dry_run=false`, re-enqueues every stored
//!   fragment id into the consolidation queue.
//! - **T4 (Chain-of-Memory reconstruction pruning, arXiv:2601.14287)**
//!   — reconstruct drops fragments whose cosine to the query falls
//!   below `CONTEXTNEST_RECONSTRUCT_COSINE_FLOOR` before top-K
//!   truncation.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::ingest::claude_code::extractor::{MemoryKind, MemoryRecord};
use contextnest::ingest::claude_code::sink::{ServicesSink, Sink};
use contextnest::services::consolidation::drain_for_test;
use contextnest::services::wal::{Wal, WalRecord};
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use tempfile::tempdir;

/// Serializes tests that mutate `CONTEXTNEST_*` env vars — cargo test
/// runs tests in the same binary in parallel by default, and each test
/// here reads process-global env at consolidate/reconstruct time. Without
/// this guard, a `set_var` from one test races the `remove_var` in
/// another. Same pattern as `tests/basin_attach_threshold_test.rs`.
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
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = create_simple_app(services.clone())
        .await
        .expect("seven-tool app should build");
    let server = TestServer::new(app).expect("test server should start");
    (services, server)
}

/// T1 — with the gate enabled and no session peers, the first
/// fragment cannot pass the recurrence-density check and is deferred
/// to the subconscious store. Basin/graph work is skipped.
#[tokio::test]
async fn t1_recmem_gate_defers_solo_fragment_when_enabled() {
    let _guard = ENV_LOCK.lock().unwrap();
    // Enable the gate for this test only. The default (unset / 0) is
    // OFF, so existing consolidation behavior is unchanged.
    std::env::set_var("CONTEXTNEST_CONSOLIDATION_RECURRENCE_MIN_COUNT", "1");
    // Ensure the peer-similarity floor is high enough that a single
    // unrelated fragment cannot accidentally match itself as its own
    // peer — the gate excludes the fragment being consolidated but
    // this pins the floor deterministically.
    std::env::set_var("CONTEXTNEST_CONNECTION_SIMILARITY_THRESHOLD", "0.7");

    let (services, _server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    sink.store(&rec("recmem test solo fragment", "cn-test-t1-solo"))
        .await
        .expect("sink store");

    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let metrics = services.consolidation_queue.snapshot_metrics();
    assert!(
        metrics.deferred_subconscious >= 1,
        "solo fragment with gate on should be deferred; got metrics={metrics:?}"
    );

    // The fragment stays retrievable via sidecar but has no canonical
    // attractor state — matches the paper's "subconscious" semantics.
    let ids = services.session_index.list_active("cn-test-t1-solo").await;
    assert_eq!(ids.len(), 1);
    let canonical = services
        .attractor_manager
        .get_fragment(&ids[0])
        .await
        .expect("get_fragment shouldn't error");
    assert!(
        canonical.is_none(),
        "deferred fragment must not have canonical state"
    );

    // The `_cn_recurrence_deferred` marker is written so a future
    // reconsideration pass can find it.
    let meta = services.fragment_metadata.read().await;
    let marker = meta
        .get(&ids[0])
        .and_then(|m| m.get("_cn_recurrence_deferred"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(marker, "deferred fragment should carry the marker");

    // Clean up env for other tests.
    std::env::remove_var("CONTEXTNEST_CONSOLIDATION_RECURRENCE_MIN_COUNT");
    std::env::remove_var("CONTEXTNEST_CONNECTION_SIMILARITY_THRESHOLD");
}

/// T1 (default-off) — with the gate DISABLED (default), a solo
/// fragment consolidates normally. Regression test: this is the
/// pre-Sprint-1 behavior and must stay unchanged for callers who do
/// not opt in.
#[tokio::test]
async fn t1_recmem_gate_disabled_default_consolidates_solo_fragment() {
    let _guard = ENV_LOCK.lock().unwrap();
    // Explicitly ensure the gate is off (in case the runner leaks env
    // from another test).
    std::env::remove_var("CONTEXTNEST_CONSOLIDATION_RECURRENCE_MIN_COUNT");

    let (services, _server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    sink.store(&rec("solo but gate disabled", "cn-test-t1-off"))
        .await
        .expect("sink store");

    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let metrics = services.consolidation_queue.snapshot_metrics();
    assert_eq!(
        metrics.deferred_subconscious, 0,
        "gate is off; nothing should be deferred"
    );

    let ids = services.session_index.list_active("cn-test-t1-off").await;
    let canonical = services
        .attractor_manager
        .get_fragment(&ids[0])
        .await
        .expect("get_fragment shouldn't error");
    assert!(
        canonical.is_some(),
        "gate-off fragment must consolidate normally"
    );
}

/// T3 — the diagnostic replay endpoint reports WAL contents and can
/// re-enqueue every stored fragment id.
#[tokio::test]
async fn t3_substrate_replay_reports_wal_and_reenqueues() {
    let (services, server) = make_setup().await;

    // Wire a temp WAL with two Store records so read_records has data.
    let dir = tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");
    let writer = Wal::open_for_append(wal_path.clone()).unwrap();
    for (id, session) in [("frag-alpha", "cn-test-t3"), ("frag-beta", "cn-test-t3")] {
        writer
            .append(&WalRecord::Store {
                fragment_id: id.into(),
                session_id: session.into(),
                content: format!("content for {id}"),
                importance: 0.5,
                metadata: HashMap::new(),
            })
            .unwrap();
    }
    drop(writer);
    let reader = Wal::open_for_append(wal_path.clone()).unwrap();
    services
        .wal
        .set(reader)
        .expect("wal cell should be settable in test");

    // Dry-run first — no queue mutation.
    let baseline_pending = services.consolidation_queue.pending_count();
    let dry: serde_json::Value = server.get("/api/v1/substrate/replay").await.json();
    assert_eq!(dry["store_records"], 2);
    assert_eq!(dry["unique_fragment_ids"], 2);
    assert_eq!(dry["enqueued"], 0);
    assert_eq!(dry["dry_run"], true);
    assert_eq!(
        services.consolidation_queue.pending_count(),
        baseline_pending,
        "dry-run must not enqueue"
    );

    // Live replay — both ids should reach the queue.
    let live: serde_json::Value = server
        .get("/api/v1/substrate/replay?dry_run=false")
        .await
        .json();
    assert_eq!(live["enqueued"], 2);
    assert_eq!(live["dry_run"], false);
    assert!(
        services.consolidation_queue.pending_count() >= baseline_pending + 2,
        "live replay must have enqueued both ids"
    );
}

/// T3 — service without an initialized WAL returns 503, not 500.
#[tokio::test]
async fn t3_substrate_replay_without_wal_returns_service_unavailable() {
    let (_services, server) = make_setup().await;
    // `make_setup` doesn't init the WAL cell (mock mode), so this
    // exercises the "wal not initialized" branch.
    let response = server.get("/api/v1/substrate/replay").await;
    response.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

/// T4 — reconstruct drops fragments whose cosine to the query is
/// below the floor. Uses the mock embedder which is deterministic on
/// content strings so we can assert relative ranking.
#[tokio::test]
async fn t4_reconstruct_prunes_below_cosine_floor() {
    let _guard = ENV_LOCK.lock().unwrap();
    // A very high floor guarantees nothing survives — the endpoint
    // must return an empty reconstruction rather than the historical
    // top-K "always return something" behavior.
    std::env::set_var("CONTEXTNEST_RECONSTRUCT_COSINE_FLOOR", "0.99");

    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    for text in ["turn one", "turn two", "turn three"] {
        sink.store(&rec(text, "cn-test-t4")).await.unwrap();
    }
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let body = json!({
        "query": "completely unrelated topic",
        "session_id": "cn-test-t4",
        "depth": 5,
    });
    let resp: serde_json::Value = server
        .post("/api/v1/tools/reconstruct")
        .json(&body)
        .await
        .json();

    // With the floor at 0.99, mock cosine similarity between "completely
    // unrelated topic" and "turn N" is < 0.99, so all fragments get
    // pruned. The response shape is preserved (compat with existing
    // clients) but source ids are empty.
    let sources = resp["source_fragment_ids"].as_array().unwrap();
    assert!(
        sources.is_empty(),
        "high floor should prune everything; got {sources:?}"
    );

    std::env::remove_var("CONTEXTNEST_RECONSTRUCT_COSINE_FLOOR");
}

/// T1 reconsideration — when a fragment consolidates Done, deferred
/// siblings in the same session get their marker cleared and are
/// re-enqueued. Simulates the sequence: a solo fragment gets
/// deferred (no peers), then a new fragment arrives + consolidates
/// normally (gate lowered), which should wake the deferred sibling.
#[tokio::test]
async fn t1_reconsideration_wakes_deferred_siblings_when_peer_arrives() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("CONTEXTNEST_CONSOLIDATION_RECURRENCE_MIN_COUNT", "1");
    // Threshold=0.0 makes anything a valid peer, so the second
    // fragment to arrive consolidates Done and triggers reconsideration.
    std::env::set_var("CONTEXTNEST_CONNECTION_SIMILARITY_THRESHOLD", "0.0");

    let (services, _server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());

    // First fragment: alone in its session, gate defers it.
    sink.store(&rec("first isolated turn", "cn-test-t1-recon"))
        .await
        .expect("sink store");
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let metrics_after_first = services.consolidation_queue.snapshot_metrics();
    assert!(
        metrics_after_first.deferred_subconscious >= 1,
        "first fragment should be deferred; got {metrics_after_first:?}"
    );
    let ids_after_first = services.session_index.list_active("cn-test-t1-recon").await;
    assert_eq!(ids_after_first.len(), 1);
    let deferred_id = ids_after_first[0].clone();

    {
        let meta = services.fragment_metadata.read().await;
        let is_deferred = meta
            .get(&deferred_id)
            .and_then(|m| m.get("_cn_recurrence_deferred"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        assert!(is_deferred, "first fragment must carry the deferred marker");
    }

    // Second fragment: with threshold=0.0 any peer counts, so this
    // one passes the gate on first try, consolidates Done, and
    // triggers reconsideration for the deferred sibling.
    sink.store(&rec("second turn brings a peer", "cn-test-t1-recon"))
        .await
        .expect("sink store");
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    // After the second drain: the reconsideration path enqueues the
    // deferred sibling; that enqueue must have happened before the
    // drain finished. Verify the marker is cleared.
    let meta = services.fragment_metadata.read().await;
    let marker_still_present = meta
        .get(&deferred_id)
        .and_then(|m| m.get("_cn_recurrence_deferred"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(
        !marker_still_present,
        "reconsideration should have cleared the deferred marker"
    );

    std::env::remove_var("CONTEXTNEST_CONSOLIDATION_RECURRENCE_MIN_COUNT");
    std::env::remove_var("CONTEXTNEST_CONNECTION_SIMILARITY_THRESHOLD");
}

/// T4 — with the floor at 0 the pre-Sprint-1 behavior is preserved
/// (top-K is filled up to `depth` regardless of similarity).
#[tokio::test]
async fn t4_reconstruct_zero_floor_matches_pre_sprint1_behavior() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("CONTEXTNEST_RECONSTRUCT_COSINE_FLOOR", "0.0");

    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    for text in ["alpha turn", "beta turn"] {
        sink.store(&rec(text, "cn-test-t4-zero")).await.unwrap();
    }
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let body = json!({
        "query": "any query",
        "session_id": "cn-test-t4-zero",
        "depth": 5,
    });
    let resp: serde_json::Value = server
        .post("/api/v1/tools/reconstruct")
        .json(&body)
        .await
        .json();

    let sources = resp["source_fragment_ids"].as_array().unwrap();
    assert_eq!(sources.len(), 2, "zero floor should return both fragments");

    std::env::remove_var("CONTEXTNEST_RECONSTRUCT_COSINE_FLOOR");
}

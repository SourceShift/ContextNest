//! Integration tests for Phase 1 of the neural-field epic — the
//! background consolidation worker (`src/services/consolidation.rs`).
//!
//! Covers the contract that distinguishes "ingest still fast" from
//! "attractor pipeline eventually runs":
//!
//! 1. A sidecar-only fragment (the cc_hooks live-ingest output) is
//!    enqueued the moment ServicesSink writes it, but the ingest call
//!    itself does NOT block on attractor work.
//! 2. Running `drain_for_test` to completion lands every queued
//!    fragment in the canonical attractor store
//!    (`MemoryAttractorManager::get_fragment` returns Some), and flips
//!    `_cn_consolidated: true` in the metadata sidecar.
//! 3. A second drain pass is a no-op (idempotent — already-flagged
//!    fragments are skipped without re-running `process_memories`).
//! 4. The `/api/v1/substrate/consolidation` HTTP endpoint reports the
//!    same numbers the worker observed.
//! 5. The initial-scan code path picks up pre-existing unflagged
//!    fragments (the WAL-replay scenario), not just freshly-enqueued
//!    ids — important because a server restart loses the in-memory
//!    queue, and only the scan re-seeds it.

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

async fn fragment_ids_for_session(services: &ContextNestServices, session: &str) -> Vec<String> {
    services.session_index.list_active(session).await
}

#[tokio::test]
async fn services_sink_enqueues_fragment_for_background_consolidation() {
    let (services, _server) = make_setup().await;

    let sink = ServicesSink::new(services.clone());
    sink.store(&rec(
        "borrow checker prevents data races",
        "cn-test-enqueue",
    ))
    .await
    .expect("sink store should succeed");

    // The sink completed without running the attractor pipeline (Phase
    // 1's whole point: ingest stays fast). The fragment is in the
    // queue waiting for the worker.
    assert_eq!(
        services.consolidation_queue.pending_count(),
        1,
        "fragment id should be queued after sink ingest"
    );

    // And no canonical attractor state yet — the fast path skips it.
    let frag_ids = fragment_ids_for_session(&services, "cn-test-enqueue").await;
    assert_eq!(frag_ids.len(), 1, "session index should have one fragment");
    let canonical = services
        .attractor_manager
        .get_fragment(&frag_ids[0])
        .await
        .expect("get_fragment shouldn't error");
    assert!(
        canonical.is_none(),
        "canonical attractor state should be empty before worker runs"
    );
}

#[tokio::test]
async fn worker_consolidates_queued_fragments_into_canonical_store() {
    let (services, _server) = make_setup().await;

    let sink = ServicesSink::new(services.clone());
    for text in [
        "tokio uses cooperative scheduling",
        "rust async cancellation is structured",
        "the borrow checker prevents data races at compile time",
    ] {
        sink.store(&rec(text, "cn-test-worker")).await.unwrap();
    }
    assert_eq!(services.consolidation_queue.pending_count(), 3);

    // Run the worker to completion synchronously (test-only helper —
    // production spawns `run_worker` in a tokio task).
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    assert_eq!(
        services.consolidation_queue.pending_count(),
        0,
        "queue should be empty after drain"
    );

    // Every fragment now resolves in the canonical store.
    let ids = fragment_ids_for_session(&services, "cn-test-worker").await;
    assert_eq!(ids.len(), 3);
    for id in &ids {
        let canonical = services
            .attractor_manager
            .get_fragment(id)
            .await
            .expect("get_fragment shouldn't error")
            .unwrap_or_else(|| panic!("fragment {id} should be canonical after consolidation"));
        assert_eq!(canonical.id, *id);
        assert!(
            !canonical.content.is_empty(),
            "canonical fragment must carry its embedding"
        );
    }

    // Metadata sidecar carries the consolidation flag.
    let metadata = services.fragment_metadata.read().await;
    for id in &ids {
        let meta = metadata
            .get(id)
            .unwrap_or_else(|| panic!("no metadata for {id}"));
        assert_eq!(
            meta.get("_cn_consolidated").and_then(|v| v.as_bool()),
            Some(true),
            "fragment {id} should be flagged consolidated"
        );
        assert!(
            meta.get("_cn_consolidated_at").is_some(),
            "fragment {id} should carry the consolidation timestamp"
        );
    }
}

#[tokio::test]
async fn second_drain_pass_is_idempotent() {
    let (services, _server) = make_setup().await;

    let sink = ServicesSink::new(services.clone());
    sink.store(&rec("first lap", "cn-test-idem")).await.unwrap();
    drain_for_test(&services, &services.consolidation_queue, 2).await;
    let after_first = services.consolidation_queue.snapshot_metrics();
    assert_eq!(after_first.consolidated, 1);
    assert_eq!(after_first.failed, 0);

    // Manually re-enqueue the same id; the worker should skip it via
    // the metadata flag, not re-run process_memories.
    let ids = fragment_ids_for_session(&services, "cn-test-idem").await;
    for id in &ids {
        services.consolidation_queue.enqueue(id.clone());
    }
    drain_for_test(&services, &services.consolidation_queue, 2).await;
    let after_second = services.consolidation_queue.snapshot_metrics();
    // No additional successful consolidations because the second pass
    // saw the flag and bailed out of `consolidate_one` cleanly.
    assert_eq!(
        after_second.consolidated, after_first.consolidated,
        "second drain should not re-consolidate already-flagged fragments"
    );
    assert_eq!(after_second.failed, 0);
}

#[tokio::test]
async fn initial_scan_picks_up_preexisting_unflagged_fragments() {
    let (services, _server) = make_setup().await;

    // Simulate the WAL-replay scenario: sidecars populated, queue
    // empty, no consolidation flag yet. The worker's `initial_scan`
    // (called inside drain_for_test) must enqueue these.
    let session = "cn-test-scan";
    services
        .fragment_texts
        .write()
        .await
        .insert("frag-1".to_string(), "scan-target one".to_string());
    services
        .fragment_texts
        .write()
        .await
        .insert("frag-2".to_string(), "scan-target two".to_string());
    services.session_index.add(session, "frag-1").await;
    services.session_index.add(session, "frag-2").await;

    assert_eq!(
        services.consolidation_queue.pending_count(),
        0,
        "queue starts empty — these were not enqueued by any ingest path"
    );

    drain_for_test(&services, &services.consolidation_queue, 2).await;

    // Both should now be canonical + flagged, proving the scan
    // discovered them.
    for id in ["frag-1", "frag-2"] {
        let canonical = services
            .attractor_manager
            .get_fragment(id)
            .await
            .expect("get_fragment shouldn't error")
            .unwrap_or_else(|| panic!("{id} should be canonical after initial scan"));
        assert_eq!(canonical.id, id);
    }
}

#[tokio::test]
async fn substrate_endpoint_reports_consolidation_status() {
    let (services, server) = make_setup().await;

    let sink = ServicesSink::new(services.clone());
    for text in ["aa", "bb", "cc"] {
        sink.store(&rec(text, "cn-test-endpoint")).await.unwrap();
    }

    // Before draining: 3 fragments, 0 consolidated, lag=3.
    let res = server.get("/api/v1/substrate/consolidation").await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["total_fragments"], 3);
    assert_eq!(body["consolidated_count"], 0);
    assert_eq!(body["lag"], 3);
    assert_eq!(body["queued"], 3);

    drain_for_test(&services, &services.consolidation_queue, 4).await;

    // After draining: same total, all consolidated, lag=0.
    let res = server.get("/api/v1/substrate/consolidation").await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["total_fragments"], 3);
    assert_eq!(body["consolidated_count"], 3);
    assert_eq!(body["lag"], 0);
    assert_eq!(body["queued"], 0);
    assert_eq!(body["succeeded_total"], 3);
    assert_eq!(body["failed_total"], 0);
    assert_eq!(body["initial_scan_complete"], true);
}

#[tokio::test]
async fn consolidation_preserves_existing_metadata_fields() {
    let (services, _server) = make_setup().await;

    let sink = ServicesSink::new(services.clone());
    let mut record = rec("preserve me", "cn-test-preserve");
    record.metadata.insert(
        "src_session".to_string(),
        Value::String("real-uuid-1234".to_string()),
    );
    record.metadata.insert(
        "project_cwd".to_string(),
        Value::String("/home/user/proj".to_string()),
    );
    sink.store(&record).await.unwrap();

    drain_for_test(&services, &services.consolidation_queue, 1).await;

    let ids = fragment_ids_for_session(&services, "cn-test-preserve").await;
    let metadata = services.fragment_metadata.read().await;
    let meta = metadata.get(&ids[0]).unwrap();
    // Original metadata survives the consolidation write.
    assert_eq!(meta.get("kind").and_then(|v| v.as_str()), Some("learning"));
    assert_eq!(
        meta.get("src_session").and_then(|v| v.as_str()),
        Some("real-uuid-1234")
    );
    assert_eq!(
        meta.get("project_cwd").and_then(|v| v.as_str()),
        Some("/home/user/proj")
    );
    // Plus the new consolidation fields.
    assert_eq!(
        meta.get("_cn_consolidated").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[tokio::test]
async fn enqueue_dedup_keeps_repeated_ids_at_one_pending() {
    let (services, _server) = make_setup().await;
    services.consolidation_queue.enqueue("repeated".to_string());
    services.consolidation_queue.enqueue("repeated".to_string());
    services.consolidation_queue.enqueue("repeated".to_string());
    assert_eq!(
        services.consolidation_queue.pending_count(),
        1,
        "queue must dedup repeated ids — caller fire-and-forget is safe"
    );
    // Adding a metadata fixture so the inevitable text-missing skip
    // doesn't fail the test setup; we're only asserting dedup here.
    let _ = services
        .fragment_texts
        .write()
        .await
        .insert("repeated".to_string(), "stub".to_string());
    // Draining is fine; the test cares about pre-drain dedup behavior.
    drain_for_test(&services, &services.consolidation_queue, 1).await;
}

// Cross-reference assertion: confirm that the existing PR #28's
// neural-field-real.md epic's Phase 1 acceptance test holds — namely
// that the consolidation worker is exactly what wires basin formation
// and connection-network nodes for cc_hooks ingest. We check the
// downstream side-effect (canonical fragment exists) rather than
// poking at the basin_manager directly because its internal API
// surface is `pub(crate)` only.
#[tokio::test]
async fn epic_phase_1_acceptance_canonical_state_exists_post_consolidation() {
    let (services, _server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    sink.store(&rec("acceptance criteria 1.1", "cn-test-acceptance"))
        .await
        .unwrap();
    drain_for_test(&services, &services.consolidation_queue, 1).await;

    let ids = fragment_ids_for_session(&services, "cn-test-acceptance").await;
    let canonical = services
        .attractor_manager
        .get_fragment(&ids[0])
        .await
        .unwrap()
        .expect("canonical fragment must exist after Phase 1 consolidation");
    assert_eq!(canonical.id, ids[0]);
}

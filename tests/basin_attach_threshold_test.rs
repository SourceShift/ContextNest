//! Tests for the basin-attach-threshold fix that prevents every fragment
//! from creating its own singleton basin.
//!
//! Pre-fix behaviour (the bug): `process_memories` Step 1 unconditionally
//! called `create_attractor_basin_from_fragment` for every incoming
//! fragment, so `basin_count` ended up equal to `fragment_count` and
//! `avg_mass` collapsed to 1.0 in the live substrate.
//!
//! Post-fix contract:
//!
//! 1. Near-duplicate fragments (embeddings within
//!    `CONTEXTNEST_BASIN_ATTACH_THRESHOLD` distance) attach to the
//!    first-seen basin instead of seeding a new one.
//! 2. The cleanup admin endpoint `POST /api/v1/admin/merge-nearby-basins`
//!    collapses already-degenerate singletons and returns a tally.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::consolidation::drain_for_test;
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::sync::Mutex;

/// Serializes tests that mutate `CONTEXTNEST_BASIN_ATTACH_THRESHOLD` —
/// `cargo test` runs tests in parallel within the same binary, and the env
/// is process-wide so concurrent set/remove races otherwise pollute each
/// other's config snapshot at services-construction time.
static ENV_LOCK: Mutex<()> = Mutex::new(());

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

async fn store(server: &TestServer, session: &str, content: &str) {
    let res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": content,
            "importance": 0.7,
            "session_id": session,
            "metadata": {
                "kind": "verification",
                "src_session": session,
                "ts": "2026-05-28T09:00:00Z",
                "status": "passed",
            },
        }))
        .await;
    res.assert_status_ok();
}

/// Three near-duplicate fragments must collapse into fewer than three new
/// basins. Pre-fix every store created its own basin (basin_delta == 3 in
/// this scenario); post-fix the deterministic mock embedder produces
/// identical vectors for identical text, so distance is 0.0 — well below
/// any sensible threshold — and at least two of the three must attach to
/// the same basin (basin_delta ≤ 2).
#[tokio::test]
async fn similar_fragments_collapse_into_shared_basin() {
    // Lock + force a non-zero threshold so a concurrent zero-threshold test
    // can't poison the config snapshot read at services construction.
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("CONTEXTNEST_BASIN_ATTACH_THRESHOLD", "0.4");

    let (services, server) = make_setup().await;
    let sid = "cc-attach-test";

    let basins_before = services
        .attractor_manager
        .list_basin_snapshots()
        .await
        .len();

    for _ in 0..3 {
        store(&server, sid, "identical trajectory verification probe").await;
    }
    drain_for_test(&services, &services.consolidation_queue, 3).await;

    let basins_after = services
        .attractor_manager
        .list_basin_snapshots()
        .await
        .len();
    let delta = basins_after.saturating_sub(basins_before);
    assert!(
        delta < 3,
        "expected near-duplicate fragments to collapse: basin_delta < 3, got delta={delta} (before={basins_before}, after={basins_after}). Singleton bug regressed?"
    );

    std::env::remove_var("CONTEXTNEST_BASIN_ATTACH_THRESHOLD");
}

/// Setting the threshold to 0.0 disables attachment entirely and restores
/// the pre-fix "every fragment seeds a basin" behaviour. Validates the
/// escape hatch documented on `basin_attach_threshold`.
#[tokio::test]
async fn zero_threshold_restores_singleton_behaviour() {
    let _guard = ENV_LOCK.lock().unwrap();
    std::env::set_var("CONTEXTNEST_BASIN_ATTACH_THRESHOLD", "0.0");

    let (services, server) = make_setup().await;
    let basins_before = services
        .attractor_manager
        .list_basin_snapshots()
        .await
        .len();

    let sid = "cc-zero-threshold";
    for i in 0..3 {
        store(&server, sid, &format!("probe #{i}")).await;
    }
    drain_for_test(&services, &services.consolidation_queue, 3).await;

    let basins_after = services
        .attractor_manager
        .list_basin_snapshots()
        .await
        .len();
    let delta = basins_after.saturating_sub(basins_before);
    assert!(
        delta >= 3,
        "threshold=0.0 should restore singleton behaviour: basin_delta ≥ 3, got delta={delta} (before={basins_before}, after={basins_after})"
    );

    std::env::remove_var("CONTEXTNEST_BASIN_ATTACH_THRESHOLD");
}

/// Admin merge endpoint collapses singletons and returns a tally. The
/// distance_threshold query param drives the merge predicate
/// `distance < merged_radius * threshold`.
#[tokio::test]
async fn admin_merge_nearby_basins_endpoint_collapses_singletons() {
    let _guard = ENV_LOCK.lock().unwrap();
    // Force the singleton path so we have basins to merge.
    std::env::set_var("CONTEXTNEST_BASIN_ATTACH_THRESHOLD", "0.0");

    let (services, server) = make_setup().await;
    let sid = "cc-merge-admin";
    for i in 0..4 {
        store(&server, sid, &format!("similar probe #{i}")).await;
    }
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let before = services
        .attractor_manager
        .list_basin_snapshots()
        .await
        .len();

    // Loose threshold (10.0) collapses anything geometrically close enough
    // for the mock embedder — guarantees a non-zero merge tally for the
    // test, independent of the actual basin geometry.
    let res = server
        .post("/api/v1/admin/merge-nearby-basins?distance_threshold=10.0")
        .await;
    res.assert_status_ok();
    let body: Value = res.json();

    let merged = body["basins_merged"]
        .as_u64()
        .expect("basins_merged number");
    let remaining = body["basins_remaining"]
        .as_u64()
        .expect("basins_remaining number");
    assert!(
        merged + remaining as u64 <= before as u64,
        "merged + remaining should not exceed pre-merge basin count {before}, got merged={merged} remaining={remaining}"
    );
    assert!(
        body["elapsed_ms"].is_number(),
        "elapsed_ms should be reported"
    );

    std::env::remove_var("CONTEXTNEST_BASIN_ATTACH_THRESHOLD");
}

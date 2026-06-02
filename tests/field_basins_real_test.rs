//! Integration tests for Phase 3 of the neural-field epic — real
//! attractor basins surface via `GET /api/v1/field/basins`, with the
//! existing project-derived basins kept as a fallback for the
//! pre-consolidation state.
//!
//! Contract under test:
//!
//! 1. Cold substrate (no consolidation has run) → response uses
//!    `source: "project"` (current behaviour, preserved). Frontend
//!    keeps rendering instead of going blank.
//! 2. After the consolidation worker has run, real attractor basins
//!    are populated → response uses `source: "attractor"` and
//!    centroids are non-empty (carry the basin's embedding-space
//!    centroid).
//! 3. Each real basin reports `mass` = the count of its still-active
//!    fragments, plus a `by_kind` histogram and a `sessions` list
//!    consistent with the metadata sidecar.
//! 4. Fragment ids that are no longer in `session_index.active`
//!    (soft-deleted) are excluded from `mass` and `sessions`.
//!
//! The mock embedding service used by `ContextNestServices::new_default`
//! is deterministic, so a small number of fragments (~3) is enough to
//! produce real basins via the test-only `drain_for_test` helper from
//! the consolidation worker.

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

fn rec(text: &str, session: &str, project: &str) -> MemoryRecord {
    let mut r = MemoryRecord::new(MemoryKind::Learning, text.to_string(), session.to_string());
    r.metadata
        .insert("kind".to_string(), Value::String("learning".to_string()));
    r.metadata.insert(
        "project_cwd".to_string(),
        Value::String(project.to_string()),
    );
    r.metadata.insert(
        "ts".to_string(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    r
}

#[tokio::test]
async fn basins_endpoint_returns_project_fallback_before_consolidation() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    sink.store(&rec("first", "cn-test-fallback", "/home/u/alpha"))
        .await
        .unwrap();
    sink.store(&rec("second", "cn-test-fallback", "/home/u/alpha"))
        .await
        .unwrap();
    // Note: NO drain_for_test — the consolidation worker hasn't run.

    let res = server.get("/api/v1/field/basins").await;
    res.assert_status_ok();
    let body: Value = res.json();
    let basins = body["basins"].as_array().expect("basins array");
    assert_eq!(basins.len(), 1, "two same-project fragments → one basin");
    assert_eq!(
        basins[0]["source"], "project",
        "pre-consolidation: source must be 'project' (fallback path)"
    );
    assert_eq!(basins[0]["label"], "alpha");
    assert_eq!(basins[0]["mass"], 2);
    // Project basins never carry a centroid — they're derived from
    // path strings, not embedding space.
    assert!(
        basins[0]["centroid"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(true),
        "project fallback basins should have no centroid"
    );
}

#[tokio::test]
async fn basins_endpoint_filters_project_fallback_by_project_and_session() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    sink.store(&rec("alpha one", "cn-test-alpha-a", "/home/u/alpha"))
        .await
        .unwrap();
    sink.store(&rec("alpha two", "cn-test-alpha-b", "/home/u/alpha"))
        .await
        .unwrap();
    sink.store(&rec("beta one", "cn-test-beta", "/home/u/beta"))
        .await
        .unwrap();

    let res = server.get("/api/v1/field/basins?project=alpha").await;
    res.assert_status_ok();
    let body: Value = res.json();
    let basins = body["basins"].as_array().expect("basins array");
    assert_eq!(basins.len(), 1);
    assert_eq!(basins[0]["label"], "alpha");
    assert_eq!(basins[0]["mass"], 2);

    let res = server
        .get("/api/v1/field/basins?project=alpha&session_id=cn-test-alpha-a")
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    let basins = body["basins"].as_array().expect("basins array");
    assert_eq!(basins.len(), 1);
    assert_eq!(basins[0]["label"], "alpha");
    assert_eq!(basins[0]["mass"], 1);
    assert_eq!(basins[0]["sessions"], json!(["cn-test-alpha-a"]));
}

#[tokio::test]
async fn basins_endpoint_returns_real_attractors_after_consolidation() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    for text in [
        "real attractor formation test fragment one",
        "real attractor formation test fragment two",
        "completely different content from the others over here",
    ] {
        sink.store(&rec(text, "cn-test-real", "/home/u/beta"))
            .await
            .unwrap();
    }

    // Run the consolidation worker to completion so basins actually
    // form. This is Phase 1's job; we only verify Phase 3's surfacing.
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let res = server.get("/api/v1/field/basins").await;
    res.assert_status_ok();
    let body: Value = res.json();
    let basins = body["basins"].as_array().expect("basins array");
    assert!(
        !basins.is_empty(),
        "after consolidation, at least one real basin should exist"
    );
    // Every returned basin should be the real-attractor kind, never
    // the fallback (we only drop to fallback when the snapshot list
    // is empty, which it isn't here).
    for b in basins {
        assert_eq!(
            b["source"], "attractor",
            "after consolidation, every basin must be source='attractor': {b:?}"
        );
        assert!(
            b["id"].as_str().unwrap().starts_with("basin-"),
            "real-basin ids should be prefixed with 'basin-': {}",
            b["id"]
        );
        // Real basins carry a centroid vector from embedding space.
        let centroid = b["centroid"].as_array().expect("centroid array");
        assert!(
            !centroid.is_empty(),
            "real basin must have non-empty centroid vector"
        );
    }
}

#[tokio::test]
async fn real_basin_mass_excludes_discarded_fragments() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    for text in ["alive one", "alive two", "soon-to-be-deleted"] {
        sink.store(&rec(text, "cn-test-discard", "/home/u/gamma"))
            .await
            .unwrap();
    }
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    // Find the third fragment's id and soft-delete it.
    let ids = services.session_index.list_active("cn-test-discard").await;
    assert_eq!(ids.len(), 3);
    // Soft-delete one — list_active drops it, but the basin's
    // associated_fragments still contains it. The handler must
    // exclude it from mass.
    services
        .session_index
        .soft_remove("cn-test-discard", &ids[0])
        .await;

    let res = server.get("/api/v1/field/basins").await;
    res.assert_status_ok();
    let body: Value = res.json();
    let basins = body["basins"].as_array().unwrap();
    // Sum of mass across all real basins should equal the count of
    // still-active fragments (2), never the basin's
    // associated_fragments count (which still includes the deleted one).
    let total_mass: u64 = basins
        .iter()
        .filter(|b| b["source"] == "attractor")
        .map(|b| b["mass"].as_u64().unwrap_or(0))
        .sum();
    assert_eq!(
        total_mass, 2,
        "total real-basin mass must reflect only active fragments \
         (got {total_mass}, expected 2 after one soft-delete)"
    );
}

#[tokio::test]
async fn real_basin_label_uses_dominant_kind_when_available() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    // All three fragments carry kind=learning, so the dominant kind
    // for whichever basin they land in must be "learning".
    for text in [
        "kind-driven basin labelling alpha",
        "kind-driven basin labelling beta",
        "kind-driven basin labelling gamma",
    ] {
        sink.store(&rec(text, "cn-test-label", "/home/u/delta"))
            .await
            .unwrap();
    }
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let res = server.get("/api/v1/field/basins").await;
    let body: Value = res.json();
    let basins = body["basins"].as_array().unwrap();
    let attractor_basins: Vec<&Value> = basins
        .iter()
        .filter(|b| b["source"] == "attractor")
        .collect();
    assert!(
        !attractor_basins.is_empty(),
        "expected at least one real basin after consolidation"
    );
    // Every real basin in this fixture has only learning-kind
    // members, so the label must be "learning" (not a basin-{slug}
    // fallback).
    for b in attractor_basins {
        assert_eq!(
            b["label"], "learning",
            "dominant kind should drive label, got {} for {:?}",
            b["label"], b
        );
    }
}

#[tokio::test]
async fn basins_handler_falls_back_to_project_when_all_real_basins_emptied() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    sink.store(&rec("solo", "cn-test-empty", "/home/u/epsilon"))
        .await
        .unwrap();
    drain_for_test(&services, &services.consolidation_queue, 1).await;

    // Soft-delete the only fragment. Real basin still exists in the
    // basin_manager (its associated_fragments wasn't pruned) but has
    // zero active members → handler retains nothing from
    // `list_basin_snapshots`, drops to the project-fallback path.
    let ids = services.session_index.list_active("cn-test-empty").await;
    services
        .session_index
        .soft_remove("cn-test-empty", &ids[0])
        .await;

    let res = server.get("/api/v1/field/basins").await;
    let body: Value = res.json();
    let basins = body["basins"].as_array().unwrap();
    // With zero active fragments left in any basin, the active map is
    // also empty and the fallback yields no project basins either.
    // That's the legitimate honest answer for "no live fragments."
    // We assert the response is well-formed (empty array, never 500).
    assert_eq!(
        basins.len(),
        0,
        "fully-deleted substrate should return empty basin list, got {basins:?}"
    );
}

#[tokio::test]
async fn project_fallback_response_is_unchanged_for_legacy_callers() {
    // Regression guard: the frontend (web/src/routes/field.tsx) reads
    // these field names. Adding source/centroid is additive, but the
    // old fields must keep the same shape so existing dashboards
    // don't break.
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    sink.store(&rec("shape stability", "cn-test-shape", "/home/u/zeta"))
        .await
        .unwrap();
    // No drain → project fallback.

    let res = server.get("/api/v1/field/basins").await;
    let body: Value = res.json();
    let first = &body["basins"].as_array().unwrap()[0];
    // Required fields preserved.
    assert!(first.get("id").is_some(), "id field missing");
    assert!(first.get("label").is_some(), "label field missing");
    assert!(first.get("source").is_some(), "source field missing");
    assert!(first.get("mass").is_some(), "mass field missing");
    assert!(first.get("by_kind").is_some(), "by_kind field missing");
    assert!(first.get("sessions").is_some(), "sessions field missing");
    // id format unchanged for project-source basins.
    assert!(
        first["id"]
            .as_str()
            .map(|s| s.starts_with("proj-"))
            .unwrap_or(false),
        "project basin id should still be 'proj-{{label}}', got {}",
        first["id"]
    );
    // serde(skip_serializing_if = "Vec::is_empty") collapses centroid
    // to absent for project basins — important wire-compat detail.
    assert!(
        first.get("centroid").is_none()
            || first["centroid"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true),
        "project basins must not emit a non-empty centroid"
    );
    // No-op assertion to silence unused-var warning if compiler
    // complains about `json` import elsewhere.
    let _ = json!({});
}

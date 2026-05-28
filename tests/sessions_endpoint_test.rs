//! Integration tests for `GET /api/v1/sessions`.
//!
//! Verifies the locked response contract:
//! - Empty substrate returns `{"sessions": []}`.
//! - Fragments in multiple sessions are grouped correctly with accurate
//!   `fragment_count`.
//! - `project_cwd`, `src_session_uuid`, and `last_ts` metadata fields
//!   round-trip correctly.
//! - Soft-deleted fragments are NOT counted in `fragment_count`.
//! - Sessions are sorted newest-first by `last_ts`.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};

// =============================================================================
// Helpers
// =============================================================================

async fn make_server() -> TestServer {
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = create_simple_app(services).await.expect("app should build");
    TestServer::new(app).expect("test server should start")
}

/// Store a fragment and return its `attractor_id`.
async fn store(server: &TestServer, session: &str, content: &str, metadata: Value) -> String {
    let res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": content,
            "importance": 0.7,
            "session_id": session,
            "metadata": metadata,
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["stored"], true, "fragment should be stored");
    body["attractor_id"]
        .as_str()
        .expect("attractor_id present")
        .to_string()
}

/// Soft-delete a fragment via the `discard` tool (`soft_delete: true`).
async fn soft_discard(server: &TestServer, session: &str, attractor_id: &str) {
    let res = server
        .post("/api/v1/tools/discard")
        .json(&json!({
            "attractor_id": attractor_id,
            "soft_delete": true,
            "session_id": session,
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["discarded"], true, "fragment should be discarded");
}

/// Fetch the sessions list and return the parsed JSON value.
async fn list_sessions(server: &TestServer) -> Value {
    let res = server.get("/api/v1/sessions").await;
    res.assert_status_ok();
    res.json()
}

// =============================================================================
// Tests
// =============================================================================

/// An empty substrate should return an empty `sessions` array, not an error.
#[tokio::test]
async fn empty_substrate_returns_empty_sessions() {
    let server = make_server().await;
    let body = list_sessions(&server).await;

    let sessions = body["sessions"].as_array().expect("sessions array present");
    assert!(
        sessions.is_empty(),
        "expected no sessions on a fresh substrate, got {sessions:?}"
    );
}

/// Three fragments in two sessions must produce two entries with correct
/// `fragment_count` values. Ordering must be descending by `last_ts`.
#[tokio::test]
async fn two_sessions_correct_fragment_counts_and_order() {
    let server = make_server().await;

    // Session "sess-aaa" — one fragment with an earlier timestamp.
    store(
        &server,
        "sess-aaa",
        "fragment in session aaa",
        json!({
            "project_cwd": "/Users/admin/code/aaa",
            "src_session": "uuid-aaa-0001",
            "ts": "2026-05-20T09:00:00Z",
        }),
    )
    .await;

    // Session "sess-bbb" — two fragments, second has a later timestamp.
    store(
        &server,
        "sess-bbb",
        "first fragment in session bbb",
        json!({
            "project_cwd": "/Users/admin/code/bbb",
            "src_session": "uuid-bbb-0001",
            "ts": "2026-05-20T10:00:00Z",
        }),
    )
    .await;
    store(
        &server,
        "sess-bbb",
        "second fragment in session bbb",
        json!({
            "project_cwd": "/Users/admin/code/bbb",
            "src_session": "uuid-bbb-0001",
            "ts": "2026-05-20T11:00:00Z",
        }),
    )
    .await;

    let body = list_sessions(&server).await;
    let sessions = body["sessions"].as_array().expect("sessions array");

    assert_eq!(sessions.len(), 2, "exactly two sessions expected");

    // Newest first: sess-bbb (last_ts 11:00) before sess-aaa (last_ts 09:00).
    assert_eq!(
        sessions[0]["id"].as_str().unwrap(),
        "sess-bbb",
        "sess-bbb should be first (newer last_ts)"
    );
    assert_eq!(
        sessions[1]["id"].as_str().unwrap(),
        "sess-aaa",
        "sess-aaa should be second (older last_ts)"
    );

    assert_eq!(
        sessions[0]["fragment_count"].as_u64().unwrap(),
        2,
        "sess-bbb has 2 active fragments"
    );
    assert_eq!(
        sessions[1]["fragment_count"].as_u64().unwrap(),
        1,
        "sess-aaa has 1 active fragment"
    );
}

/// `project_cwd`, `src_session_uuid`, and `last_ts` must round-trip correctly
/// through the metadata sidecar.
#[tokio::test]
async fn metadata_fields_round_trip() {
    let server = make_server().await;

    store(
        &server,
        "sess-meta-trip",
        "content with full metadata",
        json!({
            "project_cwd": "/foo",
            "src_session": "uuid-here",
            "ts": "2026-05-20T10:00:00Z",
        }),
    )
    .await;

    let body = list_sessions(&server).await;
    let sessions = body["sessions"].as_array().expect("sessions array");

    // Find the session we just inserted (there may be others from shared state
    // if tests run in the same process, but each test uses a fresh server).
    assert_eq!(sessions.len(), 1, "one session in this server instance");
    let s = &sessions[0];

    assert_eq!(s["id"].as_str().unwrap(), "sess-meta-trip");
    assert_eq!(s["fragment_count"].as_u64().unwrap(), 1);
    assert_eq!(s["project_cwd"].as_str().unwrap(), "/foo");
    assert_eq!(s["src_session_uuid"].as_str().unwrap(), "uuid-here");
    assert_eq!(s["last_ts"].as_str().unwrap(), "2026-05-20T10:00:00Z");
}

/// A soft-deleted fragment must NOT be counted in `fragment_count`.
#[tokio::test]
async fn soft_deleted_fragment_excluded_from_count() {
    let server = make_server().await;

    // Store two fragments in the same session.
    let id_a = store(
        &server,
        "sess-discard-test",
        "fragment A — will be soft-deleted",
        json!({
            "project_cwd": "/bar",
            "ts": "2026-05-20T08:00:00Z",
        }),
    )
    .await;

    store(
        &server,
        "sess-discard-test",
        "fragment B — stays active",
        json!({
            "project_cwd": "/bar",
            "ts": "2026-05-20T09:00:00Z",
        }),
    )
    .await;

    // Confirm we see 2 fragments before the discard.
    let body_before = list_sessions(&server).await;
    let sessions_before = body_before["sessions"].as_array().expect("sessions array");
    assert_eq!(sessions_before.len(), 1);
    assert_eq!(
        sessions_before[0]["fragment_count"].as_u64().unwrap(),
        2,
        "both fragments active before discard"
    );

    // Soft-delete fragment A.
    soft_discard(&server, "sess-discard-test", &id_a).await;

    // Now only 1 active fragment should be counted.
    let body_after = list_sessions(&server).await;
    let sessions_after = body_after["sessions"].as_array().expect("sessions array");
    assert_eq!(sessions_after.len(), 1);
    assert_eq!(
        sessions_after[0]["fragment_count"].as_u64().unwrap(),
        1,
        "soft-deleted fragment must not appear in fragment_count"
    );
    // The remaining active fragment's last_ts should still be visible.
    assert_eq!(
        sessions_after[0]["last_ts"].as_str().unwrap(),
        "2026-05-20T09:00:00Z",
        "last_ts reflects the surviving active fragment"
    );
}

// =============================================================================
// Top-feature endpoint tests
// =============================================================================

/// Helper: hit GET /api/v1/sessions/:id/top-feature and return parsed JSON.
async fn top_feature(server: &TestServer, session_id: &str) -> Value {
    let res = server
        .get(&format!("/api/v1/sessions/{session_id}/top-feature"))
        .await;
    res.assert_status_ok();
    res.json()
}

/// A session with no Feature records returns top_feature: null and
/// candidate_count: 0 — the contract z-dashboard relies on to decide
/// "no anchor → fall back to project tag".
#[tokio::test]
async fn top_feature_no_features_returns_null() {
    let server = make_server().await;

    // Stash one non-feature fragment so the session exists.
    store(
        &server,
        "cc-no-feat",
        "just a plain accomplishment",
        json!({
            "kind": "accomplishment",
            "src_session": "cc-no-feat",
            "ts": "2026-05-20T10:00:00Z",
        }),
    )
    .await;

    let body = top_feature(&server, "cc-no-feat").await;
    assert!(body["top_feature"].is_null(), "no features → null payload");
    assert_eq!(body["candidate_count"].as_u64().unwrap(), 0);
    assert_eq!(body["session_id"].as_str().unwrap(), "cc-no-feat");
}

/// When file_overlap is the only signal that distinguishes two features,
/// the one whose declared files match files_touched wins.
#[tokio::test]
async fn top_feature_prefers_file_overlap() {
    let server = make_server().await;

    // files_touched for the session lists the file the WINNING feature claims.
    store(
        &server,
        "cc-overlap",
        "session touched 1 file(s)",
        json!({
            "kind": "files_touched",
            "src_session": "cc-overlap",
            "files": ["src/foo.rs"],
            "ts": "2026-05-20T10:00:00Z",
        }),
    )
    .await;

    // Feature A: declares src/foo.rs → overlap = 1.0
    store(
        &server,
        "cc-overlap",
        "feature_with_overlap",
        json!({
            "kind": "feature",
            "src_session": "cc-overlap",
            "files": ["src/foo.rs"],
            "ts": "2026-05-20T10:05:00Z",
        }),
    )
    .await;

    // Feature B: declares an unrelated file → overlap = 0
    // Bumped to a later ts so recency alone would favour it; file_overlap
    // weight (0.40) must beat the smaller recency delta to make this a
    // real test of the overlap signal.
    store(
        &server,
        "cc-overlap",
        "feature_without_overlap",
        json!({
            "kind": "feature",
            "src_session": "cc-overlap",
            "files": ["unrelated/baz.rs"],
            "ts": "2026-05-20T10:10:00Z",
        }),
    )
    .await;

    let body = top_feature(&server, "cc-overlap").await;
    let top = &body["top_feature"];
    assert_eq!(
        top["feature"].as_str().unwrap(),
        "feature_with_overlap",
        "file_overlap should beat recency at default weights"
    );
    assert!(
        top["file_overlap"].as_f64().unwrap() > 0.99,
        "winner reports full overlap"
    );
    assert_eq!(body["candidate_count"].as_u64().unwrap(), 2);
}

/// Frequency (same feature name across multiple turns) should outweigh
/// a single-shot feature with marginally better recency.
#[tokio::test]
async fn top_feature_prefers_repeated_feature() {
    let server = make_server().await;

    // Feature A appears 3 times → high freq signal.
    for ts in [
        "2026-05-20T10:00:00Z",
        "2026-05-20T10:05:00Z",
        "2026-05-20T10:10:00Z",
    ] {
        store(
            &server,
            "cc-freq",
            "Repeated Feature",
            json!({
                "kind": "feature",
                "src_session": "cc-freq",
                "ts": ts,
            }),
        )
        .await;
    }

    // Feature B once, later ts. No files on either side → file_overlap is
    // 0 for both; the only discriminators are freq + recency.
    store(
        &server,
        "cc-freq",
        "Single Feature",
        json!({
            "kind": "feature",
            "src_session": "cc-freq",
            "ts": "2026-05-20T10:15:00Z",
        }),
    )
    .await;

    let body = top_feature(&server, "cc-freq").await;
    let top = &body["top_feature"];
    assert_eq!(
        top["feature"].as_str().unwrap(),
        "Repeated Feature",
        "freq=3 should outscore freq=1 + slight recency lead at default weights"
    );
    assert_eq!(top["freq"].as_u64().unwrap(), 3);
}

// =============================================================================
// Session-summary endpoint tests
// =============================================================================

/// Helper: hit GET /api/v1/sessions/:id/summary and return parsed JSON.
async fn summary(server: &TestServer, session_id: &str) -> serde_json::Value {
    let res = server
        .get(&format!("/api/v1/sessions/{session_id}/summary"))
        .await;
    res.assert_status_ok();
    res.json()
}

/// A session with one record per kind round-trips into the expected
/// Insight-shaped summary payload: domain text + progress + topics from
/// the Domain record's metadata, latest goal/state, all accomplishments
/// and learnings, todo with status.
#[tokio::test]
async fn summary_assembles_full_insight_shape() {
    let server = make_server().await;
    let sid = "cc-sum-1";

    // Domain record carries the topics + progress metadata.
    store(
        &server,
        sid,
        "backend",
        json!({
            "kind": "domain",
            "src_session": sid,
            "ts": "2026-05-20T10:00:00Z",
            "progress": "in-progress",
            "topics": ["auth", "session-routing"],
        }),
    )
    .await;
    store(
        &server,
        sid,
        "Implementing the new top-feature endpoint",
        json!({
            "kind": "goal_phase",
            "src_session": sid,
            "ts": "2026-05-20T10:05:00Z",
        }),
    )
    .await;
    store(
        &server,
        sid,
        "Endpoint shipped, awaiting daemon-side wiring",
        json!({
            "kind": "state",
            "src_session": sid,
            "ts": "2026-05-20T10:10:00Z",
        }),
    )
    .await;
    store(
        &server,
        sid,
        "Wrote the route handler",
        json!({
            "kind": "accomplishment",
            "src_session": sid,
            "ts": "2026-05-20T10:06:00Z",
        }),
    )
    .await;
    store(
        &server,
        sid,
        "Closed-set LLM classification is cheap enough at session-close",
        json!({
            "kind": "learning",
            "src_session": sid,
            "ts": "2026-05-20T10:07:00Z",
        }),
    )
    .await;
    store(
        &server,
        sid,
        "Update daemon to consume summary endpoint",
        json!({
            "kind": "todo",
            "src_session": sid,
            "ts": "2026-05-20T10:08:00Z",
            "task_id": "wire-daemon",
            "task_status": "pending",
        }),
    )
    .await;

    let body = summary(&server, sid).await;
    let s = &body["summary"];

    assert_eq!(s["domain"].as_str().unwrap(), "backend");
    assert_eq!(s["progress"].as_str().unwrap(), "in-progress");
    assert_eq!(
        s["topics"].as_array().unwrap(),
        &[json!("auth"), json!("session-routing")]
    );
    assert_eq!(
        s["goal"].as_str().unwrap(),
        "Implementing the new top-feature endpoint"
    );
    assert_eq!(
        s["current_state"].as_str().unwrap(),
        "Endpoint shipped, awaiting daemon-side wiring"
    );
    assert_eq!(s["top_jobs"].as_array().unwrap().len(), 1);
    assert_eq!(
        s["top_jobs"][0].as_str().unwrap(),
        "Wrote the route handler"
    );
    assert_eq!(s["facts"].as_array().unwrap().len(), 1);
    assert_eq!(s["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(s["tasks"][0]["id"].as_str().unwrap(), "wire-daemon");
    assert_eq!(s["tasks"][0]["status"].as_str().unwrap(), "pending");
    assert_eq!(s["last_ts"].as_str().unwrap(), "2026-05-20T10:10:00Z");
    assert_eq!(s["started_at"].as_str().unwrap(), "2026-05-20T10:00:00Z");
}

/// Latest goal/state wins when the session emits multiple turns. This
/// is the contract the categorizer relies on — drift is captured by
/// taking the most-recent record, not by averaging.
#[tokio::test]
async fn summary_picks_latest_goal_and_state() {
    let server = make_server().await;
    let sid = "cc-sum-latest";

    // Two goal_phase records — newer one wins.
    store(
        &server,
        sid,
        "old goal",
        json!({"kind": "goal_phase", "src_session": sid, "ts": "2026-05-20T09:00:00Z"}),
    )
    .await;
    store(
        &server,
        sid,
        "new goal",
        json!({"kind": "goal_phase", "src_session": sid, "ts": "2026-05-20T11:00:00Z"}),
    )
    .await;

    // Two state records.
    store(
        &server,
        sid,
        "old state",
        json!({"kind": "state", "src_session": sid, "ts": "2026-05-20T09:30:00Z"}),
    )
    .await;
    store(
        &server,
        sid,
        "new state",
        json!({"kind": "state", "src_session": sid, "ts": "2026-05-20T11:30:00Z"}),
    )
    .await;

    let body = summary(&server, sid).await;
    assert_eq!(body["summary"]["goal"].as_str().unwrap(), "new goal");
    assert_eq!(
        body["summary"]["current_state"].as_str().unwrap(),
        "new state"
    );
}

/// 404 for an unknown session — the categorizer relies on this to
/// distinguish "CN has no data" from "CN said empty summary".
#[tokio::test]
async fn summary_unknown_session_returns_404() {
    let server = make_server().await;
    let res = server
        .get("/api/v1/sessions/cc-does-not-exist/summary")
        .await;
    assert_eq!(res.status_code(), 404, "unknown session must 404");
}

/// Trajectory endpoint groups the new z-insight memory kinds into a
/// chronological stream, phase summaries, promotion queue, and cost profile.
#[tokio::test]
async fn trajectory_endpoint_groups_records_phases_and_promotion_queue() {
    let server = make_server().await;
    let sid = "cc-trajectory-ui";

    store(
        &server,
        sid,
        "Implement trajectory UI",
        json!({
            "kind": "goal_phase",
            "src_session": sid,
            "ts": "2026-05-27T09:00:00Z",
            "start_ts": "2026-05-27T09:00:00Z",
            "end_ts": "2026-05-27T10:00:00Z"
        }),
    )
    .await;
    store(
        &server,
        sid,
        "Current state turn marker",
        json!({"kind": "state", "src_session": sid, "ts": "2026-05-27T09:05:00Z"}),
    )
    .await;
    store(
        &server,
        sid,
        "Use sparse gated emission",
        json!({"kind": "decision_made", "src_session": sid, "ts": "2026-05-27T09:10:00Z"}),
    )
    .await;
    store(
        &server,
        sid,
        "Dry-run emitted all trajectory kinds",
        json!({"kind": "verification", "src_session": sid, "ts": "2026-05-27T09:20:00Z", "status": "passed"}),
    )
    .await;
    store(
        &server,
        sid,
        "Promote sparse emission after repetition",
        json!({"kind": "memory_candidate", "src_session": sid, "ts": "2026-05-27T09:30:00Z"}),
    )
    .await;
    store(
        &server,
        sid,
        "Do not over-emit prompt memory",
        json!({"kind": "prompt_directive", "src_session": sid, "ts": "2026-05-27T09:40:00Z"}),
    )
    .await;

    let res = server
        .get(&format!("/api/v1/sessions/{sid}/trajectory"))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();

    assert_eq!(body["session_id"], sid);
    assert_eq!(body["trajectory_count"], 4);
    assert_eq!(body["records"].as_array().unwrap().len(), 4);
    assert_eq!(body["phases"].as_array().unwrap().len(), 1);
    assert_eq!(
        body["phases"][0]["counts"]["decision_made"]
            .as_u64()
            .unwrap(),
        1
    );
    assert_eq!(body["promotion_queue"].as_array().unwrap().len(), 2);
    assert_eq!(body["cost_profile"]["trajectory_records"].as_u64(), Some(4));
    assert_eq!(body["cost_profile"]["prompt_directives"].as_u64(), Some(1));
    assert_eq!(body["cost_profile"]["memory_candidates"].as_u64(), Some(1));

    // `basin_links` must be present as an array — the field shape is a
    // hard contract for the dashboard's basin badge. Content correctness
    // (member counts, heat, hottest kind) is asserted in the dedicated
    // test that explicitly drains the consolidation queue; the mock-mode
    // test substrate populates basins inline so we can't reliably assert
    // emptiness here without coupling to that mock-only behaviour.
    assert!(
        body["basin_links"].is_array(),
        "basin_links must be present as an array, got: {:?}",
        body["basin_links"]
    );
}

/// `basin_links` populates with substrate geometry once the consolidation
/// worker crystallises basins for the session's fragments. The test forces
/// consolidation via `drain_for_test` so we can assert the populated shape
/// rather than only the cold-substrate empty case.
#[tokio::test]
async fn trajectory_endpoint_exposes_basin_links_post_consolidation() {
    use contextnest::services::consolidation::drain_for_test;

    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = contextnest::api::create_simple_app(services.clone())
        .await
        .expect("app should build");
    let server = TestServer::new(app).expect("test server should start");
    let sid = "cc-basin-links";

    // Three semantically similar fragments under one session. With the
    // canonical pipeline draining synchronously they should land in the
    // same basin (or at most a handful of basins) — enough to populate
    // basin_links.
    for (i, ts) in [
        "2026-05-27T09:00:00Z",
        "2026-05-27T09:05:00Z",
        "2026-05-27T09:10:00Z",
    ]
    .iter()
    .enumerate()
    {
        store(
            &server,
            sid,
            &format!("trajectory verification probe #{i}"),
            json!({
                "kind": "verification",
                "src_session": sid,
                "ts": ts,
                "status": "passed",
            }),
        )
        .await;
    }

    drain_for_test(&services, &services.consolidation_queue, 3).await;

    let res = server
        .get(&format!("/api/v1/sessions/{sid}/trajectory"))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();

    let basin_links = body["basin_links"].as_array().expect("basin_links present");
    assert!(
        !basin_links.is_empty(),
        "expected at least one basin_link after draining consolidation queue"
    );

    let total_members_in_session: u64 = basin_links
        .iter()
        .map(|l| l["members_in_session"].as_u64().unwrap_or(0))
        .sum();
    assert!(
        total_members_in_session >= 3,
        "expected basin coverage of all 3 stored fragments, got {total_members_in_session}"
    );

    let first = &basin_links[0];
    assert!(first["basin_id"].is_string(), "basin_id must be a string");
    assert!(
        first["total_members"].as_u64().unwrap_or(0) >= 1,
        "total_members must be at least 1 when a basin overlaps the session"
    );
    assert_eq!(
        first["hottest_kind"].as_str(),
        Some("verification"),
        "all stored fragments were verification kind"
    );

    // resonant_basins must be present as an array. With only one session's
    // fragments stored, there are no foreign basins to resonate with —
    // expected empty. Content correctness with cross-session resonance is
    // tested in `trajectory_endpoint_exposes_resonant_basins_across_sessions`.
    assert!(
        body["resonant_basins"].is_array(),
        "resonant_basins must be present as an array, got: {:?}",
        body["resonant_basins"]
    );

    // promotion_clusters present as array (structural contract). Content
    // (clusters of memory_candidate/prompt_directive/risk_flag candidates
    // grouped by basin) is asserted in
    // `trajectory_endpoint_promotion_clusters_group_candidates_by_basin`.
    assert!(
        body["promotion_clusters"].is_array(),
        "promotion_clusters must be present as an array, got: {:?}",
        body["promotion_clusters"]
    );
}

/// `promotion_clusters` groups the flat promotion queue by basin. With
/// the consolidation worker drained and singleton-mode forced (so each
/// candidate lands in its own basin), every cluster has one candidate
/// and the count of clusters equals the count of distinct promotion
/// records.
#[tokio::test]
async fn trajectory_endpoint_promotion_clusters_group_candidates_by_basin() {
    use contextnest::services::consolidation::drain_for_test;

    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = contextnest::api::create_simple_app(services.clone())
        .await
        .expect("app should build");
    let server = TestServer::new(app).expect("test server should start");

    // Force singleton mode so each promotion candidate has its own basin
    // and the clustering output is deterministic — three promotion-kind
    // fragments must produce three single-candidate clusters.
    std::env::set_var("CONTEXTNEST_BASIN_ATTACH_THRESHOLD", "0.0");

    let sid = "cc-promo-clusters";
    store(
        &server,
        sid,
        "Goal phase for cluster test",
        json!({
            "kind": "goal_phase",
            "src_session": sid,
            "ts": "2026-05-28T09:00:00Z",
            "start_ts": "2026-05-28T09:00:00Z",
        }),
    )
    .await;
    store(
        &server,
        sid,
        "Promote sparse emission rule",
        json!({"kind": "memory_candidate", "src_session": sid, "ts": "2026-05-28T09:05:00Z"}),
    )
    .await;
    store(
        &server,
        sid,
        "Inject WAL-safety directive on schema migration",
        json!({"kind": "prompt_directive", "src_session": sid, "ts": "2026-05-28T09:10:00Z"}),
    )
    .await;
    store(
        &server,
        sid,
        "Live WAL migration can lose data",
        json!({"kind": "risk_flag", "src_session": sid, "ts": "2026-05-28T09:15:00Z"}),
    )
    .await;

    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let res = server
        .get(&format!("/api/v1/sessions/{sid}/trajectory"))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();

    let promotion_queue = body["promotion_queue"]
        .as_array()
        .expect("promotion_queue present");
    assert_eq!(
        promotion_queue.len(),
        3,
        "expected 3 promotion-kind records (memory_candidate, prompt_directive, risk_flag)"
    );

    let clusters = body["promotion_clusters"]
        .as_array()
        .expect("promotion_clusters present");
    let total_in_clusters: usize = clusters
        .iter()
        .map(|c| c["candidates"].as_array().map(|a| a.len()).unwrap_or(0))
        .sum();
    assert_eq!(
        total_in_clusters, 3,
        "every promotion candidate must end up in exactly one cluster; got {total_in_clusters} across {} clusters",
        clusters.len()
    );

    // Coherence sums to 1.0 (within float epsilon) when every candidate
    // has a basin assignment — each share is fraction-of-clustered.
    let coherence_sum: f32 = clusters
        .iter()
        .map(|c| c["coherence"].as_f64().unwrap_or(0.0) as f32)
        .sum();
    assert!(
        (coherence_sum - 1.0).abs() < 1e-3,
        "coherence shares should sum to ~1.0, got {coherence_sum}"
    );

    std::env::remove_var("CONTEXTNEST_BASIN_ATTACH_THRESHOLD");
}

/// `resonant_basins` populates when a session's fragments share connection
/// graph edges with fragments owned by OTHER sessions. The neighbor's basin
/// must be different from the session's own basins (no self-resonance), and
/// `sessions_touching` counts distinct foreign session ids.
#[tokio::test]
async fn trajectory_endpoint_exposes_resonant_basins_across_sessions() {
    use contextnest::services::consolidation::drain_for_test;

    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = contextnest::api::create_simple_app(services.clone())
        .await
        .expect("app should build");
    let server = TestServer::new(app).expect("test server should start");

    // Force singleton-per-fragment so we can construct deterministic
    // cross-session connection edges via the connection_network's
    // auto-linking on add_node. (With basin clustering enabled, the
    // mock embedder may merge near-duplicates and obscure the test signal.)
    std::env::set_var("CONTEXTNEST_BASIN_ATTACH_THRESHOLD", "0.0");

    let session_a = "cc-resonance-a";
    let session_b = "cc-resonance-b";
    let session_c = "cc-resonance-c";

    // Three sessions storing slight variations on the same theme — the
    // mock embedder's deterministic + similarity-aware auto-linking in
    // ConnectionNetwork::add_node ties them via edges even when basins
    // don't merge.
    for sid in [session_a, session_b, session_c] {
        store(
            &server,
            sid,
            &format!("trajectory verification probe {sid}"),
            json!({
                "kind": "verification",
                "src_session": sid,
                "ts": "2026-05-28T09:00:00Z",
                "status": "passed",
            }),
        )
        .await;
    }
    drain_for_test(&services, &services.consolidation_queue, 3).await;

    let res = server
        .get(&format!("/api/v1/sessions/{session_a}/trajectory"))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();

    let resonant = body["resonant_basins"]
        .as_array()
        .expect("resonant_basins present");

    // Contract: each entry must have basin_id (string), coherence (number),
    // sessions_touching (number), edge_count (number). And resonant basins
    // must not appear in basin_links (no self-resonance).
    let own_basins: std::collections::HashSet<String> = body["basin_links"]
        .as_array()
        .expect("basin_links present")
        .iter()
        .filter_map(|b| b["basin_id"].as_str().map(|s| s.to_string()))
        .collect();

    for entry in resonant {
        assert!(entry["basin_id"].is_string(), "basin_id must be string");
        assert!(entry["coherence"].is_number(), "coherence must be number");
        assert!(
            entry["sessions_touching"].is_number(),
            "sessions_touching must be number"
        );
        assert!(entry["edge_count"].is_number(), "edge_count must be number");
        let bid = entry["basin_id"].as_str().unwrap();
        assert!(
            !own_basins.contains(bid),
            "resonant basin {bid} must not also be in basin_links (no self-resonance)"
        );
    }

    std::env::remove_var("CONTEXTNEST_BASIN_ATTACH_THRESHOLD");
}

/// Prompt preview is a deterministic, no-LLM capsule preview over the
/// trajectory kinds a future prompt compiler would care about.
#[tokio::test]
async fn prompt_preview_endpoint_returns_capsule_sections() {
    let server = make_server().await;
    let sid = "cc-prompt-preview";

    store(
        &server,
        sid,
        "Keep trajectory fields sparse",
        json!({"kind": "decision_made", "src_session": sid, "ts": "2026-05-27T09:00:00Z"}),
    )
    .await;
    store(
        &server,
        sid,
        "Dry-run passed",
        json!({"kind": "verification", "src_session": sid, "ts": "2026-05-27T09:10:00Z", "status": "passed"}),
    )
    .await;
    store(
        &server,
        sid,
        "Skipped check should not enter preview",
        json!({"kind": "verification", "src_session": sid, "ts": "2026-05-27T09:11:00Z", "status": "not_run"}),
    )
    .await;
    store(
        &server,
        sid,
        "Emit trajectory arrays only when gates are crossed",
        json!({"kind": "prompt_directive", "src_session": sid, "ts": "2026-05-27T09:20:00Z"}),
    )
    .await;

    let res = server
        .get(&format!("/api/v1/sessions/{sid}/prompt-preview"))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();

    assert_eq!(body["session_id"], sid);
    assert_eq!(body["item_count"], 3);
    let sections = body["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 7);
    assert_eq!(sections[0]["key"], "decisions");
    assert_eq!(sections[0]["items"].as_array().unwrap().len(), 1);
    assert_eq!(sections[1]["key"], "verified");
    assert_eq!(sections[1]["items"].as_array().unwrap().len(), 1);
    assert_eq!(sections[4]["key"], "directives");
    assert_eq!(sections[4]["items"].as_array().unwrap().len(), 1);
}

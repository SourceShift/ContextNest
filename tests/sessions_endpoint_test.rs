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

    // Session "cc-aaa" — one fragment with an earlier timestamp.
    store(
        &server,
        "cc-aaa",
        "fragment in session aaa",
        json!({
            "project_cwd": "/Users/admin/code/aaa",
            "src_session": "uuid-aaa-0001",
            "ts": "2026-05-20T09:00:00Z",
        }),
    )
    .await;

    // Session "cc-bbb" — two fragments, second has a later timestamp.
    store(
        &server,
        "cc-bbb",
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
        "cc-bbb",
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

    // Newest first: cc-bbb (last_ts 11:00) before cc-aaa (last_ts 09:00).
    assert_eq!(
        sessions[0]["id"].as_str().unwrap(),
        "cc-bbb",
        "cc-bbb should be first (newer last_ts)"
    );
    assert_eq!(
        sessions[1]["id"].as_str().unwrap(),
        "cc-aaa",
        "cc-aaa should be second (older last_ts)"
    );

    assert_eq!(
        sessions[0]["fragment_count"].as_u64().unwrap(),
        2,
        "cc-bbb has 2 active fragments"
    );
    assert_eq!(
        sessions[1]["fragment_count"].as_u64().unwrap(),
        1,
        "cc-aaa has 1 active fragment"
    );
}

/// `project_cwd`, `src_session_uuid`, and `last_ts` must round-trip correctly
/// through the metadata sidecar.
#[tokio::test]
async fn metadata_fields_round_trip() {
    let server = make_server().await;

    store(
        &server,
        "cc-meta-trip",
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

    assert_eq!(s["id"].as_str().unwrap(), "cc-meta-trip");
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
        "cc-discard-test",
        "fragment A — will be soft-deleted",
        json!({
            "project_cwd": "/bar",
            "ts": "2026-05-20T08:00:00Z",
        }),
    )
    .await;

    store(
        &server,
        "cc-discard-test",
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
    soft_discard(&server, "cc-discard-test", &id_a).await;

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

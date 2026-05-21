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

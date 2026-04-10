//! Integration tests for the seven-tool memory API.
//! These exercise the substantive (post-W9 wiring) handlers in
//! `src/api/tools.rs` against the v0.1.0 in-memory attractor store. They
//! cover the round-trip contract:
//! 1. `store` → returns an attractor_id
//! 2. `retrieve` → finds the stored content by semantic similarity
//! 3. `update` → mutates importance + content
//! 4. `discard` (soft) → marks attractor deleted; retrieve no longer surfaces it
//! 5. `reconstruct` → returns top-depth fragments stitched into reconstructed_content
//! 6. `resonate` → returns activations above threshold
//! Run with: `cargo test --test seven_tools_api`

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::ContextNestServices;
use serde_json::json;

async fn make_server() -> TestServer {
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = create_simple_app(services)
        .await
        .expect("seven-tool app should build");
    TestServer::new(app).expect("test server should start")
}

#[tokio::test]
async fn store_returns_attractor_id() {
    let server = make_server().await;

    let res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "the quick brown fox jumps over the lazy dog",
            "importance": 0.7,
            "session_id": "test-session-1",
        }))
        .await;

    res.assert_status_ok();
    let body: serde_json::Value = res.json();
    assert_eq!(body["stored"], true);
    assert!(body["attractor_id"].is_string());
}

#[tokio::test]
async fn store_then_retrieve_returns_the_fragment() {
    let server = make_server().await;
    let session = "test-session-roundtrip";

    let store_res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "pomegranate seeds are red and juicy",
            "importance": 0.8,
            "session_id": session,
        }))
        .await;
    store_res.assert_status_ok();
    let stored: serde_json::Value = store_res.json();
    let attractor_id = stored["attractor_id"].as_str().unwrap().to_string();

    let retrieve_res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": "pomegranate seeds are red and juicy",
            "top_k": 3,
            "session_id": session,
        }))
        .await;
    retrieve_res.assert_status_ok();
    let body: serde_json::Value = retrieve_res.json();
    let hits = body["hits"].as_array().expect("hits should be an array");
    assert!(!hits.is_empty(), "expected at least one hit");
    assert_eq!(hits[0]["id"].as_str().unwrap(), attractor_id);
}

#[tokio::test]
async fn retrieve_on_empty_session_returns_no_hits() {
    let server = make_server().await;

    let res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": "anything",
            "session_id": "never-stored-in",
        }))
        .await;

    res.assert_status_ok();
    let body: serde_json::Value = res.json();
    assert_eq!(body["hits"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn update_changes_importance() {
    let server = make_server().await;
    let session = "test-session-update";

    let store_res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "blue suede shoes",
            "importance": 0.3,
            "session_id": session,
        }))
        .await;
    let attractor_id = store_res.json::<serde_json::Value>()["attractor_id"]
        .as_str()
        .unwrap()
        .to_string();

    let update_res = server
        .post("/api/v1/tools/update")
        .json(&json!({
            "attractor_id": attractor_id,
            "importance": 0.95,
            "session_id": session,
        }))
        .await;
    update_res.assert_status_ok();
    assert_eq!(update_res.json::<serde_json::Value>()["updated"], true);

    let retrieve_res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": "blue suede shoes",
            "top_k": 1,
            "session_id": session,
        }))
        .await;
    let hits = retrieve_res.json::<serde_json::Value>()["hits"].clone();
    assert!(
        (hits[0]["importance"].as_f64().unwrap() - 0.95).abs() < 0.001,
        "importance should be bumped to 0.95, was {:?}",
        hits[0]["importance"]
    );
}

#[tokio::test]
async fn update_unknown_id_returns_not_found() {
    let server = make_server().await;

    let res = server
        .post("/api/v1/tools/update")
        .json(&json!({
            "attractor_id": "00000000-0000-0000-0000-000000000000",
            "importance": 1.0,
            "session_id": "any",
        }))
        .await;

    res.assert_status_not_found();
    assert_eq!(res.json::<serde_json::Value>()["updated"], false);
}

#[tokio::test]
async fn discard_soft_marks_attractor_deleted() {
    let server = make_server().await;
    let session = "test-session-discard";

    let store_res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "ephemeral fragment",
            "importance": 0.6,
            "session_id": session,
        }))
        .await;
    let attractor_id = store_res.json::<serde_json::Value>()["attractor_id"]
        .as_str()
        .unwrap()
        .to_string();

    let discard_res = server
        .post("/api/v1/tools/discard")
        .json(&json!({
            "attractor_id": attractor_id,
            "soft_delete": true,
            "reason": "test cleanup",
            "session_id": session,
        }))
        .await;
    discard_res.assert_status_ok();
    let body: serde_json::Value = discard_res.json();
    assert_eq!(body["discarded"], true);
    assert_eq!(body["soft_delete"], true);

    // Soft-deleted fragments must not appear in retrieve results.
    let retrieve_res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": "ephemeral fragment",
            "top_k": 5,
            "session_id": session,
        }))
        .await;
    let hits = retrieve_res.json::<serde_json::Value>()["hits"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(
        hits.len(),
        0,
        "soft-deleted fragments must be hidden from retrieve"
    );
}

#[tokio::test]
async fn discard_hard_removes_attractor() {
    let server = make_server().await;
    let session = "test-session-hard-discard";

    let store_res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "permadelete me",
            "session_id": session,
        }))
        .await;
    let attractor_id = store_res.json::<serde_json::Value>()["attractor_id"]
        .as_str()
        .unwrap()
        .to_string();

    let discard_res = server
        .post("/api/v1/tools/discard")
        .json(&json!({
            "attractor_id": attractor_id,
            "soft_delete": false,
            "session_id": session,
        }))
        .await;
    discard_res.assert_status_ok();

    let summarize_res = server
        .post("/api/v1/tools/summarize")
        .json(&json!({
            "session_id": session,
        }))
        .await;
    assert_eq!(summarize_res.json::<serde_json::Value>()["merged_count"], 0);
}

#[tokio::test]
async fn reconstruct_returns_stitched_content() {
    let server = make_server().await;
    let session = "test-session-reconstruct";

    for content in [
        "cats sleep about sixteen hours a day",
        "lions roar to mark territory at dawn",
        "tigers swim better than most big cats",
    ] {
        server
            .post("/api/v1/tools/store")
            .json(&json!({
                "content": content,
                "importance": 0.6,
                "session_id": session,
            }))
            .await
            .assert_status_ok();
    }

    let res = server
        .post("/api/v1/tools/reconstruct")
        .json(&json!({
            "query": "feline behavior",
            "depth": 3,
            "session_id": session,
        }))
        .await;
    res.assert_status_ok();
    let body: serde_json::Value = res.json();
    let content = body["reconstructed_content"].as_str().unwrap();
    assert!(
        !content.is_empty(),
        "reconstruction should return non-empty content"
    );
    let ids = body["source_fragment_ids"].as_array().unwrap();
    assert!(!ids.is_empty(), "should return at least one source id");
    assert_eq!(body["gaps_filled"], 0, "v0.1.0 reports no filled gaps");
}

#[tokio::test]
async fn resonate_returns_activations_above_threshold() {
    let server = make_server().await;
    let session = "test-session-resonate";

    server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "harmonic field activation",
            "importance": 0.7,
            "session_id": session,
        }))
        .await
        .assert_status_ok();

    let res = server
        .post("/api/v1/tools/resonate")
        .json(&json!({
            "pattern": "harmonic field activation",
            "threshold": 0.1,
            "session_id": session,
        }))
        .await;
    res.assert_status_ok();
    let body: serde_json::Value = res.json();
    let activations = body["activations"].as_array().unwrap();
    assert!(
        !activations.is_empty(),
        "expected at least one resonant activation"
    );
    assert!(
        body["field_coherence"].as_f64().unwrap() >= 0.0,
        "field_coherence should be non-negative"
    );
}

// =============================================================================
// Phase H additions — cross-session isolation, no-op update, ownership checks
// =============================================================================

/// Phase H load-bearing invariant: session A's `retrieve` must never see
/// fragments stored under session B. If `SessionIndex.list_active` ever
/// regressed to a global scan, this test catches it.
#[tokio::test]
async fn retrieve_isolates_sessions() {
    let server = make_server().await;

    // Store under owner.
    server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "owner-secret-payload xyzzy",
            "session_id": "isolation-owner",
        }))
        .await
        .assert_status_ok();

    // Retrieve from a different session — same query that *would* match the
    // owner-secret-payload if the index leaked.
    let res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": "owner-secret-payload xyzzy",
            "top_k": 10,
            "session_id": "isolation-other",
        }))
        .await;
    res.assert_status_ok();
    let hits = res.json::<serde_json::Value>()["hits"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        hits.is_empty(),
        "session 'isolation-other' must not see fragments owned by 'isolation-owner'; got {} hit(s)",
        hits.len(),
    );
}

/// Cross-session `update` must 404 — only the owning session can mutate.
#[tokio::test]
async fn update_wrong_session_returns_not_found() {
    let server = make_server().await;

    let stored = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "guarded payload",
            "session_id": "update-owner",
        }))
        .await;
    let attractor_id = stored.json::<serde_json::Value>()["attractor_id"]
        .as_str()
        .unwrap()
        .to_string();

    let res = server
        .post("/api/v1/tools/update")
        .json(&json!({
            "attractor_id": attractor_id,
            "importance": 0.99,
            "session_id": "update-attacker",
        }))
        .await;
    res.assert_status(axum::http::StatusCode::NOT_FOUND);
}

/// Cross-session `discard` must 404 with the same semantics as `update`.
#[tokio::test]
async fn discard_wrong_session_returns_not_found() {
    let server = make_server().await;

    let stored = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "guarded payload",
            "session_id": "discard-owner",
        }))
        .await;
    let attractor_id = stored.json::<serde_json::Value>()["attractor_id"]
        .as_str()
        .unwrap()
        .to_string();

    let res = server
        .post("/api/v1/tools/discard")
        .json(&json!({
            "attractor_id": attractor_id,
            "soft_delete": false,
            "session_id": "discard-attacker",
        }))
        .await;
    res.assert_status(axum::http::StatusCode::NOT_FOUND);
}

/// Sending `update` with neither `importance` nor `content` is a documented
/// no-op idempotent path — must return 200 with `updated: false`, not 400 / 404.
#[tokio::test]
async fn update_noop_returns_ok_with_updated_false() {
    let server = make_server().await;

    let stored = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "anything",
            "session_id": "noop-session",
        }))
        .await;
    let attractor_id = stored.json::<serde_json::Value>()["attractor_id"]
        .as_str()
        .unwrap()
        .to_string();

    let res = server
        .post("/api/v1/tools/update")
        .json(&json!({
            "attractor_id": attractor_id,
            "session_id": "noop-session",
        }))
        .await;
    res.assert_status_ok();
    assert_eq!(res.json::<serde_json::Value>()["updated"], false);
}

/// After hard-discard, the fragment must be invisible to `retrieve` (not just
/// to `summarize`). Catches the index-vs-manager drift case where one store
/// is cleaned and the other isn't.
#[tokio::test]
async fn hard_discard_removes_from_retrieve() {
    let server = make_server().await;
    let session = "hard-retrieve-session";

    let stored = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "fragment to be hard-deleted",
            "session_id": session,
        }))
        .await;
    let attractor_id = stored.json::<serde_json::Value>()["attractor_id"]
        .as_str()
        .unwrap()
        .to_string();

    server
        .post("/api/v1/tools/discard")
        .json(&json!({
            "attractor_id": attractor_id,
            "soft_delete": false,
            "session_id": session,
        }))
        .await
        .assert_status_ok();

    let res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": "fragment to be hard-deleted",
            "top_k": 5,
            "session_id": session,
        }))
        .await;
    res.assert_status_ok();
    let hits = res.json::<serde_json::Value>()["hits"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        hits.is_empty(),
        "hard-discarded fragment must not appear in retrieve results; got {} hit(s)",
        hits.len(),
    );
}

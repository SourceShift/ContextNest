//! Integration test for the real-time Claude Code hook receiver.
//!
//! Exercises the contract added in this PR:
//!
//! 1. `POST /api/v1/cc/hook/session_start` returns 204 fast and is a no-op
//!    on the substrate (memory store remains empty until a tail fires).
//! 2. `POST /api/v1/cc/hook/user_prompt_submit` with a real
//!    `transcript_path` triggers an async tail+ingest that lands memories
//!    in the substrate. `retrieve` with a metadata filter finds them
//!    within a polling window.
//! 3. Re-firing the same `user_prompt_submit` payload is idempotent —
//!    no new memories appear because the byte-offset tracker says the
//!    file hasn't grown.
//! 4. `POST /api/v1/cc/hook/task_completed` writes one accomplishment
//!    memory without touching any `.jsonl` (no `transcript_path` needed).
//! 5. Unknown event names return 404 (cheap signal that someone misspelled
//!    the hook URL during installation).

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::time::sleep;

const FIXTURE_PATH: &str = "tests/fixtures/cc_session_sample.jsonl";
const FIXTURE_SESSION_UUID: &str = "abc12345-fixture-session";
// Substrate uses the bare Claude Code session UUID as the canonical
// session_id — no `cc-` prefix, no first-8 truncation — so the
// substrate-side value equals the fixture UUID byte-for-byte.
const FIXTURE_SUBSTRATE_SESSION: &str = "abc12345-fixture-session";

async fn make_server() -> TestServer {
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = create_simple_app(services)
        .await
        .expect("simple app should build with cc_hooks routes mounted");
    TestServer::new(app).expect("test server should start")
}

fn fixture_path() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest).join(FIXTURE_PATH)
}

/// Hit /retrieve until at least `min_hits` results come back, or the
/// deadline elapses. Returns the most recent JSON response. The hook
/// receiver dispatches work into `tokio::spawn`, so callers can't
/// assume memories are visible the instant 204 returns.
async fn poll_retrieve(
    server: &TestServer,
    session_id: &str,
    metadata_filter: Value,
    min_hits: usize,
    max_wait: Duration,
) -> Value {
    let deadline = Instant::now() + max_wait;
    let mut last = json!({"hits": []});
    while Instant::now() < deadline {
        let res = server
            .post("/api/v1/tools/retrieve")
            .json(&json!({
                "query": "what does Claude need from me?",
                "top_k": 50,
                "session_id": session_id,
                "metadata_filter": metadata_filter,
            }))
            .await;
        last = res.json::<Value>();
        let hits = last
            .get("hits")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        if hits >= min_hits {
            return last;
        }
        sleep(Duration::from_millis(50)).await;
    }
    last
}

#[tokio::test]
async fn session_start_returns_204_and_is_noop() {
    let server = make_server().await;
    let res = server
        .post("/api/v1/cc/hook/session_start")
        .json(&json!({
            "session_id": FIXTURE_SESSION_UUID,
            "cwd": "/work/example",
            "hook_event_name": "SessionStart",
        }))
        .await;
    res.assert_status(axum::http::StatusCode::NO_CONTENT);

    // Substrate should hold no memories for this session yet.
    let hits = poll_retrieve(
        &server,
        FIXTURE_SUBSTRATE_SESSION,
        json!({"kind": "user_action"}),
        1,
        Duration::from_millis(200),
    )
    .await;
    let count = hits
        .get("hits")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    assert_eq!(count, 0, "session_start must not write memories on its own");
}

#[tokio::test]
async fn user_prompt_submit_tails_and_ingests_memories() {
    let server = make_server().await;
    let transcript = fixture_path();
    assert!(
        transcript.exists(),
        "fixture transcript must exist at {}",
        transcript.display()
    );

    // Register the session first — production hook layout always fires
    // SessionStart before any UserPromptSubmit. Without it, the tracker
    // implicitly starts at 0 anyway, so this is belt-and-suspenders.
    let res = server
        .post("/api/v1/cc/hook/session_start")
        .json(&json!({
            "session_id": FIXTURE_SESSION_UUID,
            "cwd": "/work/example",
            "hook_event_name": "SessionStart",
        }))
        .await;
    res.assert_status(axum::http::StatusCode::NO_CONTENT);

    // Now fire user_prompt_submit with the fixture transcript path.
    // The async tail+ingest should extract user_action + decision
    // memories from the fixture's z-insight blocks.
    let res = server
        .post("/api/v1/cc/hook/user_prompt_submit")
        .json(&json!({
            "session_id": FIXTURE_SESSION_UUID,
            "cwd": "/work/example",
            "transcript_path": transcript.to_string_lossy(),
            "hook_event_name": "UserPromptSubmit",
        }))
        .await;
    res.assert_status(axum::http::StatusCode::NO_CONTENT);

    // Wait for the async ingest. The fixture has two user_action
    // entries (in the second z-insight block).
    let hits = poll_retrieve(
        &server,
        FIXTURE_SUBSTRATE_SESSION,
        json!({"kind": "user_action"}),
        2,
        Duration::from_secs(3),
    )
    .await;
    let user_action_hits = hits
        .get("hits")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    assert!(
        user_action_hits >= 2,
        "expected ≥2 user_action memories from the fixture, got {} :: {}",
        user_action_hits,
        serde_json::to_string_pretty(&hits).unwrap_or_default()
    );

    // The fixture's second z-insight has awaiting_decision=true; that
    // memory should be queryable via the same metadata-filter pattern
    // `contextnest inbox` uses.
    let decisions = poll_retrieve(
        &server,
        FIXTURE_SUBSTRATE_SESSION,
        json!({"kind": "decision", "awaiting_decision": true}),
        1,
        Duration::from_secs(2),
    )
    .await;
    let decision_hits = decisions
        .get("hits")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    assert!(
        decision_hits >= 1,
        "expected ≥1 awaiting_decision memory, got {} :: {}",
        decision_hits,
        serde_json::to_string_pretty(&decisions).unwrap_or_default()
    );
}

#[tokio::test]
async fn re_firing_user_prompt_submit_is_idempotent() {
    let server = make_server().await;
    let transcript = fixture_path();

    // First fire — populates memories.
    let _ = server
        .post("/api/v1/cc/hook/session_start")
        .json(&json!({
            "session_id": FIXTURE_SESSION_UUID,
            "cwd": "/work/example"
        }))
        .await;
    let _ = server
        .post("/api/v1/cc/hook/user_prompt_submit")
        .json(&json!({
            "session_id": FIXTURE_SESSION_UUID,
            "cwd": "/work/example",
            "transcript_path": transcript.to_string_lossy(),
        }))
        .await;
    let first = poll_retrieve(
        &server,
        FIXTURE_SUBSTRATE_SESSION,
        json!({"kind": "user_action"}),
        2,
        Duration::from_secs(3),
    )
    .await;
    let first_count = first
        .get("hits")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    assert!(first_count >= 2, "first ingest must populate memories");

    // Second fire — no new content in the file, byte-offset tracker
    // says "nothing to read". Memory count must NOT grow.
    let _ = server
        .post("/api/v1/cc/hook/user_prompt_submit")
        .json(&json!({
            "session_id": FIXTURE_SESSION_UUID,
            "cwd": "/work/example",
            "transcript_path": transcript.to_string_lossy(),
        }))
        .await;
    // Give the spawn a chance to (not) do anything.
    sleep(Duration::from_millis(500)).await;
    let second = poll_retrieve(
        &server,
        FIXTURE_SUBSTRATE_SESSION,
        json!({"kind": "user_action"}),
        first_count + 1,
        Duration::from_millis(800),
    )
    .await;
    let second_count = second
        .get("hits")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    assert_eq!(
        second_count, first_count,
        "re-firing the same payload must be idempotent (got {} → {})",
        first_count, second_count
    );
}

#[tokio::test]
async fn subagent_stop_triggers_ingest_and_lands_fragments() {
    // Regression: pre-fix, subagent_stop was a no-op debug log.
    // Task-spawned subagents have their own transcript JSONL and emit
    // z-insight blocks just like the parent — but ContextNest threw
    // them away. This test exercises the new dispatch arm.
    //
    // The substrate's `/api/v1/tools/retrieve` is the universal
    // visibility surface for stored fragments — after the sidecar
    // fallback added in this PR it returns hits (with similarity=0)
    // for ServicesSink-stored fragments even though those don't go
    // through the canonical attractor pipeline. Polling /retrieve
    // here keeps this test self-contained within the substrate
    // hardening PR.
    let server = make_server().await;
    let transcript = fixture_path();
    assert!(
        transcript.exists(),
        "fixture transcript must exist at {}",
        transcript.display()
    );

    // A subagent has its OWN session_id, distinct from the parent's.
    let sub_session_uuid = "subagent01-fake-session-id";

    // Register subagent session start (production hook layout always
    // fires SessionStart before SubagentStop). Belt-and-suspenders.
    server
        .post("/api/v1/cc/hook/session_start")
        .json(&json!({
            "session_id": sub_session_uuid,
            "cwd": "/work/example",
            "hook_event_name": "SessionStart",
        }))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    // Fire subagent_stop. With the fix, this triggers tail_and_ingest
    // on the supplied transcript — the same code path user_prompt_submit
    // / stop use. Without the fix, it is a no-op and no fragments land.
    server
        .post("/api/v1/cc/hook/subagent_stop")
        .json(&json!({
            "session_id": sub_session_uuid,
            "cwd": "/work/example",
            "transcript_path": transcript.to_string_lossy(),
            "hook_event_name": "SubagentStop",
        }))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    // The fixture's z-insight blocks include user_action entries.
    // Look for them under the subagent's substrate session id —
    // canonical session_id is the bare Claude Code session UUID.
    let sub_substrate_session = sub_session_uuid.to_string();
    let hits = poll_retrieve(
        &server,
        &sub_substrate_session,
        json!({"kind": "user_action"}),
        1,
        Duration::from_secs(3),
    )
    .await;
    let count = hits
        .get("hits")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    assert!(
        count >= 1,
        "subagent_stop must have produced at least one user_action fragment for {sub_substrate_session} (got {count} :: {hits})"
    );
}

#[tokio::test]
async fn task_completed_stores_accomplishment_without_transcript() {
    let server = make_server().await;
    let res = server
        .post("/api/v1/cc/hook/task_completed")
        .json(&json!({
            "session_id": FIXTURE_SESSION_UUID,
            "cwd": "/work/example",
            "subject": "Fix mermaid parse error",
            "task_id": "T-7",
            "hook_event_name": "TaskCompleted",
        }))
        .await;
    res.assert_status(axum::http::StatusCode::NO_CONTENT);

    let hits = poll_retrieve(
        &server,
        FIXTURE_SUBSTRATE_SESSION,
        json!({"kind": "accomplishment", "source": "TaskCompleted"}),
        1,
        Duration::from_secs(2),
    )
    .await;
    let count = hits
        .get("hits")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    assert!(
        count >= 1,
        "expected ≥1 accomplishment memory from TaskCompleted, got {} :: {}",
        count,
        serde_json::to_string_pretty(&hits).unwrap_or_default()
    );
}

#[tokio::test]
async fn unknown_event_returns_404() {
    let server = make_server().await;
    let res = server
        .post("/api/v1/cc/hook/totally_made_up_event")
        .json(&json!({"session_id": "x"}))
        .await;
    res.assert_status(axum::http::StatusCode::NOT_FOUND);
}

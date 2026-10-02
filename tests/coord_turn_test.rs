//! Concord turn-hook end-to-end suite.
//!
//! Drives `/api/v1/coord/turn` through `create_simple_app` plus
//! `axum_test::TestServer`. Verifies:
//!
//! - dod1: explicit binding with deliver-once semantics
//! - dod2: lineage resolution (pane, tty, "??", session fallback)
//! - dod3: invalid `X-Principale` header ignored (fall-through)
//! - dod4: `hookEventName` echo; 200 + empty context on store error
//!   and on malformed body
//! - dod5: pretool lease identity through the binding

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::coord_store::{CoordStore, PrincipalUpsert};
use contextnest::services::ContextNestServices;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

async fn make_default_server() -> TestServer {
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = create_simple_app(services)
        .await
        .expect("simple app should build with coord_turn routes mounted");
    TestServer::new(app).expect("test server should start")
}

async fn make_server_with_db(db_path: PathBuf) -> (TestServer, TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join(
        db_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("coord.db")),
    );
    // Pre-create the store at the target path so CoordStore::open succeeds.
    let _ = CoordStore::open(&path).expect("pre-open file-backed store");

    let mut services = ContextNestServices::new_default()
        .await
        .expect("default services");
    services.coord_store = Arc::new(CoordStore::open(&path).expect("reopen store"));
    let app = create_simple_app(services).await.expect("simple app");
    (TestServer::new(app).expect("test server"), tmp)
}

fn url_encode(s: &str) -> String {
    // `:` is the only unsafe char we send through the URL; this matches
    // the encoding coord_principals_test uses.
    s.replace(':', "%3A")
}

// ────────────── dod1 — explicit binding + deliver-once ──────────────

#[tokio::test]
async fn dod1_explicit_principal_binds_and_subsequent_turns_get_no_messages() {
    let server = make_default_server().await;
    let pid = "loop:amir-dod1";
    let sid = "session-dod1";

    // 1. First turn with X-Concord-Principal → bound to loop:amir-dod1.
    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", pid)
        .json(&json!({
            "session_id": sid,
            "cwd": "/work",
            "hook_event_name": "SessionStart",
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["principal_id"], pid);
    assert_eq!(body["bound"], true);
    assert_eq!(body["hookSpecificOutput"]["hookEventName"], "SessionStart");

    // 2. Drop a message into loop:amir-dod1's mailbox.
    let res = server
        .post(&format!(
            "/api/v1/coord/principals/{}/messages",
            url_encode(pid)
        ))
        .json(&json!({"from": "human:amir", "body": "stay off config"}))
        .await;
    res.assert_status(axum::http::StatusCode::CREATED);

    // 3. Second turn (same session_id) → delivered=[M-id], context has
    //    the body and the Ack line.
    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", pid)
        .json(&json!({
            "session_id": sid,
            "cwd": "/work",
            "hook_event_name": "UserPromptSubmit",
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    let delivered = body["delivered"].as_array().expect("delivered array");
    assert_eq!(
        delivered.len(),
        1,
        "first delivery must grab exactly one msg"
    );
    let msg_id = delivered[0].as_str().unwrap().to_string();
    assert!(msg_id.starts_with("M-"), "delivered id looks like M-1 etc");
    let ctx = body["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(ctx.contains("stay off config"), "body in context: {ctx}");
    assert!(
        ctx.contains(&format!("Ack: mini-ork concord ack {pid} {msg_id}")),
        "Ack line present: {ctx}",
    );

    // 4. Third turn → delivered=[] and context is empty (deliver-once).
    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", pid)
        .json(&json!({
            "session_id": sid,
            "cwd": "/work",
            "hook_event_name": "UserPromptSubmit",
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(
        body["delivered"].as_array().map(|a| a.len()).unwrap_or(0),
        0,
        "deliver-once: no second delivery"
    );
    assert_eq!(
        body["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap(),
        ""
    );
}

#[tokio::test]
async fn dod1_explicit_principal_other_session_gets_no_messages() {
    // S2 binds to the same explicit principal as S1 but didn't fire the
    // first delivery, so it gets nothing.
    let server = make_default_server().await;
    let pid = "loop:shared-dod1";
    let sid1 = "session-s1";
    let sid2 = "session-s2";

    // S1 binds and consumes.
    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", pid)
        .json(&json!({"session_id": sid1, "hook_event_name": "SessionStart"}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);

    // Drop a message.
    let res = server
        .post(&format!(
            "/api/v1/coord/principals/{}/messages",
            url_encode(pid)
        ))
        .json(&json!({"from": "h", "body": "two"}))
        .await;
    res.assert_status(axum::http::StatusCode::CREATED);

    // S1 grabs it.
    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", pid)
        .json(&json!({"session_id": sid1, "hook_event_name": "UserPromptSubmit"}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(
        body["delivered"].as_array().map(|a| a.len()).unwrap_or(0),
        1
    );

    // S2 binds to the same principal, must see nothing left.
    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", pid)
        .json(&json!({"session_id": sid2, "hook_event_name": "SessionStart"}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(
        body["delivered"].as_array().map(|a| a.len()).unwrap_or(0),
        0,
        "S2 sees no messages (S1 already claimed them)"
    );
}

#[tokio::test]
async fn dod1_explicit_binding_is_visible_via_get_binding() {
    let server = make_default_server().await;
    let pid = "loop:dod1-binding";
    let sid = "session-binding-1";

    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", pid)
        .json(&json!({"session_id": sid, "hook_event_name": "SessionStart"}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);

    let res = server
        .get(&format!("/api/v1/coord/bindings/{}", url_encode(sid)))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(
        body.get("principal_id").and_then(|v| v.as_str()),
        Some(pid),
        "binding must surface the explicit principal"
    );
}

// ────────────── dod2 — lineage resolution ──────────────

#[tokio::test]
async fn dod2_lineage_pane_term_derives_session_pane_at_repo() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir repo");
    std::fs::write(repo.join(".git"), b"gitdir: /tmp/x\n").expect(".git file");
    let sub = repo.join("sub");
    std::fs::create_dir(&sub).expect("mkdir sub");

    // Build a server whose services have no specific cwd awareness
    // (we drive via headers). The repo_slug helper walks ancestors of
    // `cwd`, so we pass the inner-sub cwd.
    let server = make_default_server().await;
    let cwd = sub.to_string_lossy().to_string();

    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Pane", "%94")
        .json(&json!({
            "session_id": "s-A",
            "hook_event_name": "SessionStart",
            "cwd": cwd,
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["principal_id"], "session:p94@repo");
    assert_eq!(body["bound"], true);
}

#[tokio::test]
async fn dod2_lineage_pane_term_shared_across_two_session_ids() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir");
    std::fs::write(repo.join(".git"), b"gitdir: /tmp/x\n").expect(".git");
    let cwd = repo.to_string_lossy().to_string();

    let server = make_default_server().await;

    // Two different session ids, same pane + cwd → same principal.
    let mut principals = Vec::new();
    for sid in ["session-share-a", "session-share-b"] {
        let res = server
            .post("/api/v1/coord/turn")
            .add_header("X-Concord-Pane", "%7")
            .json(
                &json!({"session_id": sid, "hook_event_name": "SessionStart", "cwd": cwd.clone()}),
            )
            .await;
        res.assert_status(axum::http::StatusCode::OK);
        let body: Value = res.json();
        principals.push(body["principal_id"].as_str().unwrap().to_string());
    }
    assert_eq!(
        principals[0], principals[1],
        "two session_ids sharing pane+cwd collapse to one principal"
    );
    assert_eq!(principals[0], "session:p7@repo");
}

#[tokio::test]
async fn dod2_lineage_tty_only_resolves_via_tty_term() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir");
    std::fs::write(repo.join(".git"), b"gitdir: /tmp/x\n").expect(".git");
    let cwd = repo.to_string_lossy().to_string();

    let server = make_default_server().await;
    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Tty", "ttys003")
        .json(&json!({"session_id": "s-tty", "hook_event_name": "SessionStart", "cwd": cwd}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["principal_id"], "session:tty-ttys003@repo");
}

#[tokio::test]
async fn dod2_lineage_no_pane_no_tty_falls_back_to_session_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir");
    std::fs::write(repo.join(".git"), b"gitdir: /tmp/x\n").expect(".git");
    let cwd = repo.to_string_lossy().to_string();

    let server = make_default_server().await;
    let res = server
        .post("/api/v1/coord/turn")
        .json(&json!({"session_id": "s-fb-1", "hook_event_name": "SessionStart", "cwd": cwd}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["principal_id"], "session:s-fb-1");
}

#[tokio::test]
async fn dod2_lineage_tty_question_only_falls_through_to_session_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir");
    std::fs::write(repo.join(".git"), b"gitdir: /tmp/x\n").expect(".git");
    let cwd = repo.to_string_lossy().to_string();

    let server = make_default_server().await;
    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Tty", "??")
        .json(&json!({"session_id": "s-q", "hook_event_name": "SessionStart", "cwd": cwd}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    // `??` is the macOS "no tty" marker; tty_term returns None; falls
    // through to session:<sid>.
    assert_eq!(body["principal_id"], "session:s-q");
}

// ────────────── dod3 — invalid header fall-through ──────────────

#[tokio::test]
async fn dod3_invalid_principal_header_falls_through_to_lineage() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir");
    std::fs::write(repo.join(".git"), b"gitdir: /tmp/x\n").expect(".git");
    let cwd = repo.to_string_lossy().to_string();

    let server = make_default_server().await;
    for bad in ["bogus", "loop:", "loop", "human:"] {
        let res = server
            .post("/api/v1/coord/turn")
            .add_header("X-Concord-Principal", bad)
            .add_header("X-Concord-Pane", "%7")
            .json(&json!({
                "session_id": format!("s-bad-{bad}"),
                "hook_event_name": "SessionStart",
                "cwd": cwd.clone(),
            }))
            .await;
        res.assert_status(axum::http::StatusCode::OK);
        let body: Value = res.json();
        assert_eq!(
            body["principal_id"], "session:p7@repo",
            "invalid header {bad:?} must fall through to lineage",
        );
        assert_eq!(body["bound"], true);
    }
}

// ────────────── dod4 — hookEventName echo + 200 with empty context on error ──────────────

#[tokio::test]
async fn dod4_hook_event_name_is_echoed_for_both_events() {
    let server = make_default_server().await;
    for event in ["SessionStart", "UserPromptSubmit"] {
        let res = server
            .post("/api/v1/coord/turn")
            .add_header("X-Concord-Principal", "loop:echo")
            .json(&json!({"session_id": format!("s-{event}"), "hook_event_name": event}))
            .await;
        res.assert_status(axum::http::StatusCode::OK);
        let body: Value = res.json();
        assert_eq!(
            body["hookSpecificOutput"]["hookEventName"], event,
            "event must be echoed verbatim"
        );
    }
}

#[tokio::test]
async fn dod4_store_error_yields_200_with_empty_context() {
    // Use a file-backed store, then drop its tables out from under it.
    let (server, _tmp) = make_server_with_db(PathBuf::from("dod4.db")).await;

    // Drop the tables through a second connection.
    {
        let db_path = _tmp.path().join("dod4.db");
        let conn = Connection::open(&db_path).expect("second conn");
        conn.execute_batch(
            "DROP TABLE IF EXISTS messages; \
             DROP TABLE IF EXISTS bindings; \
             DROP TABLE IF EXISTS principals;",
        )
        .expect("drop tables");
    }

    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", "loop:dod4")
        .json(&json!({"session_id": "s-dod4", "hook_event_name": "SessionStart"}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["bound"], false, "store error → bound=false");
    assert_eq!(
        body["delivered"].as_array().map(|a| a.len()).unwrap_or(0),
        0,
    );
    assert_eq!(
        body["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap(),
        ""
    );
}

#[tokio::test]
async fn dod4_malformed_body_yields_200_with_empty_context() {
    // Malformed JSON body + empty session_id → no header resolves an
    // identity, no body carries one → handler returns 200 with
    // bound=false and empty context (NEVER a 4xx or a parse error
    // leak). Note that even with a malformed body, an explicit
    // X-Concord-Principal header WOULD successfully bind — that's the
    // header-driven path, separate from the body path.
    let server = make_default_server().await;
    let res = server
        .post("/api/v1/coord/turn")
        .text("this is not json at all {{{")
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["bound"], false);
    assert_eq!(body["principal_id"], Value::Null);
    assert_eq!(
        body["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap(),
        ""
    );
}

#[tokio::test]
async fn dod4_empty_session_id_returns_200_bound_false() {
    let server = make_default_server().await;
    let res = server
        .post("/api/v1/coord/turn")
        .json(&json!({"session_id": "", "hook_event_name": "SessionStart"}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["bound"], false);
    assert_eq!(body["principal_id"], Value::Null);
    assert_eq!(
        body["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap(),
        ""
    );
}

// ────────────── dod5 — pretool lease identity through binding ──────────────

#[tokio::test]
async fn dod5_bound_session_holds_lease_for_its_principal() {
    let server = make_default_server().await;
    let pid = "loop:dod5";
    let sid = "s-dod5";

    // Bind via /coord/turn.
    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", pid)
        .json(&json!({"session_id": sid, "hook_event_name": "SessionStart"}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);

    // Acquire a strict lease on src/a.rs as the principal.
    let res = server
        .post("/api/v1/coord/lease")
        .json(&json!({
            "agent_id": pid,
            "paths": ["src/a.rs"],
            "mode": "write",
            "strict": true,
            "ttl_secs": 120,
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);

    // Pretool call → 'allow' (the bound session IS the lease holder).
    let res = server
        .post("/api/v1/cc/pretool")
        .json(&json!({
            "session_id": sid,
            "tool_name": "Edit",
            "tool_input": {"file_path": "src/a.rs"},
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    let decision = body["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .unwrap();
    assert_eq!(
        decision, "allow",
        "holder must not block itself, body: {body}"
    );
    let ctx = body["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    assert!(!ctx.contains("WAIT"), "holder must not see WAIT: {ctx}",);
    assert!(
        !ctx.contains("hard block"),
        "holder must not see hard block: {ctx}",
    );
}

#[tokio::test]
async fn dod5_unbound_session_keeps_legacy_deny_behaviour() {
    // Unbound session: the pretool gate falls back to session_id as
    // the lease agent. A lease acquired by THAT agent (the session_id)
    // is honoured. To prove the regression-free path, an unbound
    // session whose session_id matches a lease holder still allows;
    // an unbound session whose session_id is fresh still allows (no
    // lease on file).
    let server = make_default_server().await;

    // Fresh unbound session → no lease on file → 'allow'.
    let res = server
        .post("/api/v1/cc/pretool")
        .json(&json!({
            "session_id": "fresh-session-never-bound",
            "tool_name": "Edit",
            "tool_input": {"file_path": "src/never.rs"},
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    let decision = body["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .unwrap();
    assert_eq!(
        decision, "allow",
        "fresh unbound session → no lease → allow (unchanged)"
    );

    // Acquire a STRICT lease as the raw session_id (the legacy path).
    let res = server
        .post("/api/v1/coord/lease")
        .json(&json!({
            "agent_id": "fresh-session-never-bound",
            "paths": ["src/legacy.rs"],
            "mode": "write",
            "strict": true,
            "ttl_secs": 120,
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);

    // That same session id → still allow (it's the holder).
    let res = server
        .post("/api/v1/cc/pretool")
        .json(&json!({
            "session_id": "fresh-session-never-bound",
            "tool_name": "Edit",
            "tool_input": {"file_path": "src/legacy.rs"},
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    let decision = body["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .unwrap();
    assert_eq!(decision, "allow", "unbound holder's own lease is honoured");

    // A different unbound session attempts the same file → denied.
    let res = server
        .post("/api/v1/cc/pretool")
        .json(&json!({
            "session_id": "another-unbound",
            "tool_name": "Edit",
            "tool_input": {"file_path": "src/legacy.rs"},
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    let decision = body["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .unwrap();
    assert_eq!(
        decision, "deny",
        "unbound stranger cannot ride the legacy lease"
    );
}

#[tokio::test]
async fn dod5_explicit_principal_heartbeat_stamps_pid() {
    // Verify the explicit-principal heartbeat path adds the new pid
    // without overwriting prior pids.
    let server = make_default_server().await;
    let pid = "loop:dod5-hb";
    let sid = "s-dod5-hb";

    // Pre-seed the principal with pids=[100].
    let store = CoordStore::open_in_memory().expect("store");
    store
        .upsert_principal(
            pid,
            PrincipalUpsert {
                harness: Some("h".into()),
                pids: Some(vec![100]),
                ..Default::default()
            },
        )
        .unwrap();

    // Bind the server's services.coord_store to the one we seeded.
    // (make_default_server uses new_default which gives a fresh
    // in-memory store; we replace it here so the heartbeat merges
    // against our pre-seeded row.)
    // Easiest path: make a server using a tmp file-backed store and
    // pre-write the principal.
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("dod5-hb.db");
    {
        let s = CoordStore::open(&path).expect("open");
        s.upsert_principal(
            pid,
            PrincipalUpsert {
                harness: Some("h".into()),
                pids: Some(vec![100]),
                ..Default::default()
            },
        )
        .unwrap();
    }
    let mut services = ContextNestServices::new_default().await.expect("services");
    services.coord_store = Arc::new(CoordStore::open(&path).expect("reopen"));
    let app = create_simple_app(services).await.expect("app");
    let server = TestServer::new(app).expect("test server");

    // Heartbeat from a new pid (200) → stored pids = [100, 200].
    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", pid)
        .add_header("X-Concord-Pid", "200")
        .json(&json!({"session_id": sid, "hook_event_name": "UserPromptSubmit"}))
        .await;
    res.assert_status(axum::http::StatusCode::OK);

    // Verify stored pids.
    let fetched = server
        .get(&format!("/api/v1/coord/principals/{}", url_encode(pid)))
        .await;
    fetched.assert_status(axum::http::StatusCode::OK);
    let body: Value = fetched.json();
    let pids = body["pids"].as_array().expect("pids array");
    assert_eq!(pids.len(), 2);
    assert!(pids.iter().any(|v| v.as_i64() == Some(100)));
    assert!(pids.iter().any(|v| v.as_i64() == Some(200)));
}

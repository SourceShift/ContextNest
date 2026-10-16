//! Concord P4 — overlap-arbitration end-to-end suite.
//!
//! Drives `POST /api/v1/coord/precheck`, `POST /api/v1/coord/turn`, the
//! new `GET/POST /api/v1/coord/overlaps` + `/freezes` routes, and the
//! `coord_store` methods directly through `create_simple_app` plus
//! `axum_test::TestServer`. Verifies every line of the Definition of Done:
//!
//! - dod1: a stale (P1) notice creates one `open` overlap row, a second
//!   notice bumps `count` without duplicating, and the warning text
//!   carries `(overlap O-<id>)`
//! - dod2: `ack_overlap(proceed)` → `acked`, a notification lands in `b`'s
//!   mailbox, a bad decision is 400, an unknown id is 404
//! - dod3: `CONTEXTNEST_CONCORD_OVERLAP_REACK_SECS=0` re-opens an acked row
//! - dod4: `CONTEXTNEST_CONCORD_ESCALATE_COUNT=3` escalates on the 3rd
//!   notice with exactly one `human:operator` message and +1 on
//!   `coord_overlaps_escalated_total`, and no duplicate on notice 4
//! - dod5: a live freeze denies an outsider, never the freezer's own
//!   lineage, and expiry removes it from both `GET /freezes` and the deny
//! - dod6: a seeded P3 topic notice creates a sorted-pair-keyed `topic` row
//! - dod7: `GET /overlaps?state=open` returns only open rows, `since` is an
//!   id-greater-than cursor, newest-first
//! - dod8: no response anywhere carries `permissionDecision == "allow"`
//!   and every hook endpoint answers 200
//!
//! All tests share one process env, so every test holds `ENV_LOCK`
//! (poison-tolerantly) and installs RAII guards that restore the env on
//! drop.

use axum::http::StatusCode;
use axum_test::TestServer;
use chrono::{Duration as ChronoDuration, Utc};
use contextnest::api::create_simple_app;
use contextnest::services::coord_store::{CoordStore, PrincipalUpsert};
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// RAII guard that sets (or clears) env vars and restores them on drop.
struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn set(vars: &[(&'static str, Option<&str>)]) -> EnvGuard {
        let mut saved = Vec::with_capacity(vars.len());
        for (k, v) in vars {
            saved.push((*k, std::env::var(k).ok()));
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
        EnvGuard { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in self.saved.drain(..) {
            match v {
                Some(val) => std::env::set_var(k, val),
                None => std::env::remove_var(k),
            }
        }
    }
}

/// Clear the three P4 overlap knobs to their defaults.
fn clear_overlap_env() -> EnvGuard {
    EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OVERLAP_REACK_SECS", None),
        ("CONTEXTNEST_CONCORD_ESCALATE_COUNT", None),
        ("CONTEXTNEST_CONCORD_ESCALATE_SECS", None),
    ])
}

/// Recursive dod8 check: no `permissionDecision` value anywhere may be
/// the string `"allow"` (that would skip Claude Code's permission prompt).
fn assert_no_permission_allow(v: &Value) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k == "permissionDecision" && val == "allow" {
                    panic!("permissionDecision must never be 'allow'");
                }
                assert_no_permission_allow(val);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                assert_no_permission_allow(item);
            }
        }
        _ => {}
    }
}

// ─────────────────── harness ───────────────────

async fn spawn() -> (TestServer, Arc<CoordStore>, TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("coord.db");
    let _ = CoordStore::open(&path).expect("pre-open");
    let mut services = ContextNestServices::new_default()
        .await
        .expect("default services");
    let store = Arc::new(CoordStore::open(&path).expect("reopen store"));
    services.coord_store = store.clone();
    let app = create_simple_app(services).await.expect("simple app");
    let server = TestServer::new(app).expect("test server");
    (server, store, tmp)
}

fn upsert_principal(store: &CoordStore, id: &str, parent: Option<&str>) {
    let mut upsert = PrincipalUpsert::default();
    upsert.labels = Some(match parent {
        Some(p) => json!({ "parent": p }),
        None => json!({}),
    });
    upsert.harness = Some("claude-code".into());
    store
        .upsert_principal(id, upsert)
        .expect("upsert principal");
}

fn seed_intent(store: &CoordStore, id: &str, text: &str, vec: &[f32]) {
    store
        .upsert_intent(id, text, vec, Utc::now())
        .expect("seed intent");
}

// ─────────────────── request helpers ───────────────────

async fn post_footprint(
    server: &TestServer,
    principal: &str,
    tool: &str,
    path: &std::path::Path,
    sid: &str,
    cwd: &str,
) {
    let tool_input = if tool == "NotebookEdit" {
        json!({ "notebook_path": path.to_string_lossy() })
    } else {
        json!({ "file_path": path.to_string_lossy() })
    };
    let body = json!({
        "session_id": sid,
        "tool_name": tool,
        "tool_input": tool_input,
        "cwd": cwd,
    });
    let res = server
        .post("/api/v1/coord/footprints")
        .add_header("X-Concord-Principal", principal)
        .add_header("X-Concord-Cwd", cwd)
        .json(&body)
        .await;
    res.assert_status(StatusCode::OK);
}

async fn post_precheck(
    server: &TestServer,
    principal: Option<&str>,
    tool: &str,
    path: &std::path::Path,
    sid: &str,
    cwd: &str,
) -> Value {
    let mut builder = server.post("/api/v1/coord/precheck");
    if let Some(p) = principal {
        builder = builder.add_header("X-Concord-Principal", p);
    }
    builder = builder.add_header("X-Concord-Cwd", cwd);
    let tool_input = if tool == "NotebookEdit" {
        json!({ "notebook_path": path.to_string_lossy() })
    } else {
        json!({ "file_path": path.to_string_lossy() })
    };
    let body = json!({
        "session_id": sid,
        "tool_name": tool,
        "tool_input": tool_input,
        "cwd": cwd,
    });
    let res = builder.json(&body).await;
    res.assert_status(StatusCode::OK);
    let v: Value = res.json();
    assert_no_permission_allow(&v);
    v
}

async fn ups(
    server: &TestServer,
    pid: &str,
    sid: &str,
    event: &str,
    prompt: Option<&str>,
) -> Value {
    let mut body = json!({
        "session_id": sid,
        "cwd": "/w/repo",
        "hook_event_name": event,
    });
    if let Some(p) = prompt {
        body["prompt"] = json!(p);
    }
    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", pid)
        .json(&body)
        .await;
    res.assert_status(StatusCode::OK);
    let v: Value = res.json();
    assert_no_permission_allow(&v);
    v
}

async fn metrics(server: &TestServer) -> Value {
    let res = server.get("/api/v1/coord/metrics").await;
    res.assert_status(StatusCode::OK);
    res.json()
}

async fn get_overlaps(server: &TestServer, state: Option<&str>, since: Option<i64>) -> Value {
    let mut url = "/api/v1/coord/overlaps".to_string();
    let mut params: Vec<String> = Vec::new();
    if let Some(s) = state {
        params.push(format!("state={s}"));
    }
    if let Some(s) = since {
        params.push(format!("since={s}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    let res = server.get(&url).await;
    res.assert_status(StatusCode::OK);
    let v: Value = res.json();
    assert_no_permission_allow(&v);
    v
}

async fn get_freezes(server: &TestServer) -> Value {
    let res = server.get("/api/v1/coord/freezes").await;
    res.assert_status(StatusCode::OK);
    let v: Value = res.json();
    assert_no_permission_allow(&v);
    v
}

// ─────────────────── DoD 1 — stale notice creates + dedups ───────────────────

#[tokio::test]
async fn dod1_stale_notice_creates_and_dedups_overlap() {
    let _lock = lock_env();
    let _guard = clear_overlap_env();
    let (server, store, tmp) = spawn().await;
    let dir = tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    std::fs::write(&f, b"v1").unwrap();
    let canonical = std::fs::canonicalize(&f).expect("canonical");
    let cwd = dir.to_string_lossy().to_string();

    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);

    post_footprint(&server, "loop:a", "Read", &canonical, "s1", &cwd).await;
    std::fs::write(&f, b"v2").unwrap();
    post_footprint(&server, "loop:b", "Edit", &canonical, "s2", &cwd).await;

    let res1 = post_precheck(&server, Some("loop:a"), "Edit", &canonical, "s1", &cwd).await;
    assert_eq!(res1["warn"], json!(true));
    let ctx1 = res1["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("ctx string");
    assert!(
        ctx1.contains("(overlap O-"),
        "stale line must carry the overlap id; got {ctx1:?}"
    );

    let res2 = post_precheck(&server, Some("loop:a"), "Edit", &canonical, "s1", &cwd).await;
    let ctx2 = res2["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("ctx string");
    assert!(
        ctx2.contains("(overlap O-"),
        "second notice keeps the id; got {ctx2:?}"
    );

    // One row, count bumped to 2 — no duplicate.
    let rows = store.list_overlaps("all", None, 200).expect("list");
    assert_eq!(rows.len(), 1, "deduped to a single row; got {rows:?}");
    let row = &rows[0];
    assert_eq!(row.kind, "stale");
    assert_eq!(row.a, "loop:a");
    assert_eq!(row.b.as_deref(), Some("loop:b"));
    assert_eq!(row.count, 2);
    assert_eq!(row.state, "open");
    assert!(
        ctx1.contains(&format!("(overlap O-{})", row.id)),
        "warning text must name the tracked row id; got {ctx1:?}"
    );
}

// ─────────────────── DoD 2 — ack proceed + validation ───────────────────

#[tokio::test]
async fn dod2_ack_proceed_delivers_and_validates() {
    let _lock = lock_env();
    let _guard = clear_overlap_env();
    let (server, store, _tmp) = spawn().await;
    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);

    let item = store
        .upsert_overlap("hot", "/x", "loop:a", Some("loop:b"), Utc::now())
        .expect("upsert");
    let id = item.id;

    let acked = store
        .ack_overlap(id, "loop:a", "proceed", Some("I'll yield"), Utc::now())
        .expect("ack");
    assert_eq!(acked.state, "acked");
    assert_eq!(acked.ack_by.as_deref(), Some("loop:a"));
    assert_eq!(acked.ack_decision.as_deref(), Some("proceed"));

    // b's mailbox got exactly one notification.
    let msgs = store.list_messages("loop:b", false).expect("msgs");
    assert_eq!(msgs.len(), 1, "one notification to b; got {msgs:?}");
    assert!(msgs[0].body.contains(&format!("O-{id}")));

    // Bad decision → 400.
    let res = server
        .post(&format!("/api/v1/coord/overlaps/{id}/ack"))
        .json(&json!({ "principal": "loop:a", "decision": "bogus" }))
        .await;
    res.assert_status(StatusCode::BAD_REQUEST);

    // Unknown id → 404.
    let res = server
        .post("/api/v1/coord/overlaps/9999/ack")
        .json(&json!({ "principal": "loop:a", "decision": "proceed" }))
        .await;
    res.assert_status(StatusCode::NOT_FOUND);
}

// ─────────────────── DoD 3 — REACK_SECS=0 re-opens ───────────────────

#[tokio::test]
async fn dod3_reack_zero_reopens_acked_overlap() {
    let _lock = lock_env();
    let _guard = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OVERLAP_REACK_SECS", Some("0")),
        ("CONTEXTNEST_CONCORD_ESCALATE_COUNT", None),
        ("CONTEXTNEST_CONCORD_ESCALATE_SECS", None),
    ]);
    let store = Arc::new(CoordStore::open_in_memory().expect("store"));
    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);

    let t0 = Utc::now();
    let item = store
        .upsert_overlap("hot", "/x", "loop:a", Some("loop:b"), t0)
        .expect("upsert");
    assert_eq!(item.state, "open");
    let acked = store
        .ack_overlap(
            item.id,
            "loop:a",
            "proceed",
            None,
            t0 + ChronoDuration::seconds(1),
        )
        .expect("ack");
    assert_eq!(acked.state, "acked");

    // A fresh notice past the (zero) re-ack window re-opens the row.
    let reopened = store
        .upsert_overlap(
            "hot",
            "/x",
            "loop:a",
            Some("loop:b"),
            t0 + ChronoDuration::seconds(2),
        )
        .expect("reopen");
    assert_eq!(
        reopened.state, "open",
        "acked row must re-open after reack window"
    );
}

// ─────────────────── DoD 4 — escalation + metric + no duplicate ───────────────────

#[tokio::test]
async fn dod4_escalation_single_operator_message_and_metric() {
    let _lock = lock_env();
    let _guard = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_ESCALATE_COUNT", Some("3")),
        ("CONTEXTNEST_CONCORD_ESCALATE_SECS", Some("3600")),
        ("CONTEXTNEST_CONCORD_OVERLAP_REACK_SECS", None),
    ]);
    let (server, store, tmp) = spawn().await;
    let dir = tmp.path().to_path_buf();
    let f = dir.join("g.txt");
    std::fs::write(&f, b"v1").unwrap();
    let canonical = std::fs::canonicalize(&f).expect("canonical");
    let cwd = dir.to_string_lossy().to_string();

    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);

    post_footprint(&server, "loop:a", "Read", &canonical, "s1", &cwd).await;
    std::fs::write(&f, b"v2").unwrap();
    post_footprint(&server, "loop:b", "Edit", &canonical, "s2", &cwd).await;

    let before = metrics(&server).await;
    let before_esc = before["coord_overlaps_escalated_total"]
        .as_u64()
        .unwrap_or(0);

    for _ in 0..3 {
        post_precheck(&server, Some("loop:a"), "Edit", &canonical, "s1", &cwd).await;
    }

    let after = metrics(&server).await;
    assert_eq!(
        after["coord_overlaps_escalated_total"]
            .as_u64()
            .unwrap_or(0),
        before_esc + 1,
        "metric must advance by exactly 1"
    );
    let msgs = store.list_messages("human:operator", false).expect("msgs");
    assert_eq!(msgs.len(), 1, "exactly one operator message; got {msgs:?}");

    // Notice 4 — no duplicate message, no second metric bump.
    post_precheck(&server, Some("loop:a"), "Edit", &canonical, "s1", &cwd).await;
    let msgs2 = store.list_messages("human:operator", false).expect("msgs");
    assert_eq!(
        msgs2.len(),
        1,
        "no second message on notice 4; got {msgs2:?}"
    );
    let after2 = metrics(&server).await;
    assert_eq!(
        after2["coord_overlaps_escalated_total"]
            .as_u64()
            .unwrap_or(0),
        before_esc + 1,
        "metric must not advance again on notice 4"
    );
}

// ─────────────────── DoD 5 — freeze deny / lineage exemption / expiry ───────────────────

#[tokio::test]
async fn dod5_freeze_denies_outsider_not_lineage_and_expires() {
    let _lock = lock_env();
    let _guard = clear_overlap_env();
    let (server, store, tmp) = spawn().await;
    let dir = tmp.path().to_path_buf();
    let f = dir.join("frozen.txt");
    std::fs::write(&f, b"locked").unwrap();
    let canonical = std::fs::canonicalize(&f).expect("canonical");
    let cwd = dir.to_string_lossy().to_string();

    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);
    upsert_principal(&store, "run:x", Some("loop:b"));

    let freeze = store
        .create_freeze("**/frozen.txt", "loop:b", "deploy freeze", 3600, Utc::now())
        .expect("freeze");

    // Outsider is denied.
    let res = post_precheck(&server, Some("loop:a"), "Edit", &canonical, "s1", &cwd).await;
    assert_eq!(
        res["hookSpecificOutput"]["permissionDecision"],
        json!("deny")
    );
    let reason = res["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .unwrap_or("");
    assert!(
        reason.contains("is frozen by loop:b"),
        "reason must name the freezer; got {reason:?}"
    );
    assert!(
        reason.contains("deploy freeze"),
        "reason must carry the freeze reason; got {reason:?}"
    );

    // The freezer is never blocked.
    let res = post_precheck(&server, Some("loop:b"), "Edit", &canonical, "s2", &cwd).await;
    assert!(
        res["hookSpecificOutput"]["permissionDecision"].is_null(),
        "freezer must not be denied"
    );

    // The freezer's lineage is never blocked.
    let res = post_precheck(&server, Some("run:x"), "Edit", &canonical, "s3", &cwd).await;
    assert!(
        res["hookSpecificOutput"]["permissionDecision"].is_null(),
        "freezer's lineage must not be denied"
    );

    // The live freeze is listed.
    let list = get_freezes(&server).await;
    assert_eq!(list["count"], json!(1));

    // Expiry: a 1s freeze disappears from GET and stops denying.
    store.delete_freeze(freeze.id).expect("delete");
    store
        .create_freeze("**/frozen.txt", "loop:b", "brief", 1, Utc::now())
        .expect("brief freeze");
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    let list = get_freezes(&server).await;
    assert_eq!(
        list["count"],
        json!(0),
        "expired freeze must be swept from GET"
    );
    let res = post_precheck(&server, Some("loop:a"), "Edit", &canonical, "s1", &cwd).await;
    assert!(
        res["hookSpecificOutput"]["permissionDecision"].is_null(),
        "expired freeze must not deny"
    );
}

// ─────────────────── DoD 6 — topic notice → sorted-pair overlap ───────────────────

#[tokio::test]
async fn dod6_topic_notice_creates_sorted_pair_overlap() {
    let _lock = lock_env();
    let _guard = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_TOPIC", Some("1")),
        ("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD", Some("0.85")),
        ("CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS", None),
        ("CONTEXTNEST_CONCORD_TOPIC_DEDUP_SECS", None),
        ("CONTEXTNEST_CONCORD_INTENT_ALPHA", None),
        ("CONTEXTNEST_CONCORD_OVERLAP_REACK_SECS", None),
        ("CONTEXTNEST_CONCORD_ESCALATE_COUNT", None),
        ("CONTEXTNEST_CONCORD_ESCALATE_SECS", None),
    ]);
    let (server, store, _tmp) = spawn().await;
    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);
    let a = [1.0_f32, 0.0, 0.0, 0.0];
    let b = [1.0_f32, 0.05, 0.0, 0.0];
    seed_intent(&store, "loop:a", "alpha intent", &a);
    seed_intent(&store, "loop:b", "beta intent", &b);

    let body = ups(&server, "loop:a", "s1", "UserPromptSubmit", None).await;
    let ctx_text = body["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("");
    assert!(
        ctx_text.contains("(overlap O-"),
        "topic line must carry the overlap id; got {ctx_text:?}"
    );

    let rows = store.list_overlaps("all", None, 200).expect("list");
    let topic_rows: Vec<_> = rows.iter().filter(|r| r.kind == "topic").collect();
    assert_eq!(topic_rows.len(), 1, "one topic row; got {rows:?}");
    let row = topic_rows[0];
    assert_eq!(row.subject, "loop:a|loop:b");
    assert_eq!(row.a, "loop:a");
    assert_eq!(row.b.as_deref(), Some("loop:b"));
}

// ─────────────────── DoD 7 — list state filter + since cursor ───────────────────

#[tokio::test]
async fn dod7_list_state_filter_and_since_cursor() {
    let _lock = lock_env();
    let _guard = clear_overlap_env();
    let (server, store, _tmp) = spawn().await;
    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);

    let t0 = Utc::now();
    let open1 = store
        .upsert_overlap("hot", "/p1", "loop:a", Some("loop:b"), t0)
        .expect("open1");
    let open2 = store
        .upsert_overlap("owns", "/p2", "loop:a", None, t0)
        .expect("open2");
    store
        .ack_overlap(
            open2.id,
            "loop:a",
            "proceed",
            None,
            t0 + ChronoDuration::seconds(1),
        )
        .expect("ack open2");

    // state=open → only open1.
    let res = get_overlaps(&server, Some("open"), None).await;
    let arr = res["overlaps"].as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], json!(open1.id));

    // state=acked → only open2.
    let res = get_overlaps(&server, Some("acked"), None).await;
    let arr = res["overlaps"].as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], json!(open2.id));

    // state=all → both, newest first.
    let res = get_overlaps(&server, Some("all"), None).await;
    let arr = res["overlaps"].as_array().expect("array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["id"], json!(open2.id));
    assert_eq!(arr[1]["id"], json!(open1.id));

    // since cursor: id > open1.id → only open2.
    let res = get_overlaps(&server, Some("all"), Some(open1.id)).await;
    let arr = res["overlaps"].as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], json!(open2.id));
}

// ─────────────────── DoD 8 — 200 + never allow ───────────────────

#[tokio::test]
async fn dod8_hooks_answer_200_and_never_allow() {
    let _lock = lock_env();
    let _guard = clear_overlap_env();
    let (server, _store, _tmp) = spawn().await;

    // Minimal bodies answer 200 with no "allow" anywhere.
    for path in ["/api/v1/coord/precheck", "/api/v1/coord/turn"] {
        let res = server.post(path).text("{}").await;
        res.assert_status(StatusCode::OK);
        let v: Value = res.json();
        assert_no_permission_allow(&v);
    }

    // The read-only list endpoints answer 200 too.
    let res = get_overlaps(&server, Some("all"), None).await;
    assert_no_permission_allow(&res);
    let res = get_freezes(&server).await;
    assert_no_permission_allow(&res);
}

// ─────────── review follow-ups: unbound freeze, atomic upsert ───────────

/// A request with no explicit principal, no pane/tty and an EMPTY session id
/// resolves to no caller at all (a non-empty id always derives
/// `session:<id>`). It is an outsider to every freeze, so it must be denied
/// too — the freeze check runs before the unbound early return.
#[tokio::test]
async fn freeze_denies_an_unbound_session() {
    let _lock = lock_env();
    let _guard = clear_overlap_env();
    let (server, store, tmp) = spawn().await;
    let dir = tmp.path().to_path_buf();
    let f = dir.join("frozen.txt");
    std::fs::write(&f, b"locked").unwrap();
    let canonical = std::fs::canonicalize(&f).expect("canonical");
    let cwd = dir.to_string_lossy().to_string();
    upsert_principal(&store, "loop:b", None);
    store
        .create_freeze("**/frozen.txt", "loop:b", "deploy freeze", 3600, Utc::now())
        .expect("freeze");

    let res = post_precheck(&server, None, "Edit", &canonical, "", &cwd).await;
    assert_eq!(
        res["hookSpecificOutput"]["permissionDecision"],
        json!("deny"),
        "an unbound session must not slip past a freeze; got {res}"
    );
}

/// Sixteen concurrent upserts of the same (kind, subject, a, b) must land on
/// ONE row with count 16 and post at most one operator escalation — the
/// SELECT and the write share a single lock acquisition.
#[test]
fn concurrent_upserts_of_one_overlap_are_atomic() {
    let _lock = lock_env();
    let _guard = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OVERLAP_REACK_SECS", None),
        ("CONTEXTNEST_CONCORD_ESCALATE_COUNT", Some("3")),
        ("CONTEXTNEST_CONCORD_ESCALATE_SECS", Some("3600")),
    ]);
    let tmp = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(CoordStore::open(&tmp.path().join("coord.db")).expect("store"));
    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);

    let handles: Vec<_> = (0..16)
        .map(|_| {
            let s = store.clone();
            std::thread::spawn(move || {
                s.upsert_overlap("stale", "/x/notes.md", "loop:a", Some("loop:b"), Utc::now())
                    .expect("upsert");
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread");
    }

    let rows = store.list_overlaps("all", None, 200).expect("list");
    assert_eq!(rows.len(), 1, "one key must be one row; got {}", rows.len());
    assert_eq!(rows[0].count, 16);
    assert_eq!(rows[0].state, "escalated");
    let msgs = store.list_messages("human:operator", false).expect("msgs");
    assert_eq!(
        msgs.len(),
        1,
        "exactly one escalation message; got {}",
        msgs.len()
    );
}

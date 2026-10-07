//! Concord P1d — Bash exec footprints and unrecorded-change attribution.
//!
//! Drives `POST /api/v1/coord/footprints` (with `tool_name: "Bash"`)
//! and `POST /api/v1/coord/precheck` through `create_simple_app` plus
//! `axum_test::TestServer`. Verifies:
//!
//! - dod1: a Bash `echo x >> notes.md` by loop:b is attributed in
//!   loop:a's unrecorded-change advisory ("changed on disk", "Probably
//!   loop:b", and the quoted command all appear).
//! - dod2: an irrelevant Bash `ls -la` produces no attribution sentence
//!   (the paragraph is still present, "Probably" is absent).
//! - dod3: a Bash command by the caller itself renders "your own shell
//!   command" instead of naming a principal.
//! - dod4: an exec-only row on a hot path never triggers a P1 warn, a
//!   turn-digest entry, or a hot claim.
//! - dod5: `CoordStore::open` upgrades a pre-P1d DB (no `detail`
//!   column) in place — it opens, and old rows read `detail == NULL`.
//! - dod6: no precheck response anywhere contains `permissionDecision`
//!   (enforced by `post_precheck`'s `assert_no_permission_decision`).
//!
//! Every async test takes ENV_LOCK first and holds it for its whole
//! body so env mutation cannot race with sibling tests in the same
//! binary (the P2a/P2d pattern).

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::coord_store::CoordStore;
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;
use tempfile::TempDir;

// ─────────────────── env lock + guard ───────────────────

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

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

/// The env set every scenario shares (mirrors coord_disk_truth_test):
/// disk check on with grace 0, hot/owns in warn/audit mode (never a
/// `permissionDecision`), default principal TTL.
fn concord_env(hot_globs: Option<&str>) -> Vec<(&'static str, Option<&str>)> {
    vec![
        ("CONTEXTNEST_CONCORD_DISK_CHECK", None),
        ("CONTEXTNEST_CONCORD_DISK_GRACE_MS", Some("0")),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", hot_globs),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]
}

// ─────────────────── DoD6 invariant ───────────────────

/// Recursive DoD6 check: no `permissionDecision` key may exist in any
/// precheck response (that would skip Claude Code's permission prompt
/// or, worse, "allow").
fn assert_no_permission_decision(v: &Value) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k == "permissionDecision" {
                    panic!("precheck response must never set permissionDecision; got: {val}");
                }
                assert_no_permission_decision(val);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                assert_no_permission_decision(item);
            }
        }
        _ => {}
    }
}

// ─────────────────── harness ───────────────────

struct Harness {
    server: TestServer,
    /// Cloned out of `services` before it was consumed by
    /// `create_simple_app`, so unit-style assertions against
    /// `list_live_hot_claims` and `take_turn_digest` work without going
    /// through HTTP.
    store: Arc<CoordStore>,
    tmp: TempDir,
}

async fn make_harness() -> Harness {
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
    Harness { server, store, tmp }
}

fn write_file(path: &Path, body: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(path, body).expect("write");
}

/// Convenience: append `body` to `path` directly (no footprint),
/// mimicking a `>> file` shell redirect.
fn append_unrecorded(path: &Path, body: &[u8]) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open append");
    f.write_all(body).expect("append");
}

// ─────────────────── request helpers ───────────────────

struct FootprintPost {
    sid: String,
    principal_header: Option<String>,
    tool: String,
    path: PathBuf,
    cwd: Option<String>,
}

async fn post_footprint(server: &TestServer, req: &FootprintPost) {
    let mut builder = server.post("/api/v1/coord/footprints");
    if let Some(p) = &req.principal_header {
        builder = builder.add_header("X-Concord-Principal", p.as_str());
    }
    let tool_input = if req.tool == "NotebookEdit" {
        json!({ "notebook_path": req.path.to_string_lossy() })
    } else {
        json!({ "file_path": req.path.to_string_lossy() })
    };
    let body = json!({
        "session_id": req.sid,
        "tool_name": req.tool,
        "tool_input": tool_input,
        "cwd": req.cwd.clone().unwrap_or_default(),
    });
    let res = builder.json(&body).await;
    res.assert_status(axum::http::StatusCode::OK);
}

/// Post a Bash PostToolUse call: `tool_input.command` carries the shell
/// string, `cwd` the working directory. The handler records an exec
/// footprint (command + cwd, never stat'ed).
async fn post_bash_footprint(
    server: &TestServer,
    principal: &str,
    sid: &str,
    cwd: &str,
    command: &str,
) {
    let res = server
        .post("/api/v1/coord/footprints")
        .add_header("X-Concord-Principal", principal)
        .json(&json!({
            "session_id": sid,
            "tool_name": "Bash",
            "tool_input": { "command": command },
            "cwd": cwd,
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
}

struct PrecheckPost {
    sid: String,
    principal_header: Option<String>,
    tool: String,
    path: PathBuf,
    cwd: Option<String>,
}

async fn post_precheck(server: &TestServer, req: &PrecheckPost) -> Value {
    let mut builder = server.post("/api/v1/coord/precheck");
    if let Some(p) = &req.principal_header {
        builder = builder.add_header("X-Concord-Principal", p.as_str());
    }
    let tool_input = if req.tool == "NotebookEdit" {
        json!({ "notebook_path": req.path.to_string_lossy() })
    } else {
        json!({ "file_path": req.path.to_string_lossy() })
    };
    let body = json!({
        "session_id": req.sid,
        "tool_name": req.tool,
        "tool_input": tool_input,
        "cwd": req.cwd.clone().unwrap_or_default(),
    });
    let res = builder.json(&body).await;
    res.assert_status(axum::http::StatusCode::OK);
    let value: Value = res.json();
    assert_no_permission_decision(&value);
    value
}

// ─────────────────── DoD 1 — echo is attributed to loop:b ───────────────────

#[tokio::test]
async fn dod1_bash_echo_is_attributed_to_loop_b() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&concord_env(None));
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("notes.md");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // loop:a records a read — establishes the premise (last footprint).
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Read".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    // loop:b posts a Bash footprint whose command appends to notes.md.
    post_bash_footprint(&h.server, "loop:b", "s2", &cwd, "echo x >> notes.md").await;

    // The test appends to notes.md directly (no footprint) — the
    // unrecorded change.
    std::thread::sleep(StdDuration::from_millis(20));
    append_unrecorded(&f, b"\nshell wrote this");

    // loop:a prechecks Edit — the advisory must name loop:b's shell.
    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    assert_eq!(res["warn"], json!(false), "no P1 warn; got: {res}");
    assert_eq!(res["unrecorded_change"], json!(true));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("ctx string");
    assert!(ctx.contains("changed on disk"), "ctx: {ctx}");
    assert!(
        ctx.contains("Probably loop:b"),
        "ctx must name loop:b; got: {ctx}"
    );
    assert!(
        ctx.contains("echo x >> notes.md"),
        "ctx must quote the command; got: {ctx}"
    );
}

// ─────────────────── DoD 2 — irrelevant Bash gets no attribution ───────────────────

#[tokio::test]
async fn dod2_irrelevant_bash_gets_no_attribution() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&concord_env(None));
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("notes.md");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Read".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    // `ls -la` does not mention notes.md — no attribution candidate.
    post_bash_footprint(&h.server, "loop:b", "s2", &cwd, "ls -la").await;

    std::thread::sleep(StdDuration::from_millis(20));
    append_unrecorded(&f, b"\nshell wrote this");

    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    assert_eq!(res["unrecorded_change"], json!(true));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("ctx string");
    assert!(ctx.contains("changed on disk"), "ctx: {ctx}");
    assert!(
        ctx.contains("Re-read it before editing."),
        "paragraph still present; got: {ctx}"
    );
    assert!(
        !ctx.contains("Probably"),
        "no attribution sentence for an irrelevant command; got: {ctx}"
    );
}

// ─────────────────── DoD 3 — own Bash is "your own shell command" ───────────────────

#[tokio::test]
async fn dod3_own_bash_is_your_own_shell_command() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&concord_env(None));
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("notes.md");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Read".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    // loop:a itself posts the Bash footprint.
    post_bash_footprint(&h.server, "loop:a", "s1", &cwd, "echo x >> notes.md").await;

    std::thread::sleep(StdDuration::from_millis(20));
    append_unrecorded(&f, b"\nshell wrote this");

    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    assert_eq!(res["unrecorded_change"], json!(true));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("ctx string");
    assert!(ctx.contains("changed on disk"), "ctx: {ctx}");
    assert!(
        ctx.contains("your own shell command"),
        "self attribution must say 'your own shell command'; got: {ctx}"
    );
    assert!(ctx.contains("echo x >> notes.md"), "ctx: {ctx}");
    assert!(
        !ctx.contains("Probably loop:a"),
        "self must not be named; got: {ctx}"
    );
}

// ─────────────────── DoD 4 — exec-only row never warns/digests/claims ───────────────────

#[tokio::test]
async fn dod4_exec_only_row_never_warns_digests_or_claims() {
    let _lock = lock_env();
    // The cwd is a hot path (`**/coord_bash_hot/**`), so a mislabelled
    // exec row WOULD claim — the assertion is non-vacuous.
    let _env = EnvGuard::set(&concord_env(Some("**/coord_bash_hot/**")));
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let hot = dir.join("coord_bash_hot");
    std::fs::create_dir_all(&hot).expect("mkdir hot");
    let f = dir.join("notes.md");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // loop:a reads notes.md — establishes a premise for the precheck.
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Read".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    // loop:b posts a Bash exec row whose cwd is the hot path.
    let hot_cwd = hot.to_string_lossy().to_string();
    post_bash_footprint(&h.server, "loop:b", "s2", &hot_cwd, "echo x >> notes.md").await;

    // 1) No hot claim, even though the exec cwd is a hot path.
    let claims = h.store.list_live_hot_claims().expect("claims");
    assert!(
        claims.is_empty(),
        "exec-only row must never claim a hot path; got: {claims:?}"
    );

    // 2) No P1 warn: loop:a prechecks Edit on notes.md (unchanged file).
    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(
        res["warn"],
        json!(false),
        "exec row must not trigger a P1 warn; got: {res}"
    );

    // 3) No digest entry: the turn digest counts write rows only.
    let digest = h
        .store
        .take_turn_digest("loop:a", 10, &|_: &str| false)
        .expect("digest");
    assert!(
        digest.changes.is_empty(),
        "exec row must not appear in the digest; got: {:?}",
        digest.changes
    );
    assert_eq!(digest.total_paths, 0);
}

// ─────────────────── DoD 5 — pre-P1d DB migrates to `detail` ───────────────────

#[test]
fn dod5_old_db_without_detail_column_opens_and_reads_null() {
    let _lock = lock_env();
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("coord-pre-p1d.db");

    // Build a pre-P1d footprints table directly via rusqlite — the
    // exact shape the pre-upgrade CoordStore would have created (no
    // `detail` column, old op CHECK).
    {
        let conn = rusqlite::Connection::open(&path).expect("open pre-p1d");
        conn.execute_batch(
            "CREATE TABLE footprints (
                seq          INTEGER PRIMARY KEY AUTOINCREMENT,
                principal_id TEXT NOT NULL,
                worker_id    TEXT NOT NULL,
                op           TEXT NOT NULL CHECK(op IN ('read','write')),
                path         TEXT NOT NULL,
                mtime_ns     INTEGER,
                size         INTEGER,
                ts           TEXT NOT NULL
             );",
        )
        .expect("create pre-p1d schema");
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        conn.execute(
            "INSERT INTO footprints (principal_id, worker_id, op, path, mtime_ns, size, ts)
             VALUES ('loop:old', 'w', 'read', '/x', 1, 2, ?1)",
            rusqlite::params![ts],
        )
        .expect("insert pre-p1d row");
    }

    // First open: migration must add the `detail` column and the old
    // row must read `detail == NULL`.
    let store = CoordStore::open(&path).expect("open with migration");
    let fp = store.latest_footprint("/x").expect("get").expect("present");
    assert_eq!(fp.detail, None, "pre-P1d row must read detail=NULL");
    assert_eq!(fp.op, "read");
    assert_eq!(fp.path, "/x");

    // The migration must also widen the op CHECK: an exec row inserts
    // successfully on the migrated DB (the pre-P1d CHECK would reject
    // it with a CHECK constraint violation).
    let exec_seq = store
        .record_exec_footprint("loop:old", "w", "/d", "echo hi >> notes.md")
        .expect("exec insert on migrated DB must succeed");
    assert!(
        exec_seq > 1,
        "exec row must receive a fresh seq beyond the migrated row; got {exec_seq}"
    );

    // Second open: idempotent — must NOT raise "duplicate column name".
    let store2 = CoordStore::open(&path).expect("re-open is a no-op");
    let again = store2
        .latest_footprint("/x")
        .expect("get")
        .expect("present");
    assert_eq!(again.detail, None);

    // Independent connection confirms the column is present.
    let probe = rusqlite::Connection::open(&path).expect("probe");
    let mut stmt = probe
        .prepare("PRAGMA table_info(footprints)")
        .expect("pragma");
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .expect("query")
        .map(|r| r.unwrap())
        .collect();
    assert!(
        cols.iter().any(|c| c == "detail"),
        "detail column must exist after migration; cols={cols:?}"
    );
}

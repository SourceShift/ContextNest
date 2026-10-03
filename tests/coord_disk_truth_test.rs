//! Concord P1b — disk-truth stale check for writers Concord never sees.
//!
//! Drives `POST /api/v1/coord/footprints` and
//! `POST /api/v1/coord/precheck` through `create_simple_app` plus
//! `axum_test::TestServer`. Verifies:
//!
//! - dod1: unrecorded append (no footprint) → "changed on disk" +
//!   `unrecorded_change == true` + `warn == false` + no
//!   `permissionDecision` + `coord_precheck_unrecorded_total` +1.
//! - dod2: fresh Read footprint clears the signal.
//! - dod3: own recorded write (the caller's own PostToolUse
//!   footprint) silences the signal.
//! - dod4: a recorded foreign write suppresses the disk signal — no
//!   double report.
//! - dod5: lineage write (run:x parented to loop:a) silences the
//!   signal — even when the caller has a premise and the disk
//!   matches the lineage write.
//! - dod6: same-length rewrite with mtime set 10s in the future is
//!   reported.
//! - dod7: deletion is reported as "was deleted on disk".
//! - dod8a: a fresh change with `CONTEXTNEST_CONCORD_DISK_GRACE_MS=600000`
//!   is NOT reported.
//! - dod8b: with `CONTEXTNEST_CONCORD_DISK_CHECK=0` and grace 0, it
//!   is NOT reported and the metric is unchanged.
//! - dod9: a caller with no prior footprint on the file gets no
//!   paragraph (and no metric bump).
//! - dod10: invariant — no `permissionDecision` in any response, every
//!   response is 200.
//!
//! Every test takes ENV_LOCK first and holds it for its whole body so
//! env mutation cannot race with sibling tests in the same binary
//! (the P2a/P2d pattern).

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::coord_store::{CoordStore, PrincipalUpsert};
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration as StdDuration, SystemTime};
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

// ─────────────────── DoD10 invariant ───────────────────

/// Recursive DoD10 check: no `permissionDecision` key may exist in any
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
    /// `record_footprint`, `latest_footprint`, etc. work without going
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
    if let Some(c) = &req.cwd {
        builder = builder.add_header("X-Concord-Cwd", c.as_str());
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
    if let Some(c) = &req.cwd {
        builder = builder.add_header("X-Concord-Cwd", c.as_str());
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

async fn metrics(server: &TestServer) -> Value {
    let res = server.get("/api/v1/coord/metrics").await;
    res.assert_status(axum::http::StatusCode::OK);
    res.json()
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

// ─────────────────── DoD 1 — unrecorded append ───────────────────

#[tokio::test]
async fn dod1_unrecorded_append_reports_changed_on_disk() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_DISK_CHECK", None),
        ("CONTEXTNEST_CONCORD_DISK_GRACE_MS", Some("0")),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // loop:a records a Read of f — establishes the premise (last footprint seq).
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

    let before = metrics(&h.server).await;
    let before_unrec = before["coord_precheck_unrecorded_total"]
        .as_u64()
        .unwrap_or(0);

    // Someone — a shell redirect — appends to f WITHOUT recording a
    // footprint. mtime advances by well over the grace window.
    std::thread::sleep(StdDuration::from_millis(20));
    append_unrecorded(&f, b"\nshell wrote this");

    // loop:a prechecks Edit on f. Expect: warn=false (no recorded
    // foreign writer), unrecorded_change=true, "changed on disk" in
    // additionalContext, metric +1.
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
        "P1 no recorded writer; got: {res}"
    );
    assert_eq!(res["unrecorded_change"], json!(true));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("ctx string");
    let canonical = std::fs::canonicalize(&f).expect("canonical");
    let canonical_str = canonical.to_string_lossy().to_string();
    assert!(
        ctx.contains("changed on disk"),
        "ctx must say 'changed on disk'; got: {ctx}"
    );
    assert!(
        ctx.contains(&canonical_str),
        "ctx must include the canonical path; got: {ctx}"
    );
    assert!(
        ctx.contains("Re-read it before editing."),
        "ctx must end with the re-read instruction; got: {ctx}"
    );

    let after = metrics(&h.server).await;
    let after_unrec = after["coord_precheck_unrecorded_total"]
        .as_u64()
        .unwrap_or(0);
    assert_eq!(after_unrec - before_unrec, 1, "exactly one unrecorded bump");
}

// ─────────────────── DoD 2 — re-read clears ───────────────────

#[tokio::test]
async fn dod2_reread_clears_unrecorded() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_DISK_CHECK", None),
        ("CONTEXTNEST_CONCORD_DISK_GRACE_MS", Some("0")),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // Setup: read, then unrecorded append, then re-read so the
    // last_footprint_seq advances past the change.
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
    std::thread::sleep(StdDuration::from_millis(20));
    append_unrecorded(&f, b"\nunrecorded");
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

    // Now another unrecorded append after the fresh read → must
    // report again. But the immediate "no further changes" precheck
    // has nothing new to compare against → no unrecorded.
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
    assert_eq!(res["unrecorded_change"], json!(false));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("");
    assert!(!ctx.contains("changed on disk"));
}

// ─────────────────── DoD 3 — own recorded write ───────────────────

#[tokio::test]
async fn dod3_own_recorded_write_silences() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_DISK_CHECK", None),
        ("CONTEXTNEST_CONCORD_DISK_GRACE_MS", Some("0")),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // loop:a reads, then its Edit footprint (recorded AFTER the file
    // changed) → its newest footprint on the path now matches disk.
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
    std::fs::write(&f, b"v2").unwrap();
    std::thread::sleep(StdDuration::from_millis(20));
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

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
    assert_eq!(res["warn"], json!(false));
    assert_eq!(res["unrecorded_change"], json!(false));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("");
    assert!(!ctx.contains("changed on disk"));
}

// ─────────────────── DoD 4 — recorded foreign writer suppresses ───────────────────

#[tokio::test]
async fn dod4_recorded_foreign_writer_suppresses_disk_signal() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_DISK_CHECK", None),
        ("CONTEXTNEST_CONCORD_DISK_GRACE_MS", Some("0")),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("f.txt");
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
    // loop:b writes AND records its footprint.
    std::fs::write(&f, b"v2-b").unwrap();
    std::thread::sleep(StdDuration::from_millis(20));
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "s2".to_string(),
            principal_header: Some("loop:b".to_string()),
            tool: "Edit".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

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
    assert_eq!(res["warn"], json!(true), "P1 must name loop:b; got: {res}");
    // Even though disk matches loop:b's recorded stat, the disk
    // signal must NOT fire — P1 already covers this case.
    assert_eq!(res["unrecorded_change"], json!(false));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("");
    assert!(
        !ctx.contains("changed on disk"),
        "must not double-report: {ctx}"
    );
}

// ─────────────────── DoD 5 — lineage write silences ───────────────────

#[tokio::test]
async fn dod5_lineage_write_silences() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_DISK_CHECK", None),
        ("CONTEXTNEST_CONCORD_DISK_GRACE_MS", Some("0")),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // Seed run:x with labels {parent: loop:a} through the store.
    h.store
        .upsert_principal(
            "run:x",
            PrincipalUpsert {
                harness: Some("claude-code".into()),
                labels: Some(json!({ "parent": "loop:a" })),
                ..Default::default()
            },
        )
        .unwrap();
    h.store
        .upsert_principal(
            "loop:a",
            PrincipalUpsert {
                harness: Some("claude-code".into()),
                ..Default::default()
            },
        )
        .unwrap();

    // loop:a reads f.
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sA".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Read".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    // run:x (lineage) writes AND records its footprint.
    std::fs::write(&f, b"v2-x").unwrap();
    std::thread::sleep(StdDuration::from_millis(20));
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sX".to_string(),
            principal_header: Some("run:x".to_string()),
            tool: "Edit".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sA".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res["warn"], json!(false));
    assert_eq!(res["unrecorded_change"], json!(false));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("");
    assert!(!ctx.contains("changed on disk"));
}

// ─────────────────── DoD 6 — same-length future-mtime rewrite ───────────────────

#[tokio::test]
async fn dod6_same_length_future_mtime_is_reported() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_DISK_CHECK", None),
        ("CONTEXTNEST_CONCORD_DISK_GRACE_MS", Some("0")),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"v1      "); // 8 bytes, padding with spaces
    let cwd = dir.to_string_lossy().to_string();

    // loop:a reads — premise established with the file's mtime/size.
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

    // Same-length rewrite (8 bytes). Bump the mtime 10s into the
    // future so the size stat still matches but the mtime diff is
    // well past grace=0. No footprint recorded.
    std::fs::write(&f, b"v2 !!!  ").unwrap();
    let f = std::fs::canonicalize(&f).expect("canonical");
    {
        use std::fs::OpenOptions;
        let fp = OpenOptions::new().write(true).open(&f).expect("open");
        let new_mtime = SystemTime::now() + StdDuration::from_secs(10);
        fp.set_modified(new_mtime).expect("set_modified");
    }

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
    assert_eq!(res["warn"], json!(false));
    assert_eq!(
        res["unrecorded_change"],
        json!(true),
        "future-mtime rewrite past grace must report; got: {res}"
    );
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("ctx");
    assert!(ctx.contains("changed on disk"));
}

// ─────────────────── DoD 7 — deleted file ───────────────────

#[tokio::test]
async fn dod7_deleted_file_reports_was_deleted_on_disk() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_DISK_CHECK", None),
        ("CONTEXTNEST_CONCORD_DISK_GRACE_MS", Some("0")),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // Canonicalize BEFORE removal so the precheck can still resolve
    // the path. canonicalize fails on a missing file, so this must
    // happen first.
    let canonical = std::fs::canonicalize(&f).expect("canonical");

    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Read".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    std::fs::remove_file(&canonical).expect("rm");

    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res["warn"], json!(false));
    assert_eq!(res["unrecorded_change"], json!(true));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("ctx");
    assert!(
        ctx.contains("was deleted on disk"),
        "ctx must say 'was deleted on disk'; got: {ctx}"
    );
    assert!(!ctx.contains("changed on disk"));
}

// ─────────────────── DoD 8a — grace suppresses ───────────────────

#[tokio::test]
async fn dod8a_grace_suppresses_just_made_change() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_DISK_CHECK", None),
        ("CONTEXTNEST_CONCORD_DISK_GRACE_MS", Some("600000")),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("f.txt");
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
    std::thread::sleep(StdDuration::from_millis(20));
    append_unrecorded(&f, b"\nshell wrote this");

    let before = metrics(&h.server).await;
    let before_unrec = before["coord_precheck_unrecorded_total"]
        .as_u64()
        .unwrap_or(0);

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
    assert_eq!(res["unrecorded_change"], json!(false));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("");
    assert!(!ctx.contains("changed on disk"));

    let after = metrics(&h.server).await;
    let after_unrec = after["coord_precheck_unrecorded_total"]
        .as_u64()
        .unwrap_or(0);
    assert_eq!(
        after_unrec, before_unrec,
        "grace-suppressed precheck must not bump the metric"
    );
}

// ─────────────────── DoD 8b — DISK_CHECK=0 disables ───────────────────

#[tokio::test]
async fn dod8b_disk_check_off_disables_completely() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_DISK_CHECK", Some("0")),
        ("CONTEXTNEST_CONCORD_DISK_GRACE_MS", Some("0")),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("f.txt");
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
    std::thread::sleep(StdDuration::from_millis(20));
    append_unrecorded(&f, b"\nshell wrote this");

    let before = metrics(&h.server).await;
    let before_unrec = before["coord_precheck_unrecorded_total"]
        .as_u64()
        .unwrap_or(0);

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
    assert_eq!(res["unrecorded_change"], json!(false));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("");
    assert!(!ctx.contains("changed on disk"));

    let after = metrics(&h.server).await;
    let after_unrec = after["coord_precheck_unrecorded_total"]
        .as_u64()
        .unwrap_or(0);
    assert_eq!(
        after_unrec, before_unrec,
        "DISK_CHECK=0 must not bump the metric"
    );
}

// ─────────────────── DoD 9 — no prior caller footprint → no signal ───────────────────

#[tokio::test]
async fn dod9_no_prior_caller_footprint_no_signal() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_DISK_CHECK", None),
        ("CONTEXTNEST_CONCORD_DISK_GRACE_MS", Some("0")),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let dir = h.tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // ANOTHER principal may have a footprint, but loop:a does not.
    // Disk may also be stale. None of this is supposed to fire —
    // "no premise, no report".
    std::fs::write(&f, b"v2").unwrap();
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "s2".to_string(),
            principal_header: Some("loop:b".to_string()),
            tool: "Edit".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

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
    assert_eq!(res["warn"], json!(false));
    assert_eq!(res["unrecorded_change"], json!(false));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("");
    assert!(!ctx.contains("changed on disk"));
}

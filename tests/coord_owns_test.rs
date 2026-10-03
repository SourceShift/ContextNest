//! Concord P2d — owns-scope enforcement end-to-end suite.
//!
//! Drives `POST /api/v1/coord/precheck` through `create_simple_app`
//! plus `axum_test::TestServer`. Verifies:
//!
//! - dod1: audit default — write outside the worktree's `labels.owns`
//!   surfaces a ✋ advisory in `additionalContext`, no
//!   `permissionDecision`, `coord_owns_violations_total` advances by 1,
//!   and one audit row lands in `owns_violations`.
//! - dod2: covered paths (equality + dir-prefix + glob) produce no
//!   violation.
//! - dod3: glob `tests/coord_*.rs` covers `tests/coord_owns_test.rs`.
//! - dod4: `CONTEXTNEST_CONCORD_OWNS_MODE=ask` → `"ask"`,
//!   `deny` → `"deny"`.
//! - dod5: with TTL=0 a stale (no-pids) worktree principal is still
//!   enforced; `end_principal` releases enforcement.
//! - dod6: nested `<tmp>/wt/a` and `<tmp>/wt/a/inner/b` resolve via
//!   longest-prefix selection; the inner principal's owns decides.
//! - dod7: empty owns and a path outside every worktree produce no
//!   violation.
//! - dod8: hot (P2a) ask + owns deny composes to deny; the response
//!   carries both the 🔒 and ✋ glyphs.
//! - dod9: `GET /api/v1/coord/owns-violations?since=0` returns the row
//!   with `next_seq >= its seq`.
//! - dod10: recursive `permissionDecision` check on every precheck
//!   response — no "allow" anywhere.
//!
//! The handlers read `CONTEXTNEST_CONCORD_OWNS_MODE` and the
//! already-existing `CONTEXTNEST_CONCORD_HOT_MODE`/`HOT_GLOBS`/
//! `HOT_TTL_SECS` fresh per request, but all tests in one binary share
//! the process env — every test therefore holds `ENV_LOCK`
//! (poison-tolerantly) and installs an RAII guard that restores the
//! vars on drop.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::coord_store::{CoordStore, PrincipalUpsert};
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Serialise access to the process-wide Concord env vars.
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

/// Recursive DoD10 check: no `permissionDecision` value anywhere may be
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

struct Harness {
    server: TestServer,
    /// Cloned out of `services` before it was consumed by
    /// `create_simple_app`, for unit-style assertions against the
    /// `owns_violations` audit table and the principal registry.
    store: Arc<CoordStore>,
    tmp: TempDir,
}

async fn make_harness() -> Harness {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("coord.db");
    // Pre-create the store so the schema migration runs once on a
    // clean file before the live service opens it again.
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

fn write_file(path: &std::path::Path, body: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir");
    }
    std::fs::write(path, body).expect("write");
}

// ─────────────────── request helpers ───────────────────

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
    // DoD10 — enforced on every precheck response, recursively.
    assert_no_permission_allow(&value);
    value
}

async fn metrics(server: &TestServer) -> Value {
    let res = server.get("/api/v1/coord/metrics").await;
    res.assert_status(axum::http::StatusCode::OK);
    res.json()
}

async fn get_owns_violations(server: &TestServer, since: &str) -> Value {
    let res = server
        .get(&format!("/api/v1/coord/owns-violations?since={since}"))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    res.json()
}

/// Insert a worktree principal whose `labels.kind = "worktree"` and
/// whose `labels.owns` is the given slice (or omitted when `owns` is
/// `None`). The worktree directory must already exist on disk so the
/// longest-prefix lookup can canonicalize it.
async fn seed_worktree(
    store: &CoordStore,
    id: &str,
    worktree_dir: &std::path::Path,
    owns: Option<&[&str]>,
) {
    let mut labels = serde_json::Map::new();
    labels.insert("kind".to_string(), json!("worktree"));
    if let Some(owns) = owns {
        labels.insert(
            "owns".to_string(),
            json!(owns.iter().map(|s| s.to_string()).collect::<Vec<String>>()),
        );
    }
    store
        .upsert_principal(
            id,
            PrincipalUpsert {
                harness: Some("claude-code".into()),
                worktree: Some(worktree_dir.to_string_lossy().to_string()),
                labels: Some(serde_json::Value::Object(labels)),
                ..Default::default()
            },
        )
        .expect("seed worktree");
}

// ─────────────────── DoD 1 — audit default ───────────────────

#[tokio::test]
async fn dod1_audit_default_advises_bumps_metric_writes_row() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;

    // Seed worktree at <tmp>/wt/a claiming `src/a.rs` + `docs`.
    let wt = h.tmp.path().join("wt/a");
    std::fs::create_dir_all(&wt).expect("mkdir wt");
    seed_worktree(&h.store, "agent:wt-a", &wt, Some(&["src/a.rs", "docs"])).await;

    // Target file: src/b.rs is NOT in `owns`. Create it so
    // canonicalize on the precheck matches what the store sees.
    let target = wt.join("src/b.rs");
    write_file(&target, b"// b\n");

    let cwd = h.tmp.path().to_string_lossy().to_string();

    let before = metrics(&h.server).await;
    let before_count = before["coord_owns_violations_total"].as_u64().unwrap_or(0);

    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sA".to_string(),
            principal_header: Some("agent:wt-a".to_string()),
            tool: "Edit".to_string(),
            path: target.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    assert_eq!(
        res["owns_violation"],
        json!(true),
        "owns_violation must be true: {res}"
    );
    assert_eq!(res["hot_conflict"], json!(false));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("ctx string");
    assert!(
        ctx.contains('\u{270B}'),
        "ctx must carry the hand glyph: {ctx}"
    );
    assert!(ctx.contains("src/b.rs"), "ctx must name the path: {ctx}");
    assert!(ctx.contains("src/a.rs, docs"), "ctx lists the owns: {ctx}");
    // No permissionDecision anywhere (audit default).
    assert!(
        res["hookSpecificOutput"]["permissionDecision"].is_null(),
        "audit must leave permissionDecision absent: {res}"
    );
    assert!(res["permissionDecision"].is_null());

    let after = metrics(&h.server).await;
    let after_count = after["coord_owns_violations_total"].as_u64().unwrap_or(0);
    assert_eq!(
        after_count - before_count,
        1,
        "exactly one owns violation metric bump"
    );

    // Exactly one row in the audit table.
    let rows = h
        .store
        .list_owns_violations(0, 200)
        .expect("list owns violations");
    assert_eq!(rows.len(), 1, "exactly one audit row");
    assert_eq!(rows[0].worktree_principal, "agent:wt-a");
    assert_eq!(rows[0].path, "src/b.rs");
    assert_eq!(rows[0].caller_principal.as_deref(), Some("agent:wt-a"));
}

// ─────────────────── DoD 2 — covered paths produce no violation ───────────────────

#[tokio::test]
async fn dod2_covered_paths_produce_no_violation() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let wt = h.tmp.path().join("wt/a");
    std::fs::create_dir_all(&wt).expect("mkdir wt");
    seed_worktree(&h.store, "agent:wt-a", &wt, Some(&["src/a.rs", "docs"])).await;

    // Equality: `src/a.rs`.
    let a = wt.join("src/a.rs");
    write_file(&a, b"// a\n");
    // Dir-prefix descendant: `docs/x/y.md`.
    let xy = wt.join("docs/x/y.md");
    write_file(&xy, b"# y\n");

    let cwd = h.tmp.path().to_string_lossy().to_string();

    let res_a = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sA".to_string(),
            principal_header: Some("agent:wt-a".to_string()),
            tool: "Edit".to_string(),
            path: a.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res_a["owns_violation"], json!(false));

    let res_xy = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sB".to_string(),
            principal_header: Some("agent:wt-a".to_string()),
            tool: "Edit".to_string(),
            path: xy.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res_xy["owns_violation"], json!(false));

    // No rows.
    let rows = h
        .store
        .list_owns_violations(0, 200)
        .expect("list owns violations");
    assert!(rows.is_empty(), "no audit rows expected: {rows:?}");
}

// ─────────────────── DoD 3 — glob coverage ───────────────────

#[tokio::test]
async fn dod3_glob_entry_covers_matching_path() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let wt = h.tmp.path().join("wt/a");
    std::fs::create_dir_all(&wt).expect("mkdir wt");
    seed_worktree(&h.store, "agent:wt-a", &wt, Some(&["tests/coord_*.rs"])).await;

    let target = wt.join("tests/coord_owns_test.rs");
    write_file(&target, b"// owned\n");
    let cwd = h.tmp.path().to_string_lossy().to_string();

    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sA".to_string(),
            principal_header: Some("agent:wt-a".to_string()),
            tool: "Edit".to_string(),
            path: target.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res["owns_violation"], json!(false));
}

// ─────────────────── DoD 4 — ask + deny modes ───────────────────

#[tokio::test]
async fn dod4_ask_and_deny_modes_set_permission_decision() {
    let _lock = lock_env();
    let _env_default = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    // ask
    {
        let _env = EnvGuard::set(&[("CONTEXTNEST_CONCORD_OWNS_MODE", Some("ask"))]);
        let h = make_harness().await;
        let wt = h.tmp.path().join("wt/a");
        std::fs::create_dir_all(&wt).expect("mkdir wt");
        seed_worktree(&h.store, "agent:wt-a", &wt, Some(&["src/a.rs"])).await;
        let target = wt.join("src/b.rs");
        write_file(&target, b"// b\n");
        let cwd = h.tmp.path().to_string_lossy().to_string();

        let res = post_precheck(
            &h.server,
            &PrecheckPost {
                sid: "sA".to_string(),
                principal_header: Some("agent:wt-a".to_string()),
                tool: "Edit".to_string(),
                path: target.clone(),
                cwd: Some(cwd.clone()),
            },
        )
        .await;
        assert_eq!(
            res["hookSpecificOutput"]["permissionDecision"],
            json!("ask")
        );
        let reason = res["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .expect("reason string");
        assert!(!reason.is_empty(), "reason must be non-empty");
        assert!(reason.contains("\u{270B}"), "reason names owns violation");
    }

    // deny
    {
        let _env = EnvGuard::set(&[("CONTEXTNEST_CONCORD_OWNS_MODE", Some("deny"))]);
        let h = make_harness().await;
        let wt = h.tmp.path().join("wt/a");
        std::fs::create_dir_all(&wt).expect("mkdir wt");
        seed_worktree(&h.store, "agent:wt-a", &wt, Some(&["src/a.rs"])).await;
        let target = wt.join("src/b.rs");
        write_file(&target, b"// b\n");
        let cwd = h.tmp.path().to_string_lossy().to_string();

        let res = post_precheck(
            &h.server,
            &PrecheckPost {
                sid: "sA".to_string(),
                principal_header: Some("agent:wt-a".to_string()),
                tool: "Edit".to_string(),
                path: target.clone(),
                cwd: Some(cwd.clone()),
            },
        )
        .await;
        assert_eq!(
            res["hookSpecificOutput"]["permissionDecision"],
            json!("deny")
        );
        let reason = res["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .expect("reason");
        assert!(reason.contains("\u{270B}"));
    }
}

// ─────────────────── DoD 5 — stale enforced; ended skipped ───────────────────

#[tokio::test]
async fn dod5_stale_enforced_ended_skipped() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        // TTL=1s + brief sleep makes the no-pids worktree `stale`.
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", Some("1")),
    ]);
    let h = make_harness().await;
    let wt = h.tmp.path().join("wt/a");
    std::fs::create_dir_all(&wt).expect("mkdir wt");
    seed_worktree(&h.store, "agent:wt-a", &wt, Some(&["src/a.rs"])).await;
    let target = wt.join("src/b.rs");
    write_file(&target, b"// b\n");
    let cwd = h.tmp.path().to_string_lossy().to_string();

    // Wait past the 1s TTL so the principal's last_seen is in the
    // past. `list_principals(true)` calls status_at under the
    // TTL-controlled env, so it classifies as `stale` (no host match
    // or pids to keep it idle).
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    let listed = h
        .store
        .list_principals(true)
        .expect("list_principals(true)");
    let principal = listed
        .iter()
        .find(|p| p.principal_id == "agent:wt-a")
        .expect("present");
    assert_eq!(
        principal.status, "stale",
        "TTL=1s + no pids + no host match must classify as stale: {principal:?}"
    );

    // Stale principal still enforced (DoD 5 — `ended_at IS NULL` is
    // the only filter on the owns lookup).
    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sA".to_string(),
            principal_header: Some("agent:wt-a".to_string()),
            tool: "Edit".to_string(),
            path: target.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res["owns_violation"], json!(true));

    // end_principal releases enforcement.
    h.store.end_principal("agent:wt-a").expect("end");
    let res_after = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sA".to_string(),
            principal_header: Some("agent:wt-a".to_string()),
            tool: "Edit".to_string(),
            path: target.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res_after["owns_violation"], json!(false));
}

// ─────────────────── DoD 6 — nested longest-prefix ───────────────────

#[tokio::test]
async fn dod6_nested_longest_prefix_uses_inner_owns() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;

    // Outer <tmp>/wt/a claims src/.
    let outer = h.tmp.path().join("wt/a");
    std::fs::create_dir_all(&outer).expect("mkdir outer");
    seed_worktree(&h.store, "agent:wt-a-outer", &outer, Some(&["src/"])).await;
    // Inner <tmp>/wt/a/inner/b claims only docs.
    let inner = outer.join("inner/b");
    std::fs::create_dir_all(&inner).expect("mkdir inner");
    seed_worktree(&h.store, "agent:wt-a-inner", &inner, Some(&["docs/"])).await;

    // Target is <tmp>/wt/a/src/x.rs — outer covers src/, so no violation.
    let in_outer = outer.join("src/x.rs");
    write_file(&in_outer, b"// x\n");
    let cwd = h.tmp.path().to_string_lossy().to_string();

    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sA".to_string(),
            principal_header: Some("agent:wt-a-outer".to_string()),
            tool: "Edit".to_string(),
            path: in_outer.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res["owns_violation"], json!(false));

    // Target is <tmp>/wt/a/inner/b/src/x.rs — inner is the longest prefix,
    // inner does NOT cover src/.
    let in_inner = inner.join("src/x.rs");
    write_file(&in_inner, b"// x\n");
    let res2 = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sB".to_string(),
            principal_header: Some("agent:wt-a-outer".to_string()),
            tool: "Edit".to_string(),
            path: in_inner.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(
        res2["owns_violation"],
        json!(true),
        "inner principal must win longest-prefix: {res2}"
    );
    let ctx2 = res2["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("");
    assert!(ctx2.contains("\u{270B}"));
    // The visible ctx lists the inner's owns (`docs`) — not the
    // outer's (`src/`) — because the inner principal's owns decides.
    assert!(
        ctx2.contains("(docs)"),
        "ctx must name inner's owns scope: {ctx2}"
    );
    assert!(
        !ctx2.contains("(src/)"),
        "ctx must NOT name outer's owns scope: {ctx2}"
    );

    // The audit row also pins the inner principal.
    let rows = h
        .store
        .list_owns_violations(0, 200)
        .expect("list owns violations");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].worktree_principal, "agent:wt-a-inner");
    assert_eq!(rows[0].path, "src/x.rs");
}

// ─────────────────── DoD 7 — empty owns and outside paths ───────────────────

#[tokio::test]
async fn dod7_empty_owns_and_outside_path_produce_no_violation() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;

    // Empty owns list → "no check".
    let wt_empty = h.tmp.path().join("wt/a");
    std::fs::create_dir_all(&wt_empty).expect("mkdir wt-empty");
    seed_worktree(&h.store, "agent:wt-empty", &wt_empty, Some(&[])).await;

    // Path outside any worktree.
    let outside = h.tmp.path().join("elsewhere/x.rs");
    write_file(&outside, b"// x\n");
    let cwd = h.tmp.path().to_string_lossy().to_string();

    let res1 = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sA".to_string(),
            principal_header: Some("agent:wt-empty".to_string()),
            tool: "Edit".to_string(),
            path: wt_empty.join("src/b.rs"),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res1["owns_violation"], json!(false));

    let res2 = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sB".to_string(),
            principal_header: Some("agent:wt-empty".to_string()),
            tool: "Edit".to_string(),
            path: outside.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res2["owns_violation"], json!(false));

    let rows = h
        .store
        .list_owns_violations(0, 200)
        .expect("list owns violations");
    assert!(rows.is_empty(), "no audit rows expected: {rows:?}");
}

// ─────────────────── DoD 8 — hot ask + owns deny composes to deny ───────────────────

#[tokio::test]
async fn dod8_hot_ask_plus_owns_deny_composes_to_deny() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OWNS_MODE", Some("deny")),
        ("CONTEXTNEST_CONCORD_HOT_MODE", Some("ask")),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;

    let wt = h.tmp.path().join("wt/a");
    std::fs::create_dir_all(&wt).expect("mkdir wt");
    seed_worktree(&h.store, "agent:wt-a", &wt, Some(&["src/a.rs"])).await;

    // Hot file claimed by an outsider (loop:b). Hot file lives at
    // .mini-ork/config/agents.yaml under <wt>; the outsider's
    // footprint is recorded via POST /footprints so claim_hot is taken.
    let hot = wt.join(".mini-ork/config/agents.yaml");
    write_file(&hot, b"agent: {}\n");
    let cwd = h.tmp.path().to_string_lossy().to_string();

    // Outsider takes the claim with a footprint POST.
    let body = json!({
        "session_id": "sB",
        "tool_name": "Edit",
        "tool_input": {"file_path": hot.to_string_lossy()},
        "cwd": cwd.clone(),
    });
    let res = h
        .server
        .post("/api/v1/coord/footprints")
        .add_header("X-Concord-Principal", "loop:b")
        .add_header("X-Concord-Cwd", cwd.as_str())
        .json(&body)
        .await;
    res.assert_status(axum::http::StatusCode::OK);

    // Now wt-a prechecks the hot file from outside its owns scope
    // (mini-ork config files are NOT in `owns`). Strictest-wins:
    // hot ask + owns deny → deny. The reason carries both glyphs.
    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sA".to_string(),
            principal_header: Some("agent:wt-a".to_string()),
            tool: "Edit".to_string(),
            path: hot.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res["hot_conflict"], json!(true));
    assert_eq!(res["owns_violation"], json!(true));
    assert_eq!(
        res["hookSpecificOutput"]["permissionDecision"],
        json!("deny"),
        "strictest-wins must produce deny: {res}"
    );
    let reason = res["hookSpecificOutput"]["permissionDecisionReason"]
        .as_str()
        .expect("reason");
    assert!(
        reason.contains("\u{1F512}") && reason.contains("\u{270B}"),
        "reason must carry both glyphs: {reason}"
    );
}

// ─────────────────── DoD 9 — GET /api/v1/coord/owns-violations ───────────────────

#[tokio::test]
async fn dod9_get_owns_violations_lists_rows_and_advances_next_seq() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let wt = h.tmp.path().join("wt/a");
    std::fs::create_dir_all(&wt).expect("mkdir wt");
    seed_worktree(&h.store, "agent:wt-a", &wt, Some(&["src/a.rs"])).await;
    let target = wt.join("src/b.rs");
    write_file(&target, b"// b\n");
    let cwd = h.tmp.path().to_string_lossy().to_string();

    // Fire one precheck to land a row.
    let _ = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sA".to_string(),
            principal_header: Some("agent:wt-a".to_string()),
            tool: "Edit".to_string(),
            path: target.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    let listed = get_owns_violations(&h.server, "0").await;
    let arr = listed["violations"].as_array().expect("array");
    assert_eq!(arr.len(), 1, "exactly one row listed: {listed}");
    assert_eq!(arr[0]["worktree_principal"], json!("agent:wt-a"));
    assert_eq!(arr[0]["path"], json!("src/b.rs"));
    assert_eq!(arr[0]["caller_principal"], json!("agent:wt-a"));
    let seq = arr[0]["seq"].as_i64().unwrap();
    assert!(
        listed["next_seq"].as_i64().unwrap() >= seq,
        "next_seq must be >= the row's seq: {listed}"
    );
}

// ────────────── regression — NEW files under a symlinked root ──────────────

/// A Write to a file that does NOT exist yet must be judged on the same
/// canonicalized path the store matched (macOS tempdirs live under
/// /var → /private/var). Previously the API re-derived the relative path
/// from the raw, non-canonical path, so a legitimate in-scope new file was
/// flagged — with a garbage audit path — and would be asked/denied in the
/// stricter modes. No `write_file` here on purpose.
#[tokio::test]
async fn new_files_are_judged_on_the_canonical_relative_path() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_OWNS_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ("CONTEXTNEST_COORD_PRINCIPAL_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let wt = h.tmp.path().join("wt/a");
    std::fs::create_dir_all(&wt).expect("mkdir wt");
    seed_worktree(&h.store, "agent:wt-a", &wt, Some(&["src"])).await;
    let cwd = h.tmp.path().to_string_lossy().to_string();

    // In scope, brand-new file → no violation.
    let inside = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sN".to_string(),
            principal_header: Some("agent:wt-a".to_string()),
            tool: "Write".to_string(),
            path: wt.join("src/new.rs"),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(
        inside["owns_violation"],
        json!(false),
        "a new in-scope file must not be a violation: {inside}"
    );

    // Out of scope, brand-new file → violation with the exact relative path.
    let outside = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sN".to_string(),
            principal_header: Some("agent:wt-a".to_string()),
            tool: "Write".to_string(),
            path: wt.join("lib/new.rs"),
            cwd: Some(cwd),
        },
    )
    .await;
    assert_eq!(outside["owns_violation"], json!(true), "{outside}");
    let rows = h
        .store
        .list_owns_violations(0, 200)
        .expect("list owns violations");
    assert_eq!(rows.len(), 1, "exactly one audit row");
    assert_eq!(
        rows[0].path, "lib/new.rs",
        "audit path must be worktree-relative"
    );
}

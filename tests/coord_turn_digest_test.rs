//! Concord P2c — UserPromptSubmit turn digest end-to-end suite.
//!
//! Drives `POST /api/v1/coord/turn` through `create_simple_app` plus
//! `axum_test::TestServer`, exercising every line of the Definition
//! of Done:
//!
//! - dod1: pre-existing footprints + first UPS → no `[concord] changed
//!   by` text, cursor advances to MAX(seq)
//! - dod2_basic: init UPS → no digest; second writer UPS lists the
//!   changed path with `(<harness>, <cwd basename>) at <RFC3339>`;
//!   third UPS lists nothing (cursor advanced)
//! - dod3_lineage_self: writes by the caller's lineage are excluded
//! - dod4_relevance: writes on a path the caller never touched are
//!   excluded
//! - dod5_sessionstart: SessionStart renders mailbox-only and never
//!   touches the digest cursor; the following UPS still surfaces the
//!   pending change
//! - dod6_cap: 7 qualifying paths → exactly 5 `\n- ` lines plus
//!   `(+2 more)`, newest first
//! - dod7_opt_out: `CONTEXTNEST_CONCORD_DIGEST=0` suppresses the
//!   digest text but still advances the cursor; unset restores
//! - dod8_upgrade: a hand-stripped pre-upgrade coord.db upgrades
//!   idempotently on `CoordStore::open`
//! - dod9: every response (including the JSON body, recursively)
//!   never carries `permissionDecision`
//!
//! Plus a metrics check that `coord_digest_lines_total` advances by
//! the emitted per-path line count.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::coord_store::{CoordStore, PrincipalUpsert};
use contextnest::services::ContextNestServices;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Poison-tolerant locker so a panic in one test doesn't cascade
/// into "all subsequent tests skip".
fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// RAII guard that sets/clears process env on construction and
/// restores the original value on drop.
struct EnvGuard {
    key: &'static str,
    saved: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: Option<&str>) -> EnvGuard {
        let saved = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        EnvGuard { key, saved }
    }
    fn clear(key: &'static str) -> EnvGuard {
        EnvGuard::set(key, None)
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.saved.take() {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

/// Recursive DoD9 assertion: no key anywhere may be
/// `permissionDecision`. The brief states the response ALWAYS returns
/// 200 without a permissionDecision; we walk every Value because axum
/// serialises nested structures by reference.
fn assert_no_permission_decision(v: &Value) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k == "permissionDecision" {
                    panic!("response must never include permissionDecision");
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
    /// Cloned out of services before it was consumed by
    /// `create_simple_app`, for direct footprint/principal
    /// seeding.
    store: Arc<CoordStore>,
    _tmp: TempDir,
    _digest_env: EnvGuard,
    /// Held for the whole test (declared after `_digest_env`, so the env
    /// value is restored before the lock is released). Dropping it at the
    /// end of make_harness let dod7's set_var race other tests (1/40 runs).
    _env_lock: std::sync::MutexGuard<'static, ()>,
}

async fn make_harness() -> Harness {
    let env_lock = lock_env();
    // Every test starts with the env var absent so a stray setvar in
    // another test doesn't leak into ours.
    let env = EnvGuard::clear("CONTEXTNEST_CONCORD_DIGEST");

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
    Harness {
        server,
        store,
        _tmp: tmp,
        _digest_env: env,
        _env_lock: env_lock,
    }
}

// ─────────────────── request helpers ───────────────────

async fn ups(server: &TestServer, pid: &str, sid: &str, event: &str) -> Value {
    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", pid)
        .json(&json!({
            "session_id": sid,
            "cwd": "/w/repo",
            "hook_event_name": event,
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let v: Value = res.json();
    assert_no_permission_decision(&v);
    v
}

async fn session_start(server: &TestServer, pid: &str, sid: &str) -> Value {
    ups(server, pid, sid, "SessionStart").await
}

fn fp(store: &CoordStore, pid: &str, op: &str, path: &str) -> i64 {
    store
        .record_footprint(pid, pid, op, path, None, None)
        .expect("record_footprint")
}

fn ctx(body: &Value) -> String {
    body["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

/// Reused inside `dod7`: writes a footprint through a fresh clone of
/// the harness store so the harness's `Arc` reference still owns the
/// last section.
fn store_clone(h: &Harness) -> Arc<CoordStore> {
    h.store.clone()
}

async fn metrics(server: &TestServer) -> Value {
    let res = server.get("/api/v1/coord/metrics").await;
    res.assert_status(axum::http::StatusCode::OK);
    res.json()
}

// ─────────────────── DoD 1 — first-turn cursor init ───────────────────

#[tokio::test]
async fn dod1_first_ups_initializes_cursor_and_emits_no_digest() {
    let h = make_harness().await;
    // Pre-existing footprints on /r/f.rs (the relevance filter relies
    // on the caller's own prior reads/writes).
    fp(&h.store, "loop:a", "read", "/r/f.rs");
    let max_before = 1_i64;
    // First UPS as loop:a.
    let body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let ctx_text = ctx(&body);
    assert!(
        !ctx_text.contains("changed by other agents"),
        "first turn must not emit a digest; got {ctx_text:?}"
    );
    assert_eq!(body["principal_id"], "loop:a");
    assert_eq!(body["bound"], true);
    let cursor = h
        .store
        .digest_cursor("loop:a")
        .expect("cursor read")
        .expect("cursor advanced");
    assert_eq!(
        cursor, max_before,
        "cursor must equal MAX(footprints.seq) on first turn"
    );
}

// ─────────────────── DoD 2 — basic once-only listing ───────────────────

#[tokio::test]
async fn dod2_basic_listing_with_harness_and_cwd_basename() {
    let h = make_harness().await;
    // Init UPS for loop:a so the principal exists with a session binding.
    ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    // loop:a reads /r/f.rs (any footprint → relevance).
    fp(&h.store, "loop:a", "read", "/r/f.rs");

    // loop:b upserts and writes /r/f.rs.
    let mut b = PrincipalUpsert::default();
    b.harness = Some("codex".into());
    b.cwd = Some("/w/repo-b".into());
    h.store.upsert_principal("loop:b", b).expect("upsert b");
    fp(&h.store, "loop:b", "write", "/r/f.rs");

    // Next UPS for loop:a surfaces the change.
    let body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let ctx_text = ctx(&body);
    assert!(
        ctx_text.contains("changed by other agents"),
        "must include header; got {ctx_text:?}"
    );
    assert!(
        ctx_text.contains("- /r/f.rs — loop:b (codex, repo-b) at "),
        "expected exact line with em-dash; got {ctx_text:?}"
    );

    // Following UPS: cursor advanced, digest is empty.
    let body2 = ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let ctx_text2 = ctx(&body2);
    assert!(
        !ctx_text2.contains("changed by other agents"),
        "second UPS must not re-emit; got {ctx_text2:?}"
    );
}

// ─────────────────── DoD 3 — lineage/self exclusion ───────────────────

#[tokio::test]
async fn dod3_lineage_self_writes_are_excluded() {
    let h = make_harness().await;
    // Init UPS first so the cursor is set BEFORE the writes below.
    ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    // loop:a (root) + run:x (lineage child) + loop:b (outsider).
    let mut x = PrincipalUpsert::default();
    x.harness = Some("claude-code".into());
    x.labels = Some(json!({"parent": "loop:a"}));
    h.store.upsert_principal("run:x", x).expect("upsert run:x");
    let mut b = PrincipalUpsert::default();
    b.harness = Some("codex".into());
    h.store.upsert_principal("loop:b", b).expect("upsert b");

    // loop:a reads the touched path.
    fp(&h.store, "loop:a", "read", "/r/f.rs");
    // lineage child + self + outsider all write to it.
    fp(&h.store, "loop:a", "write", "/r/f.rs");
    fp(&h.store, "run:x", "write", "/r/f.rs");
    fp(&h.store, "loop:b", "write", "/r/f.rs");

    let body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let ctx_text = ctx(&body);
    // Only the outsider's write appears; self and run:x are excluded
    // by lineage.
    assert!(
        ctx_text.contains("loop:b"),
        "outsider must appear; got {ctx_text:?}"
    );
    assert!(
        !ctx_text.contains("loop:a —"),
        "self must be excluded by lineage; got {ctx_text:?}"
    );
    assert!(
        !ctx_text.contains("run:x —"),
        "lineage child must be excluded; got {ctx_text:?}"
    );
    assert!(
        !ctx_text.contains("other writer"),
        "a self/lineage write leaking into the eligible set would add an \
         '(+N other writer)' suffix; got {ctx_text:?}"
    );
}

// ─────────────────── DoD 4 — relevance filter ───────────────────

#[tokio::test]
async fn dod4_relevance_excludes_paths_caller_never_touched() {
    let h = make_harness().await;
    ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    // loop:a touches /r/f.rs only.
    fp(&h.store, "loop:a", "read", "/r/f.rs");
    // loop:b writes both /r/f.rs (touched) and /r/unrelated.rs (not).
    let mut b = PrincipalUpsert::default();
    b.harness = Some("codex".into());
    h.store.upsert_principal("loop:b", b).expect("upsert b");
    fp(&h.store, "loop:b", "write", "/r/f.rs");
    fp(&h.store, "loop:b", "write", "/r/unrelated.rs");

    let body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let ctx_text = ctx(&body);
    assert!(ctx_text.contains("/r/f.rs"), "touched path appears");
    assert!(
        !ctx_text.contains("/r/unrelated.rs"),
        "untouched path excluded; got {ctx_text:?}"
    );
}

// ─────────────────── DoD 5 — SessionStart exclusion ───────────────────

#[tokio::test]
async fn dod5_sessionstart_does_not_advance_cursor_and_digest_works_next() {
    let h = make_harness().await;
    // Seed the principal row first (via init UPS), THEN post the
    // mailbox message — coord_post_message 404s on a missing principal.
    ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let res = h
        .server
        .post("/api/v1/coord/principals/loop%3Aa/messages")
        .json(&json!({"from": "human:amir", "body": "ping before start"}))
        .await;
    res.assert_status(axum::http::StatusCode::CREATED);

    let mut b = PrincipalUpsert::default();
    b.harness = Some("codex".into());
    b.cwd = Some("/w/repo-b".into());
    h.store.upsert_principal("loop:b", b).expect("upsert b");
    fp(&h.store, "loop:a", "read", "/r/f.rs");
    fp(&h.store, "loop:b", "write", "/r/f.rs");

    let start_body = session_start(&h.server, "loop:a", "s1").await;
    let start_ctx = ctx(&start_body);
    // Mailbox-first format from render_context.
    assert!(
        start_ctx.starts_with("[concord] 1 message for loop:a"),
        "mailbox header first; got {start_ctx:?}"
    );
    assert!(
        !start_ctx.contains("changed by other agents"),
        "SessionStart must NOT include digest; got {start_ctx:?}"
    );

    // The following UPS surfaces the pending change (SessionStart did
    // NOT advance the cursor).
    let next_body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let next_ctx = ctx(&next_body);
    assert!(
        next_ctx.contains("changed by other agents"),
        "UPS surfaces the change after SessionStart; got {next_ctx:?}"
    );
    assert!(
        next_ctx.contains("loop:b"),
        "outsider write listed; got {next_ctx:?}"
    );
}

// ─────────────────── DoD 5b — mailbox + digest combined ───────────────────

#[tokio::test]
async fn dod5b_mailbox_plus_digest_combined_one_ups() {
    let h = make_harness().await;
    // Seed principal before posting the mailbox message — POST
    // 404s on a missing principal.
    ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let res = h
        .server
        .post("/api/v1/coord/principals/loop%3Aa/messages")
        .json(&json!({"from": "human:amir", "body": "hi"}))
        .await;
    res.assert_status(axum::http::StatusCode::CREATED);

    let mut b = PrincipalUpsert::default();
    b.harness = Some("codex".into());
    h.store.upsert_principal("loop:b", b).expect("upsert b");
    fp(&h.store, "loop:a", "read", "/r/f.rs");
    fp(&h.store, "loop:b", "write", "/r/f.rs");

    let body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let ctx_text = ctx(&body);
    let mailbox_pos = ctx_text.find("[concord] 1 message for loop:a");
    let digest_pos = ctx_text.find("changed by other agents");
    assert!(mailbox_pos.is_some(), "mailbox present; got {ctx_text:?}");
    assert!(digest_pos.is_some(), "digest present; got {ctx_text:?}");
    assert!(
        mailbox_pos.unwrap() < digest_pos.unwrap(),
        "mailbox first, then digest; got {ctx_text:?}"
    );
    // Separator between blocks.
    assert!(
        ctx_text.contains("\n\n[concord] changed by"),
        "double-newline separator between mailbox and digest; got {ctx_text:?}"
    );
}

// ─────────────────── DoD 6 — 5-line cap plus (+N more) ───────────────────

#[tokio::test]
async fn dod6_cap_emits_five_lines_and_more_summary() {
    let h = make_harness().await;
    ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    // Touch 7 distinct paths so loop:b writes qualify for all 7.
    for i in 0..7 {
        fp(&h.store, "loop:a", "read", &format!("/r/f{i}.rs"));
    }
    let mut b = PrincipalUpsert::default();
    b.harness = Some("codex".into());
    h.store.upsert_principal("loop:b", b).expect("upsert b");
    for i in 0..7 {
        fp(&h.store, "loop:b", "write", &format!("/r/f{i}.rs"));
    }

    let body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let ctx_text = ctx(&body);
    let lines: Vec<&str> = ctx_text
        .lines()
        .filter(|l| l.starts_with("- /r/f"))
        .collect();
    assert_eq!(lines.len(), 5, "exactly 5 per-path lines; got {ctx_text:?}");
    assert!(
        ctx_text.contains("(+2 more)"),
        "summary line; got {ctx_text:?}"
    );
}

// ─────────────────── DoD 7 — env opt-out keeps cursor advancing ───────────────────

#[tokio::test]
async fn dod7_opt_out_suppresses_text_keeps_cursor_advancing() {
    let h = make_harness().await;
    ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    fp(&h.store, "loop:a", "read", "/r/f.rs");
    fp(&h.store, "loop:a", "read", "/r/g.rs");
    let mut b = PrincipalUpsert::default();
    b.harness = Some("codex".into());
    h.store.upsert_principal("loop:b", b).expect("upsert b");
    fp(&h.store, "loop:b", "write", "/r/f.rs");
    // Opt out, fire a UPS → no digest text, but cursor advances to MAX.
    std::env::set_var("CONTEXTNEST_CONCORD_DIGEST", "0");
    let body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let ctx_text = ctx(&body);
    assert!(
        !ctx_text.contains("changed by other agents"),
        "digest suppressed when env=0; got {ctx_text:?}"
    );
    let cursor_after_opt_out = h
        .store
        .digest_cursor("loop:a")
        .expect("cursor")
        .expect("advanced");
    assert!(cursor_after_opt_out > 0, "cursor advanced past the write");

    // New write on /r/g.rs.
    fp(&store_clone(&h), "loop:b", "write", "/r/g.rs");
    // Re-enable (drop the opt-out by removing the var inside the test).
    std::env::remove_var("CONTEXTNEST_CONCORD_DIGEST");

    // Next UPS surfaces g.rs (proving f.rs is gone because the cursor
    // advanced past it during the opt-out turn), and not f.rs.
    let body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let ctx_text2 = ctx(&body);
    assert!(
        ctx_text2.contains("/r/g.rs"),
        "g.rs surfaced after opt-out; got {ctx_text2:?}"
    );
    assert!(
        !ctx_text2.contains("/r/f.rs"),
        "f.rs already digested before opt-out; got {ctx_text2:?}"
    );
}

// ─────────────────── DoD 8 — upgrade migration is idempotent ───────────────────

#[tokio::test]
async fn dod8_upgrade_adds_digest_seq_idempotently() {
    let _lock = lock_env();
    // 1. Pre-create a coord.db at the upgrade path WITHOUT the digest_seq
    //    column by hand-stripping it after the first open.
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("coord.db");
    {
        let s = CoordStore::open(&path).expect("initial open");
        s.upsert_principal(
            "loop:a",
            PrincipalUpsert {
                harness: Some("h".into()),
                ..Default::default()
            },
        )
        .expect("seed principal");
        // Confirm digest_seq exists, then DROP it via a raw
        // rusqlite::Connection (bundled SQLite in rusqlite 0.32
        // supports ALTER TABLE … DROP COLUMN).
        let conn = Connection::open(&path).expect("raw conn");
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(principals)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(
            cols.iter().any(|c| c == "digest_seq"),
            "ensure_digest_seq_column must have added it; got {cols:?}"
        );
        conn.execute("ALTER TABLE principals DROP COLUMN digest_seq", [])
            .expect("drop digest_seq");
    }
    // 2. Reopen twice → both succeed (idempotent), and digest_seq is back.
    {
        let s1 = CoordStore::open(&path).expect("first reopen");
        assert!(
            s1.get_principal("loop:a").unwrap().is_some(),
            "principal still present after reopen"
        );
        let _s2 = CoordStore::open(&path).expect("second reopen (idempotent)");
        // CoordStore::lock is private; open a raw rusqlite connection
        // against the same file to query PRAGMA table_info.
        let conn = Connection::open(&path).expect("raw conn for PRAGMA");
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(principals)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(
            cols.iter().any(|c| c == "digest_seq"),
            "digest_seq restored after reopen; got {cols:?}"
        );
    }
    // 3. Build a server around a reopened store and run a UPS + a
    //    footprint to confirm digest plumbing works.
    let mut services = ContextNestServices::new_default()
        .await
        .expect("default services");
    let store = Arc::new(CoordStore::open(&path).expect("final reopen"));
    services.coord_store = store.clone();
    let app = create_simple_app(services).await.expect("simple app");
    let server = TestServer::new(app).expect("test server");

    let res = server
        .post("/api/v1/coord/turn")
        .add_header("X-Concord-Principal", "loop:a")
        .json(&json!({
            "session_id": "s-up",
            "cwd": "/w/repo",
            "hook_event_name": "UserPromptSubmit",
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_no_permission_decision(&body);
    assert_eq!(body["principal_id"], "loop:a");

    // The pre-upgrade row's NULL cursor is initialised by that first UPS…
    assert!(
        store
            .digest_cursor("loop:a")
            .expect("cursor read")
            .is_some(),
        "first UPS on the upgraded DB must initialise digest_seq"
    );
    // …and the upgraded DB then digests normally.
    let mut b = PrincipalUpsert::default();
    b.harness = Some("codex".into());
    store.upsert_principal("loop:b", b).expect("upsert b");
    fp(&store, "loop:a", "read", "/r/upgraded.rs");
    fp(&store, "loop:b", "write", "/r/upgraded.rs");
    let body2 = ups(&server, "loop:a", "s-upgrade", "UserPromptSubmit").await;
    assert!(
        ctx(&body2).contains("changed by other agents"),
        "upgraded DB must digest normally; got {:?}",
        ctx(&body2)
    );
}

// ─────────────────── metrics bump on emitted lines ───────────────────

#[tokio::test]
async fn metrics_coord_digest_lines_total_advances_by_emitted_lines() {
    let h = make_harness().await;
    ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    // Touch 3 paths so 3 lines will be emitted.
    for i in 0..3 {
        fp(&h.store, "loop:a", "read", &format!("/r/f{i}.rs"));
    }
    let mut b = PrincipalUpsert::default();
    b.harness = Some("codex".into());
    h.store.upsert_principal("loop:b", b).expect("upsert b");
    for i in 0..3 {
        fp(&h.store, "loop:b", "write", &format!("/r/f{i}.rs"));
    }

    let before = metrics(&h.server).await;
    let before_total = before["coord_digest_lines_total"].as_u64().unwrap_or(0);

    let body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit").await;
    let ctx_text = ctx(&body);
    let emitted = ctx_text.lines().filter(|l| l.starts_with("- /r/f")).count();
    assert_eq!(emitted, 3, "expected 3 per-path lines; got {ctx_text:?}");

    let after = metrics(&h.server).await;
    let after_total = after["coord_digest_lines_total"].as_u64().unwrap_or(0);
    assert_eq!(
        after_total,
        before_total + emitted as u64,
        "metric advanced by exactly the emitted per-path line count"
    );
}

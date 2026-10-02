//! Concord P1 — per-principal footprint capture + PreToolUse
//! precheck end-to-end suite.
//!
//! Drives `POST /api/v1/coord/footprints` and
//! `POST /api/v1/coord/precheck` through `create_simple_app` plus
//! `axum_test::TestServer`. Verifies:
//!
//! - dod1: precise warn when another principal wrote after the
//!   caller's last read on the same path
//! - dod2: fresh read resets the warn state
//! - dod3: same-principal write doesn't warn
//! - dod4: lineage (parent in `labels.parent`) does not warn,
//!   non-lineage DOES warn (control)
//! - dod5: no prior footprint on a path → no warn
//! - dod6: non-Edit-class tools (Bash) → recorded=false / warn=false
//! - dod7: `get_footprint` returns stat fields, retention prunes
//!   rows older than the configured window at `open()` time
//! - dod9: `coord_precheck_total` and `coord_precheck_warn` counters
//!   advance on each precheck
//!
//! All endpoints always return 200; malformed bodies also return
//! 200 with the no-op shape.

use axum_test::TestServer;
use chrono::{Duration as ChronoDuration, Utc};
use contextnest::api::create_simple_app;
use contextnest::services::coord_store::{CoordStore, PrincipalUpsert};
use contextnest::services::ContextNestServices;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

// ─────────────────── harness ───────────────────

struct Harness {
    server: TestServer,
    /// Cloned out of `services` before it was consumed by
    /// `create_simple_app`, so unit-style assertions against
    /// `record_footprint`, `get_footprint`, `lineage` etc. work
    /// without going through HTTP.
    store: Arc<CoordStore>,
    _tmp: TempDir,
}

async fn make_harness() -> Harness {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("coord.db");
    // Pre-create the store so `coord_footprints` (which reads
    // the env-controlled retention on first open) sees the right
    // baseline; then re-open for the live service.
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
    }
}

fn write_file(path: &std::path::Path, body: &[u8]) {
    std::fs::write(path, body).expect("write file");
}

// ─────────────────── footprint helper ───────────────────

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
    res.json()
}

async fn metrics(server: &TestServer) -> Value {
    let res = server.get("/api/v1/coord/metrics").await;
    res.assert_status(axum::http::StatusCode::OK);
    res.json()
}

// ─────────────────── DoD 1 — precise case ───────────────────

#[tokio::test]
async fn dod1_precise_warn_with_binding_path_fallback() {
    let h = make_harness().await;
    let dir = h._tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"first version");

    let canonical = std::fs::canonicalize(&f).expect("canonical");
    let canonical_str = canonical.to_string_lossy().to_string();
    let cwd = dir.to_string_lossy().to_string();

    // S1 reads f with the explicit loop:a header.
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
    // S2 writes f (loop:b via explicit header).
    std::fs::write(&f, b"second version").unwrap();
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "s2".to_string(),
            principal_header: Some("loop:b".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    // S1 wants to edit f. S1 has no principal header — resolve
    // should fall through to the session binding, which the
    // first footprint set up. Expect warn=true, others[0] is
    // loop:b, and permissionDecision is absent.
    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "s1".to_string(),
            principal_header: None,
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res["warn"], json!(true));
    let others = res["others"].as_array().expect("others array");
    assert_eq!(others.len(), 1);
    assert_eq!(others[0]["principal_id"], json!("loop:b"));
    assert!(
        res["hookSpecificOutput"]["permissionDecision"].is_null(),
        "precheck must never set permissionDecision (allow would skip the user's prompt)"
    );
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("ctx string");
    assert!(
        ctx.contains(&canonical_str),
        "additionalContext must include the canonical path; got: {ctx}",
    );
    assert!(
        ctx.contains("after you last read it"),
        "additionalContext must surface the read-then-write race; got: {ctx}",
    );
}

// ─────────────────── DoD 2 — fresh read resets warn ───────────────────

#[tokio::test]
async fn dod2_fresh_read_resets_warn_state() {
    let h = make_harness().await;
    let dir = h._tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // S1 reads, S2 writes, S1 prechecks (warn=true).
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
    let first = post_precheck(
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
    assert_eq!(first["warn"], json!(true));

    // S1 reads again → last_footprint_seq advances past the
    // loop:b write → no warn.
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
    let second = post_precheck(
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
    assert_eq!(second["warn"], json!(false));
    assert!(second["others"].as_array().unwrap().is_empty());
}

// ─────────────────── DoD 3 — same principal doesn't warn ───────────────────

#[tokio::test]
async fn dod3_same_principal_write_does_not_warn() {
    let h = make_harness().await;
    let dir = h._tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // S1 (loop:a) reads, then S3 (also loop:a) writes. Same
    // principal — precheck should not warn.
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
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "s3".to_string(),
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
}

// ─────────────────── DoD 4 — lineage (parent in labels) ───────────────────

#[tokio::test]
async fn dod4_lineage_excludes_parent_but_not_sibling() {
    let h = make_harness().await;
    let dir = h._tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // Seed run:x with labels {parent: loop:a} through the
    // cloned store. It walks DOWN from loop:a — the lineage
    // set for loop:a includes run:x. So a run:x write should
    // NOT warn loop:a, and a loop:a write should NOT warn
    // run:x.
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
    h.store
        .upsert_principal(
            "loop:b",
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
    // run:x (lineage) writes → must NOT warn.
    std::fs::write(&f, b"v2").unwrap();
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
    let no_warn = post_precheck(
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
    assert_eq!(
        no_warn["warn"],
        json!(false),
        "lineage descendant must not warn; got: {no_warn}"
    );

    // run:x's labels.parent must SURVIVE its own footprint
    // posts (regression for the bulk-upsert clobber).
    let x = h
        .store
        .get_principal("run:x")
        .unwrap()
        .expect("run:x present");
    assert_eq!(
        x.labels
            .as_ref()
            .and_then(|v| v.get("parent"))
            .and_then(|v| v.as_str()),
        Some("loop:a"),
        "labels.parent must survive footprint posts (no clobber); got: {:?}",
        x.labels,
    );

    // Reverse direction: an ANCESTOR (loop:a) writing must not warn its
    // descendant (run:x) either — lineage excludes both ways.
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sX".to_string(),
            principal_header: Some("run:x".to_string()),
            tool: "Read".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    std::fs::write(&f, b"v2b").unwrap();
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sA".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    let no_warn_rev = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sX".to_string(),
            principal_header: Some("run:x".to_string()),
            tool: "Edit".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(
        no_warn_rev["warn"],
        json!(false),
        "lineage ancestor must not warn its descendant; got: {no_warn_rev}"
    );

    // CONTROL: loop:b is NOT in loop:a's lineage. loop:b writes
    // and then loop:a prechecks → MUST warn. Without this
    // control the dod4 test is vacuous.
    std::fs::write(&f, b"v3").unwrap();
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sB".to_string(),
            principal_header: Some("loop:b".to_string()),
            tool: "Edit".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    let warn = post_precheck(
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
    assert_eq!(
        warn["warn"],
        json!(true),
        "non-lineage writer MUST warn; got: {warn}"
    );
    let others = warn["others"].as_array().unwrap();
    assert!(
        others.iter().any(|o| o["principal_id"] == "loop:b"),
        "loop:b must appear in others; got: {others:?}",
    );
}

// ─────────────────── DoD 5 — no prior footprint → no warn ───────────────────

#[tokio::test]
async fn dod5_no_prior_footprint_means_no_warn() {
    let h = make_harness().await;
    let dir = h._tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // S1 (loop:a) never touched f. S2 (loop:b) writes. S1
    // prechecks → no warn (no prior footprint on f, so we
    // don't know what to compare against).
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
    assert!(res["others"].as_array().unwrap().is_empty());
}

// ─────────────────── DoD 6 — non-Edit tools are no-ops ───────────────────

#[tokio::test]
async fn dod6_non_file_tools_are_no_ops() {
    let h = make_harness().await;
    let dir = h._tmp.path().to_path_buf();
    let cwd = dir.to_string_lossy().to_string();

    // Bash footprint → recorded=false (not a file tool).
    let res = h
        .server
        .post("/api/v1/coord/footprints")
        .add_header("X-Concord-Principal", "loop:a")
        .json(&json!({
            "session_id": "s1",
            "tool_name": "Bash",
            "tool_input": { "command": "ls" },
            "cwd": cwd.clone(),
        }))
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["recorded"], json!(false));

    // Bash precheck → warn=false, empty context, 'allow'.
    let body = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Bash".to_string(),
            path: dir.join("f.txt"),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(body["warn"], json!(false));
    assert!(body["others"].as_array().unwrap().is_empty());
    assert!(
        body["hookSpecificOutput"]["permissionDecision"].is_null(),
        "precheck must never set permissionDecision (allow would skip the user's prompt)"
    );
    assert_eq!(body["hookSpecificOutput"]["additionalContext"], json!(""),);
}

// ─────────────────── DoD 7 — stat + retention ───────────────────

#[tokio::test]
async fn dod7_stat_fields_populated_for_existing_files() {
    let h = make_harness().await;
    let dir = h._tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"hello");
    let size = std::fs::metadata(&f).unwrap().len() as i64;
    let cwd = dir.to_string_lossy().to_string();
    // The handler canonicalizes the file path (macOS /var → /private/var).
    // Look it up via the canonical form so the seq lookup matches the
    // stored path.
    let canonical = std::fs::canonicalize(&f).expect("canonical");

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
    // Pull the seq from the most recent footprint for loop:a.
    let seq = h
        .store
        .last_footprint_seq("loop:a", &canonical.to_string_lossy())
        .unwrap()
        .expect("seq present");
    let fp = h.store.get_footprint(seq).unwrap().expect("present");
    assert!(fp.mtime_ns.is_some());
    assert_eq!(fp.size, Some(size));

    // Missing path → record with mtime/size None. The path is kept
    // as the joined-but-not-canonicalized form (canonicalize fails
    // for a missing file).
    let missing = dir.join("does_not_exist");
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Read".to_string(),
            path: missing.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    let stored_missing = h
        .store
        .last_footprint_seq("loop:a", &missing.to_string_lossy())
        .unwrap()
        .expect("seq present");
    let fp2 = h
        .store
        .get_footprint(stored_missing)
        .unwrap()
        .expect("present");
    assert!(fp2.mtime_ns.is_none());
    assert!(fp2.size.is_none());
}

#[test]
fn dod7_retention_prunes_at_open() {
    // File-backed store; record two footprints, backdate one, drop
    // and reopen. The backdated row is gone; the fresh row is kept.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("coord.db");
    let s = CoordStore::open(&path).expect("open");
    s.upsert_principal(
        "loop:a",
        PrincipalUpsert {
            harness: Some("h".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let seq_fresh = s
        .record_footprint("loop:a", "w1", "read", "/f.rs", Some(1), Some(10))
        .unwrap();
    let seq_old = s
        .record_footprint("loop:a", "w1", "read", "/f.rs", Some(2), Some(20))
        .unwrap();
    // Backdate the second row to now - 8 days (default retention is 7).
    let cutoff =
        (Utc::now() - ChronoDuration::days(8)).to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
    {
        let conn = Connection::open(&path).expect("raw conn");
        conn.execute(
            "UPDATE footprints SET ts = ?1 WHERE seq = ?2",
            params![cutoff, seq_old],
        )
        .expect("backdate");
    }
    drop(s);
    // Reopen — retention-at-open must prune the backdated row.
    let s2 = CoordStore::open(&path).expect("reopen");
    assert!(
        s2.get_footprint(seq_old).unwrap().is_none(),
        "old row pruned"
    );
    assert!(
        s2.get_footprint(seq_fresh).unwrap().is_some(),
        "fresh row kept"
    );
}

// ─────────────────── DoD 9 — precheck metrics ───────────────────

#[tokio::test]
async fn dod9_precheck_metrics_advance() {
    let h = make_harness().await;
    let dir = h._tmp.path().to_path_buf();
    let f = dir.join("f.txt");
    write_file(&f, b"v1");
    let cwd = dir.to_string_lossy().to_string();

    // Establish a prior footprint for loop:a so the precheck
    // has something to compare against.
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
    // loop:b writes.
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

    let before = metrics(&h.server).await;
    let before_total = before["coord_precheck_total"].as_u64().unwrap_or(0);
    let before_warn = before["coord_precheck_warn"].as_u64().unwrap_or(0);

    // Precheck #1: warn=true (loop:b wrote after loop:a read).
    let one = post_precheck(
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
    assert_eq!(one["warn"], json!(true));

    // Precheck #2: no further writes since the last warn →
    // still warn=true (others query looks after last seq).
    let two = post_precheck(
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
    assert_eq!(two["warn"], json!(true));

    // Precheck #3: a no-op (non-file tool). It must still count toward
    // coord_precheck_total, but not toward coord_precheck_warn.
    let three = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "s1".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Bash".to_string(),
            path: f.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(three["warn"], json!(false));

    let after = metrics(&h.server).await;
    let after_total = after["coord_precheck_total"].as_u64().unwrap_or(0);
    let after_warn = after["coord_precheck_warn"].as_u64().unwrap_or(0);
    assert_eq!(
        after_total - before_total,
        3,
        "every precheck counts, no-ops included"
    );
    assert_eq!(
        after_warn - before_warn,
        2,
        "only the two warnings count as warns"
    );
}

// ─────────────────── Malformed body / 200 contract ───────────────────

#[tokio::test]
async fn malformed_body_returns_200_with_no_op_shape() {
    let h = make_harness().await;
    // Footprints endpoint: malformed JSON → 200, recorded=false.
    let res = h
        .server
        .post("/api/v1/coord/footprints")
        .add_header("content-type", "application/json")
        .text("this is not json")
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["recorded"], json!(false));

    // Precheck endpoint: malformed JSON → 200, warn=false, allow.
    let res = h
        .server
        .post("/api/v1/coord/precheck")
        .add_header("content-type", "application/json")
        .text("this is not json either")
        .await;
    res.assert_status(axum::http::StatusCode::OK);
    let body: Value = res.json();
    assert_eq!(body["warn"], json!(false));
    assert!(
        body["hookSpecificOutput"]["permissionDecision"].is_null(),
        "precheck must never set permissionDecision (allow would skip the user's prompt)"
    );
}

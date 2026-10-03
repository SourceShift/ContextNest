//! Concord P2 — hot shared-config claims end-to-end suite.
//!
//! Drives `POST /api/v1/coord/footprints`, `POST /api/v1/coord/precheck`
//! and `GET /api/v1/coord/hot-claims` through `create_simple_app` plus
//! `axum_test::TestServer`. Verifies:
//!
//! - dod1: default warn — a live claim on a hot path surfaces the 🔒
//!   advisory in `additionalContext` (canonical path + holder) with no
//!   `permissionDecision` key
//! - dod2: `CONTEXTNEST_CONCORD_HOT_MODE=ask` → `permissionDecision=="ask"`,
//!   `deny` → `"deny"`, both with a non-empty reason
//! - dod3: the holder and a `labels.parent` lineage child see no conflict
//! - dod4: a non-hot path (`src/main.rs`) never claims and never conflicts,
//!   even in deny mode
//! - dod5: after the 1s TTL expires the outsider write becomes the holder
//! - dod6: a contended write keeps the holder and advances
//!   `coord_hot_contended_total` by exactly 1
//! - dod7: `GET /api/v1/coord/hot-claims` returns only live claims
//! - dod8: the shared precheck helper asserts no response anywhere carries
//!   `permissionDecision == "allow"`
//!
//! The handlers read `CONTEXTNEST_CONCORD_HOT_{GLOBS,MODE,TTL_SECS}` fresh
//! per request, but all tests in one binary share the process env — every
//! test therefore holds `ENV_LOCK` (poison-tolerantly) and installs an RAII
//! guard that restores the three vars on drop.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::services::coord_store::{CoordStore, PrincipalUpsert};
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Serialise access to the three process-wide hot-config env vars.
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

    /// Clear all three hot-config vars (the default warn/glob/ttl set).
    fn clear_hot() -> EnvGuard {
        EnvGuard::set(&[
            ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
            ("CONTEXTNEST_CONCORD_HOT_MODE", None),
            ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ])
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

/// Recursive DoD8 check: no `permissionDecision` value anywhere may be
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
    /// `create_simple_app`, for unit-style seeding of lineage labels.
    store: Arc<CoordStore>,
    _tmp: TempDir,
}

async fn make_harness() -> Harness {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("coord.db");
    // Pre-create the store so `coord_footprints` (which reads the
    // env-controlled retention on first open) sees the right baseline;
    // then re-open for the live service.
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

/// Create a real hot file under `<tmp>/.mini-ork/config/agents.yaml` and
/// return its (un-canonicalized) path. The file must exist so both the
/// footprint and precheck canonicalize to the same `/private/var/...`
/// string on macOS.
fn hot_file(dir: &std::path::Path) -> PathBuf {
    let p = dir.join(".mini-ork/config/agents.yaml");
    std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
    std::fs::write(&p, b"agent: {}").expect("write");
    p
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
    // DoD8 — enforced on every precheck response, recursively.
    assert_no_permission_allow(&value);
    value
}

async fn metrics(server: &TestServer) -> Value {
    let res = server.get("/api/v1/coord/metrics").await;
    res.assert_status(axum::http::StatusCode::OK);
    res.json()
}

async fn get_hot_claims(server: &TestServer) -> Value {
    let res = server.get("/api/v1/coord/hot-claims").await;
    res.assert_status(axum::http::StatusCode::OK);
    res.json()
}

// ─────────────────── DoD 1 — default warn ───────────────────

#[tokio::test]
async fn dod1_warn_default_hot_conflict_advisory_only() {
    let _lock = lock_env();
    let _env = EnvGuard::clear_hot();
    let h = make_harness().await;
    let f = hot_file(h._tmp.path());
    let canonical = std::fs::canonicalize(&f).expect("canonical");
    let canonical_str = canonical.to_string_lossy().to_string();
    let cwd = h._tmp.path().to_string_lossy().to_string();

    // loop:a writes the hot file → becomes the holder.
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sA".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    // loop:b (outsider) prechecks → hot conflict with holder loop:a.
    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sB".to_string(),
            principal_header: Some("loop:b".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res["hot_conflict"], json!(true));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("ctx string");
    assert!(
        ctx.contains('\u{1F512}'),
        "ctx must carry the lock glyph: {ctx}"
    );
    assert!(
        ctx.contains(&canonical_str),
        "ctx must carry the canonical path: {ctx}",
    );
    assert!(ctx.contains("loop:a"), "ctx must name the holder: {ctx}");
    // Warn mode (default) → no permissionDecision anywhere.
    assert!(
        res["hookSpecificOutput"]["permissionDecision"].is_null(),
        "warn mode must leave permissionDecision absent"
    );
    assert!(
        res["permissionDecision"].is_null(),
        "no top-level permissionDecision either"
    );
}

// ─────────────────── DoD 2 — ask / deny modes ───────────────────

#[tokio::test]
async fn dod2_ask_and_deny_modes_set_permission_decision() {
    let _lock = lock_env();

    // ask
    {
        let _env = EnvGuard::set(&[
            ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
            ("CONTEXTNEST_CONCORD_HOT_MODE", Some("ask")),
            ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ]);
        let h = make_harness().await;
        let f = hot_file(h._tmp.path());
        let canonical = std::fs::canonicalize(&f).expect("canonical");
        let cwd = h._tmp.path().to_string_lossy().to_string();
        post_footprint(
            &h.server,
            &FootprintPost {
                sid: "sA".to_string(),
                principal_header: Some("loop:a".to_string()),
                tool: "Edit".to_string(),
                path: canonical.clone(),
                cwd: Some(cwd.clone()),
            },
        )
        .await;
        let res = post_precheck(
            &h.server,
            &PrecheckPost {
                sid: "sB".to_string(),
                principal_header: Some("loop:b".to_string()),
                tool: "Edit".to_string(),
                path: canonical.clone(),
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
        assert!(
            reason.contains("loop:a"),
            "reason names the holder: {reason}"
        );
    }

    // deny
    {
        let _env = EnvGuard::set(&[
            ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
            ("CONTEXTNEST_CONCORD_HOT_MODE", Some("deny")),
            ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
        ]);
        let h = make_harness().await;
        let f = hot_file(h._tmp.path());
        let canonical = std::fs::canonicalize(&f).expect("canonical");
        let cwd = h._tmp.path().to_string_lossy().to_string();
        post_footprint(
            &h.server,
            &FootprintPost {
                sid: "sA".to_string(),
                principal_header: Some("loop:a".to_string()),
                tool: "Edit".to_string(),
                path: canonical.clone(),
                cwd: Some(cwd.clone()),
            },
        )
        .await;
        let res = post_precheck(
            &h.server,
            &PrecheckPost {
                sid: "sB".to_string(),
                principal_header: Some("loop:b".to_string()),
                tool: "Edit".to_string(),
                path: canonical.clone(),
                cwd: Some(cwd.clone()),
            },
        )
        .await;
        assert_eq!(
            res["hookSpecificOutput"]["permissionDecision"],
            json!("deny")
        );
        assert!(res["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false));
    }
}

// ─────────────────── DoD 3 — lineage exemption ───────────────────

#[tokio::test]
async fn dod3_lineage_holder_and_child_see_no_conflict() {
    let _lock = lock_env();
    let _env = EnvGuard::clear_hot();
    let h = make_harness().await;

    // Seed run:x as a lineage child of loop:a (labels.parent).
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

    let f = hot_file(h._tmp.path());
    let canonical = std::fs::canonicalize(&f).expect("canonical");
    let cwd = h._tmp.path().to_string_lossy().to_string();

    // loop:a writes → holder loop:a.
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sA".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    // Lineage child run:x prechecks → no 🔒, no permissionDecision.
    let child = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sX".to_string(),
            principal_header: Some("run:x".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(child["hot_conflict"], json!(false));
    assert!(child["hookSpecificOutput"]["permissionDecision"].is_null());
    let child_ctx = child["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("");
    assert!(
        !child_ctx.contains('\u{1F512}'),
        "lineage child must not see the lock: {child_ctx}",
    );

    // The holder itself prechecks → no conflict either.
    let holder = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sA".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(holder["hot_conflict"], json!(false));
    assert!(holder["hookSpecificOutput"]["permissionDecision"].is_null());
}

// ─────────────────── DoD 4 — non-hot path is a no-op ───────────────────

#[tokio::test]
async fn dod4_non_hot_path_never_claims_or_conflicts() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_MODE", Some("deny")),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", None),
    ]);
    let h = make_harness().await;
    let f = h._tmp.path().join("src/main.rs");
    std::fs::create_dir_all(f.parent().expect("parent")).expect("mkdir");
    std::fs::write(&f, b"fn main() {}").expect("write");
    let canonical = std::fs::canonicalize(&f).expect("canonical");
    let cwd = h._tmp.path().to_string_lossy().to_string();

    // loop:a writes a non-hot file.
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sA".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    // No claim was created.
    let claims = get_hot_claims(&h.server).await;
    assert_eq!(
        claims["claims"].as_array().expect("claims array").len(),
        0,
        "non-hot write must not claim"
    );

    // Even in deny mode, a non-hot path never conflicts.
    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sB".to_string(),
            principal_header: Some("loop:b".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res["hot_conflict"], json!(false));
    assert!(res["hookSpecificOutput"]["permissionDecision"].is_null());
}

// ─────────────────── DoD 5 — TTL expiry transfers the holder ───────────────────

#[tokio::test]
async fn dod5_ttl_expiry_transfers_holder() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", Some("1")),
    ]);
    let h = make_harness().await;
    let f = hot_file(h._tmp.path());
    let canonical = std::fs::canonicalize(&f).expect("canonical");
    let cwd = h._tmp.path().to_string_lossy().to_string();

    // loop:a writes → holder loop:a for 1s.
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sA".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    // Sleep past the TTL.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    // loop:b writes → takes the expired claim.
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sB".to_string(),
            principal_header: Some("loop:b".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    // loop:a now sees the conflict with holder loop:b.
    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sA".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res["hot_conflict"], json!(true));
    let ctx = res["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("ctx string");
    assert!(ctx.contains('\u{1F512}'), "ctx must carry the lock: {ctx}");
    assert!(
        ctx.contains("loop:b"),
        "ctx must name the new holder: {ctx}"
    );

    // And the stored holder really is loop:b.
    let claims = get_hot_claims(&h.server).await;
    let arr = claims["claims"].as_array().expect("claims array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["principal_id"], json!("loop:b"));
}

// ─────────────────── DoD 6 — contention keeps holder + counter ───────────────────

#[tokio::test]
async fn dod6_contended_write_keeps_holder_and_bumps_counter() {
    let _lock = lock_env();
    let _env = EnvGuard::clear_hot();
    let h = make_harness().await;
    let f = hot_file(h._tmp.path());
    let canonical = std::fs::canonicalize(&f).expect("canonical");
    let cwd = h._tmp.path().to_string_lossy().to_string();

    // loop:a writes → holder.
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sA".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    let m0 = metrics(&h.server).await;
    let contended0 = m0["coord_hot_contended_total"].as_u64().unwrap_or(0);
    let conflicts0 = m0["coord_hot_conflicts_total"].as_u64().unwrap_or(0);

    // loop:b writes during the live claim → contended, holder unchanged.
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sB".to_string(),
            principal_header: Some("loop:b".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    let m1 = metrics(&h.server).await;
    let contended1 = m1["coord_hot_contended_total"].as_u64().unwrap_or(0);
    assert_eq!(contended1 - contended0, 1, "exactly one contended write");

    // Holder unchanged in the listing.
    let claims = get_hot_claims(&h.server).await;
    let arr = claims["claims"].as_array().expect("claims array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["principal_id"], json!("loop:a"));

    // loop:b prechecks → hot conflict → conflicts counter +1.
    let res = post_precheck(
        &h.server,
        &PrecheckPost {
            sid: "sB".to_string(),
            principal_header: Some("loop:b".to_string()),
            tool: "Edit".to_string(),
            path: canonical.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;
    assert_eq!(res["hot_conflict"], json!(true));

    let m2 = metrics(&h.server).await;
    let conflicts2 = m2["coord_hot_conflicts_total"].as_u64().unwrap_or(0);
    assert_eq!(
        conflicts2 - conflicts0,
        1,
        "exactly one conflicting precheck"
    );
}

// ─────────────────── DoD 7 — listing returns only live claims ───────────────────

#[tokio::test]
async fn dod7_list_returns_only_live_claims() {
    let _lock = lock_env();
    let _env = EnvGuard::set(&[
        ("CONTEXTNEST_CONCORD_HOT_GLOBS", None),
        ("CONTEXTNEST_CONCORD_HOT_MODE", None),
        ("CONTEXTNEST_CONCORD_HOT_TTL_SECS", Some("1")),
    ]);
    let h = make_harness().await;
    let cwd = h._tmp.path().to_string_lossy().to_string();

    // Path A: a 1s claim that will expire.
    let fa = hot_file(h._tmp.path());
    let canon_a = std::fs::canonicalize(&fa).expect("canonical");
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sA".to_string(),
            principal_header: Some("loop:a".to_string()),
            tool: "Edit".to_string(),
            path: canon_a.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    // Sleep past the 1s TTL so A expires.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    // Path B: a long-lived claim on a different hot file.
    std::env::set_var("CONTEXTNEST_CONCORD_HOT_TTL_SECS", "600");
    let fb = h._tmp.path().join(".env");
    std::fs::write(&fb, b"KEY=1").expect("write");
    let canon_b = std::fs::canonicalize(&fb).expect("canonical");
    let canon_b_str = canon_b.to_string_lossy().to_string();
    post_footprint(
        &h.server,
        &FootprintPost {
            sid: "sB".to_string(),
            principal_header: Some("loop:b".to_string()),
            tool: "Edit".to_string(),
            path: canon_b.clone(),
            cwd: Some(cwd.clone()),
        },
    )
    .await;

    // Only B is live.
    let claims = get_hot_claims(&h.server).await;
    let arr = claims["claims"].as_array().expect("claims array");
    assert_eq!(arr.len(), 1, "only the live claim survives: {claims}");
    assert_eq!(arr[0]["path"], json!(canon_b_str));
    assert_eq!(arr[0]["principal_id"], json!("loop:b"));
}

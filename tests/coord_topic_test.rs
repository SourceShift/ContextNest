//! Concord P3 — topic-overlap advisory end-to-end suite.
//!
//! Drives `POST /api/v1/coord/turn` (and the two calibration GETs)
//! through `create_simple_app` plus `axum_test::TestServer`, exercising
//! every line of the Definition of Done:
//!
//! - dod1: prompt capture is async (does not block the hook) and is
//!   polled into the intents table within ~2s
//! - dod2: `CONTEXTNEST_CONCORD_TOPIC=1` enables a per-turn advisory
//!   line with similarity ≥ threshold, deduped via the topic_notices
//!   table; coord_topic_notices_total advances by exactly 1
//! - dod3: env unset → no advisory line; calibration GETs still serve
//!   the live intents + pairs with no embedding leakage
//! - dod4: lineage, ended, stale, dim-mismatch, and below-threshold
//!   exclusions each suppress the line; the lineage case also omits the
//!   pair from `topic-pairs`
//! - dod5: prompt with <20 non-whitespace chars is NOT captured
//! - dod6: blob round-trip preserves every defined bit; a 7-byte blob
//!   surfaces `InvalidBody`
//!
//! Plus the recursive `assert_no_permission_decision` check on every
//! response, matching the P2c / P2d suites.

use axum_test::TestServer;
use chrono::{Duration as ChronoDuration, Utc};
use contextnest::api::create_simple_app;
use contextnest::services::coord_store::{
    decode_embedding, encode_embedding, CoordStore, PrincipalUpsert,
};
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

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
    store: Arc<CoordStore>,
    _tmp: TempDir,
    // Declared BEFORE _env_lock so Drop order restores env vars first
    // (Drop runs fields in declaration order, reverse). Holding the env
    // lock for the whole test applies the P2c flake fix (1/40 stale-setvar).
    _env_topic: EnvGuard,
    _env_threshold: EnvGuard,
    _env_window: EnvGuard,
    _env_dedup: EnvGuard,
    _env_alpha: EnvGuard,
    _env_lock: std::sync::MutexGuard<'static, ()>,
}

async fn make_harness() -> Harness {
    let env_lock = lock_env();
    let topic = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC");
    let threshold = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD");
    let window = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS");
    let dedup = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_DEDUP_SECS");
    let alpha = EnvGuard::clear("CONTEXTNEST_CONCORD_INTENT_ALPHA");

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
        _env_topic: topic,
        _env_threshold: threshold,
        _env_window: window,
        _env_dedup: dedup,
        _env_alpha: alpha,
        _env_lock: env_lock,
    }
}

/// Cosine of two equal-length vectors, accumulating in f64 for
/// stability. Mirrors the unit-norm assumption the blended path
/// encodes — both inputs are expected to already be normalized; this
/// helper divides by their norms defensively so a caller that passes
/// raw vectors still gets a comparable answer.
fn cos(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len(), "cos: dim mismatch");
    let mut dot = 0.0_f64;
    let mut na = 0.0_f64;
    let mut nb = 0.0_f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let xf = f64::from(*x);
        let yf = f64::from(*y);
        dot += xf * yf;
        na += xf * xf;
        nb += yf * yf;
    }
    let denom = (na.sqrt()) * (nb.sqrt());
    if denom == 0.0 {
        0.0
    } else {
        (dot / denom) as f32
    }
}

// ─────────────────── request helpers ───────────────────

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
    res.assert_status(axum::http::StatusCode::OK);
    let v: Value = res.json();
    assert_no_permission_decision(&v);
    v
}

fn ctx(body: &Value) -> String {
    body["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

async fn metrics(server: &TestServer) -> Value {
    let res = server.get("/api/v1/coord/metrics").await;
    res.assert_status(axum::http::StatusCode::OK);
    res.json()
}

async fn list_intents(server: &TestServer) -> Value {
    let res = server.get("/api/v1/coord/intents").await;
    res.assert_status(axum::http::StatusCode::OK);
    let v: Value = res.json();
    assert_no_permission_decision(&v);
    v
}

async fn topic_pairs(server: &TestServer, min: Option<f32>) -> Value {
    let mut url = "/api/v1/coord/topic-pairs".to_string();
    if let Some(m) = min {
        url.push_str(&format!("?min={m}"));
    }
    let res = server.get(&url).await;
    res.assert_status(axum::http::StatusCode::OK);
    let v: Value = res.json();
    assert_no_permission_decision(&v);
    v
}

fn upsert_principal(store: &CoordStore, id: &str, parent: Option<&str>) {
    let mut upsert = PrincipalUpsert::default();
    if let Some(p) = parent {
        upsert.labels = Some(json!({ "parent": p }));
    } else {
        upsert.labels = Some(json!({}));
    }
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

// ─────────────────── DoD 1 — async capture polled ───────────────────

#[tokio::test]
async fn dod1_capture_is_async_and_polled() {
    let h = make_harness().await;
    // Send a >500-char prompt: capture_text must clamp to exactly 500
    // chars, the spawn must land within ~2s, dim > 0.
    let prompt: String = (0..600).map(|i| ((i % 26) as u8 + b'a') as char).collect();
    assert!(prompt.chars().count() > 500);
    let body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit", Some(&prompt)).await;
    assert_eq!(body["principal_id"], "loop:a");
    assert_eq!(body["bound"], true);

    let mut found: Option<(String, usize, usize)> = None;
    for _ in 0..80 {
        // ~2s @ 25ms.
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        if let Some(intent) = h.store.get_intent("loop:a").expect("get_intent") {
            found = Some((intent.text, intent.embedding.len(), intent.dim));
            break;
        }
    }
    let (text, emb_len, dim) = found.expect("intent row present within 2s");
    assert_eq!(
        text.chars().count(),
        500,
        "captured text truncated to 500 chars"
    );
    assert!(dim > 0, "dim > 0, got {dim}");
    assert_eq!(emb_len, dim, "embedding.len() == dim");
}

// ─────────────────── DoD 2 — notice gating + dedup ───────────────────

#[tokio::test]
async fn dod2_notice_gating_with_dedup_and_metric() {
    let env_lock = lock_env();
    let topic = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC");
    let threshold = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD");
    let window = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS");
    let dedup = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_DEDUP_SECS");

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

    // Flip TOPIC=1 after wiring but before any UPS — avoid the harness's
    // blanket clear.
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC", "1");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD", "0.85");

    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);
    // Near-identical vectors: cosine ≈ 0.9988.
    let a = [1.0_f32, 0.0, 0.0, 0.0];
    let b = [1.0_f32, 0.05, 0.0, 0.0];
    seed_intent(&store, "loop:a", "alpha intent", &a);
    seed_intent(&store, "loop:b", "beta intent", &b);

    let before = metrics(&server).await;
    let before_total = before["coord_topic_notices_total"].as_u64().unwrap_or(0);

    // Critical: NO prompt on this UPS so the spawned task cannot
    // overwrite loop:a's hand-made 4-d vector with a local-embedder
    // vector of another dim. Compare path must see the seeded vec.
    let body = ups(&server, "loop:a", "s1", "UserPromptSubmit", None).await;
    let ctx_text = ctx(&body);
    assert!(
        ctx_text.contains("↔ loop:b"),
        "must include the topic line; got {ctx_text:?}"
    );
    assert!(
        ctx_text.contains("(similarity "),
        "similarity must render with two-decimal format; got {ctx_text:?}"
    );
    assert!(
        ctx_text.contains("1.00") || ctx_text.contains("0.99"),
        "near-identical vectors render ~1.00 or ~0.99; got {ctx_text:?}"
    );
    assert!(
        ctx_text.contains("\"beta intent\""),
        "first 120 chars of other_text quoted; got {ctx_text:?}"
    );
    assert!(
        ctx_text.contains("mini-ork concord send loop:b \"...\""),
        "send hint present; got {ctx_text:?}"
    );

    let after = metrics(&server).await;
    let after_total = after["coord_topic_notices_total"].as_u64().unwrap_or(0);
    assert_eq!(
        after_total,
        before_total + 1,
        "metric advanced by exactly 1 on first notice"
    );

    // Second UPS — claim_topic_notice is in the dedup window, no line,
    // no metric bump.
    let body2 = ups(&server, "loop:a", "s1", "UserPromptSubmit", None).await;
    let ctx_text2 = ctx(&body2);
    assert!(
        !ctx_text2.contains("↔"),
        "second UPS suppressed by dedup; got {ctx_text2:?}"
    );
    let after2 = metrics(&server).await;
    let after2_total = after2["coord_topic_notices_total"].as_u64().unwrap_or(0);
    assert_eq!(
        after2_total, after_total,
        "metric unchanged when dedup suppresses"
    );

    drop(topic);
    drop(threshold);
    drop(window);
    drop(dedup);
    drop(env_lock);
}

// ─────────────────── DoD 3 — default off, calibration on ───────────────────

#[tokio::test]
async fn dod3_default_off_still_serves_calibration_endpoints() {
    let h = make_harness().await;
    // TOPIC env is unset.
    upsert_principal(&h.store, "loop:a", None);
    upsert_principal(&h.store, "loop:b", None);
    let a = [1.0_f32, 0.0, 0.0, 0.0];
    let b = [1.0_f32, 0.05, 0.0, 0.0];
    seed_intent(&h.store, "loop:a", "alpha intent", &a);
    seed_intent(&h.store, "loop:b", "beta intent", &b);

    // Without TOPIC, no advisory line even with near-identical vectors.
    let body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit", None).await;
    let ctx_text = ctx(&body);
    assert!(
        !ctx_text.contains("↔"),
        "default-off means no advisory line; got {ctx_text:?}"
    );

    // topic-pairs still serves the match.
    let pairs = topic_pairs(&h.server, Some(0.5)).await;
    let arr = pairs["pairs"].as_array().expect("pairs array");
    assert_eq!(arr.len(), 1, "exactly one pair; got {arr:?}");
    let pair = &arr[0];
    let a_id = pair["a"].as_str().unwrap_or("");
    let b_id = pair["b"].as_str().unwrap_or("");
    let s = pair["similarity"].as_f64().unwrap_or(0.0);
    assert!(
        (a_id == "loop:a" && b_id == "loop:b") || (a_id == "loop:b" && b_id == "loop:a"),
        "pair is {{loop:a, loop:b}} in either order; got a={a_id:?} b={b_id:?}"
    );
    assert!(
        s >= 0.99,
        "near-identical vectors → similarity ≥ 0.99; got {s}"
    );
    // Text attribution follows the principal ordering in the pair.
    let a_text = pair["a_text"].as_str().unwrap_or("");
    let b_text = pair["b_text"].as_str().unwrap_or("");
    let (expect_a, expect_b) = if a_id == "loop:a" {
        ("alpha intent", "beta intent")
    } else {
        ("beta intent", "alpha intent")
    };
    assert!(
        a_text.contains(expect_a),
        "a_text must match the {a_id} side; got a_text={a_text:?} expected={expect_a:?}"
    );
    assert!(
        b_text.contains(expect_b),
        "b_text must match the {b_id} side; got b_text={b_text:?} expected={expect_b:?}"
    );

    // /intents lists both with no embedding field.
    let intents = list_intents(&h.server).await;
    let arr = intents["intents"].as_array().expect("intents array");
    assert_eq!(arr.len(), 2, "two intents; got {arr:?}");
    for entry in arr {
        let obj = entry.as_object().expect("object");
        assert!(
            !obj.contains_key("embedding"),
            "intents must not include embedding; got {obj:?}"
        );
        assert!(obj.contains_key("principal_id"));
        assert!(obj.contains_key("text"));
        assert!(obj.contains_key("updated_at"));
    }
}

// ─────────────────── DoD 4 — exclusion matrix ───────────────────

#[tokio::test]
async fn dod4_lineage_excluded() {
    let env_lock = lock_env();
    let topic = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC");
    let threshold = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD");
    let window = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS");
    let dedup = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_DEDUP_SECS");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC", "1");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD", "0.85");

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

    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "run:x", Some("loop:a"));
    let a = [1.0_f32, 0.0, 0.0, 0.0];
    let b = [1.0_f32, 0.05, 0.0, 0.0];
    seed_intent(&store, "loop:a", "alpha intent", &a);
    seed_intent(&store, "run:x", "lineage child", &b);

    let body = ups(&server, "loop:a", "s1", "UserPromptSubmit", None).await;
    let ctx_text = ctx(&body);
    assert!(
        !ctx_text.contains("↔"),
        "lineage child must be excluded; got {ctx_text:?}"
    );

    // topic-pairs must also omit the pair.
    let pairs = topic_pairs(&server, Some(0.5)).await;
    let arr = pairs["pairs"].as_array().expect("pairs array");
    assert!(arr.is_empty(), "lineage pair must be omitted; got {arr:?}");

    drop(topic);
    drop(threshold);
    drop(window);
    drop(dedup);
    drop(env_lock);
}

#[tokio::test]
async fn dod4_ended_excluded() {
    let env_lock = lock_env();
    let topic = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC");
    let threshold = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD");
    let window = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS");
    let dedup = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_DEDUP_SECS");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC", "1");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD", "0.85");

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

    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);
    store.end_principal("loop:b").expect("end loop:b");
    let a = [1.0_f32, 0.0, 0.0, 0.0];
    let b = [1.0_f32, 0.05, 0.0, 0.0];
    seed_intent(&store, "loop:a", "alpha intent", &a);
    seed_intent(&store, "loop:b", "ended peer", &b);

    let body = ups(&server, "loop:a", "s1", "UserPromptSubmit", None).await;
    let ctx_text = ctx(&body);
    assert!(
        !ctx_text.contains("↔"),
        "ended peer must be excluded; got {ctx_text:?}"
    );

    drop(topic);
    drop(threshold);
    drop(window);
    drop(dedup);
    drop(env_lock);
}

#[tokio::test]
async fn dod4_stale_excluded() {
    let env_lock = lock_env();
    let topic = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC");
    let threshold = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD");
    let window = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS");
    let dedup = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_DEDUP_SECS");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC", "1");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD", "0.85");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS", "3600");

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

    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);
    let a = [1.0_f32, 0.0, 0.0, 0.0];
    let b = [1.0_f32, 0.05, 0.0, 0.0];
    let stale = Utc::now() - ChronoDuration::seconds(7200);
    store
        .upsert_intent("loop:a", "alpha", &a, Utc::now())
        .expect("seed a");
    store
        .upsert_intent("loop:b", "beta", &b, stale)
        .expect("seed b at now-7200s");

    let body = ups(&server, "loop:a", "s1", "UserPromptSubmit", None).await;
    let ctx_text = ctx(&body);
    assert!(
        !ctx_text.contains("↔"),
        "stale peer (now-7200s, window=3600) must be excluded; got {ctx_text:?}"
    );

    drop(topic);
    drop(threshold);
    drop(window);
    drop(dedup);
    drop(env_lock);
}

#[tokio::test]
async fn dod4_dim_mismatch_excluded() {
    let env_lock = lock_env();
    let topic = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC");
    let threshold = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD");
    let window = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS");
    let dedup = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_DEDUP_SECS");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC", "1");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD", "0.85");

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

    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);
    // Different dims.
    seed_intent(&store, "loop:a", "alpha", &[1.0_f32, 0.0, 0.0, 0.0]);
    seed_intent(&store, "loop:b", "beta", &[1.0_f32, 0.0, 0.0]);

    let body = ups(&server, "loop:a", "s1", "UserPromptSubmit", None).await;
    let ctx_text = ctx(&body);
    assert!(
        !ctx_text.contains("↔"),
        "dim mismatch must exclude; got {ctx_text:?}"
    );

    drop(topic);
    drop(threshold);
    drop(window);
    drop(dedup);
    drop(env_lock);
}

#[tokio::test]
async fn dod4_below_threshold_excluded() {
    let env_lock = lock_env();
    let topic = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC");
    let threshold = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD");
    let window = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS");
    let dedup = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_DEDUP_SECS");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC", "1");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD", "0.85");

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

    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);
    // Orthogonal vectors → cosine 0.0.
    seed_intent(&store, "loop:a", "alpha", &[1.0_f32, 0.0, 0.0, 0.0]);
    seed_intent(&store, "loop:b", "beta", &[0.0_f32, 1.0, 0.0, 0.0]);

    let body = ups(&server, "loop:a", "s1", "UserPromptSubmit", None).await;
    let ctx_text = ctx(&body);
    assert!(
        !ctx_text.contains("↔"),
        "below-threshold cosine must not trigger; got {ctx_text:?}"
    );

    drop(topic);
    drop(threshold);
    drop(window);
    drop(dedup);
    drop(env_lock);
}

// ─────────────────── DoD 5 — short prompt not captured ───────────────────

#[tokio::test]
async fn dod5_short_prompt_not_captured() {
    let h = make_harness().await;
    // 19 non-whitespace chars padded with whitespace. capture_text's
    // threshold is strict >= 20.
    let prompt = format!("{}        ", "a".repeat(19));
    let body = ups(&h.server, "loop:a", "s1", "UserPromptSubmit", Some(&prompt)).await;
    assert_eq!(body["bound"], true);

    // Allow time for any (incorrect) spawn to land.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        h.store.get_intent("loop:a").expect("get_intent").is_none(),
        "short prompt must NOT be captured into intents"
    );
}

// ─────────────────── DoD 6 — blob round-trip + InvalidBody ───────────────────

#[tokio::test]
async fn dod6_blob_roundtrip_and_invalid_body() {
    let h = make_harness().await;
    // Bit-exact round-trip on every defined float edge.
    let cases: Vec<f32> = vec![
        -0.0,
        0.0,
        f32::MIN_POSITIVE,
        f32::MAX,
        f32::MIN,
        f32::EPSILON,
        // Subnormal — below MIN_POSITIVE, smaller exponent.
        1.0e-40_f32,
        1.234567_f32,
        -98765.4321_f32,
    ];
    let v = cases.clone();
    let blob = encode_embedding(&v);
    assert_eq!(blob.len(), v.len() * 4, "blob is 4 bytes per element");
    let round = decode_embedding(&blob).expect("decode ok");
    assert_eq!(round.len(), v.len());
    for (orig, got) in v.iter().zip(round.iter()) {
        assert_eq!(
            orig.to_bits(),
            got.to_bits(),
            "round-trip preserves bits for {orig}"
        );
    }

    // Round-trip via the store layer.
    upsert_principal(&h.store, "loop:blob", None);
    h.store
        .upsert_intent("loop:blob", "payload", &v, Utc::now())
        .expect("upsert_intent");
    let intent = h
        .store
        .get_intent("loop:blob")
        .expect("get_intent")
        .expect("present");
    assert_eq!(intent.embedding.len(), v.len());
    assert_eq!(intent.dim, v.len());
    for (orig, got) in v.iter().zip(intent.embedding.iter()) {
        assert_eq!(orig.to_bits(), got.to_bits(), "store round-trip");
    }
    assert_eq!(intent.text, "payload");

    // 7-byte blob → InvalidBody, never a panic.
    let err = decode_embedding(&[0u8; 7]).unwrap_err();
    match err {
        contextnest::services::coord_store::CoordStoreError::InvalidBody(_) => {}
        other => panic!("expected InvalidBody, got {other:?}"),
    }
}

// ─────────────────── DoD P3b — blended intent capture ───────────────────

/// Plain cosine between two 4-d unit vectors with no embedder
/// involved. Used to assert blend outputs match the spec math to
/// 1e-3 without standing up an embedding service.
fn cos_4(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let mut dot = 0.0_f64;
    let mut na = 0.0_f64;
    let mut nb = 0.0_f64;
    for i in 0..4 {
        let x = f64::from(a[i]);
        let y = f64::from(b[i]);
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    (dot / (na.sqrt() * nb.sqrt())) as f32
}

#[tokio::test]
async fn p3b_blend_mixes_and_counts_samples() {
    let _env_lock = lock_env();
    let store = CoordStore::open_in_memory().expect("in-memory store");
    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);

    let v1: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
    let v2: [f32; 4] = [0.0, 1.0, 0.0, 0.0];
    let v3: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
    let text1 = "work prompt one — long enough to clear the capture_text filter";
    let text2 = "update prompt — also long enough to clear the filter";
    let text3 = "on-topic follow-up — yet another long enough prompt";

    // Seed the first vector via the explicit replace path (raw).
    store
        .upsert_intent("loop:a", text1, &v1, Utc::now())
        .expect("seed v1");

    // First blend — orthogonal prompt, text must NOT change (cos < 0.65).
    store
        .upsert_intent_blended("loop:a", text2, &v2, Utc::now(), 0.3, 3600)
        .expect("blend v2");

    let after_v2 = store.get_intent("loop:a").expect("get").expect("present");
    let stored_v2: [f32; 4] = [
        after_v2.embedding[0],
        after_v2.embedding[1],
        after_v2.embedding[2],
        after_v2.embedding[3],
    ];
    // Math: e_old = [1,0,0,0], e_new = [0,1,0,0], α=0.3.
    // n_old = n_new = their unit selves (already unit).
    // mixed = 0.7·[1,0,0,0] + 0.3·[0,1,0,0] = [0.7, 0.3, 0, 0].
    // stored = normalize(mixed) ≈ [0.9191, 0.3939, 0, 0].
    let expected_v2: [f32; 4] = [0.9191, 0.3939, 0.0, 0.0];
    let c = cos_4(&stored_v2, &expected_v2);
    assert!(
        c > 0.999,
        "blend must mix to [0.9191, 0.3939, 0, 0]; got stored={stored_v2:?} cos={c}"
    );
    assert!(
        (stored_v2[0] - 0.9191).abs() < 1e-3 && (stored_v2[1] - 0.3939).abs() < 1e-3,
        "per-component within 1e-3; got {stored_v2:?}"
    );
    assert_eq!(after_v2.samples, 2, "samples must be 2 after first blend");
    assert_eq!(
        after_v2.text, text1,
        "off-topic (orthogonal) blend must keep the old text"
    );

    // Second blend — same direction as v1 (cos == 1.0 with the
    // stored unit vector), text must update.
    store
        .upsert_intent_blended("loop:a", text3, &v3, Utc::now(), 0.3, 3600)
        .expect("blend v3");

    let after_v3 = store.get_intent("loop:a").expect("get").expect("present");
    assert_eq!(after_v3.samples, 3, "samples must be 3 after second blend");
    assert_eq!(
        after_v3.text, text3,
        "on-topic (cos=1.0) blend must adopt the new text"
    );
}

#[tokio::test]
async fn p3b_stale_replaces() {
    let _env_lock = lock_env();
    let store = CoordStore::open_in_memory().expect("in-memory store");
    upsert_principal(&store, "loop:a", None);

    let v1: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
    let v2: [f32; 4] = [0.0, 1.0, 0.0, 0.0];
    let now = Utc::now();
    let two_hours_ago = now - ChronoDuration::seconds(7200);
    store
        .upsert_intent(
            "loop:a",
            "old long prompt — set long ago",
            &v1,
            two_hours_ago,
        )
        .expect("seed v1 stale");

    // Blend a fresh vector at `now` with window=3600 → row is stale.
    store
        .upsert_intent_blended("loop:a", "fresh long prompt", &v2, now, 0.3, 3600)
        .expect("blend v2 now");

    let got = store.get_intent("loop:a").expect("get").expect("present");
    assert_eq!(
        got.embedding,
        v2.to_vec(),
        "stale row must be replaced with raw v2"
    );
    assert_eq!(got.samples, 1, "replace resets samples to 1");
    assert_eq!(got.text, "fresh long prompt");
}

#[tokio::test]
async fn p3b_dim_change_replaces() {
    let _env_lock = lock_env();
    let store = CoordStore::open_in_memory().expect("in-memory store");
    upsert_principal(&store, "loop:a", None);

    let v4: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
    let v3: [f32; 3] = [1.0, 0.0, 0.0];
    store
        .upsert_intent("loop:a", "four-dim prompt", &v4, Utc::now())
        .expect("seed v4");
    store
        .upsert_intent_blended("loop:a", "three-dim prompt", &v3, Utc::now(), 0.3, 3600)
        .expect("blend v3");

    let got = store.get_intent("loop:a").expect("get").expect("present");
    assert_eq!(got.dim, 3, "dim must follow the new vector");
    assert_eq!(got.embedding, v3.to_vec());
    assert_eq!(got.samples, 1);
}

#[tokio::test]
async fn p3b_alpha_one_is_exact_replace() {
    let _env_lock = lock_env();
    let store = CoordStore::open_in_memory().expect("in-memory store");
    upsert_principal(&store, "loop:a", None);

    let v1: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
    let v2: [f32; 4] = [0.0, 2.0, 0.0, 0.0]; // non-unit magnitude
    store
        .upsert_intent("loop:a", "first long prompt", &v1, Utc::now())
        .expect("seed v1");
    store
        .upsert_intent_blended(
            "loop:a",
            "second long prompt — orthogonally off-topic",
            &v2,
            Utc::now(),
            1.0,
            3600,
        )
        .expect("blend v2 alpha=1.0");

    let got = store.get_intent("loop:a").expect("get").expect("present");
    assert_eq!(
        got.embedding,
        v2.to_vec(),
        "alpha=1.0 must store the raw v2 (no normalization)"
    );
    assert_eq!(got.samples, 1, "alpha=1.0 takes the replace branch");
    assert_eq!(
        got.text, "second long prompt — orthogonally off-topic",
        "alpha=1.0 always adopts the new text"
    );
}

#[tokio::test]
async fn p3b_parse_intent_alpha() {
    // None → default.
    assert_eq!(contextnest::api::coord_turn::parse_intent_alpha(None), 0.3);
    assert_eq!(
        contextnest::api::coord_turn::parse_intent_alpha(Some("0.5")),
        0.5
    );
    // Whitespace is trimmed.
    assert_eq!(
        contextnest::api::coord_turn::parse_intent_alpha(Some(" 1 ")),
        1.0
    );
    // Out-of-range / unparseable → default.
    for bad in ["0", "-1", "nan", "abc", "inf", "1.5"] {
        assert_eq!(
            contextnest::api::coord_turn::parse_intent_alpha(Some(bad)),
            0.3,
            "bad input {bad:?} must fall back to default 0.3"
        );
    }
}

#[tokio::test]
async fn p3b_status_turn_barely_moves_topic() {
    let env_lock = lock_env();
    let topic = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC");
    let threshold = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD");
    let window = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_WINDOW_SECS");
    let dedup = EnvGuard::clear("CONTEXTNEST_CONCORD_TOPIC_DEDUP_SECS");
    let alpha = EnvGuard::clear("CONTEXTNEST_CONCORD_INTENT_ALPHA");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC", "1");
    std::env::set_var("CONTEXTNEST_CONCORD_TOPIC_THRESHOLD", "0.85");

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

    upsert_principal(&store, "loop:a", None);
    upsert_principal(&store, "loop:b", None);
    let text_a_old = "loop:a long work prompt — first capture long enough";
    let text_b = "loop:b long work prompt — distinct other capture";
    let t: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
    let u: [f32; 4] = [0.0, 1.0, 0.0, 0.0];
    // Seed both as raw unit vectors — distinct dims are 4, opposite axes.
    store
        .upsert_intent("loop:a", text_a_old, &t, Utc::now())
        .expect("seed a");
    store
        .upsert_intent("loop:b", text_b, &t, Utc::now())
        .expect("seed b");

    // Blend an orthogonal vector into loop:a with α=0.3, window 3600.
    // Stored vec becomes ≈ [0.9191, 0.3939, 0, 0]; cos with [1,0,0,0] is
    // ≈ 0.9191 (on-topic with the original) but with loop:b=[1,0,0,0]
    // also ≈ 0.9191, so a notice fires.
    store
        .upsert_intent_blended(
            "loop:a",
            "loop:a long status question — orthogonal to the work topic",
            &u,
            Utc::now(),
            0.3,
            3600,
        )
        .expect("blend a orthogonally");

    // Now a status turn (no prompt) must NOT clobber the seeded vec.
    let body = ups(&server, "loop:a", "s1", "UserPromptSubmit", None).await;
    let ctx_text = ctx(&body);
    assert!(
        ctx_text.contains("↔ loop:b"),
        "notice must fire against loop:b; got {ctx_text:?}"
    );
    assert!(
        ctx_text.contains(text_b),
        "notice must quote loop:b's text; got {ctx_text:?}"
    );

    // DoD-6: the status turn barely moved loop:a's topic — the direct
    // store match against loop:b is still >= 0.9, not merely past the
    // 0.85 notice threshold.
    let m = store
        .best_topic_match("loop:a", 3600, Utc::now(), cos)
        .expect("best_topic_match")
        .expect("loop:a has a match");
    assert_eq!(m.other, "loop:b");
    assert!(
        m.similarity >= 0.9,
        "status turn must barely move the topic; similarity {}",
        m.similarity
    );

    // /intents shows loop:a with samples==2 (the blend) and its OLD text:
    // the status prompt is off-topic, cos([1,0,0,0], [0,1,0,0]) == 0 < 0.5.
    let intents = list_intents(&server).await;
    let arr = intents["intents"].as_array().expect("intents array");
    let mut a_samples = None;
    let mut a_text = None;
    let mut b_samples = None;
    for entry in arr {
        match entry["principal_id"].as_str().unwrap_or("") {
            "loop:a" => {
                a_samples = entry["samples"].as_i64();
                a_text = entry["text"].as_str().map(str::to_string);
            }
            "loop:b" => {
                b_samples = entry["samples"].as_i64();
            }
            _ => {}
        }
    }
    assert_eq!(a_samples, Some(2), "loop:a must have samples=2 after blend");
    assert_eq!(b_samples, Some(1), "loop:b must still have samples=1");
    assert_eq!(
        a_text.as_deref(),
        Some(text_a_old),
        "loop:a kept its old text (orthogonal blend kept text)"
    );

    drop(topic);
    drop(threshold);
    drop(window);
    drop(dedup);
    drop(alpha);
    drop(env_lock);
}

#[tokio::test]
async fn p3b_upgrade_adds_samples_column() {
    let _env_lock = lock_env();
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("coord-pre-p3b.db");
    // Build a pre-P3b schema directly via rusqlite, exactly the shape
    // the pre-upgrade CoordStore would have created.
    {
        let conn = rusqlite::Connection::open(&path).expect("open pre-p3b");
        conn.execute_batch(
            "CREATE TABLE intents (
                principal_id TEXT PRIMARY KEY,
                text         TEXT NOT NULL,
                embedding    BLOB NOT NULL,
                dim          INTEGER NOT NULL,
                updated_at   TEXT NOT NULL
             );",
        )
        .expect("create pre-p3b schema");
        let v = vec![1.0_f32, 0.0, 0.0, 0.0];
        let blob = encode_embedding(&v);
        let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        conn.execute(
            "INSERT INTO intents (principal_id, text, embedding, dim, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["loop:legacy", "legacy long prompt", blob, 4_i64, ts],
        )
        .expect("insert pre-p3b row");
    }

    // First open: migration must add the column without losing the row.
    let store = CoordStore::open(&path).expect("open with migration");
    let intent = store
        .get_intent("loop:legacy")
        .expect("get")
        .expect("present");
    assert_eq!(
        intent.samples, 1,
        "pre-P3b row must default to samples=1 after upgrade"
    );
    assert_eq!(intent.text, "legacy long prompt");

    // Second open: idempotent — must NOT raise "duplicate column name".
    let store2 = CoordStore::open(&path).expect("re-open is a no-op");
    let again = store2
        .get_intent("loop:legacy")
        .expect("get")
        .expect("present");
    assert_eq!(again.samples, 1);

    // Independent connection confirms the column is present.
    let probe = rusqlite::Connection::open(&path).expect("probe");
    let mut stmt = probe.prepare("PRAGMA table_info(intents)").expect("pragma");
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .expect("query")
        .map(|r| r.unwrap())
        .collect();
    assert!(
        cols.iter().any(|c| c == "samples"),
        "samples column must exist after migration; cols={cols:?}"
    );
}

/// DoD-2 boundary: the stored text is replaced at cosine >= 0.5 to the
/// old blend and kept below it. Vectors are hand-made so cos is exact.
#[tokio::test]
async fn p3b_text_replace_threshold_is_half() {
    let env_lock = lock_env();
    let tmp = tempfile::tempdir().expect("tempdir");
    let store = CoordStore::open(&tmp.path().join("coord.db")).expect("store");
    upsert_principal(&store, "loop:on", None);
    upsert_principal(&store, "loop:off", None);
    let base: [f32; 4] = [1.0, 0.0, 0.0, 0.0];
    // cos(base, on) = 0.55 (on-topic, >= 0.5); cos(base, off) = 0.45.
    let on: [f32; 4] = [0.55, (1.0_f32 - 0.55 * 0.55).sqrt(), 0.0, 0.0];
    let off: [f32; 4] = [0.45, (1.0_f32 - 0.45 * 0.45).sqrt(), 0.0, 0.0];
    assert!((cos(&base, &on) - 0.55).abs() < 1e-4);
    assert!((cos(&base, &off) - 0.45).abs() < 1e-4);

    store
        .upsert_intent("loop:on", "original work prompt for on", &base, Utc::now())
        .expect("seed on");
    store
        .upsert_intent(
            "loop:off",
            "original work prompt for off",
            &base,
            Utc::now(),
        )
        .expect("seed off");
    store
        .upsert_intent_blended(
            "loop:on",
            "rephrased work prompt, on topic",
            &on,
            Utc::now(),
            0.3,
            3600,
        )
        .expect("blend on");
    store
        .upsert_intent_blended(
            "loop:off",
            "unrelated status question here",
            &off,
            Utc::now(),
            0.3,
            3600,
        )
        .expect("blend off");

    let on_row = store
        .get_intent("loop:on")
        .expect("get on")
        .expect("on row");
    let off_row = store
        .get_intent("loop:off")
        .expect("get off")
        .expect("off row");
    assert_eq!(
        on_row.text, "rephrased work prompt, on topic",
        "cos 0.55 >= 0.5 replaces the text"
    );
    assert_eq!(
        off_row.text, "original work prompt for off",
        "cos 0.45 < 0.5 keeps the old text"
    );
    assert_eq!(on_row.samples, 2);
    assert_eq!(off_row.samples, 2);
    drop(env_lock);
}

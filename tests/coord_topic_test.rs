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
    _env_lock: std::sync::MutexGuard<'static, ()>,
}

async fn make_harness() -> Harness {
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

    Harness {
        server,
        store,
        _tmp: tmp,
        _env_topic: topic,
        _env_threshold: threshold,
        _env_window: window,
        _env_dedup: dedup,
        _env_lock: env_lock,
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

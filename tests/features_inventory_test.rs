//! Integration tests for `GET /api/v1/features` — the daily-driver
//! feature inventory endpoint. Pairs with the `delivered_features[]`
//! z-insight field and its extractor (in
//! `src/ingest/claude_code/extractor.rs`).
//!
//! Contract:
//!
//! 1. Default `?since=` window is 24h. Features older than that are
//!    excluded; features inside the window are returned newest-first.
//! 2. Custom durations `5m` / `2h` / `7d` / `30d` parse correctly.
//!    Garbage like `?since=garbage` falls back to the 24h default
//!    (no 4xx) — operators' typos shouldn't break dashboards.
//! 3. `?layer=backend` narrows the set to features whose
//!    `metadata.layer` matches case-insensitively.
//! 4. Each returned entry carries `how_to_test` and `defs` when the
//!    agent supplied them; missing fields render as `null` /
//!    `[]` respectively.
//! 5. Features whose timestamp is unparseable are still surfaced
//!    (over-report > silent-drop semantics).

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::ingest::claude_code::extractor::{MemoryKind, MemoryRecord};
use contextnest::ingest::claude_code::sink::{ServicesSink, Sink};
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};

async fn make_server() -> (ContextNestServices, TestServer) {
    let services = ContextNestServices::new_default().await.unwrap();
    let app = create_simple_app(services.clone()).await.unwrap();
    let server = TestServer::new(app).unwrap();
    (services, server)
}

#[allow(clippy::too_many_arguments)]
async fn push_feature(
    services: &ContextNestServices,
    session_uuid: &str,
    feature: &str,
    ts_rfc3339: &str,
    layer: &str,
    files: Vec<&str>,
    how_to_test: Option<&str>,
    defs: Vec<&str>,
) {
    let sink = ServicesSink::new(services.clone());
    let mut meta = std::collections::HashMap::new();
    meta.insert("kind".to_string(), json!("feature"));
    meta.insert("ts".to_string(), json!(ts_rfc3339));
    meta.insert("src_session".to_string(), json!(session_uuid));
    if !files.is_empty() {
        meta.insert("files".to_string(), json!(files));
    }
    meta.insert("layer".to_string(), json!(layer));
    if let Some(h) = how_to_test {
        meta.insert("how_to_test".to_string(), json!(h));
    }
    if !defs.is_empty() {
        meta.insert("defs".to_string(), json!(defs));
    }
    let record = MemoryRecord {
        kind: MemoryKind::Feature,
        text: feature.to_string(),
        importance: 0.90,
        session_id_cn: session_uuid.to_string(),
        metadata: meta,
    };
    sink.store(&record).await.unwrap();
}

fn iso_minutes_ago(mins: i64) -> String {
    (chrono::Utc::now() - chrono::Duration::minutes(mins)).to_rfc3339()
}

#[tokio::test]
async fn default_window_is_24h_and_excludes_older_features() {
    let (services, server) = make_server().await;
    // Inside window (1h ago) — kept.
    push_feature(
        &services,
        "sess-recent",
        "fresh feature",
        &iso_minutes_ago(60),
        "backend",
        vec!["src/x.rs"],
        Some("cargo test --test x"),
        vec!["fn x()"],
    )
    .await;
    // Outside window (3 days ago) — dropped.
    push_feature(
        &services,
        "sess-stale",
        "ancient feature",
        &iso_minutes_ago(60 * 24 * 3),
        "backend",
        vec!["src/y.rs"],
        None,
        vec![],
    )
    .await;

    let res = server.get("/api/v1/features").await;
    res.assert_status_ok();
    let body: Value = res.json();
    let feats = body["features"].as_array().unwrap();
    assert_eq!(feats.len(), 1, "default window keeps only recent feature");
    assert_eq!(feats[0]["feature"], "fresh feature");
    assert_eq!(feats[0]["session_id"], "sess-recent");
    assert_eq!(feats[0]["how_to_test"], "cargo test --test x");
    let defs = feats[0]["defs"].as_array().unwrap();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0], "fn x()");
}

#[tokio::test]
async fn custom_since_window_parses_minute_hour_day_suffixes() {
    let (services, server) = make_server().await;
    push_feature(
        &services,
        "sess-15m",
        "quarter-hour-old",
        &iso_minutes_ago(15),
        "frontend",
        vec![],
        None,
        vec![],
    )
    .await;
    push_feature(
        &services,
        "sess-2h",
        "two-hour-old",
        &iso_minutes_ago(120),
        "frontend",
        vec![],
        None,
        vec![],
    )
    .await;

    // 5m window — only the 15m one is OUT.
    let res = server.get("/api/v1/features?since=5m").await;
    assert_eq!(res.json::<Value>()["features"].as_array().unwrap().len(), 0);

    // 1h window — keeps the 15m one, drops the 2h one.
    let res = server.get("/api/v1/features?since=1h").await;
    let body: Value = res.json();
    let feats = body["features"].as_array().unwrap();
    assert_eq!(feats.len(), 1);
    assert_eq!(feats[0]["feature"], "quarter-hour-old");

    // 7d window — both kept, newest-first.
    let res = server.get("/api/v1/features?since=7d").await;
    let body: Value = res.json();
    let feats = body["features"].as_array().unwrap();
    assert_eq!(feats.len(), 2);
    assert_eq!(feats[0]["feature"], "quarter-hour-old");
    assert_eq!(feats[1]["feature"], "two-hour-old");
}

#[tokio::test]
async fn garbage_since_falls_back_to_default() {
    let (services, server) = make_server().await;
    push_feature(
        &services,
        "sess-typo",
        "feature one",
        &iso_minutes_ago(30),
        "backend",
        vec![],
        None,
        vec![],
    )
    .await;
    let res = server.get("/api/v1/features?since=garbage").await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["since"], "garbage", "echoes the raw input back");
    assert_eq!(body["features"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn layer_filter_is_case_insensitive() {
    let (services, server) = make_server().await;
    push_feature(
        &services,
        "sess-be",
        "backend feature",
        &iso_minutes_ago(10),
        "backend",
        vec![],
        None,
        vec![],
    )
    .await;
    push_feature(
        &services,
        "sess-fe",
        "frontend feature",
        &iso_minutes_ago(10),
        "frontend",
        vec![],
        None,
        vec![],
    )
    .await;

    let res = server.get("/api/v1/features?layer=BACKEND").await;
    let body: Value = res.json();
    let feats = body["features"].as_array().unwrap();
    assert_eq!(feats.len(), 1);
    assert_eq!(feats[0]["feature"], "backend feature");
}

#[tokio::test]
async fn newest_first_ordering_holds() {
    let (services, server) = make_server().await;
    for (sess, name, mins) in [
        ("sess-a", "oldest", 60),
        ("sess-b", "middle", 30),
        ("sess-c", "newest", 5),
    ] {
        push_feature(
            &services,
            sess,
            name,
            &iso_minutes_ago(mins),
            "backend",
            vec![],
            None,
            vec![],
        )
        .await;
    }
    let res = server.get("/api/v1/features?since=2h").await;
    let body: Value = res.json();
    let feats = body["features"].as_array().unwrap();
    assert_eq!(feats.len(), 3);
    let names: Vec<&str> = feats
        .iter()
        .map(|f| f.get("feature").and_then(Value::as_str).unwrap())
        .collect();
    assert_eq!(names, vec!["newest", "middle", "oldest"]);
}

#[tokio::test]
async fn missing_how_to_test_and_defs_render_as_null_and_empty() {
    let (services, server) = make_server().await;
    push_feature(
        &services,
        "sess-bare",
        "bare-bones feature",
        &iso_minutes_ago(5),
        "docs",
        vec!["README.md"],
        None,
        vec![],
    )
    .await;
    let res = server.get("/api/v1/features").await;
    let body: Value = res.json();
    let f = &body["features"][0];
    assert!(f["how_to_test"].is_null(), "missing how_to_test → null");
    assert!(
        f["defs"].as_array().unwrap().is_empty(),
        "missing defs → []"
    );
}

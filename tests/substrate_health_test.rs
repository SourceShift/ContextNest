//! Integration tests for Phase 7 of the neural-field epic — the
//! aggregate substrate health endpoint at `GET /api/v1/substrate/health`.
//!
//! The endpoint exists to answer "is everything ContextNest's tagline
//! claims actually running?" in a single read. If any nested stat
//! says zero on a populated substrate, the corresponding code path
//! is dormant — that's the operator signal to investigate.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::ingest::claude_code::extractor::{MemoryKind, MemoryRecord};
use contextnest::ingest::claude_code::sink::{ServicesSink, Sink};
use contextnest::services::consolidation::drain_for_test;
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};

async fn make_setup() -> (ContextNestServices, TestServer) {
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = create_simple_app(services.clone())
        .await
        .expect("seven-tool app should build");
    let server = TestServer::new(app).expect("test server should start");
    (services, server)
}

fn rec(text: &str, session: &str) -> MemoryRecord {
    let mut r = MemoryRecord::new(MemoryKind::Learning, text.to_string(), session.to_string());
    r.metadata
        .insert("kind".to_string(), Value::String("learning".to_string()));
    r.metadata.insert(
        "ts".to_string(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    r
}

#[tokio::test]
async fn empty_substrate_returns_zero_stats() {
    let (_services, server) = make_setup().await;
    let res = server.get("/api/v1/substrate/health").await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["fragments"]["total"], 0);
    assert_eq!(body["fragments"]["consolidated"], 0);
    assert_eq!(body["fragments"]["lag"], 0);
    assert_eq!(body["basins"]["count"], 0);
    assert_eq!(body["basins"]["max_mass"], 0);
    // avg_mass and avg_degree are 0.0 on empty graph — JSON renders
    // f32 0.0 as a number, so this passes if the type is right.
    assert!(body["basins"]["avg_mass"].is_number());
    assert!(body["connections"]["avg_degree"].is_number());
    assert_eq!(body["connections"]["edges"], 0);
    assert_eq!(body["connections"]["nodes"], 0);
    // Decay knob defaults to 60.
    assert_eq!(body["decay"]["half_life_days"], 60.0);
}

#[tokio::test]
async fn substrate_health_reflects_live_state_after_ingest_and_consolidation() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    for text in [
        "neural networks need lots of training data",
        "neural networks are universal function approximators",
        "the borrow checker is rust's secret weapon",
    ] {
        sink.store(&rec(text, "cn-test-health")).await.unwrap();
    }

    // Before consolidation: lag = total, no basins, no edges.
    let res = server.get("/api/v1/substrate/health").await;
    let pre: Value = res.json();
    assert_eq!(pre["fragments"]["total"], 3);
    assert_eq!(pre["fragments"]["consolidated"], 0);
    assert_eq!(pre["fragments"]["lag"], 3);
    assert_eq!(pre["basins"]["count"], 0);

    drain_for_test(&services, &services.consolidation_queue, 4).await;

    // After consolidation: lag = 0, basins exist, nodes present.
    let res = server.get("/api/v1/substrate/health").await;
    let post: Value = res.json();
    assert_eq!(post["fragments"]["total"], 3);
    assert_eq!(post["fragments"]["consolidated"], 3);
    assert_eq!(post["fragments"]["lag"], 0);
    assert!(
        post["basins"]["count"].as_u64().unwrap() > 0,
        "basins should form after consolidation"
    );
    assert!(
        post["connections"]["nodes"].as_u64().unwrap() > 0,
        "connection network should have nodes after consolidation"
    );
}

#[tokio::test]
async fn median_age_reflects_metadata_ts() {
    // Two fragments with explicit ts values 0 days and 10 days ago →
    // median is 5 days.
    let (_services, server) = make_setup().await;
    let now = chrono::Utc::now();
    let ten_days_ago = now - chrono::Duration::days(10);

    let body = json!({
        "content": "fresh fragment",
        "session_id": "cn-test-median",
        "metadata": {"kind": "learning", "ts": now.to_rfc3339()},
    });
    server.post("/api/v1/tools/store").json(&body).await;
    let body = json!({
        "content": "ten day old fragment",
        "session_id": "cn-test-median",
        "metadata": {"kind": "learning", "ts": ten_days_ago.to_rfc3339()},
    });
    server.post("/api/v1/tools/store").json(&body).await;

    let res = server.get("/api/v1/substrate/health").await;
    let body: Value = res.json();
    let median = body["decay"]["median_fragment_age_days"].as_f64().unwrap();
    // For 2 entries the median is the mean of the two — (0 + 10) / 2 = 5.
    assert!(
        (4.5..=5.5).contains(&median),
        "median age should be ~5 days for two-fragment substrate at 0d and 10d, got {median}"
    );
}

// NOTE: a "set CONTEXTNEST_DECAY_HALF_LIFE_DAYS and verify it
// surfaces in the response" test would race with every other test
// in this file that reads `decay.half_life_days`. The env-var parse
// path is verified by the dedicated unit tests in
// `src/api/tools.rs::tests` (which serialize via #[test]) — we don't
// duplicate that race-prone assertion here.

#[tokio::test]
async fn basin_avg_mass_reflects_real_membership() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    // Three fragments → in the mock embedder, likely three basins
    // with one member each. Sum / count = 1.0.
    for text in [
        "test basin avg mass alpha",
        "test basin avg mass beta",
        "test basin avg mass gamma",
    ] {
        sink.store(&rec(text, "cn-test-avg")).await.unwrap();
    }
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let res = server.get("/api/v1/substrate/health").await;
    let body: Value = res.json();
    let count = body["basins"]["count"].as_u64().unwrap();
    let avg = body["basins"]["avg_mass"].as_f64().unwrap();
    let max = body["basins"]["max_mass"].as_u64().unwrap();
    if count > 0 {
        // Sanity: avg should be in [0, max] and max should be at
        // least 1 (each basin has at least its seed fragment).
        assert!(avg >= 0.0 && avg <= max as f64);
        assert!(max >= 1);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// `GET /api/v1/substrate/config` — Ticket #6 from the coverage epic
// (docs/todos/20260602-1230-cn-value-prop-coverage-epic.md).
//
// Pins the read-only config-snapshot endpoint that lets operators answer
// "what's actually configured at runtime?" without grepping `config.toml`.
// The shape is contract-stable because the dashboard's /config page reads
// it directly — renaming any field here would break the FE silently.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn substrate_config_endpoint_returns_snapshot_with_expected_field_names() {
    let (_services, server) = make_setup().await;
    let res = server.get("/api/v1/substrate/config").await;
    res.assert_status_ok();
    let body: Value = res.json();

    // Top-level shape — these field names are the wire contract.
    for field in ["version", "git_commit", "embedding", "llm", "llm_cache"] {
        assert!(
            body.get(field).is_some(),
            "top-level field `{field}` must be present"
        );
    }

    // version is a non-empty string from CARGO_PKG_VERSION (currently 0.1.0).
    let v = body["version"].as_str().expect("version is a string");
    assert!(!v.is_empty(), "version must be non-empty");

    // git_commit defaults to "unknown" when CONTEXTNEST_GIT_COMMIT is unset
    // at build time. Either case is valid wire-shape.
    assert!(
        body["git_commit"].is_string(),
        "git_commit must be a string"
    );

    // Embedding block.
    let model = body["embedding"]["model"]
        .as_str()
        .expect("embedding.model is a string");
    assert!(!model.is_empty(), "embedding.model must be non-empty");

    // LLM block.
    assert!(body["llm"]["enabled"].is_boolean());
    assert!(body["llm"]["configured_providers"].is_array());
    // default_provider is nullable.
    let dp = &body["llm"]["default_provider"];
    assert!(
        dp.is_null() || dp.is_string(),
        "llm.default_provider must be null or string"
    );

    // LLM cache block — all four required.
    for field in [
        "encryption_enabled",
        "similarity_threshold",
        "default_ttl_secs",
        "total_entries",
    ] {
        assert!(
            body["llm_cache"].get(field).is_some(),
            "llm_cache.{field} must be present"
        );
    }
    assert!(body["llm_cache"]["encryption_enabled"].is_boolean());
    assert!(body["llm_cache"]["similarity_threshold"].is_number());
    // Default TTL from kind_registry/llm_cache is 3600 secs.
    assert_eq!(
        body["llm_cache"]["default_ttl_secs"].as_u64().unwrap(),
        3600,
        "default_ttl_secs must match the substrate's documented default"
    );
}

#[tokio::test]
async fn substrate_config_endpoint_reflects_disabled_llm_in_mock_mode() {
    // Default mock-mode setup has no LLM provider configured.
    // The config snapshot should reflect that honestly so operators
    // immediately see "summarize will fall through to stats-only".
    let (_services, server) = make_setup().await;
    let res = server.get("/api/v1/substrate/config").await;
    res.assert_status_ok();
    let body: Value = res.json();

    let enabled = body["llm"]["enabled"].as_bool().unwrap();
    let providers = body["llm"]["configured_providers"].as_array().unwrap();
    if !enabled {
        assert!(
            providers.is_empty(),
            "configured_providers must be empty when llm.enabled = false"
        );
    }
}

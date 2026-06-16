//! Integration tests for Phase 2 of the neural-field epic — age-based
//! decay applied to cosine similarity at retrieve time.
//!
//! Exercises:
//!
//! 1. Identical content stored with different `metadata.ts` returns
//!    different similarity scores — old fragments score lower than new
//!    ones even when their embeddings match the query identically.
//! 2. The decay multiplier is monotonic in age (older = lower).
//! 3. After a retrieve, every returned hit gets `last_accessed` written
//!    to its metadata sidecar. A follow-up retrieve uses that timestamp
//!    in preference to the original `ts`, so frequently-accessed
//!    fragments don't fade.
//! 4. Fragments with no parseable timestamp are not penalised (decay=1).

use axum_test::TestServer;
use chrono::{Duration, Utc};
use contextnest::api::create_simple_app;
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};

async fn make_server() -> TestServer {
    let services = ContextNestServices::new_default()
        .await
        .expect("default services should init in mock mode");
    let app = create_simple_app(services)
        .await
        .expect("seven-tool app should build");
    TestServer::new(app).expect("test server should start")
}

async fn store(server: &TestServer, session: &str, content: &str, metadata: Value) -> String {
    let res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": content,
            "importance": 0.7,
            "session_id": session,
            "metadata": metadata,
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["stored"], true);
    body["attractor_id"].as_str().unwrap().to_string()
}

async fn retrieve(server: &TestServer, session: &str, query: &str) -> Value {
    let res = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": query,
            "top_k": 50,
            "session_id": session,
        }))
        .await;
    res.assert_status_ok();
    res.json()
}

fn iso_days_ago(days: i64) -> String {
    (Utc::now() - Duration::days(days)).to_rfc3339()
}

fn hit_by_id<'a>(hits: &'a [Value], id: &str) -> &'a Value {
    hits.iter()
        .find(|h| h["id"].as_str() == Some(id))
        .unwrap_or_else(|| panic!("no hit with id {id} in retrieve response"))
}

#[tokio::test]
async fn old_fragments_score_lower_than_fresh_ones_with_identical_content() {
    let server = make_server().await;
    let session = "decay-monotonic";

    // Both fragments carry the SAME content — embeddings will match the
    // query identically, so any score difference comes purely from decay.
    // Use a Standard-durability kind ("todo") so the global half-life
    // (60d) applies unscaled — durable kinds like "learning" get a ×2.0
    // half-life multiplier (see kind_registry::durability) which would
    // move the expected ratio out of the band asserted below.
    let fresh_id = store(
        &server,
        session,
        "rust async runtime tokio is the de facto standard",
        json!({"kind": "todo", "ts": iso_days_ago(0)}),
    )
    .await;
    let old_id = store(
        &server,
        session,
        "rust async runtime tokio is the de facto standard",
        json!({"kind": "todo", "ts": iso_days_ago(100)}),
    )
    .await;

    let body = retrieve(&server, session, "rust async runtime tokio").await;
    let hits = body["hits"].as_array().expect("hits array");

    let fresh = hit_by_id(hits, &fresh_id);
    let old = hit_by_id(hits, &old_id);
    let fresh_sim = fresh["similarity"].as_f64().unwrap();
    let old_sim = old["similarity"].as_f64().unwrap();

    assert!(
        fresh_sim > 0.0,
        "fresh fragment should still have positive similarity"
    );
    assert!(
        old_sim < fresh_sim,
        "100-day-old fragment ({old_sim}) should score lower than fresh one ({fresh_sim})"
    );
    // exp(-100/60 * ln2) ≈ 0.315, so 100-day fragment ≈ 31.5% of fresh.
    // Wide tolerance because cosine on identical content can drift if the
    // mock embedder is anything but identity.
    let ratio = old_sim / fresh_sim;
    assert!(
        (0.20..0.45).contains(&ratio),
        "decay ratio for 100-day-old vs fresh should be ~0.31, got {ratio}"
    );
}

#[tokio::test]
async fn missing_timestamp_means_no_decay() {
    let server = make_server().await;
    let session = "decay-no-ts";

    // No `ts` in metadata at all → decay_multiplier returns 1.0, so the
    // base cosine similarity passes through untouched.
    let id = store(
        &server,
        session,
        "linear types prevent double-free at compile time",
        json!({"kind": "learning"}),
    )
    .await;

    let body = retrieve(&server, session, "linear types prevent double-free").await;
    let hit = hit_by_id(body["hits"].as_array().unwrap(), &id);
    let sim = hit["similarity"].as_f64().unwrap();
    assert!(
        sim > 0.4,
        "no-ts fragment should score at full cosine similarity, got {sim}"
    );
}

#[tokio::test]
async fn retrieve_bumps_last_accessed_so_followups_dont_re_decay() {
    let server = make_server().await;
    let session = "decay-recency-bump";

    // Old fragment by ts, but the retrieve below will write `last_accessed`
    // back, so the second retrieve should treat it as fresh.
    let id = store(
        &server,
        session,
        "the typestate pattern is a borrowed-checker hack",
        json!({"kind": "learning", "ts": iso_days_ago(200)}),
    )
    .await;

    let body1 = retrieve(&server, session, "typestate pattern").await;
    let sim1 = hit_by_id(body1["hits"].as_array().unwrap(), &id)["similarity"]
        .as_f64()
        .unwrap();

    // The first retrieve's response carries metadata that was cloned
    // BEFORE the last_accessed bump (sidecar write happens after the
    // response is built), so we don't assert it on `body1`. Behavioral
    // verification is the second retrieve scoring higher than the first.
    let body2 = retrieve(&server, session, "typestate pattern").await;
    let sim2 = hit_by_id(body2["hits"].as_array().unwrap(), &id)["similarity"]
        .as_f64()
        .unwrap();

    // The first retrieve saw an aged fragment (decayed). The second one
    // sees `last_accessed ≈ now`, so decay ≈ 1.0 — similarity should be
    // strictly higher.
    assert!(
        sim2 > sim1,
        "follow-up retrieve should see a higher similarity (sim1={sim1}, sim2={sim2}) \
         because last_accessed was bumped on the first call"
    );
}

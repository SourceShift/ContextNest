//! Integration tests for Option C — `GET /api/v1/sessions/by-intent`.
//!
//! Pins the three properties that make intent-search useful:
//!
//! 1. The endpoint embeds the query, embeds each session's intent
//!    text, and returns sessions ranked by cosine similarity.
//! 2. The `domain` filter (when set) drops sessions whose intent
//!    domain doesn't match.
//! 3. Sessions with no structured intent (no domain/goal/state
//!    fragments) are skipped from the ranking — they would have
//!    nothing to embed.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::ingest::claude_code::extractor::{MemoryKind, MemoryRecord};
use contextnest::ingest::claude_code::sink::{ServicesSink, Sink};
use contextnest::services::consolidation::drain_for_test;
use contextnest::services::ContextNestServices;
use serde_json::Value;

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

fn rec(text: &str, session: &str, kind: MemoryKind, extra: &[(&str, Value)]) -> MemoryRecord {
    let kind_str = match kind {
        MemoryKind::Domain => "domain",
        MemoryKind::GoalPhase => "goal_phase",
        MemoryKind::State => "state",
        MemoryKind::Learning => "learning",
        _ => "learning",
    };
    let mut r = MemoryRecord::new(kind, text.to_string(), session.to_string());
    r.metadata
        .insert("kind".to_string(), Value::String(kind_str.to_string()));
    r.metadata.insert(
        "ts".to_string(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    for (k, v) in extra {
        r.metadata.insert(k.to_string(), v.clone());
    }
    r
}

async fn seed_session(
    sink: &ServicesSink,
    session: &str,
    domain: &str,
    topics: &[&str],
    goal: &str,
) {
    let topics_val = Value::Array(
        topics
            .iter()
            .map(|t| Value::String(t.to_string()))
            .collect(),
    );
    sink.store(&rec(
        domain,
        session,
        MemoryKind::Domain,
        &[("topics", topics_val)],
    ))
    .await
    .unwrap();
    sink.store(&rec(goal, session, MemoryKind::GoalPhase, &[]))
        .await
        .unwrap();
}

#[tokio::test]
async fn by_intent_ranks_sessions_by_intent_text_similarity() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());

    // Session A: research arxiv digest — should win the arxiv query.
    seed_session(
        &sink,
        "cn-intent-arxiv",
        "research",
        &["arxiv-digest", "llm-research-2026", "post-training"],
        "Produce a comprehensive jina-ranked digest of last 200 May 2026 arxiv papers",
    )
    .await;

    // Session B: backend infra work — should lose the arxiv query.
    seed_session(
        &sink,
        "cn-intent-infra",
        "backend",
        &["docker-compose", "kubernetes", "postgresql"],
        "Migrate the inbox feed from polling to websocket push for live updates",
    )
    .await;

    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let res = server
        .get("/api/v1/sessions/by-intent?q=arxiv+research+trending+techniques&top_k=5")
        .await;
    res.assert_status_ok();
    let body: Value = res.json();

    assert_eq!(body["query"], "arxiv research trending techniques");
    let hits = body["hits"].as_array().expect("hits is array");
    assert!(!hits.is_empty(), "expected at least one hit");

    // The research session should be ranked above the infra session.
    let top_sid = hits[0]["session_id"].as_str().unwrap();
    assert_eq!(
        top_sid, "cn-intent-arxiv",
        "research session should win arxiv query; got {top_sid}"
    );

    // Score should be a number; intent_text should reflect what we
    // seeded (domain/topic/goal fields concatenated).
    let intent_text = hits[0]["intent_text"].as_str().unwrap();
    assert!(
        intent_text.contains("research") && intent_text.contains("arxiv"),
        "intent_text should mention research and arxiv: {intent_text}"
    );
}

#[tokio::test]
async fn by_intent_domain_filter_drops_non_matching_sessions() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());

    seed_session(
        &sink,
        "cn-intent-research",
        "research",
        &["arxiv"],
        "Read latest arxiv techniques",
    )
    .await;
    seed_session(
        &sink,
        "cn-intent-backend",
        "backend",
        &["arxiv-api"],
        "Build arxiv proxy backend",
    )
    .await;

    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let res = server
        .get("/api/v1/sessions/by-intent?q=arxiv&domain=research")
        .await;
    res.assert_status_ok();
    let body: Value = res.json();

    let hits = body["hits"].as_array().expect("hits is array");
    // Only the research session should be returned.
    assert_eq!(
        hits.len(),
        1,
        "expected only the research session, got {}",
        hits.len()
    );
    assert_eq!(hits[0]["session_id"], "cn-intent-research");
    assert_eq!(body["domain_filter"], "research");
}

#[tokio::test]
async fn by_intent_skips_sessions_with_no_structured_intent() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());

    // Session A: has a proper intent (domain + goal).
    seed_session(
        &sink,
        "cn-intent-with-intent",
        "research",
        &["topic-a"],
        "Goal text describing the session",
    )
    .await;

    // Session B: only has learning fragments — no domain, no goal,
    // no state. Should be excluded from the by-intent ranking.
    let mut r = MemoryRecord::new(
        MemoryKind::Learning,
        "incidental learning without structured intent".to_string(),
        "cn-intent-no-intent".to_string(),
    );
    r.metadata
        .insert("kind".to_string(), Value::String("learning".to_string()));
    r.metadata.insert(
        "ts".to_string(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    sink.store(&r).await.unwrap();

    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let res = server.get("/api/v1/sessions/by-intent?q=anything").await;
    res.assert_status_ok();
    let body: Value = res.json();

    let hits = body["hits"].as_array().unwrap();
    let sids: Vec<&str> = hits
        .iter()
        .map(|h| h["session_id"].as_str().unwrap())
        .collect();
    assert!(
        sids.contains(&"cn-intent-with-intent"),
        "session with structured intent must be in hits: {sids:?}"
    );
    assert!(
        !sids.contains(&"cn-intent-no-intent"),
        "session without structured intent must be excluded: {sids:?}"
    );
    // considered count must reflect only sessions with intent text.
    let considered = body["considered"].as_u64().unwrap();
    assert_eq!(
        considered, 1,
        "considered should be 1 (only one session had intent)"
    );
}

#[tokio::test]
async fn by_intent_empty_query_returns_400() {
    let (_services, server) = make_setup().await;
    let res = server.get("/api/v1/sessions/by-intent?q=").await;
    assert_eq!(res.status_code(), 400, "empty q should be a 400");
}

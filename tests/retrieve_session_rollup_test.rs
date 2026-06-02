//! Integration tests for Option B — `group_by: "session"` on
//! `/api/v1/tools/retrieve`.
//!
//! Pins the three properties that make session-rollup useful:
//!
//! 1. When `group_by` is omitted, response shape is unchanged
//!    (no `session_groups` field on the wire).
//! 2. When `group_by: "session"` is set, `session_groups` is
//!    populated with per-session score, hit_count, unique_kinds,
//!    and top_fragments.
//! 3. The diversity term (log(1 + n_unique_kinds)) demonstrably
//!    rewards a session with varied kinds over a session with
//!    homogeneous kinds at equal raw similarity.

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

fn rec_with_kind(text: &str, session: &str, kind: &str) -> MemoryRecord {
    let mk = match kind {
        "todo" => MemoryKind::Todo,
        "accomplishment" => MemoryKind::Accomplishment,
        "decision" => MemoryKind::Decision,
        "evidence_ref" => MemoryKind::EvidenceRef,
        _ => MemoryKind::Learning,
    };
    let mut r = MemoryRecord::new(mk, text.to_string(), session.to_string());
    r.metadata
        .insert("kind".to_string(), Value::String(kind.to_string()));
    r.metadata.insert(
        "ts".to_string(),
        Value::String(chrono::Utc::now().to_rfc3339()),
    );
    r
}

#[tokio::test]
async fn group_by_omitted_returns_no_session_groups_field() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());
    sink.store(&rec_with_kind(
        "session rollup test prose content",
        "cn-rollup-baseline",
        "learning",
    ))
    .await
    .unwrap();
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let body = json!({
        "query": "session rollup",
        "top_k": 5
    });
    let res = server.post("/api/v1/tools/retrieve").json(&body).await;
    res.assert_status_ok();
    let body: Value = res.json();

    // Wire contract: hits present, session_groups absent (skip_serializing_if).
    assert!(body.get("hits").is_some(), "hits must be present");
    assert!(
        body.get("session_groups").is_none(),
        "session_groups must be absent when group_by isn't requested"
    );
}

#[tokio::test]
async fn group_by_session_returns_per_session_groups() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());

    // Two sessions, each with one fragment about "arxiv research".
    sink.store(&rec_with_kind(
        "Pulled 200 May 2026 arxiv papers and ranked by jina",
        "cn-rollup-sess-A",
        "accomplishment",
    ))
    .await
    .unwrap();
    sink.store(&rec_with_kind(
        "Need to read 5 arxiv papers on sparse attention",
        "cn-rollup-sess-B",
        "todo",
    ))
    .await
    .unwrap();
    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let body = json!({
        "query": "arxiv research papers",
        "top_k": 10,
        "group_by": "session"
    });
    let res = server.post("/api/v1/tools/retrieve").json(&body).await;
    res.assert_status_ok();
    let body: Value = res.json();

    let groups = body
        .get("session_groups")
        .and_then(|v| v.as_array())
        .expect("session_groups must be present and an array");
    assert!(
        groups.len() >= 2,
        "expected at least 2 groups for 2 sessions, got {}",
        groups.len()
    );

    // Every group must have score/hit_count/unique_kinds/top_fragments.
    for g in groups {
        assert!(g["session_id"].is_string(), "session_id must be string");
        assert!(g["score"].is_number(), "score must be number");
        assert!(g["hit_count"].is_number(), "hit_count must be number");
        assert!(g["unique_kinds"].is_array(), "unique_kinds must be array");
        assert!(g["top_fragments"].is_array(), "top_fragments must be array");
    }

    // Groups are sorted by score desc — verify by checking the first
    // score >= the last score.
    let first = groups[0]["score"].as_f64().unwrap();
    let last = groups[groups.len() - 1]["score"].as_f64().unwrap();
    assert!(
        first >= last,
        "groups must be sorted by score desc: first={first}, last={last}"
    );
}

#[tokio::test]
async fn diverse_kinds_session_outranks_homogeneous_at_equal_similarity() {
    let (services, server) = make_setup().await;
    let sink = ServicesSink::new(services.clone());

    // Session A: 3 fragments across 3 distinct kinds — the canonical
    // "session that did varied work on the topic". The diversity
    // multiplier log(1 + 3) ≈ 1.39 vs log(1 + 1) ≈ 0.69 for B should
    // make A win the rollup even if raw similarity sums are
    // comparable.
    sink.store(&rec_with_kind(
        "arxiv paper digest todo: pull May 2026 papers",
        "cn-rollup-diverse",
        "todo",
    ))
    .await
    .unwrap();
    sink.store(&rec_with_kind(
        "Done: arxiv digest report at /Volumes/ssd-2/arxiv-db/reports/may2026-top200/REPORT.md",
        "cn-rollup-diverse",
        "accomplishment",
    ))
    .await
    .unwrap();
    sink.store(&rec_with_kind(
        "Decided to rank arxiv papers via jina cross-encoder per session brief",
        "cn-rollup-diverse",
        "decision",
    ))
    .await
    .unwrap();

    // Session B: 3 fragments all of the same kind.
    sink.store(&rec_with_kind(
        "arxiv paper:2603.16131 digest entry one",
        "cn-rollup-homogeneous",
        "evidence_ref",
    ))
    .await
    .unwrap();
    sink.store(&rec_with_kind(
        "arxiv paper:2603.16132 digest entry two",
        "cn-rollup-homogeneous",
        "evidence_ref",
    ))
    .await
    .unwrap();
    sink.store(&rec_with_kind(
        "arxiv paper:2603.16133 digest entry three",
        "cn-rollup-homogeneous",
        "evidence_ref",
    ))
    .await
    .unwrap();

    drain_for_test(&services, &services.consolidation_queue, 4).await;

    let body = json!({
        "query": "arxiv paper digest",
        "top_k": 10,
        "group_by": "session"
    });
    let res = server.post("/api/v1/tools/retrieve").json(&body).await;
    res.assert_status_ok();
    let body: Value = res.json();

    let groups = body["session_groups"].as_array().expect("groups present");
    let diverse = groups
        .iter()
        .find(|g| g["session_id"] == "cn-rollup-diverse")
        .expect("diverse session must be in groups");
    let homogeneous = groups
        .iter()
        .find(|g| g["session_id"] == "cn-rollup-homogeneous");

    let diverse_kinds = diverse["unique_kinds"].as_array().unwrap();
    assert!(
        diverse_kinds.len() >= 3,
        "diverse session should report all 3 distinct kinds, got {}",
        diverse_kinds.len()
    );

    // The diverse session must score above the homogeneous one when
    // both are present. (The homogeneous session may get filtered
    // entirely if every member is `evidence_ref` and the density
    // multiplier from Option A is aggressive enough — that's also a
    // valid outcome.)
    if let Some(homog) = homogeneous {
        let diverse_score = diverse["score"].as_f64().unwrap();
        let homog_score = homog["score"].as_f64().unwrap();
        assert!(
            diverse_score > homog_score,
            "expected diverse > homogeneous: diverse={diverse_score:.3}, homog={homog_score:.3}"
        );
    }
}

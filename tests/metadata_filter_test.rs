//! Integration tests for `metadata_filter` on the `retrieve` HTTP handler.
//!
//! Exercises the round-trip contract added in this PR:
//!
//! 1. `store` accepts and persists arbitrary `metadata: HashMap<String, Value>`
//! 2. `retrieve` without `metadata_filter` returns all matching fragments
//!    (backward-compatible — pre-existing tests still pass)
//! 3. `retrieve` with `metadata_filter` only returns fragments whose stored
//!    metadata contains every (key, value) pair from the filter
//! 4. Fragments stored without metadata are filtered out by any non-empty
//!    filter (the conservative semantics: "no metadata at all" never
//!    matches a positive constraint)
//! 5. The `RetrieveHit` response carries the fragment's full metadata so
//!    consumers can see what they matched on without a second request

use axum_test::TestServer;
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
    body["attractor_id"]
        .as_str()
        .expect("attractor_id present")
        .to_string()
}

async fn retrieve(server: &TestServer, session: &str, query: &str, filter: Option<Value>) -> Value {
    let mut body = json!({
        "query": query,
        "top_k": 50,
        "session_id": session,
    });
    if let Some(f) = filter {
        body["metadata_filter"] = f;
    }
    let res = server.post("/api/v1/tools/retrieve").json(&body).await;
    res.assert_status_ok();
    res.json()
}

#[tokio::test]
async fn retrieve_without_filter_returns_every_matching_fragment() {
    let server = make_server().await;
    let session = "test-no-filter";

    store(
        &server,
        session,
        "the user wants to fix the mermaid parse error",
        json!({"kind": "decision"}),
    )
    .await;
    store(
        &server,
        session,
        "another mermaid fix note",
        json!({"kind": "learning"}),
    )
    .await;

    let body = retrieve(&server, session, "mermaid", None).await;
    let hits = body["hits"].as_array().expect("hits array");
    // Both fragments come back when no filter is set — backward-compat.
    assert_eq!(hits.len(), 2, "no filter → both fragments returned");
}

#[tokio::test]
async fn retrieve_with_filter_only_returns_matching_fragments() {
    let server = make_server().await;
    let session = "test-filter-kind";

    store(
        &server,
        session,
        "deciding whether to use embeddings or token overlap",
        json!({"kind": "decision", "urgency": "now"}),
    )
    .await;
    store(
        &server,
        session,
        "learned that mermaid sequenceDiagram does not support br tags",
        json!({"kind": "learning"}),
    )
    .await;
    store(
        &server,
        session,
        "shipped the schema extension",
        json!({"kind": "accomplishment"}),
    )
    .await;

    // Filter by kind=decision → exactly one match
    let body = retrieve(
        &server,
        session,
        "what should we do next?",
        Some(json!({"kind": "decision"})),
    )
    .await;
    let hits = body["hits"].as_array().expect("hits array");
    assert_eq!(hits.len(), 1, "exactly one decision in this session");
    assert!(hits[0]["content"]
        .as_str()
        .unwrap()
        .contains("deciding whether"));
    // Response carries the matched metadata back
    assert_eq!(hits[0]["metadata"]["kind"], "decision");
    assert_eq!(hits[0]["metadata"]["urgency"], "now");
}

#[tokio::test]
async fn filter_requires_all_keys_to_match() {
    let server = make_server().await;
    let session = "test-multi-key-filter";

    // Two decisions, one urgent, one not
    store(
        &server,
        session,
        "urgent thing to decide",
        json!({"kind": "decision", "urgency": "now"}),
    )
    .await;
    store(
        &server,
        session,
        "later thing to decide",
        json!({"kind": "decision", "urgency": "later"}),
    )
    .await;

    // Filter on kind alone → both
    let body = retrieve(
        &server,
        session,
        "what should we decide?",
        Some(json!({"kind": "decision"})),
    )
    .await;
    assert_eq!(body["hits"].as_array().unwrap().len(), 2);

    // Filter on kind + urgency → only the urgent one
    let body = retrieve(
        &server,
        session,
        "what should we decide?",
        Some(json!({"kind": "decision", "urgency": "now"})),
    )
    .await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["metadata"]["urgency"], "now");
}

#[tokio::test]
async fn fragments_without_metadata_dont_match_a_positive_filter() {
    let server = make_server().await;
    let session = "test-missing-meta";

    // One with metadata, one without
    store(
        &server,
        session,
        "has metadata",
        json!({"kind": "learning"}),
    )
    .await;
    let bare = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "no metadata at all",
            "importance": 0.7,
            "session_id": session,
            // metadata omitted entirely
        }))
        .await;
    bare.assert_status_ok();

    // Without filter → both come back
    let body = retrieve(&server, session, "metadata", None).await;
    assert_eq!(body["hits"].as_array().unwrap().len(), 2);

    // With filter requiring kind=learning → only the one with metadata
    let body = retrieve(
        &server,
        session,
        "metadata",
        Some(json!({"kind": "learning"})),
    )
    .await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0]["content"]
        .as_str()
        .unwrap()
        .contains("has metadata"));
}

#[tokio::test]
async fn empty_filter_is_a_no_op_not_a_no_match() {
    let server = make_server().await;
    let session = "test-empty-filter";

    store(&server, session, "thing A", json!({"kind": "thing"})).await;
    store(&server, session, "thing B", json!({})).await;

    // `metadata_filter: {}` should match every fragment (vacuous filter)
    let body = retrieve(&server, session, "thing", Some(json!({}))).await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 2, "empty filter matches everything");
}

#[tokio::test]
async fn nonmatching_filter_returns_empty_hits_not_an_error() {
    let server = make_server().await;
    let session = "test-no-match";

    store(
        &server,
        session,
        "only fragment",
        json!({"kind": "learning"}),
    )
    .await;

    let body = retrieve(
        &server,
        session,
        "anything",
        Some(json!({"kind": "decision"})),
    )
    .await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 0, "no kind=decision in this session");
}

#[tokio::test]
async fn nested_value_filter_works_for_simple_types() {
    let server = make_server().await;
    let session = "test-nested-values";

    // Boolean values + numeric step are common in user_action metadata.
    store(
        &server,
        session,
        "awaiting decision text",
        json!({
            "kind": "decision",
            "awaiting_decision": true,
            "step": 1,
        }),
    )
    .await;
    store(
        &server,
        session,
        "settled decision text",
        json!({
            "kind": "decision",
            "awaiting_decision": false,
            "step": 1,
        }),
    )
    .await;

    // Filter by boolean → only awaiting one
    let body = retrieve(
        &server,
        session,
        "decision",
        Some(json!({"awaiting_decision": true})),
    )
    .await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0]["content"].as_str().unwrap().contains("awaiting"));
}

// ─────────────────────────────────────────────────────────────────────────────
// exclude_kinds tests
//
// Pins the false-positive defence built after a real-world incident where
// every Claude Code session has a fragment of kind `initial_prompt_window`
// containing literal `[user turn N] <user text>` — which then matched every
// topic query containing "user" via the TF-IDF embedder. The fix is the
// new `exclude_kinds` knob; these tests ensure the semantics stay correct
// across future refactors.
// ─────────────────────────────────────────────────────────────────────────────

async fn retrieve_with(server: &TestServer, session: &str, query: &str, extra: Value) -> Value {
    let mut body = json!({
        "query": query,
        "top_k": 50,
        "session_id": session,
    });
    if let Some(obj) = extra.as_object() {
        for (k, v) in obj {
            body[k] = v.clone();
        }
    }
    let res = server.post("/api/v1/tools/retrieve").json(&body).await;
    res.assert_status_ok();
    res.json()
}

#[tokio::test]
async fn exclude_kinds_filters_out_named_kinds() {
    let server = make_server().await;
    let session = "test-exclude-kinds";
    store(
        &server,
        session,
        "[user turn 1] tell me about chat rendering",
        json!({"kind": "initial_prompt_window"}),
    )
    .await;
    store(
        &server,
        session,
        "shipped chat orchestrator phase 2",
        json!({"kind": "accomplishment"}),
    )
    .await;
    store(
        &server,
        session,
        "decided to use SSE for chat streaming",
        json!({"kind": "decision"}),
    )
    .await;

    // Baseline: no exclusion → all 3 come back.
    let body = retrieve_with(&server, session, "chat", json!({})).await;
    assert_eq!(body["hits"].as_array().unwrap().len(), 3);

    // With exclude_kinds=["initial_prompt_window"] → only the 2 real
    // work fragments remain.
    let body = retrieve_with(
        &server,
        session,
        "chat",
        json!({"exclude_kinds": ["initial_prompt_window"]}),
    )
    .await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 2);
    for h in hits {
        assert_ne!(
            h["metadata"]["kind"].as_str().unwrap_or(""),
            "initial_prompt_window",
            "excluded kind must NEVER appear in results"
        );
    }
}

#[tokio::test]
async fn exclude_kinds_empty_list_is_no_op() {
    // Empty list ≡ no filter — preserves backward compat for callers
    // that always send the parameter even when not filtering.
    let server = make_server().await;
    let session = "test-exclude-empty";
    store(
        &server,
        session,
        "noise",
        json!({"kind": "initial_prompt_window"}),
    )
    .await;
    store(
        &server,
        session,
        "real content",
        json!({"kind": "accomplishment"}),
    )
    .await;

    let body = retrieve_with(&server, session, "content", json!({"exclude_kinds": []})).await;
    assert_eq!(body["hits"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn exclude_kinds_keeps_fragments_with_no_kind_field() {
    // "Excluding requires a positive match" — a fragment without a
    // `kind` metadata field is never excluded.
    let server = make_server().await;
    let session = "test-exclude-no-kind";
    store(&server, session, "fragment without kind", json!({})).await;
    store(
        &server,
        session,
        "boilerplate",
        json!({"kind": "initial_prompt_window"}),
    )
    .await;

    let body = retrieve_with(
        &server,
        session,
        "fragment",
        json!({"exclude_kinds": ["initial_prompt_window"]}),
    )
    .await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0]["content"]
        .as_str()
        .unwrap()
        .contains("fragment without kind"));
}

#[tokio::test]
async fn exclude_kinds_composes_with_metadata_filter() {
    // Both filters must pass — fragment included only if it passes
    // `metadata_filter` AND is not in `exclude_kinds`.
    let server = make_server().await;
    let session = "test-exclude-compose";
    store(
        &server,
        session,
        "matches both",
        json!({"kind": "decision", "project": "p1"}),
    )
    .await;
    store(
        &server,
        session,
        "wrong project",
        json!({"kind": "decision", "project": "p2"}),
    )
    .await;
    store(
        &server,
        session,
        "right project but excluded kind",
        json!({"kind": "initial_prompt_window", "project": "p1"}),
    )
    .await;

    let body = retrieve_with(
        &server,
        session,
        "project",
        json!({
            "metadata_filter": {"project": "p1"},
            "exclude_kinds": ["initial_prompt_window"]
        }),
    )
    .await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0]["content"]
        .as_str()
        .unwrap()
        .contains("matches both"));
}

#[tokio::test]
async fn exclude_kinds_supports_multiple_kinds() {
    let server = make_server().await;
    let session = "test-exclude-multi";
    store(&server, session, "frag a", json!({"kind": "noise_a"})).await;
    store(&server, session, "frag b", json!({"kind": "noise_b"})).await;
    store(&server, session, "frag c", json!({"kind": "signal"})).await;

    let body = retrieve_with(
        &server,
        session,
        "frag",
        json!({"exclude_kinds": ["noise_a", "noise_b"]}),
    )
    .await;
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["metadata"]["kind"].as_str().unwrap(), "signal");
}

// ─────────────────────────────────────────────────────────────────────────────
// Kind taxonomy (D2) end-to-end tests
//
// The kind_registry assigns each ingest kind a SignalClass + default
// weight. The retrieve handler multiplies similarity by that weight.
// The compound effect is what these tests assert: boilerplate
// fragments effectively drop out of ranking WITHOUT the caller
// passing `exclude_kinds`. The band-aid still works but is no longer
// required for the common case.
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn boilerplate_kinds_are_zero_weighted_at_retrieve() {
    let server = make_server().await;
    let session = "test-d2-zero-weight";
    store(
        &server,
        session,
        "[user turn 1] hello chat about rendering",
        json!({"kind": "initial_prompt_window"}),
    )
    .await;
    store(
        &server,
        session,
        "decided to render chat via streaming SSE",
        json!({"kind": "decision"}),
    )
    .await;

    // No `exclude_kinds` passed — the kind taxonomy alone must
    // zero-weight the boilerplate fragment.
    let body = retrieve_with(&server, session, "chat rendering", json!({})).await;
    let hits = body["hits"].as_array().unwrap();
    assert!(!hits.is_empty(), "at least one hit expected");

    let decision = hits
        .iter()
        .find(|h| h["metadata"]["kind"].as_str() == Some("decision"))
        .expect("decision fragment must appear in results");
    let dec_sim = decision["similarity"].as_f64().unwrap_or(0.0);
    assert!(
        dec_sim > 0.0,
        "decision fragment must have non-zero similarity (got {dec_sim})"
    );

    // If boilerplate appears, its similarity must be zero.
    if let Some(bp) = hits
        .iter()
        .find(|h| h["metadata"]["kind"].as_str() == Some("initial_prompt_window"))
    {
        let bp_sim = bp["similarity"].as_f64().unwrap_or(-1.0);
        assert_eq!(
            bp_sim, 0.0,
            "boilerplate fragment must score zero via kind_weight (got {bp_sim})"
        );
    }
}

#[tokio::test]
async fn system_state_kinds_are_halved() {
    // SystemState kinds (like `state`) get 0.5× signal. Same text,
    // different kind tags → 2× score gap.
    let server = make_server().await;
    let session = "test-d2-half-weight";
    store(
        &server,
        session,
        "investigating the chat orchestrator timing bug",
        json!({"kind": "state"}),
    )
    .await;
    store(
        &server,
        session,
        "investigating the chat orchestrator timing bug",
        json!({"kind": "decision"}),
    )
    .await;

    let body = retrieve_with(&server, session, "chat orchestrator", json!({})).await;
    let hits = body["hits"].as_array().unwrap();
    let state = hits
        .iter()
        .find(|h| h["metadata"]["kind"].as_str() == Some("state"))
        .expect("state fragment present");
    let decision = hits
        .iter()
        .find(|h| h["metadata"]["kind"].as_str() == Some("decision"))
        .expect("decision fragment present");
    let state_sim = state["similarity"].as_f64().unwrap_or(0.0);
    let dec_sim = decision["similarity"].as_f64().unwrap_or(0.0);
    assert!(
        state_sim < dec_sim,
        "state ({state_sim}) must rank below decision ({dec_sim}) due to 0.5x SystemState weight"
    );
    if dec_sim > 0.001 {
        let ratio = state_sim / dec_sim;
        assert!(
            (0.3..=0.7).contains(&ratio),
            "ratio state/decision should be ~0.5 (allowing for decay + embed wiggle); got {ratio}"
        );
    }
}

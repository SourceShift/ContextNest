//! Integration tests for the files-touched + delivered-features
//! extraction added by feat/files-touched-index.
//!
//! Covers:
//!
//! 1. `tool_use` events with `Edit` / `Write` / `MultiEdit` /
//!    `NotebookEdit` populate a single `MemoryKind::FilesTouched`
//!    record per session, with `metadata.files` carrying the
//!    deduplicated path list.
//! 2. Read-only tools (`Read`, `Grep`, `Glob`, `Bash`) do NOT pull a
//!    file into the touched set.
//! 3. A `delivered_features[]` array in a z-insight block produces
//!    one `MemoryKind::Feature` record per entry, with the feature
//!    name as the fragment text and `files`/`refs`/`layer`
//!    propagated into metadata.
//! 4. `GET /api/v1/sessions/by-file?path=<basename>` finds the
//!    session(s) whose `files_touched` array contains a path
//!    matching the substring.
//! 5. `GET /api/v1/sessions/by-feature?q=<word>` finds the sessions
//!    whose declared feature text contains the substring.

use axum_test::TestServer;
use contextnest::api::create_simple_app;
use contextnest::ingest::claude_code::event::parse_session_string;
use contextnest::ingest::claude_code::extractor::{extract_memories, MemoryKind, MemoryRecord};
use contextnest::ingest::claude_code::sink::{ServicesSink, Sink};
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};

fn assistant_with_tool_use(name: &str, file_path: &str, ts: &str) -> String {
    let line = json!({
        "type": "assistant",
        "timestamp": ts,
        "sessionId": "fixture-uuid",
        "message": {
            "role": "assistant",
            "content": [
                {"type": "text", "text": "doing the thing"},
                {"type": "tool_use", "name": name, "input": {"file_path": file_path}},
            ],
        },
    });
    format!("{line}")
}

fn assistant_with_feature_block(features: Value, ts: &str) -> String {
    let z = json!({
        "domain": "backend",
        "goal": "test fixture",
        "progress": "in-progress",
        "current_state": "writing the test",
        "delivered_features": features,
    });
    let text = format!("<z-insight>{z}</z-insight>");
    let line = json!({
        "type": "assistant",
        "timestamp": ts,
        "sessionId": "fixture-uuid",
        "message": {
            "role": "assistant",
            "content": [
                {"type": "text", "text": text},
            ],
        },
    });
    format!("{line}")
}

fn extract_from_lines(lines: &[String], session_uuid: &str, project: &str) -> Vec<MemoryRecord> {
    let joined = lines.join("\n");
    let (events, _meta) = parse_session_string(&joined);
    extract_memories(&events, session_uuid, project)
}

#[test]
fn tool_use_writes_collect_into_one_files_touched_record() {
    let lines = vec![
        assistant_with_tool_use("Edit", "src/api/tools.rs", "2026-05-22T10:00:00Z"),
        assistant_with_tool_use("Write", "tests/new_test.rs", "2026-05-22T10:01:00Z"),
        // Re-edit the same file — should dedup to one entry.
        assistant_with_tool_use("MultiEdit", "src/api/tools.rs", "2026-05-22T10:02:00Z"),
    ];
    let records = extract_from_lines(&lines, "uuid-files", "/work/proj");
    let files_touched: Vec<&MemoryRecord> = records
        .iter()
        .filter(|r| r.kind == MemoryKind::FilesTouched)
        .collect();
    assert_eq!(
        files_touched.len(),
        1,
        "expected exactly one FilesTouched record per session"
    );
    let files = files_touched[0]
        .metadata
        .get("files")
        .and_then(|v| v.as_array())
        .expect("metadata.files must be an array");
    let names: Vec<&str> = files.iter().filter_map(|v| v.as_str()).collect();
    assert_eq!(names.len(), 2, "dedup should collapse repeated edits");
    assert!(names.contains(&"src/api/tools.rs"));
    assert!(names.contains(&"tests/new_test.rs"));
}

#[test]
fn read_only_tools_do_not_count_as_touched() {
    let lines = vec![
        assistant_with_tool_use("Read", "src/api/tools.rs", "2026-05-22T10:00:00Z"),
        assistant_with_tool_use("Grep", "src/", "2026-05-22T10:01:00Z"),
        assistant_with_tool_use("Glob", "**/*.rs", "2026-05-22T10:02:00Z"),
        assistant_with_tool_use("Bash", "/usr/bin/cargo", "2026-05-22T10:03:00Z"),
    ];
    let records = extract_from_lines(&lines, "uuid-readonly", "/work/proj");
    let has_touched = records.iter().any(|r| r.kind == MemoryKind::FilesTouched);
    assert!(
        !has_touched,
        "read-only tool_use must not produce a FilesTouched record"
    );
}

#[test]
fn delivered_features_emit_one_feature_record_per_entry() {
    let features = json!([
        {
            "feature": "query-overlay mode for /field viz",
            "files": ["web/src/routes/field.tsx", "web/src/styles.css"],
            "layer": "frontend",
            "refs": ["PR #39"]
        },
        {
            "feature": "background consolidation worker",
            "files": ["src/services/consolidation.rs"],
            "layer": "backend"
        },
    ]);
    let lines = vec![assistant_with_feature_block(
        features,
        "2026-05-22T11:00:00Z",
    )];
    let records = extract_from_lines(&lines, "uuid-features", "/work/proj");
    let feature_recs: Vec<&MemoryRecord> = records
        .iter()
        .filter(|r| r.kind == MemoryKind::Feature)
        .collect();
    assert_eq!(
        feature_recs.len(),
        2,
        "one record per delivered_features entry"
    );

    let names: Vec<&str> = feature_recs.iter().map(|r| r.text.as_str()).collect();
    assert!(names.contains(&"query-overlay mode for /field viz"));
    assert!(names.contains(&"background consolidation worker"));

    // First record should carry files + layer + refs.
    let first = feature_recs
        .iter()
        .find(|r| r.text == "query-overlay mode for /field viz")
        .unwrap();
    let files = first
        .metadata
        .get("files")
        .and_then(|v| v.as_array())
        .expect("files field must be present");
    assert_eq!(files.len(), 2);
    assert_eq!(
        first.metadata.get("layer").and_then(|v| v.as_str()),
        Some("frontend")
    );
    let refs = first
        .metadata
        .get("refs")
        .and_then(|v| v.as_array())
        .expect("refs field must be present");
    assert_eq!(refs.len(), 1);
}

#[test]
fn empty_feature_name_is_skipped() {
    let features = json!([
        {"feature": "", "files": ["foo.rs"]},
        {"feature": "real one"},
    ]);
    let lines = vec![assistant_with_feature_block(
        features,
        "2026-05-22T11:00:00Z",
    )];
    let records = extract_from_lines(&lines, "uuid-skip-empty", "/work/proj");
    let feats: Vec<&MemoryRecord> = records
        .iter()
        .filter(|r| r.kind == MemoryKind::Feature)
        .collect();
    assert_eq!(feats.len(), 1, "empty feature name must be skipped");
    assert_eq!(feats[0].text, "real one");
}

// ---------------------------------------------------------------------------
// Endpoint tests
// ---------------------------------------------------------------------------

async fn make_server() -> (ContextNestServices, TestServer) {
    let services = ContextNestServices::new_default().await.unwrap();
    let app = create_simple_app(services.clone()).await.unwrap();
    let server = TestServer::new(app).unwrap();
    (services, server)
}

async fn push_files_touched(services: &ContextNestServices, session_uuid: &str, files: Vec<&str>) {
    let sink = ServicesSink::new(services.clone());
    let session_id_cn = session_uuid.to_string();
    let summary = format!("session touched {} file(s)", files.len());
    let files_value: Vec<Value> = files.iter().map(|f| json!(f)).collect();
    let mut meta = std::collections::HashMap::new();
    meta.insert("kind".to_string(), json!("files_touched"));
    meta.insert("ts".to_string(), json!("2026-05-22T10:00:00Z"));
    meta.insert("src_session".to_string(), json!(session_uuid));
    meta.insert("files".to_string(), json!(files_value));
    let record = contextnest::ingest::claude_code::extractor::MemoryRecord {
        kind: MemoryKind::FilesTouched,
        text: summary,
        importance: 0.85,
        session_id_cn,
        metadata: meta,
    };
    sink.store(&record).await.unwrap();
}

async fn push_feature(
    services: &ContextNestServices,
    session_uuid: &str,
    feature_name: &str,
    files: Vec<&str>,
) {
    let sink = ServicesSink::new(services.clone());
    let session_id_cn = session_uuid.to_string();
    let files_value: Vec<Value> = files.iter().map(|f| json!(f)).collect();
    let mut meta = std::collections::HashMap::new();
    meta.insert("kind".to_string(), json!("feature"));
    meta.insert("ts".to_string(), json!("2026-05-22T11:00:00Z"));
    meta.insert("src_session".to_string(), json!(session_uuid));
    meta.insert("files".to_string(), json!(files_value));
    meta.insert("layer".to_string(), json!("frontend"));
    let record = contextnest::ingest::claude_code::extractor::MemoryRecord {
        kind: MemoryKind::Feature,
        text: feature_name.to_string(),
        importance: 0.90,
        session_id_cn,
        metadata: meta,
    };
    sink.store(&record).await.unwrap();
}

#[tokio::test]
async fn sessions_by_file_returns_matching_session() {
    let (services, server) = make_server().await;
    push_files_touched(
        &services,
        "uuid-session-alpha",
        vec!["web/src/components/AgentStreamRail.tsx", "src/lib/util.ts"],
    )
    .await;
    push_files_touched(
        &services,
        "uuid-session-beta",
        vec!["src/api/tools.rs", "Cargo.toml"],
    )
    .await;

    let res = server
        .get("/api/v1/sessions/by-file?path=AgentStreamRail.tsx")
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1, "only the alpha session touched that file");
    assert_eq!(matches[0]["session_id"], "uuid-session-alpha");
    let matched_files = matches[0]["matched_files"].as_array().unwrap();
    assert_eq!(matched_files.len(), 1);
    assert!(matched_files[0]
        .as_str()
        .unwrap()
        .contains("AgentStreamRail.tsx"));
}

#[tokio::test]
async fn sessions_by_file_match_is_case_insensitive() {
    let (services, server) = make_server().await;
    push_files_touched(&services, "uuid-case", vec!["src/api/Tools.rs"]).await;
    let res = server.get("/api/v1/sessions/by-file?path=tools.rs").await;
    res.assert_status_ok();
    let body: Value = res.json();
    let matches = body["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1);
}

#[tokio::test]
async fn sessions_by_file_empty_path_returns_empty_list() {
    let (_services, server) = make_server().await;
    let res = server.get("/api/v1/sessions/by-file?path=").await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["matches"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn sessions_by_feature_returns_matching_session() {
    let (services, server) = make_server().await;
    push_feature(
        &services,
        "uuid-feature-overlay",
        "query-overlay mode for /field viz",
        vec!["web/src/routes/field.tsx"],
    )
    .await;
    push_feature(
        &services,
        "uuid-feature-decay",
        "decay at retrieve time",
        vec!["src/api/tools.rs"],
    )
    .await;

    let res = server
        .get("/api/v1/sessions/by-feature?q=query-overlay")
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["session_id"], "uuid-feature-overlay");
    assert!(hits[0]["feature"]
        .as_str()
        .unwrap()
        .contains("query-overlay"));
    let files = hits[0]["files"].as_array().unwrap();
    assert!(!files.is_empty());
}

#[tokio::test]
async fn sessions_by_feature_match_is_case_insensitive() {
    let (services, server) = make_server().await;
    push_feature(
        &services,
        "uuid-case-feat",
        "Background CONSOLIDATION worker",
        vec!["src/services/consolidation.rs"],
    )
    .await;
    let res = server
        .get("/api/v1/sessions/by-feature?q=consolidation")
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    let hits = body["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
}

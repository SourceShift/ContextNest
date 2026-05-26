//! Integration tests for the Claude Code session ingester.
//!
//! Uses a small representative fixture .jsonl at
//! `tests/fixtures/cc_session_sample.jsonl` to exercise the full pipeline:
//! parse → extract → push (DryRun) → assert the resulting memory shape.
//!
//! No network calls; no real substrate required. The HTTP sink is
//! exercised by separate tests once a live substrate is available.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use contextnest::ingest::claude_code::{
    decode_project_dir_name, discover_sessions, ingest_session_file, parse_session_file,
    parse_since, DiscoveredSession, DryRunSink, MemoryKind, Sink,
};

fn fixture_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push("cc_session_sample.jsonl");
    p
}

#[tokio::test]
async fn end_to_end_parses_extracts_and_pushes_to_dry_run_sink() {
    let path = fixture_path();
    assert!(
        path.exists(),
        "fixture missing — expected at {}",
        path.display()
    );

    let session = DiscoveredSession {
        jsonl_path: path.clone(),
        project_cwd: "/work/example".to_string(),
        session_uuid: "abc12345-fixture-session".to_string(),
        modified: None,
        size_bytes: 0,
    };

    let sink = DryRunSink::silent();
    let report = ingest_session_file(&session, &sink).await.unwrap();

    // Every record went to the sink successfully (DryRun never fails).
    assert!(report.success > 0, "expected memories, got 0");
    assert_eq!(report.failed, 0, "DryRunSink should never fail");

    // The fixture has every memory kind that matters for the inbox path.
    let by_kind = sink.captured_by_kind().await;
    let kinds: Vec<&str> = by_kind.keys().map(|s| s.as_str()).collect();

    for required in [
        "session_title",
        "initial_prompt_window",
        "goal_phase",
        "state",
        "current_task",
        "accomplishment",
        "learning",
        "todo",
        "user_action",
        "decision",
        "summary",
    ] {
        assert!(
            kinds.contains(&required),
            "missing kind: {} (got: {:?})",
            required,
            kinds
        );
    }
}

#[tokio::test]
async fn awaiting_decision_records_carry_decision_metadata() {
    let path = fixture_path();
    let session = DiscoveredSession {
        jsonl_path: path,
        project_cwd: "/work/example".to_string(),
        session_uuid: "abc12345-fixture-session".to_string(),
        modified: None,
        size_bytes: 0,
    };

    let sink = DryRunSink::silent();
    ingest_session_file(&session, &sink).await.unwrap();

    let captured = sink.captured.read().await;
    let decisions: Vec<_> = captured
        .iter()
        .filter(|r| r.kind == MemoryKind::Decision)
        .collect();
    assert_eq!(decisions.len(), 1, "fixture has one decision");
    let d = decisions[0];

    // Decision text matches what the z-insight block emitted
    assert!(d.text.contains("GitHub-rendered"));

    // awaiting_decision metadata flag is set to true
    assert_eq!(
        d.metadata.get("awaiting_decision"),
        Some(&serde_json::Value::Bool(true))
    );
    // decision_text duplicated into metadata for ease of filtering
    assert!(d
        .metadata
        .get("decision_text")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.contains("GitHub-rendered")));
}

#[tokio::test]
async fn user_actions_carry_urgency_metadata() {
    let path = fixture_path();
    let session = DiscoveredSession {
        jsonl_path: path,
        project_cwd: "/work/example".to_string(),
        session_uuid: "abc12345-fixture-session".to_string(),
        modified: None,
        size_bytes: 0,
    };

    let sink = DryRunSink::silent();
    ingest_session_file(&session, &sink).await.unwrap();

    let captured = sink.captured.read().await;
    let actions: Vec<_> = captured
        .iter()
        .filter(|r| r.kind == MemoryKind::UserAction)
        .collect();
    // Fixture has 2 user actions in the second z-insight block
    assert_eq!(actions.len(), 2, "fixture has two user actions");
    for a in &actions {
        assert_eq!(
            a.metadata.get("urgency").and_then(|v| v.as_str()),
            Some("now"),
            "fixture marks both actions urgency=now"
        );
        assert!(a.metadata.contains_key("reason"));
        assert!(a.metadata.contains_key("step"));
    }
}

#[tokio::test]
async fn todos_collapse_to_final_state_per_id() {
    let path = fixture_path();
    let session = DiscoveredSession {
        jsonl_path: path,
        project_cwd: "/work/example".to_string(),
        session_uuid: "abc12345-fixture-session".to_string(),
        modified: None,
        size_bytes: 0,
    };

    let sink = DryRunSink::silent();
    ingest_session_file(&session, &sink).await.unwrap();

    let captured = sink.captured.read().await;
    let todos: Vec<_> = captured
        .iter()
        .filter(|r| r.kind == MemoryKind::Todo)
        .collect();

    // Fixture has T-1 emitted twice (in_progress then completed). After
    // dedup it should appear ONCE with its final status (completed).
    assert_eq!(todos.len(), 1, "T-1 deduped to one todo");
    let final_status = todos[0]
        .metadata
        .get("task_status")
        .and_then(|v| v.as_str());
    assert_eq!(final_status, Some("completed"), "final status wins");
}

#[tokio::test]
async fn every_record_carries_session_metadata() {
    let path = fixture_path();
    let session = DiscoveredSession {
        jsonl_path: path,
        project_cwd: "/work/example".to_string(),
        session_uuid: "abc12345-fixture-session".to_string(),
        modified: None,
        size_bytes: 0,
    };

    let sink = DryRunSink::silent();
    ingest_session_file(&session, &sink).await.unwrap();

    let captured = sink.captured.read().await;
    for r in captured.iter() {
        // Substrate session id is the bare Claude Code session UUID — no
        // legacy `cc-` prefix.
        assert!(
            !r.session_id_cn.starts_with("cc-"),
            "session id should not carry the legacy cc- prefix: kind={:?} sess={}",
            r.kind,
            r.session_id_cn
        );
        assert_eq!(r.session_id_cn, "abc12345-fixture-session");
        // Every record carries src_session + kind + project_cwd metadata
        assert!(r.metadata.contains_key("kind"));
        assert_eq!(
            r.metadata.get("src_session").and_then(|v| v.as_str()),
            Some("abc12345-fixture-session"),
        );
        assert_eq!(
            r.metadata.get("project_cwd").and_then(|v| v.as_str()),
            Some("/work/example"),
        );
    }
}

#[test]
fn discover_sessions_filters_by_project_and_since() {
    // Build a temp ~/.claude/projects/-fake-X layout and verify
    // discovery returns the right .jsonl files.
    use std::fs;
    let tmp = tempfile::tempdir().unwrap();
    let projects = tmp.path();
    let proj_a = projects.join("-work-projectA");
    let proj_b = projects.join("-work-projectB");
    fs::create_dir_all(&proj_a).unwrap();
    fs::create_dir_all(&proj_b).unwrap();
    fs::write(proj_a.join("aaaa1111.jsonl"), "{}\n").unwrap();
    fs::write(proj_a.join("aaaa2222.jsonl"), "{}\n").unwrap();
    fs::write(proj_b.join("bbbb1111.jsonl"), "{}\n").unwrap();

    // No filters → 3 sessions across 2 projects
    let all = discover_sessions(projects, None, None).unwrap();
    assert_eq!(all.len(), 3);

    // Filter by project A substring → 2 sessions
    let a = discover_sessions(projects, Some("projectA"), None).unwrap();
    assert_eq!(a.len(), 2);
    assert!(a.iter().all(|s| s.project_cwd.contains("projectA")));

    // Filter case-insensitive
    let a_ci = discover_sessions(projects, Some("PROJECTA"), None).unwrap();
    assert_eq!(a_ci.len(), 2);

    // since cutoff in the future → 0 sessions match
    let future = SystemTime::now() + Duration::from_secs(3600);
    let none = discover_sessions(projects, None, Some(future)).unwrap();
    assert_eq!(none.len(), 0);

    // since cutoff in the past → all 3 match
    let past = SystemTime::now() - Duration::from_secs(3600);
    let all_past = discover_sessions(projects, None, Some(past)).unwrap();
    assert_eq!(all_past.len(), 3);
}

#[test]
fn decode_project_dir_name_is_lossy_on_dashes_as_documented() {
    // The Claude Code encoding replaces `/` with `-`, so the decode is
    // lossy on paths whose components contain dashes — that's documented
    // behaviour and the integration tests rely on the substring-match
    // semantics, not strict equality.
    let decoded = decode_project_dir_name("-Volumes-docker-ssd-Migration-Development-ContextNest");
    assert!(decoded.starts_with("/Volumes/"));
    assert!(decoded.contains("ContextNest"));

    // Paths without internal dashes round-trip cleanly.
    assert_eq!(decode_project_dir_name("-Users-admin"), "/Users/admin");
}

#[test]
fn parse_since_known_units() {
    use std::time::Duration;
    assert_eq!(parse_since("7d"), Some(Duration::from_secs(7 * 86400)));
    assert_eq!(parse_since("24h"), Some(Duration::from_secs(24 * 3600)));
    assert_eq!(parse_since("30m"), Some(Duration::from_secs(1800)));
    assert_eq!(parse_since("2w"), Some(Duration::from_secs(2 * 7 * 86400)));
    assert!(parse_since("nope").is_none());
}

#[test]
fn parse_session_file_recovers_metadata_from_fixture() {
    let (events, metadata) = parse_session_file(&fixture_path()).unwrap();
    assert!(events.len() >= 5, "fixture should have 6+ events");
    assert_eq!(
        metadata.session_uuid.as_deref(),
        Some("abc12345-fixture-session")
    );
    assert_eq!(metadata.cwd.as_deref(), Some("/work/example"));
    assert_eq!(
        metadata.ai_title.as_deref(),
        Some("Fix mermaid parse error in architecture docs")
    );
}

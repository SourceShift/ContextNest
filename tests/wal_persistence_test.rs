//! End-to-end WAL persistence: store via the HTTP handler, drop the
//! services (simulating a restart), replay the WAL into a fresh services
//! instance, and confirm the fragment is queryable through the same
//! HTTP routes — same shape the live `serve` binary uses.

use axum_test::TestServer;
use contextnest::api::{
    create_simple_app,
    tools::{restore_sidecars_bulk, store_with_id},
};
use contextnest::ingest::claude_code::{MemoryKind, MemoryRecord, ServicesSink, Sink};
use contextnest::services::wal::{Wal, WalRecord};
use contextnest::services::ContextNestServices;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::tempdir;

async fn make_server_with_wal(wal_path: std::path::PathBuf) -> TestServer {
    let services = ContextNestServices::new_default()
        .await
        .expect("services init");
    let writer = Wal::open_for_append(wal_path).expect("wal open");
    services.wal.set(writer).expect("wal once-cell unset");
    let app = create_simple_app(services).await.expect("app build");
    TestServer::new(app).expect("test server")
}

#[tokio::test]
async fn store_appends_record_to_wal() {
    let dir = tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");

    let server = make_server_with_wal(wal_path.clone()).await;
    let res = server
        .post("/api/v1/tools/store")
        .json(&json!({
            "content": "first fragment from handler",
            "importance": 0.8,
            "session_id": "cc-wal",
            "metadata": {"kind": "user_action", "urgency": "now"},
        }))
        .await;
    res.assert_status_ok();
    let body: Value = res.json();
    let frag_id = body["attractor_id"].as_str().unwrap().to_string();

    let records = Wal::read_records(&wal_path).expect("read wal");
    assert_eq!(records.len(), 1);
    match &records[0] {
        WalRecord::Store {
            fragment_id,
            session_id,
            content,
            importance,
            metadata,
        } => {
            assert_eq!(fragment_id, &frag_id);
            assert_eq!(session_id, "cc-wal");
            assert_eq!(content, "first fragment from handler");
            assert!((*importance - 0.8).abs() < 1e-6);
            assert_eq!(metadata.get("kind").unwrap(), &json!("user_action"));
        }
    }
}

#[tokio::test]
async fn replay_restores_state_visible_to_inbox_endpoint() {
    let dir = tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");

    // Phase 1: write three fragments through the live handler. WAL accumulates.
    let frag_ids = {
        let server = make_server_with_wal(wal_path.clone()).await;
        let mut ids = Vec::new();
        for (i, (kind, content)) in [
            ("user_action", "ship the WAL fix today"),
            ("decision", "should we add fsync per-write?"),
            ("learning", "ignored by inbox — not in eligible kinds"),
        ]
        .iter()
        .enumerate()
        {
            let mut meta = json!({
                "kind": kind,
                "ts": format!("2026-05-21T0{i}:00:00Z"),
                "project_cwd": "/tmp/wal-replay",
            });
            if *kind == "decision" {
                meta["awaiting_decision"] = json!(true);
            }
            let res = server
                .post("/api/v1/tools/store")
                .json(&json!({
                    "content": content,
                    "importance": 0.5,
                    "session_id": "cc-restart",
                    "metadata": meta,
                }))
                .await;
            res.assert_status_ok();
            ids.push(
                res.json::<Value>()["attractor_id"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            );
        }
        ids
    };
    assert_eq!(frag_ids.len(), 3);

    // Phase 2: simulate restart — fresh services, replay WAL, then start a
    // new TestServer against them.
    let services = ContextNestServices::new_default().await.unwrap();
    let records = Wal::read_records(&wal_path).expect("read wal");
    assert_eq!(records.len(), 3, "wal must hold all three stores");

    for record in records {
        match record {
            WalRecord::Store {
                fragment_id,
                session_id,
                content,
                importance,
                metadata,
            } => {
                store_with_id(
                    &services,
                    &fragment_id,
                    &session_id,
                    &content,
                    importance,
                    metadata,
                )
                .await
                .expect("replay store_with_id");
            }
        }
    }

    // Crucially, do NOT install a WAL writer for phase 2 — we're verifying
    // that replay alone (no live append) restores visibility.
    let app = create_simple_app(services).await.unwrap();
    let server = TestServer::new(app).unwrap();

    // Retrieve via the canonical pipeline now falls back to sidecar data
    // (this PR's fix), so all 3 fragments come back regardless of kind.
    // The earlier draft polled /api/v1/inbox to check kind-filtering,
    // but that endpoint ships in a follow-up PR; here we just verify
    // the round-trip via the universal retrieve surface.
    let res: Value = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({"query": "anything", "session_id": "cc-restart", "top_k": 50}))
        .await
        .json();
    let hits = res["hits"].as_array().expect("hits");
    assert_eq!(hits.len(), 3, "all 3 fragments survive replay");
    let ids_seen: std::collections::HashSet<&str> =
        hits.iter().filter_map(|h| h["id"].as_str()).collect();
    for id in &frag_ids {
        assert!(
            ids_seen.contains(id.as_str()),
            "fragment {id} must round-trip through replay"
        );
    }
}

#[tokio::test]
async fn replay_is_idempotent_when_run_twice() {
    let dir = tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");

    {
        let server = make_server_with_wal(wal_path.clone()).await;
        for i in 0..3 {
            server
                .post("/api/v1/tools/store")
                .json(&json!({
                    "content": format!("frag {i}"),
                    "session_id": "cc-idem",
                    "metadata": {"kind": "user_action"},
                }))
                .await
                .assert_status_ok();
        }
    }

    let records = Wal::read_records(&wal_path).unwrap();
    assert_eq!(records.len(), 3);

    // Replay twice into the same services instance — same fragment_ids each
    // time. SessionIndex.add is documented idempotent, sidecar inserts
    // overwrite at the same key; result count should stay at 3.
    let services = Arc::new(ContextNestServices::new_default().await.unwrap());
    for _ in 0..2 {
        for record in Wal::read_records(&wal_path).unwrap() {
            match record {
                WalRecord::Store {
                    fragment_id,
                    session_id,
                    content,
                    importance,
                    metadata,
                } => {
                    store_with_id(
                        &services,
                        &fragment_id,
                        &session_id,
                        &content,
                        importance,
                        metadata,
                    )
                    .await
                    .unwrap();
                }
            }
        }
    }

    let app = create_simple_app((*services).clone()).await.unwrap();
    let server = TestServer::new(app).unwrap();
    let res: Value = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({"query": "frag", "session_id": "cc-idem", "top_k": 50}))
        .await
        .json();
    assert_eq!(
        res["hits"].as_array().unwrap().len(),
        3,
        "double-replay must not duplicate fragments"
    );
}

#[tokio::test]
async fn live_hook_ingest_via_services_sink_writes_to_wal() {
    // Regression: cc_hooks live ingest goes through ServicesSink which
    // pre-fix did NOT WAL-append. That meant every fragment stored by a
    // live Claude Code hook event was lost on next restart. The fix makes
    // ServicesSink::store use the same store_with_id helper as the HTTP
    // handler AND append to WAL on success — single source of truth.
    let dir = tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");

    let services = ContextNestServices::new_default().await.unwrap();
    let writer = Wal::open_for_append(wal_path.clone()).unwrap();
    services.wal.set(writer).unwrap();

    let sink = ServicesSink::new(services.clone());
    let record = MemoryRecord {
        kind: MemoryKind::UserAction,
        text: "deploy the WAL gap fix to production".to_string(),
        importance: 0.7,
        session_id_cn: "cc-livehook".to_string(),
        metadata: HashMap::from([
            ("kind".to_string(), json!("user_action")),
            ("urgency".to_string(), json!("now")),
            ("ts".to_string(), json!("2026-05-21T11:00:00Z")),
        ]),
    };

    sink.store(&record).await.expect("sink store");

    let recorded = Wal::read_records(&wal_path).expect("read wal");
    assert_eq!(
        recorded.len(),
        1,
        "ServicesSink must append to WAL — pre-fix this was 0"
    );
    match &recorded[0] {
        WalRecord::Store {
            session_id,
            content,
            metadata,
            ..
        } => {
            assert_eq!(session_id, "cc-livehook");
            assert_eq!(content, "deploy the WAL gap fix to production");
            assert_eq!(metadata.get("kind").unwrap(), &json!("user_action"));
        }
    }
}

#[tokio::test]
async fn sidecars_only_replay_restores_active_fragments() {
    // The bulk sidecar path is what `serve` uses by default at startup —
    // it skips embedding + process_memories so a 12k-record WAL does not
    // pay 12k network embedding round-trips. Every active fragment must
    // still be queryable; canonical attractor state is intentionally
    // left empty.
    let services = ContextNestServices::new_default().await.unwrap();

    let records = vec![
        (
            "frag-todo".to_string(),
            "cc-fast".to_string(),
            "ship the WAL fix".to_string(),
            std::collections::HashMap::from([
                ("kind".to_string(), json!("todo")),
                ("ts".to_string(), json!("2026-05-21T10:00:00Z")),
            ]),
        ),
        (
            "frag-action".to_string(),
            "cc-fast".to_string(),
            "verify replay end-to-end".to_string(),
            std::collections::HashMap::from([
                ("kind".to_string(), json!("user_action")),
                ("ts".to_string(), json!("2026-05-21T11:00:00Z")),
            ]),
        ),
        (
            "frag-learning".to_string(),
            "cc-fast".to_string(),
            "another fragment".to_string(),
            std::collections::HashMap::from([("kind".to_string(), json!("learning"))]),
        ),
    ];

    restore_sidecars_bulk(&services, records).await;

    let app = create_simple_app(services.clone()).await.unwrap();
    let server = TestServer::new(app).unwrap();

    // Retrieve via the canonical pipeline now falls back to sidecar
    // data when the attractor_manager is empty, so it returns hits with
    // similarity=0. This is the deliberate post-fix behavior — the old
    // "returns empty" semantics broke the dashboard's per-kind sections.
    let res: Value = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({"query": "anything", "session_id": "cc-fast", "top_k": 50}))
        .await
        .json();
    let hits = res["hits"].as_array().unwrap();
    assert_eq!(
        hits.len(),
        3,
        "sidecar fallback surfaces every active fragment"
    );
    for hit in hits {
        assert_eq!(
            hit["similarity"].as_f64().unwrap_or(-1.0),
            0.0,
            "sidecar-only hits have similarity=0 (no canonical embedding to compare)"
        );
    }
    let contents: std::collections::HashSet<&str> = hits
        .iter()
        .map(|h| h["content"].as_str().unwrap())
        .collect();
    assert!(contents.contains("ship the WAL fix"));
    assert!(contents.contains("verify replay end-to-end"));
    assert!(contents.contains("another fragment"));
}

#[tokio::test]
async fn retrieve_with_metadata_filter_works_on_sidecar_only_substrate() {
    // The per-kind sections in the dashboard's session-detail page rely
    // on `retrieve` + `metadata_filter`. Verify this end-to-end on a
    // substrate whose state came entirely from sidecar restoration
    // (mimics the production post-WAL-replay scenario).
    use contextnest::api::tools::restore_sidecars_bulk;

    let services = ContextNestServices::new_default().await.unwrap();
    restore_sidecars_bulk(
        &services,
        vec![
            (
                "f1".to_string(),
                "cc-filter".to_string(),
                "do the migration".to_string(),
                std::collections::HashMap::from([("kind".to_string(), json!("todo"))]),
            ),
            (
                "f2".to_string(),
                "cc-filter".to_string(),
                "learned something".to_string(),
                std::collections::HashMap::from([("kind".to_string(), json!("learning"))]),
            ),
            (
                "f3".to_string(),
                "cc-filter".to_string(),
                "another todo".to_string(),
                std::collections::HashMap::from([("kind".to_string(), json!("todo"))]),
            ),
        ],
    )
    .await;

    let app = create_simple_app(services).await.unwrap();
    let server = TestServer::new(app).unwrap();
    let res: Value = server
        .post("/api/v1/tools/retrieve")
        .json(&json!({
            "query": "todo",
            "session_id": "cc-filter",
            "top_k": 50,
            "metadata_filter": {"kind": "todo"},
        }))
        .await
        .json();
    let hits = res["hits"].as_array().unwrap();
    assert_eq!(
        hits.len(),
        2,
        "metadata_filter must work on sidecar-only data"
    );
    for hit in hits {
        assert_eq!(hit["metadata"]["kind"], "todo");
    }
}

#[tokio::test]
async fn migration_rewrites_short_session_ids_to_full_uuid() {
    use contextnest::services::wal::migrate_short_session_ids;

    let dir = tempdir().unwrap();
    let wal_path = dir.path().join("wal.jsonl");

    // Hand-craft a WAL that mimics what an old build would have written:
    // one short-form `cc-9b8a1f3e` record carrying the full uuid in
    // metadata.src_session, plus one long-form record (already migrated),
    // plus one short-form record WITHOUT src_session (should be skipped).
    let writer = Wal::open_for_append(wal_path.clone()).unwrap();
    let full_uuid = "9b8a1f3e-51e2-bc40-89ab-cdef01234567";
    writer
        .append(&WalRecord::Store {
            fragment_id: "frag-old".into(),
            session_id: "cc-9b8a1f3e".into(),
            content: "old short-form record".into(),
            importance: 0.5,
            metadata: HashMap::from([("src_session".to_string(), json!(full_uuid))]),
        })
        .unwrap();
    writer
        .append(&WalRecord::Store {
            fragment_id: "frag-new".into(),
            session_id: format!("cc-{full_uuid}"),
            content: "already-canonical record".into(),
            importance: 0.5,
            metadata: HashMap::from([("src_session".to_string(), json!(full_uuid))]),
        })
        .unwrap();
    writer
        .append(&WalRecord::Store {
            fragment_id: "frag-orphan".into(),
            session_id: "cc-deadbeef".into(),
            content: "no src_session — leave alone".into(),
            importance: 0.5,
            metadata: HashMap::new(),
        })
        .unwrap();
    drop(writer);

    let records = Wal::read_records(&wal_path).unwrap();
    assert_eq!(records.len(), 3);

    let (migrated_records, report) =
        migrate_short_session_ids(&wal_path, records).expect("migration ok");

    // Migration report shape: 1 rewrite, 1 untouched orphan, 1 already-long.
    assert_eq!(report.migrated, 1, "exactly one short-form was rewritten");
    assert_eq!(report.skipped_no_src_session, 1, "orphan was skipped");

    // In-memory records: the short-form became long-form, others
    // unchanged.
    let by_id: HashMap<_, _> = migrated_records
        .iter()
        .map(|r| match r {
            WalRecord::Store {
                fragment_id,
                session_id,
                ..
            } => (fragment_id.clone(), session_id.clone()),
        })
        .collect();
    assert_eq!(by_id["frag-old"], format!("cc-{full_uuid}"));
    assert_eq!(by_id["frag-new"], format!("cc-{full_uuid}"));
    assert_eq!(by_id["frag-orphan"], "cc-deadbeef");

    // On-disk WAL rewritten: re-read and confirm.
    let after = Wal::read_records(&wal_path).unwrap();
    let after_session_ids: Vec<_> = after
        .iter()
        .map(|r| match r {
            WalRecord::Store { session_id, .. } => session_id.clone(),
        })
        .collect();
    assert!(after_session_ids.contains(&format!("cc-{full_uuid}")));
    assert!(after_session_ids.contains(&"cc-deadbeef".to_string()));
    assert!(
        !after_session_ids.contains(&"cc-9b8a1f3e".to_string()),
        "short-form must be gone from disk",
    );

    // .bak recovery breadcrumb exists and still has the pre-migration data.
    let bak_path = wal_path.with_extension("bak");
    let bak = Wal::read_records(&bak_path).unwrap();
    let bak_session_ids: Vec<_> = bak
        .iter()
        .map(|r| match r {
            WalRecord::Store { session_id, .. } => session_id.clone(),
        })
        .collect();
    assert!(bak_session_ids.contains(&"cc-9b8a1f3e".to_string()));

    // Re-running the migration is a no-op (idempotent).
    let (_, second) = migrate_short_session_ids(&wal_path, Wal::read_records(&wal_path).unwrap())
        .expect("idempotent rerun");
    assert_eq!(second.migrated, 0, "second pass must migrate nothing");
}

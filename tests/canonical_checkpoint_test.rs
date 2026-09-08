use contextnest::{
    ingest::claude_code::{MemoryKind, MemoryRecord, ServicesSink, Sink},
    services::{checkpoint, consolidation::drain_for_test, wal::Wal, ContextNestServices},
};

#[tokio::test]
async fn terminal_retry_budget_survives_restart_and_model_change_clears_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("canonical.sqlite");
    let service = ContextNestServices::new_default().await.unwrap();
    service
        .fragment_texts
        .write()
        .await
        .insert("bad-record".into(), String::new());
    checkpoint::bootstrap(&service, &path).await.unwrap();
    drain_for_test(&service, &service.consolidation_queue, 1).await;
    assert_eq!(
        service
            .consolidation_queue
            .snapshot_metrics()
            .terminal_failed,
        1
    );
    drop(service);
    let restored = ContextNestServices::new_default().await.unwrap();
    restored
        .fragment_texts
        .write()
        .await
        .insert("bad-record".into(), String::new());
    checkpoint::bootstrap(&restored, &path).await.unwrap();
    assert_eq!(
        restored
            .consolidation_queue
            .snapshot_metrics()
            .terminal_failed,
        1
    );
    restored.consolidation_queue.enqueue("bad-record".into());
    assert_eq!(restored.consolidation_queue.pending_count(), 0);
    drop(restored);
    let changed = checkpoint::CheckpointStore::open(&path, "new-embedding-space").unwrap();
    assert!(changed.load_retries().unwrap().is_empty());
}

#[tokio::test]
async fn duplicate_ingest_preserves_completion_and_restart_restores_graph_without_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("canonical.sqlite");
    let record = MemoryRecord::new(
        MemoryKind::Learning,
        "session-scoped durable canonical vectors".into(),
        "session-A".into(),
    );
    let service = ContextNestServices::new_default().await.unwrap();
    service
        .wal
        .set(Wal::open_for_append(dir.path().join("wal.jsonl")).unwrap())
        .unwrap();
    checkpoint::bootstrap(&service, &path).await.unwrap();
    let sink = ServicesSink::new(service.clone());
    sink.store(&record).await.unwrap();
    drain_for_test(&service, &service.consolidation_queue, 1).await;
    let ids = service.attractor_manager.list_fragment_ids().await;
    assert_eq!(ids.len(), 1);
    sink.store(&record).await.unwrap();
    assert_eq!(service.consolidation_queue.snapshot_metrics().queued, 0);
    let texts = service.fragment_texts.read().await.clone();
    let metadata = service.fragment_metadata.read().await.clone();
    let before = service.attractor_manager.durable_snapshot(None, None).await;
    assert_eq!(before.nodes.len(), 1);
    assert_eq!(before.basins.len(), 1);
    drop(sink);
    drop(service);
    let restored = ContextNestServices::new_default().await.unwrap();
    // Simulates input-only WAL replay, which carries no trusted completion flag.
    let rows = texts
        .into_iter()
        .map(|(id, text)| {
            let mut meta = metadata[&id].clone();
            meta.retain(|key, _| !key.starts_with("_cn_"));
            (id, "session-A".into(), text, meta)
        })
        .collect();
    contextnest::api::tools::restore_sidecars_bulk(&restored, rows).await;
    checkpoint::bootstrap(&restored, &path).await.unwrap();
    drain_for_test(&restored, &restored.consolidation_queue, 1).await;
    assert_eq!(
        restored.consolidation_queue.snapshot_metrics().consolidated,
        0
    );
    let after = restored
        .attractor_manager
        .durable_snapshot(None, None)
        .await;
    assert_eq!(before.fragments[0].content, after.fragments[0].content);
    assert_eq!(before.basins[0].id, after.basins[0].id);
    assert_eq!(before.nodes[0].id, after.nodes[0].id);
}

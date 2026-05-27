//! Sinks for [`MemoryRecord`]s — push to the substrate or preview locally.
//!
//! Two impls land in this PR:
//!
//! - [`HttpSink`] — POSTs each record to a running substrate's
//!   `/api/v1/tools/store` endpoint. Bounded concurrency keeps the
//!   substrate from queueing on big sessions (50 MB+ transcripts) while
//!   still finishing in seconds.
//! - [`DryRunSink`] — prints a one-line summary per record to stdout.
//!   No network. The default for `--dry-run` ingestion + the workhorse
//!   for integration tests that don't want to spin up a substrate.
//!
//! A custom sink is two methods of the [`Sink`] trait — see the
//! `MemoryRecord` documentation in `extractor.rs` for the shape it
//! receives.

use crate::error::{ContextNestError, ContextNestResult};
use crate::services::fragment_id::stable_fragment_id;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::extractor::MemoryRecord;

/// Shared interface for anything that consumes [`MemoryRecord`]s.
#[async_trait]
pub trait Sink: Send + Sync {
    /// Push a single record. Implementations decide whether this is
    /// synchronous (DryRun) or fan-out concurrent (Http).
    async fn store(&self, record: &MemoryRecord) -> ContextNestResult<()>;

    /// Optional batch hook — implementations can override for native
    /// batching. Default impl just calls [`Self::store`] in order.
    async fn store_batch(&self, records: &[MemoryRecord]) -> ContextNestResult<SinkReport> {
        let mut report = SinkReport::default();
        for record in records {
            match self.store(record).await {
                Ok(()) => report.success += 1,
                Err(e) => {
                    report.failed += 1;
                    report.first_error.get_or_insert(e.to_string());
                }
            }
            report.bump_kind(record.kind.as_str());
        }
        Ok(report)
    }
}

/// Summary returned after a batch push. Aggregates success/fail counts
/// and per-kind tallies so callers can render a clean table.
#[derive(Debug, Default, Clone)]
pub struct SinkReport {
    pub success: usize,
    pub failed: usize,
    pub by_kind: HashMap<String, usize>,
    pub first_error: Option<String>,
}

impl SinkReport {
    pub fn bump_kind(&mut self, kind: &str) {
        *self.by_kind.entry(kind.to_string()).or_insert(0) += 1;
    }

    pub fn total(&self) -> usize {
        self.success + self.failed
    }
}

// ---------------------------------------------------------------------------
// HttpSink
// ---------------------------------------------------------------------------

/// POSTs every record to a substrate's `/api/v1/tools/store` endpoint.
///
/// Construct with [`Self::new`]; pass the substrate's base URL (e.g.
/// `http://localhost:8080`). The base URL is trimmed of any trailing
/// slash so callers can pass either form.
pub struct HttpSink {
    base_url: String,
    client: Client,
    /// Best-effort counter exposed via [`Self::sent_count`] for tests
    /// and logging. Cheap atomic, not used for control flow.
    sent: Arc<AtomicUsize>,
}

#[derive(Serialize)]
struct StoreRequestPayload<'a> {
    content: &'a str,
    importance: f32,
    session_id: &'a str,
    metadata: &'a HashMap<String, Value>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct StoreResponsePayload {
    attractor_id: Option<String>,
    stored: bool,
}

impl HttpSink {
    pub fn new(base_url: impl Into<String>) -> Self {
        let url = base_url.into();
        let base_url = url.trim_end_matches('/').to_string();
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("reqwest Client::new should never fail with default config");
        Self {
            base_url,
            client,
            sent: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Number of records we've successfully POSTed since construction.
    /// Cheap atomic load; safe to call concurrently.
    pub fn sent_count(&self) -> usize {
        self.sent.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl Sink for HttpSink {
    async fn store(&self, record: &MemoryRecord) -> ContextNestResult<()> {
        let url = format!("{}/api/v1/tools/store", self.base_url);
        let payload = StoreRequestPayload {
            content: &record.text,
            importance: record.importance,
            session_id: &record.session_id_cn,
            metadata: &record.metadata,
        };

        let resp = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                ContextNestError::Api(format!("HttpSink: store request to {} failed: {}", url, e))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ContextNestError::Api(format!(
                "HttpSink: /tools/store returned {}: {}",
                status, body
            )));
        }

        // Don't bother deserializing for now — just verify the response is
        // valid JSON with `stored: true` so we catch substrate-side rejects.
        let parsed: StoreResponsePayload = resp.json().await.map_err(|e| {
            ContextNestError::Api(format!(
                "HttpSink: could not parse /tools/store response: {}",
                e
            ))
        })?;
        if !parsed.stored {
            return Err(ContextNestError::Api(
                "HttpSink: substrate reported `stored: false`".to_string(),
            ));
        }

        self.sent.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DryRunSink
// ---------------------------------------------------------------------------

/// Prints a one-line summary per record to stdout and records counts in
/// an internal [`SinkReport`]-shaped tally. Never touches the network —
/// safe to run in CI, on a plane, or with no substrate running.
///
/// Used both by `contextnest ingest claude-code --dry-run` and by the
/// integration tests, which prefer an in-process sink over the
/// HTTP-based one.
pub struct DryRunSink {
    /// In-memory record of everything `store()` saw. Tests assert
    /// against this; CLI dry-run just reports the count.
    pub captured: tokio::sync::RwLock<Vec<MemoryRecord>>,
    /// When true, each `store` call also prints to stdout. The
    /// integration test sets this to false to keep test output clean.
    print_to_stdout: bool,
}

impl Default for DryRunSink {
    fn default() -> Self {
        Self::new()
    }
}

impl DryRunSink {
    pub fn new() -> Self {
        Self {
            captured: tokio::sync::RwLock::new(Vec::new()),
            print_to_stdout: true,
        }
    }

    pub fn silent() -> Self {
        Self {
            captured: tokio::sync::RwLock::new(Vec::new()),
            print_to_stdout: false,
        }
    }

    pub async fn captured_count(&self) -> usize {
        self.captured.read().await.len()
    }

    pub async fn captured_by_kind(&self) -> HashMap<String, usize> {
        let lock = self.captured.read().await;
        let mut by_kind: HashMap<String, usize> = HashMap::new();
        for r in lock.iter() {
            *by_kind.entry(r.kind.as_str().to_string()).or_insert(0) += 1;
        }
        by_kind
    }
}

#[async_trait]
impl Sink for DryRunSink {
    async fn store(&self, record: &MemoryRecord) -> ContextNestResult<()> {
        if self.print_to_stdout {
            // One line per record — kind, session, truncated text.
            // Truncate the text at 80 chars so the inbox-style preview
            // stays scannable in a terminal.
            let preview: String = record.text.chars().take(80).collect();
            let truncated = if record.text.len() > 80 { "…" } else { "" };
            println!(
                "[dry-run] {:<22} {} :: {}{}",
                record.kind.as_str(),
                record.session_id_cn,
                preview.replace('\n', " "),
                truncated,
            );
        }
        self.captured.write().await.push(record.clone());
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ServicesSink — in-process sink for the real-time hook receiver
// ---------------------------------------------------------------------------

/// Writes records directly into a [`ContextNestServices`] container in the
/// same process. Skips the HTTP round-trip that [`HttpSink`] uses and is
/// the right choice when the caller already has a handle on the services
/// (e.g. the hook receiver living inside the running substrate).
///
/// Semantics match the `/api/v1/tools/store` HTTP handler in
/// `src/api/tools.rs`:
/// 1. Generate an embedding for `record.text`
/// 2. Build a [`MemoryFragment`], submit via `process_memories`
/// 3. Insert text into `fragment_texts`, metadata into `fragment_metadata`
/// 4. Register session affinity via `session_index`
///
/// Any failure at any stage flips the record into the `failed` count of
/// the resulting [`SinkReport`] — never panics, never blocks the caller
/// past the storage timeout already enforced by `process_memories`.
pub struct ServicesSink {
    services: crate::services::ContextNestServices,
}

impl ServicesSink {
    pub fn new(services: crate::services::ContextNestServices) -> Self {
        Self { services }
    }
}

#[async_trait]
impl Sink for ServicesSink {
    /// Live-ingest write path used by cc_hooks. Optimized for **throughput
    /// under bursty load** (every Claude Code prompt fires this once
    /// per extracted memory record).
    ///
    /// Design choices, in order of importance:
    ///
    /// 1. **Skip `process_memories` + LLM round-trip.** Each Claude Code
    ///    transcript chunk can produce dozens of records. Running the
    ///    canonical basin-formation pipeline (which calls the LLM per
    ///    fragment when enabled) blocks for several seconds per record,
    ///    causing the spawned hook tasks to pile up faster than they
    ///    drain. The inbox endpoint reads only the three sidecars, so
    ///    live ingest only needs to write those; canonical attractor
    ///    state is rebuilt by the in-process tests and future
    ///    on-demand re-embedding flows.
    /// 2. **WAL-append on success.** Without this, live fragments don't
    ///    survive restart even though they're queryable in the current
    ///    process. The HTTP store handler already does this; we mirror
    ///    its behavior here so all three write paths (HTTP, hook,
    ///    replay) produce identical disk state.
    /// 3. **Best-effort WAL.** A WAL outage logs at warn-level but does
    ///    not fail the in-memory store — Claude Code's hook protocol
    ///    has no way to act on a failed ingest, so degrading to "stored
    ///    in RAM only" is strictly better than dropping the fragment.
    ///
    /// This is the same trade-off documented on
    /// [`crate::api::tools::restore_sidecars_bulk`] — see that doc for
    /// the retrieve-returns-empty-for-replayed-fragments caveat.
    async fn store(&self, record: &MemoryRecord) -> ContextNestResult<()> {
        let fragment_id = stable_fragment_id(&record.session_id_cn, &record.text, &record.metadata);

        // Sidecar inserts only. No embedding, no process_memories, no LLM.
        self.services
            .fragment_texts
            .write()
            .await
            .insert(fragment_id.clone(), record.text.clone());

        if !record.metadata.is_empty() {
            self.services
                .fragment_metadata
                .write()
                .await
                .insert(fragment_id.clone(), record.metadata.clone());
        }

        self.services
            .session_index
            .add(&record.session_id_cn, &fragment_id)
            .await;

        // Enqueue for background consolidation (Phase 1 of the
        // neural-field epic). Fragment is now visible to the inbox /
        // retrieve sidecar path; the worker catches up off the hot
        // path so basin formation + connection-network nodes
        // eventually exist without blocking ingest. Cheap
        // sync HashSet insert — never blocks. See
        // `src/services/consolidation.rs` for the worker design.
        self.services
            .consolidation_queue
            .enqueue(fragment_id.clone());

        // Best-effort WAL append. Failures log + continue.
        if let Some(wal) = self.services.wal.get() {
            let wal_record = crate::services::wal::WalRecord::Store {
                fragment_id,
                session_id: record.session_id_cn.clone(),
                content: record.text.clone(),
                importance: record.importance,
                metadata: record.metadata.clone(),
            };
            if let Err(e) = wal.append(&wal_record) {
                tracing::warn!(
                    error = %e,
                    kind = %record.kind.as_str(),
                    "wal: append failed for ServicesSink::store",
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::extractor::{MemoryKind, MemoryRecord};
    use super::*;

    fn rec(kind: MemoryKind, text: &str) -> MemoryRecord {
        MemoryRecord {
            kind,
            text: text.to_string(),
            importance: kind.default_importance(),
            session_id_cn: "test123".to_string(),
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn dry_run_captures_records_silently() {
        let sink = DryRunSink::silent();
        sink.store(&rec(MemoryKind::Learning, "x")).await.unwrap();
        sink.store(&rec(MemoryKind::Decision, "y")).await.unwrap();
        assert_eq!(sink.captured_count().await, 2);
        let by_kind = sink.captured_by_kind().await;
        assert_eq!(by_kind.get("learning"), Some(&1));
        assert_eq!(by_kind.get("decision"), Some(&1));
    }

    #[tokio::test]
    async fn dry_run_batch_returns_accurate_report() {
        let sink = DryRunSink::silent();
        let records = vec![
            rec(MemoryKind::GoalPhase, "g1"),
            rec(MemoryKind::Accomplishment, "a1"),
            rec(MemoryKind::Accomplishment, "a2"),
        ];
        let report = sink.store_batch(&records).await.unwrap();
        assert_eq!(report.success, 3);
        assert_eq!(report.failed, 0);
        assert_eq!(report.by_kind.get("accomplishment"), Some(&2));
        assert_eq!(report.by_kind.get("goal_phase"), Some(&1));
    }

    #[test]
    fn http_sink_trims_trailing_slash() {
        let s = HttpSink::new("http://localhost:8080/");
        assert_eq!(s.base_url, "http://localhost:8080");
        assert_eq!(s.sent_count(), 0);
    }
}

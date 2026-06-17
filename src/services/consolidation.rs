//! Background consolidation worker — Phase 1 of the neural-field epic
//! (`docs/roadmap/epics/neural-field-real.md`).
//!
//! ## Why this exists
//!
//! The cc_hooks ingest path ([`crate::ingest::claude_code::ServicesSink`])
//! and the WAL-replay path ([`crate::api::tools::restore_sidecars_bulk`])
//! both deliberately skip [`crate::memory::attractors::MemoryAttractorManager::process_memories`]
//! because each call costs an embedding round-trip (~250 ms on
//! DeepInfra) plus basin / connection / reconstruction work. For 25k
//! fragments that's hours of latency in the ingest hot path.
//!
//! The result is correct ingest (sidecars hydrated, inbox works,
//! retrieve sees fragments) but a **dormant attractor pipeline**:
//! basins never form, the connection network has zero nodes, the
//! reconstruction store is empty. The README's "neural-field substrate"
//! tagline doesn't survive a `grep` of the runtime.
//!
//! This module fixes the gap without re-introducing the latency. It
//! runs in the background, lazily processing fragments through the
//! attractor pipeline at the embedder's natural pace. Live ingest stays
//! fast; the substrate fills in behind it.
//!
//! ## Architecture
//!
//! - A [`ConsolidationQueue`] dedup'd via a `HashSet` lives inside
//!   [`crate::services::ContextNestServices`]. Every code path that
//!   creates a sidecar-only fragment enqueues its id.
//! - One worker task spawned at server startup (see
//!   `src/bin/contextnest.rs`) ticks every `interval_ms`, drains up to
//!   `batch_size` ids, and processes them through
//!   `process_memories` with conservative `ProcessingOptions` (one
//!   fragment per request keeps the O(N²) Step 3 disabled, but
//!   Step 1 + Step 1.5 still run — basin + connection-network node
//!   formation).
//! - Persistence: a fragment is "consolidated" when its sidecar
//!   metadata contains `_cn_consolidated == true`. This survives
//!   restart naturally — no separate watermark file. On startup the
//!   worker scans `fragment_metadata` once and enqueues every id
//!   that's missing the flag.
//! - Concurrency: `buffer_unordered(config.concurrency)` caps in-flight
//!   embedding calls to avoid hammering the network when the
//!   embedder is remote.
//!
//! ## What's intentionally NOT here
//!
//! - **No re-consolidation.** Once flagged, a fragment is left alone
//!   even if its embedding drifts (e.g. the embedder model changes).
//!   That's a separate epic — re-embed-on-model-change.
//! - **No backpressure to ingest.** Live ingest never blocks on
//!   consolidation. The queue is unbounded; in pathological cases the
//!   only signal is the lag visible at `/api/v1/substrate/consolidation`.
//! - **No retry on failure.** A failed `process_memories` call logs
//!   warn-level and the id stays unflagged, so the next startup scan
//!   re-enqueues it.

use crate::memory::attractors::memory_attractor_manager::{
    MemoryProcessingRequest, ProcessingOptions, ProcessingPriority,
};
use crate::memory::attractors::MemoryFragment;
use crate::services::ContextNestServices;
use chrono::Utc;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CONSOLIDATED_FLAG: &str = "_cn_consolidated";
const CONSOLIDATED_AT_FIELD: &str = "_cn_consolidated_at";
/// Per-fragment content_density scalar (0-1), computed at consolidation
/// time and read by `retrieve` to multiply into the final score. See
/// `services::content_density` for the formula. Storing in metadata
/// keeps `retrieve` lock-free for the scoring pass (no per-hit text
/// re-tokenization). Backwards-compatible: legacy fragments without
/// this field fall back to neutral (1.0) at retrieve time.
const CONTENT_DENSITY_FIELD: &str = "_cn_content_density";

/// Tunable knobs read from environment at worker startup. Each one has
/// a sensible default so out-of-the-box behavior is "consolidate
/// continuously in the background, 4-way parallel, 32 fragments per
/// batch."
#[derive(Debug, Clone)]
pub struct ConsolidationConfig {
    /// Idle tick interval when the queue is empty. The worker doesn't
    /// poll continuously — it sleeps this long between drain attempts.
    /// Also the *base* delay for the rate-limit backoff schedule (the
    /// loop sleeps `interval_ms << consecutive_rate_limit_batches`,
    /// capped at `max_backoff_ms`).
    pub interval_ms: u64,
    /// Max in-flight `process_memories` calls under normal conditions.
    /// Caps embedder concurrency to avoid rate-limit spikes. The worker
    /// MAY drop below this when the embedder is returning rate-limit
    /// errors — see `backoff_concurrency_floor`.
    pub concurrency: usize,
    /// Max ids drained per tick. Bounds memory pressure when a backlog
    /// of 25k+ ids is waiting after a WAL replay.
    pub batch_size: usize,
    /// Master kill switch. Worker exits its loop on next tick when
    /// false. Useful in tests and for ops who want to pause
    /// consolidation under load.
    pub enabled: bool,
    /// Upper bound on the exponential backoff delay after consecutive
    /// rate-limited batches. The schedule is
    /// `min(interval_ms * 2^k, max_backoff_ms)` where k is the number
    /// of consecutive batches that returned at least one
    /// `engine_overloaded` / `429` / "Model busy" error. 30s default.
    pub max_backoff_ms: u64,
    /// Floor on concurrency while backoff is active. Default 1 — when
    /// the embedder is asking us to slow down, fall back to serial
    /// requests. Set higher only if your provider's "rate limit"
    /// returns are spurious (rare).
    pub backoff_concurrency_floor: usize,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            interval_ms: 500,
            concurrency: 4,
            batch_size: 32,
            enabled: true,
            max_backoff_ms: 30_000,
            backoff_concurrency_floor: 1,
        }
    }
}

impl ConsolidationConfig {
    /// Resolve every knob from `CONTEXTNEST_CONSOLIDATION_*` env vars,
    /// falling back to the [`Default`] when a var is absent / unparseable.
    pub fn from_env() -> Self {
        let d = Self::default();
        Self {
            interval_ms: std::env::var("CONTEXTNEST_CONSOLIDATION_INTERVAL_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(d.interval_ms),
            concurrency: std::env::var("CONTEXTNEST_CONSOLIDATION_CONCURRENCY")
                .ok()
                .and_then(|s| s.parse().ok())
                .filter(|n: &usize| *n > 0)
                .unwrap_or(d.concurrency),
            batch_size: std::env::var("CONTEXTNEST_CONSOLIDATION_BATCH_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .filter(|n: &usize| *n > 0)
                .unwrap_or(d.batch_size),
            enabled: std::env::var("CONTEXTNEST_CONSOLIDATION_ENABLED")
                .ok()
                .map(|s| s != "false" && s != "0")
                .unwrap_or(d.enabled),
            max_backoff_ms: std::env::var("CONTEXTNEST_CONSOLIDATION_MAX_BACKOFF_MS")
                .ok()
                .and_then(|s| s.parse().ok())
                .filter(|n: &u64| *n >= 100)
                .unwrap_or(d.max_backoff_ms),
            backoff_concurrency_floor: std::env::var(
                "CONTEXTNEST_CONSOLIDATION_BACKOFF_CONCURRENCY_FLOOR",
            )
            .ok()
            .and_then(|s| s.parse().ok())
            .filter(|n: &usize| *n > 0)
            .unwrap_or(d.backoff_concurrency_floor),
        }
    }
}

/// Snapshot returned by [`ConsolidationQueue::snapshot_metrics`] and
/// surfaced at `GET /api/v1/substrate/consolidation`. All counters are
/// monotonic across the worker's lifetime (no resets) so callers can
/// compute rates by differencing two snapshots.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ConsolidationMetrics {
    /// Fragments currently waiting in the queue (post-dedup).
    pub queued: usize,
    /// Cumulative fragments successfully consolidated since startup.
    pub consolidated: usize,
    /// Cumulative process_memories failures since startup.
    pub failed: usize,
    /// Wall-clock duration of the most recent non-empty batch, in ms.
    pub last_lap_ms: u64,
    /// Whether the worker has completed its initial sidecar scan. Tests
    /// poll this to know when "everything that existed at startup has
    /// been queued" is true.
    pub initial_scan_complete: bool,
}

/// Dedup'd queue of fragment ids waiting for the consolidation worker.
/// Lives behind an [`Arc`] inside [`ContextNestServices`] so every
/// ingest path can enqueue without taking an async lock — the
/// underlying [`Mutex`] is `std::sync` (never held across `.await`).
pub struct ConsolidationQueue {
    pending: Mutex<HashSet<String>>,
    metrics: Mutex<ConsolidationMetrics>,
}

impl Default for ConsolidationQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl ConsolidationQueue {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashSet::new()),
            metrics: Mutex::new(ConsolidationMetrics::default()),
        }
    }

    /// Enqueue a fragment id for background consolidation. Idempotent —
    /// duplicate ids collapse to one entry. Cheap (single sync
    /// `HashSet::insert`) so callers can fire-and-forget from ingest
    /// hot paths.
    pub fn enqueue(&self, id: String) {
        let mut p = self.pending.lock().expect("consolidation queue poisoned");
        p.insert(id);
        // Keep the queued counter consistent with pending size so the
        // metrics endpoint doesn't lag a full tick.
        let mut m = self.metrics.lock().expect("metrics lock poisoned");
        m.queued = p.len();
    }

    /// Drain up to `max` ids from the queue. Returns immediately when
    /// the queue is empty so the worker can fall through to its sleep.
    pub fn drain_batch(&self, max: usize) -> Vec<String> {
        let mut p = self.pending.lock().expect("consolidation queue poisoned");
        let take: Vec<String> = p.iter().take(max).cloned().collect();
        for id in &take {
            p.remove(id);
        }
        // Update queued counter under the same lock to avoid a
        // race-window where two callers see different sizes.
        let mut m = self.metrics.lock().expect("metrics lock poisoned");
        m.queued = p.len();
        take
    }

    pub fn snapshot_metrics(&self) -> ConsolidationMetrics {
        self.metrics.lock().expect("metrics lock poisoned").clone()
    }

    pub fn pending_count(&self) -> usize {
        self.pending
            .lock()
            .expect("consolidation queue poisoned")
            .len()
    }
}

/// True when a fragment's metadata sidecar marks it as already
/// consolidated. The flag is opaque to callers — they should never
/// `metadata_filter` on it (it's an internal control flag).
fn is_consolidated(meta: &HashMap<String, Value>) -> bool {
    meta.get(CONSOLIDATED_FLAG)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Walks the metadata sidecar once and enqueues every fragment id
/// that's missing the consolidation flag. Runs only at worker startup
/// to seed the queue with anything carried over from WAL replay or a
/// previous crash. Also enqueues fragments that have `text` but no
/// metadata at all (since the absence of metadata means the flag
/// can't be there either).
async fn initial_scan(services: &ContextNestServices, queue: &ConsolidationQueue) {
    // Snapshot every fragment id we know about via the union of
    // metadata + texts sidecars. Either alone misses fragments stored
    // without metadata (sidecar fast path) or restored from WAL with
    // empty metadata.
    let metadata = services.fragment_metadata.read().await;
    let texts = services.fragment_texts.read().await;
    let mut candidates: HashSet<&String> = metadata.keys().collect();
    candidates.extend(texts.keys());

    let mut enqueued = 0usize;
    for id in candidates {
        let meta = metadata.get(id);
        let already = meta.map(is_consolidated).unwrap_or(false);
        if !already {
            queue.enqueue(id.clone());
            enqueued += 1;
        }
    }
    drop(metadata);
    drop(texts);

    {
        let mut m = queue.metrics.lock().expect("metrics lock poisoned");
        m.initial_scan_complete = true;
    }
    if enqueued > 0 {
        tracing::info!(
            enqueued,
            queued = queue.pending_count(),
            "consolidation: initial scan enqueued unconsolidated fragments"
        );
    } else {
        tracing::debug!("consolidation: initial scan found no work");
    }
}

/// Outcome of attempting to consolidate one fragment. Distinct from
/// `Result<(), _>` because "skipped because already flagged" is not a
/// failure and should not bump the success counter — duplicate
/// enqueues would otherwise inflate `consolidated_total` past the
/// number of fragments that actually have basins.
#[derive(Debug, PartialEq, Eq)]
enum ConsolidationOutcome {
    /// First time through the pipeline — basin + connection-network
    /// node were just created.
    Done,
    /// Already flagged consolidated; the pipeline did not run.
    Skipped,
}

/// Process one fragment through the attractor pipeline. Returns the
/// outcome so the caller (`process_batch`) can decide whether to bump
/// the success counter.
///
/// Skips fragments whose text is missing (sidecar-orphaned ids — index
/// drift) and fragments already flagged consolidated (race-protection
/// in case `initial_scan` and a live `enqueue` raced).
async fn consolidate_one(
    services: &ContextNestServices,
    id: &str,
) -> Result<ConsolidationOutcome, String> {
    let text = {
        let texts = services.fragment_texts.read().await;
        match texts.get(id) {
            Some(t) if !t.is_empty() => t.clone(),
            _ => return Err("no text sidecar".to_string()),
        }
    };

    // Read existing metadata so we can preserve everything except the
    // consolidation flag we're about to write. Also lets us short-circuit
    // if a concurrent path already consolidated this id.
    let existing_meta: HashMap<String, Value> = {
        let metadata = services.fragment_metadata.read().await;
        metadata.get(id).cloned().unwrap_or_default()
    };
    if is_consolidated(&existing_meta) {
        return Ok(ConsolidationOutcome::Skipped);
    }

    let importance = existing_meta
        .get("importance")
        .and_then(|v| v.as_f64())
        .map(|f| f as f32)
        .unwrap_or(0.5);

    // Reuse the cache so we don't re-embed the same text twice across
    // a worker tick — populates on first read, cheap thereafter.
    let embedding = {
        let cache = services.embeddings_by_id.read().await;
        cache.get(id).cloned()
    };
    let embedding = match embedding {
        Some(e) => e,
        None => {
            let emb = services
                .embedding
                .generate_embedding(&text)
                .await
                .map_err(|e| format!("embed: {e}"))?;
            // Best-effort cache write — failure here just means the
            // next consolidation pass re-embeds.
            services
                .embeddings_by_id
                .write()
                .await
                .insert(id.to_string(), emb.clone());
            emb
        }
    };

    let now = Utc::now();
    let fragment = MemoryFragment {
        id: id.to_string(),
        content: embedding,
        importance,
        created_at: now,
        last_accessed: now,
        attractor_basin_id: None,
        connections: Default::default(),
        confidence: importance,
    };

    // Single-fragment request → Step 3 (O(N²) pairwise connection
    // creation, guarded by `request.fragments.len() > 1`) is a no-op
    // for us, but Step 1 (basin formation) and Step 1.5 (connection
    // network node + implicit similarity-driven auto-connection inside
    // `add_node`) still run. Reconstruction is disabled to keep the
    // per-fragment cost bounded — Phase 6 of the epic enables it on
    // demand at retrieve time.
    let req = MemoryProcessingRequest {
        id: format!("consolidate_{id}"),
        fragments: vec![fragment],
        options: ProcessingOptions {
            enable_attractor_creation: true,
            enable_reconstruction: false,
            enable_gap_filling: false,
            enable_connections: true,
            quality_threshold: 0.0,
            max_processing_time: Duration::from_secs(30),
        },
        priority: ProcessingPriority::Low,
        created_at: now,
    };

    services
        .attractor_manager
        .process_memories(req)
        .await
        .map_err(|e| format!("process_memories: {e}"))?;

    // Compute content density from the raw text once, before flipping
    // the consolidated flag. Cheap (microseconds, pure function), but
    // we still do it inside the same critical section as the flag
    // write so a concurrent retrieve never sees the consolidated flag
    // without the density paired alongside it.
    let density = crate::services::content_density::content_density(&text);

    // Flip the flag on success. Preserves any pre-existing metadata
    // (kind, ts, src_session, project_cwd, etc.) by updating the
    // entry in place rather than replacing it.
    {
        let mut meta_w = services.fragment_metadata.write().await;
        let entry = meta_w.entry(id.to_string()).or_default();
        entry.insert(CONSOLIDATED_FLAG.to_string(), Value::Bool(true));
        entry.insert(
            CONSOLIDATED_AT_FIELD.to_string(),
            Value::String(now.to_rfc3339()),
        );
        entry.insert(
            CONTENT_DENSITY_FIELD.to_string(),
            Value::from(density as f64),
        );
    }

    Ok(ConsolidationOutcome::Done)
}

/// Per-batch outcome buckets that drive both the metrics counters and
/// the worker's backoff decision. `RateLimited` is broken out from
/// `Failed` because the right reaction is opposite: a generic failure
/// should NOT slow the worker (probably a per-fragment data issue), but
/// a rate-limit failure means the embedder is asking us to back off —
/// continuing at full speed wastes CPU on retries that will keep
/// failing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchOutcome {
    pub done: usize,
    pub skipped: usize,
    /// Generic failures (per-fragment data issues, parse errors, etc.).
    pub failed: usize,
    /// Subset of failures that look like embedder rate-limit / overload
    /// signals (OpenAI `engine_overloaded`, HTTP 429, "Model busy",
    /// "rate limit", "rate_limit_exceeded"). Detected by substring
    /// match against the error string bubbled up from `consolidate_one`.
    /// Any non-zero value here trips the worker's exponential backoff.
    pub rate_limited: usize,
}

/// Substring-detect rate-limit error patterns in an embedder error
/// string. Patterns are case-insensitive and cover the common shapes
/// emitted by OpenAI-compatible providers (DeepInfra, Z.AI, OpenAI
/// proper, etc.) plus generic HTTP 429.
///
/// This is intentionally a string match, not a typed error: the
/// embedder layer collapses provider responses into `ContextNestError`
/// which loses the original error code shape. A typed wrapper would
/// require touching every provider integration; the substring match
/// matches what already shows up in logs without that surgery.
pub(crate) fn looks_rate_limited(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    // Substring needles. `429` is matched as a standalone digit run via
    // regex-free heuristic (preceded by non-digit, followed by non-digit
    // or end-of-string) so we don't false-positive on e.g. "fragment id
    // 1429ab".
    const NEEDLES: &[&str] = &[
        "engine_overloaded",
        "model busy",
        "rate limit",
        "rate_limit",
        "too many requests",
    ];
    if NEEDLES.iter().any(|needle| lower.contains(needle)) {
        return true;
    }
    // 429 boundary check: scan for "429" preceded by non-digit (or
    // start) and followed by non-digit (or end). Catches "HTTP 429",
    // "status: 429", "code\":429", " 429,", but rejects "12429" or
    // "4290" or "fragment-id-429-extra-7".
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        if &bytes[i..i + 3] == b"429" {
            let prev_ok = i == 0 || !bytes[i - 1].is_ascii_digit();
            let next_ok = i + 3 == bytes.len() || !bytes[i + 3].is_ascii_digit();
            if prev_ok && next_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Drive one batch through `consolidate_one` with bounded concurrency.
/// Returns a [`BatchOutcome`] so the caller can fold it into metrics
/// AND decide whether to back off on the next tick.
async fn process_batch(
    services: &ContextNestServices,
    batch: Vec<String>,
    concurrency: usize,
) -> BatchOutcome {
    use futures::stream::{self, StreamExt};

    #[derive(Clone)]
    enum Tag {
        Done,
        Skipped,
        Failed,
        RateLimited,
    }

    let results: Vec<Tag> = stream::iter(batch.into_iter().map(|id| {
        let services = services.clone();
        async move {
            match consolidate_one(&services, &id).await {
                Ok(ConsolidationOutcome::Done) => Tag::Done,
                Ok(ConsolidationOutcome::Skipped) => Tag::Skipped,
                Err(e) => {
                    if looks_rate_limited(&e) {
                        tracing::warn!(fragment_id = %id, error = %e, "consolidation: process rate-limited");
                        Tag::RateLimited
                    } else {
                        tracing::warn!(fragment_id = %id, error = %e, "consolidation: process failed");
                        Tag::Failed
                    }
                }
            }
        }
    }))
    .buffer_unordered(concurrency)
    .collect()
    .await;

    BatchOutcome {
        done: results.iter().filter(|t| matches!(t, Tag::Done)).count(),
        skipped: results.iter().filter(|t| matches!(t, Tag::Skipped)).count(),
        failed: results.iter().filter(|t| matches!(t, Tag::Failed)).count(),
        rate_limited: results
            .iter()
            .filter(|t| matches!(t, Tag::RateLimited))
            .count(),
    }
}

/// Run the worker loop forever. Spawned once per server lifetime from
/// `src/bin/contextnest.rs`. Holds clones (Arc internally) of the
/// services + queue + config — cheap to share, no special shutdown
/// signaling needed because the process exit tears down the tokio
/// runtime.
pub async fn run_worker(
    services: ContextNestServices,
    queue: Arc<ConsolidationQueue>,
    config: ConsolidationConfig,
) {
    if !config.enabled {
        tracing::info!("consolidation: worker disabled by config");
        return;
    }
    tracing::info!(
        interval_ms = config.interval_ms,
        concurrency = config.concurrency,
        batch_size = config.batch_size,
        "consolidation: worker starting"
    );

    initial_scan(&services, &queue).await;

    let base_interval = Duration::from_millis(config.interval_ms);
    let max_backoff = Duration::from_millis(config.max_backoff_ms);
    // Exponential backoff state. Each consecutive rate-limited batch
    // bumps this by 1; a clean batch resets it to 0. Sleep duration is
    // `base_interval << consecutive_rl_batches`, capped at max_backoff.
    let mut consecutive_rl_batches: u32 = 0;

    loop {
        let batch = queue.drain_batch(config.batch_size);
        if batch.is_empty() {
            // Idle: use base interval. Don't carry forward backoff
            // state across an idle period — by the time work returns,
            // the embedder's rate limit window has likely reset.
            consecutive_rl_batches = 0;
            tokio::time::sleep(base_interval).await;
            continue;
        }

        // Adaptive concurrency: under backoff, drop to floor (default
        // 1) so we're not spinning up parallel requests the embedder is
        // already refusing. Linear ramp down then snap-back keeps the
        // logic simple — production-grade AIMD can come later if the
        // simple form proves insufficient.
        let effective_concurrency = if consecutive_rl_batches > 0 {
            config.backoff_concurrency_floor
        } else {
            config.concurrency
        };

        let start = Instant::now();
        let outcome = process_batch(&services, batch, effective_concurrency).await;
        let lap_ms = start.elapsed().as_millis() as u64;

        {
            let mut m = queue.metrics.lock().expect("metrics lock poisoned");
            m.consolidated += outcome.done;
            // Rate-limited counts as "failed" in the public metric — it
            // surfaces in /substrate/consolidation so ops can see the
            // pressure, plus the per-batch warn-level logs above
            // distinguish the two kinds.
            m.failed += outcome.failed + outcome.rate_limited;
            m.last_lap_ms = lap_ms;
        }

        // Update backoff state BEFORE the post-batch sleep so the sleep
        // duration reflects the most recent batch's outcome.
        if outcome.rate_limited > 0 {
            consecutive_rl_batches = consecutive_rl_batches.saturating_add(1);
            // 2^N can grow fast; cap shift to 16 (= 65536x base) to
            // avoid silly shifts that would overflow Duration anyway.
            let shift = consecutive_rl_batches.min(16);
            let raw_ms = config.interval_ms.saturating_mul(1u64 << shift);
            let backoff = Duration::from_millis(raw_ms).min(max_backoff);
            tracing::warn!(
                done = outcome.done,
                skipped = outcome.skipped,
                failed = outcome.failed,
                rate_limited = outcome.rate_limited,
                lap_ms,
                consecutive_rl_batches,
                effective_concurrency,
                backoff_ms = backoff.as_millis() as u64,
                "consolidation: batch hit rate-limit — backing off"
            );
            tokio::time::sleep(backoff).await;
        } else {
            // Reset backoff on a clean (or merely-data-failed) batch.
            if consecutive_rl_batches > 0 {
                tracing::info!(
                    previous_rl_streak = consecutive_rl_batches,
                    "consolidation: rate-limit backoff cleared by clean batch"
                );
            }
            consecutive_rl_batches = 0;
            if outcome.failed > 0 {
                tracing::warn!(
                    done = outcome.done,
                    skipped = outcome.skipped,
                    failed = outcome.failed,
                    lap_ms,
                    "consolidation: batch completed with failures"
                );
            } else {
                tracing::debug!(
                    done = outcome.done,
                    skipped = outcome.skipped,
                    lap_ms,
                    "consolidation: batch completed"
                );
            }
            // No post-batch sleep on clean runs — keep draining work as
            // fast as the embedder allows. The base_interval sleep
            // happens only on empty queues (above) and after rate-limit
            // backoff. This matches the pre-PR-2 hot-loop behavior so
            // we don't slow steady-state throughput.
        }
    }
}

/// Drive consolidation to completion synchronously. Test helper used by
/// integration tests so they don't need to sleep-poll the worker tick.
/// Returns when the queue is empty AND a final batch ran clean.
#[doc(hidden)]
pub async fn drain_for_test(
    services: &ContextNestServices,
    queue: &ConsolidationQueue,
    concurrency: usize,
) {
    initial_scan(services, queue).await;
    loop {
        let batch = queue.drain_batch(256);
        if batch.is_empty() {
            return;
        }
        let start = Instant::now();
        let outcome = process_batch(services, batch, concurrency).await;
        let mut m = queue.metrics.lock().expect("metrics lock poisoned");
        m.consolidated += outcome.done;
        m.failed += outcome.failed + outcome.rate_limited;
        m.last_lap_ms = start.elapsed().as_millis() as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_dedups_repeated_ids() {
        let q = ConsolidationQueue::new();
        q.enqueue("a".to_string());
        q.enqueue("a".to_string());
        q.enqueue("b".to_string());
        assert_eq!(q.pending_count(), 2);
    }

    #[test]
    fn drain_batch_returns_at_most_max_items() {
        let q = ConsolidationQueue::new();
        for i in 0..10 {
            q.enqueue(format!("id-{i}"));
        }
        let first = q.drain_batch(4);
        assert_eq!(first.len(), 4);
        assert_eq!(q.pending_count(), 6);
    }

    #[test]
    fn config_from_env_uses_defaults_when_unset() {
        // All env-touching tests merged into config_from_env_full_sweep
        // below because cargo's default parallel test runner races env-
        // var mutations across threads — each individual test gets
        // intermittent failures when a sibling test sets a var it
        // expects to be unset. Single-test ownership of the env mutex
        // is the cleanest fix without adding the serial_test dep.
        config_from_env_full_sweep();
    }

    fn config_from_env_full_sweep() {
        // Save + restore relevant env vars so we don't leak state to
        // sibling tests OR to the next `cargo test` invocation.
        let keys = [
            "CONTEXTNEST_CONSOLIDATION_INTERVAL_MS",
            "CONTEXTNEST_CONSOLIDATION_CONCURRENCY",
            "CONTEXTNEST_CONSOLIDATION_BATCH_SIZE",
            "CONTEXTNEST_CONSOLIDATION_ENABLED",
            "CONTEXTNEST_CONSOLIDATION_MAX_BACKOFF_MS",
            "CONTEXTNEST_CONSOLIDATION_BACKOFF_CONCURRENCY_FLOOR",
        ];
        let saved: Vec<_> = keys.iter().map(|k| (*k, std::env::var(k).ok())).collect();
        let restore = || {
            for (k, v) in &saved {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        };

        // 1. Defaults
        for k in &keys {
            std::env::remove_var(k);
        }
        let d = ConsolidationConfig::from_env();
        assert_eq!(d.interval_ms, 500);
        assert_eq!(d.concurrency, 4);
        assert_eq!(d.batch_size, 32);
        assert!(d.enabled);
        assert_eq!(d.max_backoff_ms, 30_000);
        assert_eq!(d.backoff_concurrency_floor, 1);

        // 2. enabled=false explicitly
        std::env::set_var("CONTEXTNEST_CONSOLIDATION_ENABLED", "false");
        assert!(!ConsolidationConfig::from_env().enabled);
        std::env::remove_var("CONTEXTNEST_CONSOLIDATION_ENABLED");

        // 3. backoff knobs parse correctly
        std::env::set_var("CONTEXTNEST_CONSOLIDATION_MAX_BACKOFF_MS", "60000");
        std::env::set_var("CONTEXTNEST_CONSOLIDATION_BACKOFF_CONCURRENCY_FLOOR", "2");
        let cfg = ConsolidationConfig::from_env();
        assert_eq!(cfg.max_backoff_ms, 60_000);
        assert_eq!(cfg.backoff_concurrency_floor, 2);

        // 4. floor=0 rejected → default 1
        std::env::set_var("CONTEXTNEST_CONSOLIDATION_BACKOFF_CONCURRENCY_FLOOR", "0");
        assert_eq!(ConsolidationConfig::from_env().backoff_concurrency_floor, 1);

        // 5. max_backoff<100ms rejected → default 30_000
        std::env::set_var("CONTEXTNEST_CONSOLIDATION_MAX_BACKOFF_MS", "50");
        assert_eq!(ConsolidationConfig::from_env().max_backoff_ms, 30_000);

        restore();
    }

    #[test]
    fn looks_rate_limited_matches_known_provider_shapes() {
        // OpenAI rate-limit envelope:
        assert!(looks_rate_limited(
            r#"{"error":{"message":"Model busy, retry later","code":"engine_overloaded"}}"#
        ));
        // Bare HTTP 429:
        assert!(looks_rate_limited("HTTP 429 Too Many Requests"));
        assert!(looks_rate_limited("status: 429"));
        // OpenAI rate_limit_exceeded:
        assert!(looks_rate_limited(
            r#"{"error":{"code":"rate_limit_exceeded"}}"#
        ));
        // Case-insensitive match:
        assert!(looks_rate_limited("RATE LIMIT REACHED"));
        // Negative: ordinary errors should NOT trip backoff.
        assert!(!looks_rate_limited("connection refused"));
        assert!(!looks_rate_limited("malformed json"));
        assert!(!looks_rate_limited(""));
    }

    #[test]
    fn batch_outcome_buckets_default_to_zero() {
        let o = BatchOutcome::default();
        assert_eq!(o.done, 0);
        assert_eq!(o.skipped, 0);
        assert_eq!(o.failed, 0);
        assert_eq!(o.rate_limited, 0);
    }

    // Backoff-knob env-parsing assertions moved into
    // `config_from_env_full_sweep` above to avoid parallel env-mutation
    // races with the older `config_from_env_disables_via_false_string`
    // test (which runs concurrently under cargo's default test
    // scheduler and would race on the ENABLED key).

    #[test]
    fn config_from_env_disables_via_false_string() {
        std::env::set_var("CONTEXTNEST_CONSOLIDATION_ENABLED", "false");
        let cfg = ConsolidationConfig::from_env();
        assert!(!cfg.enabled);
        std::env::remove_var("CONTEXTNEST_CONSOLIDATION_ENABLED");
    }

    #[test]
    fn is_consolidated_reads_the_bool_flag() {
        let mut meta = HashMap::new();
        assert!(!is_consolidated(&meta));
        meta.insert(CONSOLIDATED_FLAG.to_string(), Value::Bool(false));
        assert!(!is_consolidated(&meta));
        meta.insert(CONSOLIDATED_FLAG.to_string(), Value::Bool(true));
        assert!(is_consolidated(&meta));
    }
}

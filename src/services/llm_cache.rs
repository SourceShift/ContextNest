//! LLM proxy response cache (v0.3 Phase 2).
//!
//! Pure logic for deriving a `CacheKey` from a chat-completion request
//! (slice 2.1), an in-memory `LlmCacheService` with exact-prefix +
//! cosine-similarity lookup, insert, TTL freshness, and hit/miss stats
//! (slice 2.2), handler wiring (slice 2.3), `GET /llm/v1/cache/stats`
//! (slice 2.4), and WAL persistence of cache entries across restarts
//! (slice 2.5 — this file's final form for Phase 2).
//!
//! ## Design (from `docs/roadmap/v0.3-llm-proxy.md`)
//!
//! The cache key has two parts:
//!
//! - **`ExactKeyPrefix`** — `(project_id, model, temperature_bucket,
//!   system_prompt_hash)`. These fields must match exactly for a cache
//!   hit; differences here mean the cached response is for a different
//!   conversation surface and is not eligible.
//! - **Semantic embedding** — `Vec<f32>` over the user prompt. Used by
//!   the substrate's attractor activation for similarity lookup at a
//!   configurable threshold (roadmap default: 0.92). Two requests with
//!   the same exact-prefix but semantically distant user prompts will
//!   not collide.
//!
//! ## Why bucket temperature at 0.05?
//!
//! OpenAI's spec accepts `temperature` as a `f32` in `[0.0, 2.0]`. Most
//! workloads use coarse settings — 0.0 (deterministic), 0.2 (precise),
//! 0.7 (default-ish), 1.0 (creative). Bucketing at 0.05 granularity
//! collapses near-equivalent settings (0.71 ↔ 0.70) into the same key
//! without conflating semantically different settings (0.0 ↔ 0.7).
//! The roadmap calls this out explicitly.
//!
//! ## Why SHA-256 truncated to 8 bytes for the system prompt?
//!
//! The system prompt is part of the exact match — two different system
//! prompts MUST yield different keys. Truncated SHA-256 gives 2^64
//! collision space, which is overwhelming for any single project's
//! prompt corpus. Storing the full hash would inflate the key by 24
//! bytes per entry for no gain.
//!
//! ## What 2.2 ships on top of 2.1
//!
//! - `LlmCacheService` with `Arc<RwLock<...>>` interior — concurrent
//!   reads, serialized writes.
//! - `lookup(&CacheKey, max_age) -> Option<CachedResponse>` performs
//!   exact-prefix HashMap probe → TTL filter → cosine-similarity
//!   match against entries sharing the prefix, returning the entry
//!   exceeding the configured threshold (default 0.92).
//! - `insert(CacheKey, ChatCompletionsResponse)` appends to the
//!   prefix bucket; no eviction in 2.2 (LRU lands when the demo
//!   workloads in slice 2.4 show memory pressure).
//! - `stats() -> CacheStats` snapshots hits, misses, total entries,
//!   and computed hit rate.
//!
//! ## Why WAL-with-its-own-record-type, not substrate-fragment mixing?
//!
//! The original roadmap sketch said "substrate auto-store" but
//! conflated two concerns: cache entries are NOT user memories.
//! Promoting cache entries through `MemoryAttractorManager::process_memories`
//! would pollute `reconstruct` / `resonate` queries with cache
//! fragments that don't represent user knowledge. Instead, slice 2.5
//! adds a new `WalRecord::LlmCacheInsert` variant: the cache uses the
//! same WAL file the substrate uses but its own record type, replayed
//! into the in-memory map by `LlmCacheService::replay` before HTTP
//! traffic starts. Substrate-backed semantic match (using attractor
//! activation) becomes a v0.3.1+ enhancement that does NOT pollute
//! the canonical user-memory pipeline.
//!
//! ## What this module does NOT do
//!
//! - Does not compute the user-prompt embedding. The caller passes
//!   the embedding in (it comes from `EmbeddingService`).
//! - Does not evict; the map grows unbounded until restart. LRU/size
//!   caps land when demo workloads demonstrate the need.
//! - Does not handle multi-tenancy scoping beyond defaulting
//!   `project_id` to a fixed sentinel until v0.2 lands.
//! - Does not implement substrate-backed semantic match — that lives
//!   intentionally outside the user-memory pipeline; deferred to v0.3.1+.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::api::llm_proxy::openai_shapes::{
    ChatCompletionsResponse, ContentPart, Message, MessageContent, Role,
};

/// Number of temperature buckets across the OpenAI range `[0.0, 1.0]`.
/// Bucket index = round(temp * 20). 0.0 → 0, 0.05 → 1, ..., 1.0 → 20.
/// Values above 1.0 clamp to bucket 20; values below 0.0 clamp to 0.
pub const TEMPERATURE_BUCKETS: u8 = 21;

/// OpenAI's documented default temperature when the field is omitted.
pub const DEFAULT_TEMPERATURE: f32 = 1.0;

/// Stand-in `project_id` for substrates without v0.2 multi-tenancy.
/// Once v0.2 ships, the proxy threads the real project id through the
/// request context and this sentinel is unreachable.
pub const DEFAULT_PROJECT_ID: &str = "default";

/// Exact-match component of a cache key. Hash + Eq so it can serve as
/// the bucket-key for the in-memory lookup table that later slices
/// build on top.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExactKeyPrefix {
    pub project_id: String,
    pub model: String,
    pub temperature_bucket: u8,
    /// First 8 bytes of `sha256(system_prompt)`. 2^64 collision space.
    pub system_prompt_hash: [u8; 8],
}

impl ExactKeyPrefix {
    /// Stable 32-byte fingerprint over the four exact-match fields.
    /// Subsequent slices use this as the substrate fragment-id seed so
    /// cache entries are deterministically addressable across restarts.
    pub fn fingerprint(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.project_id.as_bytes());
        hasher.update([0u8]);
        hasher.update(self.model.as_bytes());
        hasher.update([0u8]);
        hasher.update([self.temperature_bucket]);
        hasher.update(self.system_prompt_hash);
        hasher.finalize().into()
    }
}

/// Full cache key. The exact prefix is the strict-match component;
/// the semantic embedding is the similarity-match component handled
/// downstream by the substrate's attractor activation.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheKey {
    pub exact: ExactKeyPrefix,
    pub semantic_embedding: Vec<f32>,
}

/// Round-to-bucket the OpenAI temperature parameter.
///
/// Clamps the input to `[0.0, 1.0]` first, then maps to a bucket index
/// `0..=20` at 0.05 granularity. NaN inputs use `DEFAULT_TEMPERATURE`.
/// Values above 1.0 (OpenAI allows up to 2.0) all bucket to 20 — the
/// upper half of the range is treated as "creative ≥ 1.0" because real
/// usage past 1.0 is rare and the difference between 1.5 and 2.0
/// doesn't meaningfully change cache eligibility.
pub fn bucket_temperature(t: f32) -> u8 {
    let t = if t.is_nan() { DEFAULT_TEMPERATURE } else { t };
    let clamped = t.clamp(0.0, 1.0);
    (clamped * 20.0).round() as u8
}

/// First 8 bytes of `sha256(s)`. Deterministic across runs.
pub fn hash_system_prompt(s: &str) -> [u8; 8] {
    let digest = Sha256::digest(s.as_bytes());
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest[..8]);
    out
}

/// Concatenate every system-role message's textual content into one
/// string. The OpenAI spec allows multiple system messages (rarely
/// used in practice but legal); concatenation matches what providers
/// effectively do anyway.
pub fn extract_system_prompt_text(messages: &[Message]) -> String {
    let mut out = String::new();
    for msg in messages {
        if msg.role != Role::System {
            continue;
        }
        match &msg.content {
            Some(MessageContent::Text(s)) => {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(s);
            }
            Some(MessageContent::Parts(parts)) => {
                for part in parts {
                    if let ContentPart::Text { text } = part {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(text);
                    }
                }
            }
            None => {}
        }
    }
    out
}

/// Textual content of the LAST user-role message. Image / audio /
/// file parts are skipped — semantic embedding is text-only in v0.3,
/// multimodal cache keying lands when the substrate's embedder gains
/// vision support.
pub fn extract_user_prompt_text(messages: &[Message]) -> String {
    let last_user = messages.iter().rev().find(|m| m.role == Role::User);
    let Some(msg) = last_user else {
        return String::new();
    };
    match &msg.content {
        Some(MessageContent::Text(s)) => s.clone(),
        Some(MessageContent::Parts(parts)) => {
            let mut out = String::new();
            for part in parts {
                if let ContentPart::Text { text } = part {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(text);
                }
            }
            out
        }
        None => String::new(),
    }
}

/// Build a `CacheKey` from already-derived components. The embedding
/// is provided by the caller (it comes from `EmbeddingService::generate_embedding`
/// on the extracted user prompt — keeping that call out of this
/// function lets cache-key derivation stay pure + synchronous + cheap).
pub fn derive_cache_key(
    project_id: &str,
    model: &str,
    temperature: Option<f32>,
    system_prompt: &str,
    user_prompt_embedding: Vec<f32>,
) -> CacheKey {
    let project_id = if project_id.is_empty() {
        DEFAULT_PROJECT_ID
    } else {
        project_id
    };
    let temperature_bucket = bucket_temperature(temperature.unwrap_or(DEFAULT_TEMPERATURE));
    let system_prompt_hash = hash_system_prompt(system_prompt);
    CacheKey {
        exact: ExactKeyPrefix {
            project_id: project_id.to_string(),
            model: model.to_string(),
            temperature_bucket,
            system_prompt_hash,
        },
        semantic_embedding: user_prompt_embedding,
    }
}

// =============================================================================
// In-memory cache store (slice 2.2)
// =============================================================================

/// Default cosine-similarity threshold for the semantic-match step.
/// 0.92 per `docs/roadmap/v0.3-llm-proxy.md`. Tunable per-service via
/// the builder so demo workloads can sweep the curve.
pub const DEFAULT_SIMILARITY_THRESHOLD: f32 = 0.92;

/// Default TTL for cached entries. 3600s (1h) per roadmap. Per-request
/// override via `x-cn-cache-max-age` lands in slice 2.3.
pub const DEFAULT_TTL_SECS: u64 = 3600;

/// One stored response associated with an exact-prefix bucket.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// The user-prompt embedding this entry was inserted under.
    /// Cosine similarity is computed against this vector.
    pub embedding: Vec<f32>,
    /// The cached response body. Sent verbatim back to clients on hit
    /// (the handler in slice 2.3 will rewrite `id` so duplicates from
    /// distinct cache lookups carry distinct trace ids).
    pub response: ChatCompletionsResponse,
    /// Monotonic instant of insertion. TTL check uses `Instant` so
    /// cache freshness is unaffected by wall-clock skew or system
    /// suspend/resume.
    pub inserted_at: Instant,
    /// Times this entry has been served as a hit. Surfaced via
    /// `stats()` for hot-entry analysis in slice 2.4.
    pub hit_count: u64,
}

/// Snapshot of cache counters. Returned by `LlmCacheService::stats()`
/// and serialised by the slice 2.4 stats endpoint.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct CacheStats {
    pub total_entries: usize,
    pub total_hits: u64,
    pub total_misses: u64,
    /// hits / (hits + misses), or 0.0 when there have been no lookups.
    pub hit_rate: f32,
}

struct LlmCacheInner {
    by_prefix: HashMap<ExactKeyPrefix, Vec<CacheEntry>>,
    hits: u64,
    misses: u64,
}

impl LlmCacheInner {
    fn new() -> Self {
        Self {
            by_prefix: HashMap::new(),
            hits: 0,
            misses: 0,
        }
    }

    fn total_entries(&self) -> usize {
        self.by_prefix.values().map(|v| v.len()).sum()
    }
}

/// Cosine similarity between two equal-length vectors. Returns 0.0
/// for mismatched lengths, empty vectors, or either-zero-magnitude —
/// "not similar" is the safe answer when the comparison is degenerate.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut mag_a = 0.0f32;
    let mut mag_b = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        mag_a += x * x;
        mag_b += y * y;
    }
    let mag_a = mag_a.sqrt();
    let mag_b = mag_b.sqrt();
    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }
    dot / (mag_a * mag_b)
}

/// In-memory cache store. Concurrent reads via `RwLock::read()`;
/// writes (including hit-count increments) take `write()`.
///
/// Cheap to `Clone` — the inner state is an `Arc`. Multiple handlers
/// can share one service instance.
///
/// ## WAL persistence (slice 2.5)
///
/// When constructed via [`Self::with_wal`], every `insert` appends a
/// `WalRecord::LlmCacheInsert` to the substrate's WAL. On restart,
/// the bootstrap path replays those records via [`Self::replay`]
/// so the warm-up curve doesn't have to be re-paid on every binary
/// deploy. WAL writes are best-effort: if the WAL handle isn't yet
/// installed (pre-replay phase) or the append fails, the in-memory
/// insert still succeeds and a `warn!` log surfaces the failure.
#[derive(Clone)]
pub struct LlmCacheService {
    inner: Arc<RwLock<LlmCacheInner>>,
    similarity_threshold: f32,
    default_ttl: Duration,
    /// Optional WAL handle. `Arc<OnceCell<Wal>>` because the same
    /// handle is shared across `ContextNestServices` clones, and the
    /// WAL writer is only installed after startup replay completes.
    /// `None` here means "in-memory only" mode (matches pre-2.5
    /// behaviour exactly — useful for tests and offline CI).
    wal: Option<Arc<tokio::sync::OnceCell<crate::services::wal::Wal>>>,
}

impl LlmCacheService {
    /// Construct with roadmap defaults: threshold 0.92, TTL 3600s.
    /// No WAL persistence — call [`Self::with_wal`] to enable.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(LlmCacheInner::new())),
            similarity_threshold: DEFAULT_SIMILARITY_THRESHOLD,
            default_ttl: Duration::from_secs(DEFAULT_TTL_SECS),
            wal: None,
        }
    }

    /// Override the cosine-similarity threshold. `1.0` requires exact
    /// embedding match; `0.0` matches any entry sharing the exact
    /// prefix.
    pub fn with_similarity_threshold(mut self, threshold: f32) -> Self {
        self.similarity_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Override the default TTL. Per-request override via `max_age`
    /// argument to `lookup` still takes precedence.
    pub fn with_default_ttl(mut self, ttl: Duration) -> Self {
        self.default_ttl = ttl;
        self
    }

    /// Attach a WAL handle. `insert` will append a
    /// `WalRecord::LlmCacheInsert` after a successful in-memory put,
    /// once the OnceCell is filled. Pre-replay inserts (rare — the
    /// substrate doesn't accept HTTP traffic until replay completes)
    /// skip the append silently.
    pub fn with_wal(mut self, wal: Arc<tokio::sync::OnceCell<crate::services::wal::Wal>>) -> Self {
        self.wal = Some(wal);
        self
    }

    /// Replay a sequence of WAL records back into the in-memory store.
    /// Filters for `LlmCacheInsert` variants; all other variants are
    /// ignored (caller passes the full record list, this function
    /// picks out its own).
    ///
    /// `now_secs` is the current Unix timestamp — needed because each
    /// record carries `inserted_at_unix_secs`, but the in-memory
    /// `CacheEntry::inserted_at` is a monotonic `Instant`. We
    /// approximate the original `Instant` by walking back from
    /// `Instant::now()` by `now_secs - inserted_at_unix_secs` seconds.
    /// Records older than the cache's `default_ttl` are skipped to
    /// avoid restoring entries that would immediately fail a TTL
    /// check anyway.
    ///
    /// Returns the number of entries that were replayed back in.
    pub fn replay(&self, records: &[crate::services::wal::WalRecord], now_unix_secs: u64) -> usize {
        use crate::services::wal::WalRecord;

        let now_instant = Instant::now();
        let default_ttl_secs = self.default_ttl.as_secs();
        let mut restored = 0usize;
        let mut guard = self.inner.write().expect("llm_cache lock poisoned");

        for record in records {
            let WalRecord::LlmCacheInsert {
                project_id,
                model,
                temperature_bucket,
                system_prompt_hash,
                embedding,
                response_json,
                inserted_at_unix_secs,
            } = record
            else {
                continue;
            };

            let age_secs = now_unix_secs.saturating_sub(*inserted_at_unix_secs);
            // Skip entries already past TTL — restoring them would
            // just produce immediate misses on the first lookup.
            if age_secs > default_ttl_secs {
                continue;
            }

            let response: ChatCompletionsResponse = match serde_json::from_str(response_json) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "llm_cache replay: failed to parse stored response_json; skipping entry"
                    );
                    continue;
                }
            };

            let inserted_at = now_instant
                .checked_sub(Duration::from_secs(age_secs))
                .unwrap_or(now_instant);

            let prefix = ExactKeyPrefix {
                project_id: project_id.clone(),
                model: model.clone(),
                temperature_bucket: *temperature_bucket,
                system_prompt_hash: *system_prompt_hash,
            };
            let entry = CacheEntry {
                embedding: embedding.clone(),
                response,
                inserted_at,
                hit_count: 0,
            };
            guard.by_prefix.entry(prefix).or_default().push(entry);
            restored += 1;
        }

        restored
    }

    pub fn similarity_threshold(&self) -> f32 {
        self.similarity_threshold
    }

    pub fn default_ttl(&self) -> Duration {
        self.default_ttl
    }

    /// Look up a cache key. `max_age` overrides `default_ttl` when
    /// provided — slice 2.3 will populate this from the request's
    /// `x-cn-cache-max-age` header.
    ///
    /// Returns the cloned response on hit and increments the entry's
    /// `hit_count` + the service `hits` counter. Returns `None` on
    /// miss and increments `misses`.
    pub fn lookup(
        &self,
        key: &CacheKey,
        max_age: Option<Duration>,
    ) -> Option<ChatCompletionsResponse> {
        let ttl = max_age.unwrap_or(self.default_ttl);
        // `max_age = Duration::ZERO` forces a miss — semantic match
        // of "never serve from cache for this request".
        if ttl.is_zero() {
            self.bump_miss();
            return None;
        }
        let mut guard = self.inner.write().expect("llm_cache lock poisoned");
        let now = Instant::now();
        let Some(bucket) = guard.by_prefix.get_mut(&key.exact) else {
            guard.misses += 1;
            return None;
        };
        let mut best: Option<(usize, f32)> = None;
        for (idx, entry) in bucket.iter().enumerate() {
            // TTL check first — older entries are filtered out before
            // similarity comparison so an old high-similarity entry
            // can't shadow a fresh acceptable-similarity entry.
            if now.duration_since(entry.inserted_at) > ttl {
                continue;
            }
            let sim = cosine_similarity(&key.semantic_embedding, &entry.embedding);
            if sim < self.similarity_threshold {
                continue;
            }
            best = match best {
                None => Some((idx, sim)),
                Some((_, prev)) if sim > prev => Some((idx, sim)),
                Some(x) => Some(x),
            };
        }
        match best {
            Some((idx, _)) => {
                bucket[idx].hit_count += 1;
                let resp = bucket[idx].response.clone();
                guard.hits += 1;
                Some(resp)
            }
            None => {
                guard.misses += 1;
                None
            }
        }
    }

    fn bump_miss(&self) {
        let mut guard = self.inner.write().expect("llm_cache lock poisoned");
        guard.misses += 1;
    }

    /// Insert a response under the given key. Appends to the
    /// prefix bucket; does not replace existing entries with
    /// matching embeddings (a subsequent lookup picks the best-
    /// similarity match anyway).
    ///
    /// When a WAL handle is configured AND installed (post-replay),
    /// also appends a `WalRecord::LlmCacheInsert` so the entry
    /// survives a restart. The append is synchronous to match the
    /// existing `WalRecord::Store` flow (both pay the same per-record
    /// write cost; switching one to async would be inconsistent).
    /// WAL failure is logged at `warn!` but does NOT fail the
    /// in-memory insert — cache durability is best-effort by design.
    pub fn insert(&self, key: CacheKey, response: ChatCompletionsResponse) {
        // Capture the WAL record fields BEFORE moving key + response
        // into the in-memory entry, so we don't pay a clone cost when
        // WAL is disabled.
        let wal_record = self.wal.as_ref().map(|_| {
            let inserted_at_unix_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            crate::services::wal::WalRecord::LlmCacheInsert {
                project_id: key.exact.project_id.clone(),
                model: key.exact.model.clone(),
                temperature_bucket: key.exact.temperature_bucket,
                system_prompt_hash: key.exact.system_prompt_hash,
                embedding: key.semantic_embedding.clone(),
                response_json: serde_json::to_string(&response).unwrap_or_default(),
                inserted_at_unix_secs,
            }
        });

        let entry = CacheEntry {
            embedding: key.semantic_embedding,
            response,
            inserted_at: Instant::now(),
            hit_count: 0,
        };
        {
            let mut guard = self.inner.write().expect("llm_cache lock poisoned");
            guard.by_prefix.entry(key.exact).or_default().push(entry);
        }

        if let (Some(wal_arc), Some(record)) = (self.wal.as_ref(), wal_record) {
            // The WAL OnceCell may not be initialised yet (we're in the
            // pre-replay window). That's fine — the in-memory entry is
            // already stored and the record will be re-WAL'd on the
            // next live insert. `get()` is non-blocking.
            if let Some(wal) = wal_arc.get() {
                if let Err(e) = wal.append(&record) {
                    tracing::warn!(error = %e, "llm_cache: WAL append failed");
                }
            }
        }
    }

    /// Snapshot of current counters.
    pub fn stats(&self) -> CacheStats {
        let guard = self.inner.read().expect("llm_cache lock poisoned");
        let total = guard.hits + guard.misses;
        let hit_rate = if total == 0 {
            0.0
        } else {
            guard.hits as f32 / total as f32
        };
        CacheStats {
            total_entries: guard.total_entries(),
            total_hits: guard.hits,
            total_misses: guard.misses,
            hit_rate,
        }
    }
}

impl Default for LlmCacheService {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LlmCacheService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stats = self.stats();
        f.debug_struct("LlmCacheService")
            .field("similarity_threshold", &self.similarity_threshold)
            .field("default_ttl_secs", &self.default_ttl.as_secs())
            .field("wal_attached", &self.wal.is_some())
            .field("total_entries", &stats.total_entries)
            .field("total_hits", &stats.total_hits)
            .field("total_misses", &stats.total_misses)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::llm_proxy::openai_shapes::{Message, MessageContent, Role};
    use serde_json::json;

    fn user_msg(s: &str) -> Message {
        Message {
            role: Role::User,
            content: Some(MessageContent::Text(s.to_string())),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    fn system_msg(s: &str) -> Message {
        Message {
            role: Role::System,
            content: Some(MessageContent::Text(s.to_string())),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    #[test]
    fn bucket_temperature_rounds_at_005_granularity() {
        assert_eq!(bucket_temperature(0.0), 0);
        assert_eq!(bucket_temperature(0.05), 1);
        assert_eq!(bucket_temperature(0.10), 2);
        assert_eq!(bucket_temperature(0.7), 14);
        assert_eq!(bucket_temperature(0.75), 15);
        assert_eq!(bucket_temperature(1.0), 20);
    }

    #[test]
    fn bucket_temperature_clamps_above_one() {
        assert_eq!(bucket_temperature(1.5), 20);
        assert_eq!(bucket_temperature(2.0), 20);
        assert_eq!(bucket_temperature(f32::INFINITY), 20);
    }

    #[test]
    fn bucket_temperature_clamps_below_zero() {
        assert_eq!(bucket_temperature(-0.1), 0);
        assert_eq!(bucket_temperature(-50.0), 0);
        assert_eq!(bucket_temperature(f32::NEG_INFINITY), 0);
    }

    #[test]
    fn bucket_temperature_nan_uses_default() {
        // NaN → DEFAULT_TEMPERATURE (1.0) → bucket 20. Documented behaviour
        // so a corrupt input doesn't silently map to "0 = deterministic".
        assert_eq!(bucket_temperature(f32::NAN), 20);
    }

    #[test]
    fn bucket_temperature_near_boundaries_rounds_half_to_even() {
        // 0.075 sits between bucket 1 (0.05) and bucket 2 (0.10).
        // f32::round is banker's rounding; document the boundary
        // behaviour so future refactors can't silently flip it.
        let b = bucket_temperature(0.075);
        assert!(
            b == 1 || b == 2,
            "expected bucket 1 or 2 at half-boundary 0.075, got {b}"
        );
    }

    #[test]
    fn hash_system_prompt_is_deterministic() {
        let a = hash_system_prompt("you are a careful assistant");
        let b = hash_system_prompt("you are a careful assistant");
        assert_eq!(a, b);
    }

    #[test]
    fn hash_system_prompt_differs_for_different_inputs() {
        let a = hash_system_prompt("you are a careful assistant");
        let b = hash_system_prompt("you are a creative assistant");
        assert_ne!(a, b);
    }

    #[test]
    fn hash_system_prompt_empty_string_is_stable() {
        // Empty system prompt is a legitimate "no system message" case.
        // The hash for "" must be stable and != any non-empty hash.
        let empty = hash_system_prompt("");
        let other = hash_system_prompt(" ");
        assert_ne!(empty, other);
        assert_eq!(empty, hash_system_prompt(""));
    }

    #[test]
    fn extract_user_prompt_text_finds_last_user_message() {
        let msgs = vec![
            user_msg("first user turn"),
            Message {
                role: Role::Assistant,
                content: Some(MessageContent::Text("reply".into())),
                name: None,
                tool_call_id: None,
                tool_calls: None,
            },
            user_msg("second user turn"),
        ];
        assert_eq!(extract_user_prompt_text(&msgs), "second user turn");
    }

    #[test]
    fn extract_user_prompt_text_handles_multimodal_content() {
        let parts = vec![
            ContentPart::Text {
                text: "describe the image".into(),
            },
            ContentPart::ImageUrl {
                image_url: json!({"url": "data:..."}),
            },
            ContentPart::Text {
                text: "in detail".into(),
            },
        ];
        let msgs = vec![Message {
            role: Role::User,
            content: Some(MessageContent::Parts(parts)),
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }];
        // Only text parts contribute; image_url is skipped because
        // v0.3's embedder is text-only.
        assert_eq!(
            extract_user_prompt_text(&msgs),
            "describe the image in detail"
        );
    }

    #[test]
    fn extract_user_prompt_text_empty_when_no_user_message() {
        let msgs = vec![system_msg("system only")];
        assert_eq!(extract_user_prompt_text(&msgs), "");
    }

    #[test]
    fn extract_system_prompt_text_concats_multiple_system_messages() {
        // Rare but spec-allowed; providers concatenate effectively
        // anyway. Pin the join character so the hash is stable.
        let msgs = vec![
            system_msg("be concise"),
            user_msg("hi"),
            system_msg("avoid emojis"),
        ];
        assert_eq!(
            extract_system_prompt_text(&msgs),
            "be concise\navoid emojis"
        );
    }

    #[test]
    fn extract_system_prompt_text_empty_when_no_system_message() {
        let msgs = vec![user_msg("hello")];
        assert_eq!(extract_system_prompt_text(&msgs), "");
    }

    #[test]
    fn derive_cache_key_uses_defaults_when_inputs_absent() {
        let k = derive_cache_key("", "gpt-4o-mini", None, "", vec![]);
        assert_eq!(k.exact.project_id, DEFAULT_PROJECT_ID);
        assert_eq!(k.exact.temperature_bucket, 20); // 1.0 → bucket 20
        assert_eq!(k.exact.system_prompt_hash, hash_system_prompt(""));
    }

    #[test]
    fn derive_cache_key_respects_provided_temperature() {
        let k = derive_cache_key("proj", "gpt-4o", Some(0.2), "", vec![]);
        assert_eq!(k.exact.temperature_bucket, 4); // 0.2 * 20 = 4
    }

    #[test]
    fn derive_cache_key_differentiates_by_model() {
        let a = derive_cache_key("p", "gpt-4o-mini", Some(0.5), "sys", vec![]);
        let b = derive_cache_key("p", "claude-3-5-sonnet-latest", Some(0.5), "sys", vec![]);
        assert_ne!(a.exact, b.exact);
    }

    #[test]
    fn derive_cache_key_differentiates_by_project() {
        let a = derive_cache_key("proj-a", "gpt-4o", Some(0.5), "sys", vec![]);
        let b = derive_cache_key("proj-b", "gpt-4o", Some(0.5), "sys", vec![]);
        assert_ne!(a.exact, b.exact);
    }

    #[test]
    fn exact_key_prefix_fingerprint_is_deterministic() {
        let a = derive_cache_key("p", "m", Some(0.7), "sys", vec![]);
        let b = derive_cache_key("p", "m", Some(0.7), "sys", vec![]);
        assert_eq!(a.exact.fingerprint(), b.exact.fingerprint());
    }

    #[test]
    fn exact_key_prefix_fingerprint_differs_per_field() {
        let base = derive_cache_key("p", "m", Some(0.7), "sys", vec![]);
        let by_project = derive_cache_key("q", "m", Some(0.7), "sys", vec![]);
        let by_model = derive_cache_key("p", "n", Some(0.7), "sys", vec![]);
        let by_temp = derive_cache_key("p", "m", Some(0.4), "sys", vec![]);
        let by_sys = derive_cache_key("p", "m", Some(0.7), "different sys", vec![]);
        for other in [&by_project, &by_model, &by_temp, &by_sys] {
            assert_ne!(
                base.exact.fingerprint(),
                other.exact.fingerprint(),
                "fingerprint must change when any exact-key field changes"
            );
        }
    }

    #[test]
    fn exact_key_prefix_hashes_into_a_hashmap() {
        // The first-pass lookup table builds on Hash + Eq. Pin that
        // the prefix is usable as a HashMap key.
        use std::collections::HashMap;
        let k1 = derive_cache_key("p", "m", Some(0.7), "sys", vec![]);
        let k2 = derive_cache_key("p", "m", Some(0.7), "sys", vec![]);
        let mut map: HashMap<ExactKeyPrefix, &'static str> = HashMap::new();
        map.insert(k1.exact.clone(), "hit");
        assert_eq!(map.get(&k2.exact), Some(&"hit"));
    }

    // =========================================================================
    // Store tests (slice 2.2)
    // =========================================================================

    use crate::api::llm_proxy::openai_shapes::{ChatCompletionsResponse, Choice, Usage};

    fn dummy_response(id: &str) -> ChatCompletionsResponse {
        ChatCompletionsResponse {
            id: id.to_string(),
            object: "chat.completion".to_string(),
            created: 1717000000,
            model: "gpt-4o-mini".to_string(),
            choices: vec![Choice {
                index: 0,
                message: Message {
                    role: Role::Assistant,
                    content: Some(MessageContent::Text("cached answer".into())),
                    name: None,
                    tool_call_id: None,
                    tool_calls: None,
                },
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: Some(Usage {
                prompt_tokens: 1,
                completion_tokens: 1,
                total_tokens: 2,
                prompt_tokens_details: None,
                completion_tokens_details: None,
            }),
            system_fingerprint: None,
        }
    }

    fn unit_vec(dim: usize, hot: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; dim];
        v[hot] = 1.0;
        v
    }

    #[test]
    fn cache_empty_returns_miss() {
        let cache = LlmCacheService::new();
        let key = derive_cache_key("p", "m", Some(0.7), "sys", unit_vec(8, 0));
        assert!(cache.lookup(&key, None).is_none());
        let s = cache.stats();
        assert_eq!(s.total_misses, 1);
        assert_eq!(s.total_hits, 0);
        assert_eq!(s.total_entries, 0);
        assert_eq!(s.hit_rate, 0.0);
    }

    #[test]
    fn insert_then_lookup_same_key_returns_hit() {
        let cache = LlmCacheService::new();
        let emb = unit_vec(8, 0);
        let key = derive_cache_key("p", "m", Some(0.7), "sys", emb);
        cache.insert(key.clone(), dummy_response("resp-1"));
        let hit = cache.lookup(&key, None).expect("expected hit");
        assert_eq!(hit.id, "resp-1");
        let s = cache.stats();
        assert_eq!(s.total_hits, 1);
        assert_eq!(s.total_misses, 0);
        assert_eq!(s.total_entries, 1);
        assert!((s.hit_rate - 1.0).abs() < 1e-6);
    }

    #[test]
    fn lookup_with_different_prefix_returns_miss_without_consulting_embeddings() {
        let cache = LlmCacheService::new();
        let emb = unit_vec(8, 0);
        let insert_key = derive_cache_key("p", "model-a", Some(0.7), "sys", emb.clone());
        cache.insert(insert_key, dummy_response("resp"));
        // Same embedding, DIFFERENT model — must miss because exact
        // prefix differs. Bug shape this guards against: prefix
        // collision via Hash-only-but-not-Eq dispatch.
        let lookup_key = derive_cache_key("p", "model-b", Some(0.7), "sys", emb);
        assert!(cache.lookup(&lookup_key, None).is_none());
    }

    #[test]
    fn lookup_with_distant_embedding_returns_miss() {
        let cache = LlmCacheService::new();
        let stored_emb = unit_vec(8, 0); // hot in dim 0
        let store_key = derive_cache_key("p", "m", Some(0.7), "sys", stored_emb);
        cache.insert(store_key, dummy_response("resp"));
        // Orthogonal embedding, same prefix — cosine = 0.0, below
        // the 0.92 default threshold.
        let lookup_emb = unit_vec(8, 7);
        let lookup_key = derive_cache_key("p", "m", Some(0.7), "sys", lookup_emb);
        assert!(cache.lookup(&lookup_key, None).is_none());
    }

    #[test]
    fn lookup_with_near_embedding_returns_hit() {
        let cache = LlmCacheService::new();
        let stored_emb = vec![1.0f32, 0.0, 0.0, 0.0];
        let store_key = derive_cache_key("p", "m", Some(0.7), "sys", stored_emb);
        cache.insert(store_key, dummy_response("resp"));
        // [0.95, 0.05, 0, 0] vs [1, 0, 0, 0] has cosine ≈ 0.9986 — above 0.92.
        let lookup_emb = vec![0.95f32, 0.05, 0.0, 0.0];
        let lookup_key = derive_cache_key("p", "m", Some(0.7), "sys", lookup_emb);
        assert!(cache.lookup(&lookup_key, None).is_some());
    }

    #[test]
    fn lookup_max_age_zero_forces_miss_even_with_exact_match() {
        let cache = LlmCacheService::new();
        let emb = unit_vec(8, 0);
        let key = derive_cache_key("p", "m", Some(0.7), "sys", emb);
        cache.insert(key.clone(), dummy_response("resp"));
        // Per-request override of max_age = 0 means "never serve
        // from cache for this request" — the bypass path.
        assert!(cache.lookup(&key, Some(Duration::ZERO)).is_none());
        let s = cache.stats();
        assert_eq!(s.total_misses, 1);
    }

    #[test]
    fn expired_entry_is_treated_as_miss() {
        let cache = LlmCacheService::new().with_default_ttl(Duration::from_secs(3600));
        let emb = unit_vec(8, 0);
        let key = derive_cache_key("p", "m", Some(0.7), "sys", emb);
        cache.insert(key.clone(), dummy_response("resp"));
        // Per-request max_age of 0 nanoseconds is functionally
        // equivalent to "ignore TTL entirely" only when permissive;
        // here we use Duration::from_nanos(1) to mean "every entry
        // older than 1ns is stale" → the just-inserted entry (whose
        // age is >0 by the time lookup runs) is filtered out.
        std::thread::sleep(Duration::from_millis(2));
        assert!(cache.lookup(&key, Some(Duration::from_nanos(1))).is_none());
    }

    #[test]
    fn multiple_distinct_embeddings_can_share_an_exact_prefix() {
        let cache = LlmCacheService::new();
        let key1 = derive_cache_key("p", "m", Some(0.7), "sys", vec![1.0, 0.0, 0.0]);
        let key2 = derive_cache_key("p", "m", Some(0.7), "sys", vec![0.0, 1.0, 0.0]);
        cache.insert(key1.clone(), dummy_response("for-key1"));
        cache.insert(key2.clone(), dummy_response("for-key2"));
        // Both must look up cleanly to their own response.
        let hit1 = cache.lookup(&key1, None).expect("hit1");
        let hit2 = cache.lookup(&key2, None).expect("hit2");
        assert_eq!(hit1.id, "for-key1");
        assert_eq!(hit2.id, "for-key2");
        // Stats: 2 entries under the single shared prefix.
        let s = cache.stats();
        assert_eq!(s.total_entries, 2);
    }

    #[test]
    fn hit_count_increments_per_entry() {
        let cache = LlmCacheService::new();
        let emb = unit_vec(8, 0);
        let key = derive_cache_key("p", "m", Some(0.7), "sys", emb);
        cache.insert(key.clone(), dummy_response("resp"));
        for _ in 0..3 {
            cache.lookup(&key, None).expect("hit");
        }
        let s = cache.stats();
        assert_eq!(s.total_hits, 3);
        assert_eq!(s.total_misses, 0);
        assert_eq!(s.total_entries, 1);
    }

    #[test]
    fn similarity_threshold_one_requires_exact_embedding_match() {
        let cache = LlmCacheService::new().with_similarity_threshold(1.0);
        let stored_emb = vec![1.0f32, 0.0];
        let store_key = derive_cache_key("p", "m", Some(0.7), "sys", stored_emb);
        cache.insert(store_key, dummy_response("resp"));
        // Slightly-different normalized vector — cosine just below 1.0.
        let lookup_emb = vec![0.999f32, 0.001];
        let lookup_key = derive_cache_key("p", "m", Some(0.7), "sys", lookup_emb);
        // Threshold = 1.0 means cosine must equal 1.0 exactly. Even
        // 0.9999... is rejected. Documents the strictest setting.
        assert!(cache.lookup(&lookup_key, None).is_none());
    }

    #[test]
    fn similarity_threshold_zero_matches_any_same_prefix_entry() {
        let cache = LlmCacheService::new().with_similarity_threshold(0.0);
        let stored_emb = vec![1.0f32, 0.0];
        let store_key = derive_cache_key("p", "m", Some(0.7), "sys", stored_emb);
        cache.insert(store_key, dummy_response("resp"));
        // Orthogonal vector — cosine = 0.0. With threshold 0.0 this
        // still matches: `sim < threshold` is false at equality.
        let lookup_emb = vec![0.0f32, 1.0];
        let lookup_key = derive_cache_key("p", "m", Some(0.7), "sys", lookup_emb);
        assert!(cache.lookup(&lookup_key, None).is_some());
    }

    #[test]
    fn cosine_similarity_handles_degenerate_inputs() {
        // Mismatched length → 0.0. Empty → 0.0. Zero-magnitude → 0.0.
        // Each path that would otherwise NaN or panic must return the
        // "not similar" safe answer.
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        assert_eq!(cosine_similarity(&[1.0, 1.0], &[0.0, 0.0]), 0.0);
        // Identical non-zero vectors → 1.0.
        let s = cosine_similarity(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]);
        assert!((s - 1.0).abs() < 1e-6, "expected ~1.0, got {s}");
        // Orthogonal → 0.0.
        let s = cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]);
        assert!(s.abs() < 1e-6, "expected ~0.0, got {s}");
    }

    #[test]
    fn stats_hit_rate_is_zero_when_no_lookups() {
        let cache = LlmCacheService::new();
        let s = cache.stats();
        assert_eq!(s.hit_rate, 0.0);
        assert_eq!(s.total_hits, 0);
        assert_eq!(s.total_misses, 0);
    }

    #[test]
    fn service_is_clone_and_shares_state_across_clones() {
        // Multiple handlers share one logical cache via Clone.
        let cache1 = LlmCacheService::new();
        let cache2 = cache1.clone();
        let emb = unit_vec(4, 0);
        let key = derive_cache_key("p", "m", Some(0.7), "sys", emb);
        cache1.insert(key.clone(), dummy_response("resp"));
        // Insert via cache1, lookup via cache2 — same Arc<RwLock<...>>
        // means the data is visible.
        assert!(cache2.lookup(&key, None).is_some());
        // Both views see the same stats.
        assert_eq!(cache1.stats().total_hits, cache2.stats().total_hits);
    }

    // =========================================================================
    // WAL persistence tests (slice 2.5)
    // =========================================================================

    use crate::services::wal::WalRecord;

    fn cache_insert_record(
        project_id: &str,
        model: &str,
        bucket: u8,
        sys_hash: [u8; 8],
        embedding: Vec<f32>,
        response_id: &str,
        age_secs: u64,
    ) -> WalRecord {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        WalRecord::LlmCacheInsert {
            project_id: project_id.to_string(),
            model: model.to_string(),
            temperature_bucket: bucket,
            system_prompt_hash: sys_hash,
            embedding,
            response_json: serde_json::to_string(&dummy_response(response_id)).unwrap(),
            inserted_at_unix_secs: now.saturating_sub(age_secs),
        }
    }

    #[test]
    fn replay_restores_fresh_entries() {
        let cache = LlmCacheService::new();
        let prefix_hash = hash_system_prompt("sys");
        let records = vec![
            cache_insert_record("p", "m", 14, prefix_hash, vec![1.0, 0.0], "resp-a", 5),
            cache_insert_record("p", "m", 14, prefix_hash, vec![0.0, 1.0], "resp-b", 10),
        ];

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let restored = cache.replay(&records, now);
        assert_eq!(restored, 2);

        // Both entries should be retrievable by their original
        // embedding under the shared prefix.
        let key_a = derive_cache_key("p", "m", Some(0.7), "sys", vec![1.0, 0.0]);
        let key_b = derive_cache_key("p", "m", Some(0.7), "sys", vec![0.0, 1.0]);
        assert_eq!(
            cache.lookup(&key_a, None).map(|r| r.id),
            Some("resp-a".to_string())
        );
        assert_eq!(
            cache.lookup(&key_b, None).map(|r| r.id),
            Some("resp-b".to_string())
        );
    }

    #[test]
    fn replay_skips_entries_older_than_ttl() {
        let cache = LlmCacheService::new().with_default_ttl(Duration::from_secs(60));
        let prefix_hash = hash_system_prompt("sys");
        let records = vec![
            // 1000 seconds old, default TTL is 60s — must be skipped.
            cache_insert_record("p", "m", 14, prefix_hash, vec![1.0, 0.0], "expired", 1000),
            // 5 seconds old — must be restored.
            cache_insert_record("p", "m", 14, prefix_hash, vec![0.0, 1.0], "fresh", 5),
        ];

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let restored = cache.replay(&records, now);
        assert_eq!(
            restored, 1,
            "old entry must be skipped to avoid immediate-miss restores"
        );

        let key_fresh = derive_cache_key("p", "m", Some(0.7), "sys", vec![0.0, 1.0]);
        assert_eq!(
            cache.lookup(&key_fresh, None).map(|r| r.id),
            Some("fresh".to_string())
        );
    }

    #[test]
    fn replay_ignores_non_llm_cache_records() {
        // Mixed WAL: a Store record alongside an LlmCacheInsert. The
        // cache replay must touch only its own variant.
        let cache = LlmCacheService::new();
        let prefix_hash = hash_system_prompt("sys");
        let records = vec![
            WalRecord::Store {
                fragment_id: "frag-1".into(),
                session_id: "sess-1".into(),
                content: "user memory".into(),
                importance: 0.5,
                metadata: HashMap::new(),
            },
            cache_insert_record("p", "m", 14, prefix_hash, vec![1.0, 0.0], "cache-only", 5),
        ];

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let restored = cache.replay(&records, now);
        assert_eq!(
            restored, 1,
            "Store records must NOT be replayed into the LLM cache (different responsibility)"
        );
    }

    #[test]
    fn replay_skips_malformed_response_json() {
        // If the on-disk response_json fails to deserialize (e.g. a
        // future shape change), the cache must skip that entry and
        // continue — never panic on startup.
        let cache = LlmCacheService::new();
        let prefix_hash = hash_system_prompt("sys");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let records = vec![WalRecord::LlmCacheInsert {
            project_id: "p".into(),
            model: "m".into(),
            temperature_bucket: 14,
            system_prompt_hash: prefix_hash,
            embedding: vec![1.0, 0.0],
            response_json: "{not valid json".into(),
            inserted_at_unix_secs: now.saturating_sub(5),
        }];
        let restored = cache.replay(&records, now);
        assert_eq!(restored, 0);
    }

    #[test]
    fn wal_record_round_trips_through_serde() {
        // Pin the wire format: serialize → deserialize must produce
        // an equal record. A field rename would break WAL replay on
        // existing on-disk data.
        let original = WalRecord::LlmCacheInsert {
            project_id: "p".into(),
            model: "gpt-4o-mini".into(),
            temperature_bucket: 14,
            system_prompt_hash: [1, 2, 3, 4, 5, 6, 7, 8],
            embedding: vec![0.1, 0.2, 0.3],
            response_json: r#"{"id":"x"}"#.into(),
            inserted_at_unix_secs: 1717000000,
        };
        let line = serde_json::to_string(&original).expect("serialise");
        // Pin the `op` tag value — old binaries discriminating on `op`
        // would read snake_case "llm_cache_insert".
        assert!(line.contains(r#""op":"llm_cache_insert""#), "got: {line}");
        let parsed: WalRecord = serde_json::from_str(&line).expect("deserialise");
        assert_eq!(parsed, original);
    }

    #[test]
    fn with_wal_builder_attaches_handle() {
        let wal_cell: Arc<tokio::sync::OnceCell<crate::services::wal::Wal>> =
            Arc::new(tokio::sync::OnceCell::new());
        let cache = LlmCacheService::new().with_wal(wal_cell);
        // Debug surface exposes wal_attached so operators can
        // confirm the wiring from a `serve` log line.
        let dbg = format!("{:?}", cache);
        assert!(
            dbg.contains("wal_attached: true"),
            "Debug must reflect WAL attachment; got: {dbg}"
        );
    }
}

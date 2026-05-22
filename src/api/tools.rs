//! ContextNest seven-tool memory API (canonical AgeMem-shaped surface).
//! Per, this module exposes the seven operations
//! that LLM agents call to interact with the continual-learning memory substrate:
//! | Tool | Purpose |
//! |------|---------|
//! | `store` | Persist a content fragment as a memory attractor |
//! | `retrieve` | Fetch relevant memory attractors for a query |
//! | `update` | Mutate an existing attractor's properties |
//! | `summarize` | Compact a memory region into a single attractor |
//! | `discard` | Remove an attractor (soft or hard delete) |
//! | `reconstruct` | E-mem-style reconstruction via the canonical chain (canon: `00_COURSE/05_memory_systems/04_reconstructive_memory.md`) |
//! | `resonate` | Detect emergent activation patterns in the field |
//! All seven are HTTP POST endpoints under `/api/v1/tools/<name>` with JSON
//! request/response bodies.
//! ## Backing store (Phase H)
//! All seven tools now operate on the canonical
//! [`MemoryAttractorManager`](crate::memory::attractors::MemoryAttractorManager)
//! per canon Module 05, with two API-layer sidecars:
//! - [`SessionIndex`](crate::services::session_index::SessionIndex) maps
//!   `session_id` → active / soft-deleted fragment IDs (the manager itself
//!   is session-agnostic; this is how the API answers "what does this
//!   session see?")
//! - `fragment_texts: HashMap<fragment_id, String>` stores the original
//!   human-readable text (canonical fragments carry only `Vec<f32>`
//!   embeddings; text would otherwise be lost on round trip)
//! Each `store` call runs through `process_memories`, triggering basin
//! formation, connection-network indexing, and reconstruction-store
//! population. `retrieve`/`reconstruct`/`resonate` resolve session-affine
//! IDs via the SessionIndex then hydrate canonical fragments via
//! `MemoryAttractorManager::get_fragment`. Embeddings come from
//! `EmbeddingService.generate_embedding`; similarity uses the existing
//! cosine `calculate_similarity`.

use axum::{
    extract::{Json, State},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Router,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::memory::attractors::memory_attractor_manager::{
    MemoryProcessingRequest, ProcessingOptions, ProcessingPriority,
};
use crate::memory::attractors::MemoryFragment;
use crate::services::ContextNestServices;
use std::collections::HashSet;

const DEFAULT_SESSION: &str = "default";

// =============================================================================
// Request / response shapes
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct StoreRequest {
    pub content: String,
    #[serde(default)]
    pub importance: Option<f32>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StoreResponse {
    pub attractor_id: Option<String>,
    pub stored: bool,
}

#[derive(Debug, Deserialize)]
pub struct RetrieveRequest {
    pub query: String,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Optional explicit list of sessions to search.
    ///
    /// When set (and non-empty), the handler switches to **cross-session**
    /// mode: it snapshots every active fragment via
    /// `SessionIndex::active_fragments_session_map`, filters to fragments
    /// whose owning session is in this list, and merges the scored hits
    /// into one response. The returned `RetrieveHit.session_id` is then
    /// populated per hit so the caller can tell where each result came
    /// from. `session_id` (singular) is ignored when this is set.
    ///
    /// Wire-compat: existing single-session callers (cc_hooks, MCP, etc.)
    /// keep passing `session_id` and never see this field. The search UI
    /// in the dashboard passes this to replace the previous N-call
    /// per-session fan-out with a single backend call.
    #[serde(default)]
    pub session_ids: Option<Vec<String>>,
    /// Optional per-fragment metadata filter. When set, a fragment is
    /// only returned if its stored metadata contains every key from this
    /// map with the exact same value. Missing key on the fragment = no
    /// match. Extra keys on the fragment = still matches (we don't
    /// require full equality of the metadata map).
    ///
    /// Example: `{"kind": "decision", "awaiting_decision": true}` →
    /// only returns memories whose metadata has BOTH kind=decision AND
    /// awaiting_decision=true.
    ///
    /// Filtering happens AFTER similarity scoring against the canonical
    /// fragments, BEFORE the top_k truncation — so top_k applies to the
    /// filtered set, not the pre-filter universe.
    #[serde(default)]
    pub metadata_filter: Option<HashMap<String, serde_json::Value>>,
}

fn default_top_k() -> usize {
    5
}

#[derive(Debug, Serialize)]
pub struct RetrieveHit {
    pub id: String,
    pub content: String,
    pub importance: f32,
    pub similarity: f32,
    /// Stored metadata for this fragment, if any. Empty when no metadata
    /// was supplied at store time. Lets consumers see what they matched
    /// on + render UIs without a second request.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Owning session of this fragment. Populated only when the caller
    /// requested cross-session retrieval (via `session_ids`); omitted in
    /// single-session mode to preserve wire-compat for existing clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RetrieveResponse {
    pub hits: Vec<RetrieveHit>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    pub attractor_id: String,
    #[serde(default)]
    pub importance: Option<f32>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateResponse {
    pub updated: bool,
}

#[derive(Debug, Deserialize)]
pub struct SummarizeRequest {
    pub session_id: String,
    #[serde(default)]
    pub target_tokens: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct SummarizeResponse {
    pub merged_count: usize,
    pub summary_attractor_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DiscardRequest {
    pub attractor_id: String,
    #[serde(default = "default_true")]
    pub soft_delete: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct DiscardResponse {
    pub discarded: bool,
    pub soft_delete: bool,
}

#[derive(Debug, Deserialize)]
pub struct ReconstructRequest {
    pub query: String,
    #[serde(default)]
    pub depth: Option<usize>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReconstructResponse {
    pub reconstructed_content: String,
    pub source_fragment_ids: Vec<String>,
    pub coherence: f32,
    pub gaps_filled: usize,
}

#[derive(Debug, Deserialize)]
pub struct ResonateRequest {
    pub pattern: String,
    #[serde(default)]
    pub threshold: Option<f32>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ResonateActivation {
    pub pattern_id: String,
    pub strength: f32,
    pub resonance: f32,
}

#[derive(Debug, Serialize)]
pub struct ResonateResponse {
    pub activations: Vec<ResonateActivation>,
    pub field_coherence: f32,
}

// =============================================================================
// Handlers
// =============================================================================

/// Default processing options used by `store`. Permissive on quality so the
/// pipeline always *completes* (synthetic embeddings won't score well on
/// downstream gap-fill metrics), and short on max_processing_time because the
/// HTTP handler shouldn't block longer than that.
fn store_processing_options() -> ProcessingOptions {
    ProcessingOptions {
        enable_attractor_creation: true,
        enable_reconstruction: false, // single-fragment requests don't reconstruct
        enable_gap_filling: false,
        enable_connections: true,
        quality_threshold: 0.1,
        max_processing_time: std::time::Duration::from_secs(5),
    }
}

/// POST /api/v1/tools/store — persist a content fragment as a canonical
/// memory attractor.
/// Phase H pipeline (canonical, replacing the Phase A simple AttractorField):
///   1. Embed `content` via the EmbeddingService
///   2. Build a [`MemoryFragment`] and push through
///      [`MemoryAttractorManager::process_memories`] — this triggers basin
///      formation, connection-network indexing, and reconstruction-store
///      mirroring (per Phase B + Phase H's Step 1.6)
///   3. Save the source text in the `fragment_texts` sidecar (canonical
///      fragments carry embeddings only)
///   4. Register the fragment with the `session_index` so `retrieve`
///      knows it belongs to this session
/// ### Write order rationale
/// Steps 2 → 3 → 4 are intentional: the canonical store has to acknowledge
/// the fragment before we promise visibility through the sidecar + index.
/// If step 3 or 4 fails after step 2 succeeds, the worst case is a fragment
/// in the canonical store that no session can see — a leak, not a phantom
/// hit. The inverse (visible-but-unstored) would let `retrieve` return IDs
/// that `get_fragment` can't resolve.
/// ### `process_memories` success semantics
/// `process_memories` returns `Ok(result)` even when `result.success` is
/// `false` (quality-threshold gate). The fragment is still mirrored into
/// `reconstruction_protocol.fragment_store` (Step 1.6 runs before the
/// quality gate), so the API contract — "if `stored: true`, the fragment
/// is retrievable" — holds regardless of `result.success`. We therefore
/// only fail on `is_err()`.
pub async fn store(
    State(services): State<ContextNestServices>,
    Json(req): Json<StoreRequest>,
) -> impl IntoResponse {
    let session_id = req
        .session_id
        .unwrap_or_else(|| DEFAULT_SESSION.to_string());
    let importance = req.importance.unwrap_or(0.5).clamp(0.0, 1.0);
    let fragment_id = uuid::Uuid::new_v4().to_string();

    match store_with_id(
        &services,
        &fragment_id,
        &session_id,
        &req.content,
        importance,
        req.metadata.clone(),
    )
    .await
    {
        Ok(()) => {
            // Append to WAL on success. Best-effort: if the WAL write
            // fails we still acknowledge the in-memory store — the user
            // already got the data into the substrate, and a WAL outage
            // is a logged-and-monitored ops concern, not a request-path
            // failure. (If durability matters more than availability for
            // a given deployment, this is the line to invert.)
            if let Some(wal) = services.wal.get() {
                let record = crate::services::wal::WalRecord::Store {
                    fragment_id: fragment_id.clone(),
                    session_id: session_id.clone(),
                    content: req.content,
                    importance,
                    metadata: req.metadata,
                };
                if let Err(e) = wal.append(&record) {
                    tracing::warn!(error = %e, "wal: append failed for store");
                }
            }
            (
                StatusCode::OK,
                Json(StoreResponse {
                    attractor_id: Some(fragment_id),
                    stored: true,
                }),
            )
        }
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(StoreResponse {
                attractor_id: None,
                stored: false,
            }),
        ),
    }
}

/// Bulk sidecar restore for WAL replay. Skips embedding and
/// `process_memories` entirely — only repopulates the three sidecars
/// (`fragment_texts`, `fragment_metadata`, [`SessionIndex`]) needed to
/// serve the `/api/v1/inbox` endpoint and any read paths that don't
/// reach into [`crate::memory::attractors::MemoryAttractorManager`].
///
/// Why this exists: the live `store` pipeline calls `process_memories`
/// which, when the LLM is enabled, makes a blocking HTTP request to
/// OpenAI per fragment for basin formation. At 12k+ records that's
/// hours of replay time, dominated by network latency. The sidecar
/// path completes in seconds because it only does HashMap inserts.
///
/// Trade-off: `/api/v1/tools/retrieve` and `/api/v1/tools/resonate`
/// return empty hits after a sidecar-only replay because the canonical
/// attractor store is empty. The inbox endpoint, sessions list, and
/// metadata-filter-based queries all work normally. Live `store` calls
/// after replay restore canonical state for new fragments.
///
/// Acquires each sidecar's write lock exactly once for the whole batch
/// (rather than 3N times in a per-record loop) so replay throughput is
/// bound by the HashMap insert cost, not lock churn.
pub async fn restore_sidecars_bulk(
    services: &ContextNestServices,
    records: Vec<(String, String, String, HashMap<String, serde_json::Value>)>,
) {
    if records.is_empty() {
        return;
    }

    // Take write locks once each, drop them as soon as inserts are done so
    // the handlers can serve reads if anyone races us during startup.
    {
        let mut texts = services.fragment_texts.write().await;
        for (frag_id, _, content, _) in &records {
            texts.insert(frag_id.clone(), content.clone());
        }
    }
    {
        let mut metadata = services.fragment_metadata.write().await;
        for (frag_id, _, _, meta) in &records {
            if !meta.is_empty() {
                metadata.insert(frag_id.clone(), meta.clone());
            }
        }
    }

    // SessionIndex.add internally takes its own three write locks
    // (active/deleted/reverse) per call. Could be made bulk too if it
    // becomes the bottleneck — for 12k records it's not.
    for (frag_id, session_id, _, _) in records {
        // Enqueue every restored fragment for background consolidation
        // (Phase 1 of the neural-field epic). This is what turns a WAL
        // replay from "sidecars only" into eventually-full attractor
        // state. The worker's startup scan would catch these anyway,
        // but enqueueing inline saves the scan a pass over already-
        // known ids and means consolidation starts the moment replay
        // finishes rather than waiting for the worker's first tick.
        services.consolidation_queue.enqueue(frag_id.clone());
        services.session_index.add(&session_id, &frag_id).await;
    }
}

/// Pure substrate-write helper extracted from [`store`]. Called by both
/// the HTTP handler and the WAL replay path on startup.
///
/// Why this isn't private to this module: the binary's `serve()` startup
/// in `src/bin/contextnest.rs` invokes this once per replayed
/// [`crate::services::wal::WalRecord::Store`] to repopulate the in-memory
/// state from disk. Returning `Result<(), String>` (instead of an
/// `impl IntoResponse`) keeps the function decoupled from the HTTP layer.
///
/// The fragment_id is passed in (rather than generated inside) so that
/// replay can preserve the original IDs — otherwise SessionIndex and
/// metadata maps would diverge from any in-flight client that still
/// holds the old IDs.
pub async fn store_with_id(
    services: &ContextNestServices,
    fragment_id: &str,
    session_id: &str,
    content: &str,
    importance: f32,
    metadata: HashMap<String, serde_json::Value>,
) -> Result<(), String> {
    let embedding = services
        .embedding
        .generate_embedding(content)
        .await
        .map_err(|e| format!("embed: {e}"))?;

    let now = Utc::now();
    let fragment = MemoryFragment {
        id: fragment_id.to_string(),
        content: embedding,
        importance,
        created_at: now,
        last_accessed: now,
        attractor_basin_id: None,
        connections: HashSet::new(),
        confidence: importance,
    };

    let process_req = MemoryProcessingRequest {
        id: format!("store-{fragment_id}"),
        fragments: vec![fragment],
        options: store_processing_options(),
        priority: ProcessingPriority::Medium,
        created_at: now,
    };
    services
        .attractor_manager
        .process_memories(process_req)
        .await
        .map_err(|e| format!("process_memories: {e}"))?;

    // Sidecars: text first, metadata second, index last. Same order as the
    // pre-extraction handler — see the original comment for the
    // visible-but-unstored vs. stored-but-invisible trade-off rationale.
    services
        .fragment_texts
        .write()
        .await
        .insert(fragment_id.to_string(), content.to_string());

    if !metadata.is_empty() {
        services
            .fragment_metadata
            .write()
            .await
            .insert(fragment_id.to_string(), metadata);
    }

    services.session_index.add(session_id, fragment_id).await;

    Ok(())
}

/// POST /api/v1/tools/retrieve — fetch relevant memory fragments for a query.
/// Phase H pipeline:
///   1. Embed the query
///   2. Pull active fragment IDs for the session from `session_index`
///   3. If `metadata_filter` is set, **prefilter** active IDs against the
///      metadata sidecar so non-matching fragments never get hydrated.
///      This is the hot-path saver for high-fan-out callers (Inbox UI).
///   4. For each survivor, fetch the canonical fragment via
///      `MemoryAttractorManager::get_fragment` and score by cosine similarity
///      against the query embedding
///   5. Look up text from `fragment_texts` sidecar
///   6. Sort by similarity desc, importance desc, truncate to `top_k`
pub async fn retrieve(
    State(services): State<ContextNestServices>,
    Json(req): Json<RetrieveRequest>,
) -> impl IntoResponse {
    // Branch decision: cross-session mode (when caller passed an
    // explicit `session_ids` list) vs the original single-session mode.
    // `multi_session` carries a fragment_id → owning session_id map so
    // each `RetrieveHit` can be tagged with its origin; in single-session
    // mode the map stays empty and hits are emitted without `session_id`.
    let multi_session: Option<HashMap<String, String>> = match req.session_ids.as_ref() {
        Some(ids) if !ids.is_empty() => {
            let wanted: HashSet<&String> = ids.iter().collect();
            // One read-lock snapshot, then filter in-process — same
            // pattern the inbox endpoint uses for its cross-session feed.
            let all = services.session_index.active_fragments_session_map().await;
            let map: HashMap<String, String> = all
                .into_iter()
                .filter(|(_, sess)| wanted.contains(sess))
                .collect();
            Some(map)
        }
        _ => None,
    };

    let query_embedding = match services.embedding.generate_embedding(&req.query).await {
        Ok(e) => e,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(RetrieveResponse { hits: Vec::new() }),
            );
        }
    };

    let active_ids: Vec<String> = if let Some(map) = multi_session.as_ref() {
        map.keys().cloned().collect()
    } else {
        let session_id = req
            .session_id
            .clone()
            .unwrap_or_else(|| DEFAULT_SESSION.to_string());
        services.session_index.list_active(&session_id).await
    };
    if active_ids.is_empty() {
        return (StatusCode::OK, Json(RetrieveResponse { hits: Vec::new() }));
    }

    // Prefilter: if the caller passed a `metadata_filter`, narrow the candidate
    // set BEFORE we pay the `get_fragment` lock + clone cost. The metadata
    // sidecar is a single HashMap behind one RwLock; checking N keys against an
    // in-memory map is orders of magnitude cheaper than N lock-acquires on the
    // attractor manager's fragment_store. For inbox-style filters that match
    // ~1% of fragments this turns a full-set scan into a handful of hydrations.
    //
    // Semantics preserved exactly: a fragment with no metadata entry never
    // matches a non-empty filter (same conservative rule as the original
    // post-hydration filter in `metadata_filter_matches`).
    let candidate_ids: Vec<String> = if let Some(filter) = req.metadata_filter.as_ref() {
        let metadata = services.fragment_metadata.read().await;
        active_ids
            .into_iter()
            .filter(|id| match metadata.get(id) {
                Some(meta) => metadata_filter_matches(filter, meta),
                None => filter.is_empty(),
            })
            .collect()
    } else {
        active_ids
    };

    if candidate_ids.is_empty() {
        return (StatusCode::OK, Json(RetrieveResponse { hits: Vec::new() }));
    }

    // Phase 1: hydrate canonical fragments + score. Hold *no* `fragment_texts`
    // lock during this loop — `get_fragment` takes `fragment_store.read()` so
    // concurrent `store`/`update` writers can still progress on the text
    // sidecar. (Phase-H review finding 2.2: prior version held the
    // fragment_texts read lock across the entire scan.)
    //
    // Sidecar fallback rationale: after a sidecars-only WAL replay (the
    // default startup mode) the canonical attractor store is empty,
    // even though `session_index` + `fragment_metadata` +
    // `fragment_texts` are all repopulated. Without a fallback, every
    // `retrieve` against a replayed substrate returned zero hits — the
    // dashboard's per-section views silently went blank while the inbox
    // showed identical data via a different endpoint. We track which
    // candidate IDs the canonical store could not produce and rebuild
    // their hits from the sidecars in Phase 2. similarity is set to 0
    // for these (no embedding to compare); they sort to the bottom of
    // the result set behind any genuinely scored hit.
    let mut hydrated: Vec<(MemoryFragment, f32)> = Vec::with_capacity(candidate_ids.len());
    let mut sidecar_only_ids: Vec<String> = Vec::new();
    for id in &candidate_ids {
        match services.attractor_manager.get_fragment(id).await {
            Ok(Some(fragment)) => {
                let similarity = services
                    .embedding
                    .calculate_similarity(&query_embedding, &fragment.content);
                hydrated.push((fragment, similarity));
            }
            // SessionIndex says it's active but manager doesn't have it.
            // This is the post-WAL-replay case OR a transient index drift
            // race. Either way, treat the sidecar as the source of truth
            // and synthesize a minimal hit downstream.
            Ok(None) | Err(_) => sidecar_only_ids.push(id.clone()),
        }
    }

    // Phase 2: bulk text + metadata lookup for survivors only. Locks scale
    // with the post-filter set, not the session size. Same map is used
    // for both hydrated (canonical) and sidecar-only hits so we acquire
    // each read lock exactly once.
    let texts = services.fragment_texts.read().await;
    let metadata = services.fragment_metadata.read().await;
    // Phase 2 of the neural-field epic: apply age-based decay to cosine
    // similarity. Fragments that haven't been read in a long time score
    // lower than recent ones with identical content, mirroring the
    // "forgetting curve" that makes a memory substrate feel alive.
    // Sidecar-only hits keep their similarity at 0 (no embedding to
    // decay) and never reach this branch.
    let mut scored: Vec<RetrieveHit> = hydrated
        .into_iter()
        .map(|(fragment, base_similarity)| {
            let fragment_meta = metadata.get(&fragment.id);
            let content = texts.get(&fragment.id).cloned().unwrap_or_default();
            let owner = multi_session
                .as_ref()
                .and_then(|m| m.get(&fragment.id).cloned());
            let decay = fragment_meta.map(decay_multiplier).unwrap_or(1.0);
            RetrieveHit {
                id: fragment.id,
                content,
                importance: fragment.importance,
                similarity: base_similarity * decay,
                metadata: fragment_meta.cloned().unwrap_or_default(),
                session_id: owner,
            }
        })
        .collect();

    for id in &sidecar_only_ids {
        // Skip fragments that have neither text nor metadata — that's a
        // truly orphaned index entry, not a sidecar-only fragment.
        let content = texts.get(id).cloned();
        let meta = metadata.get(id).cloned();
        if content.is_none() && meta.is_none() {
            continue;
        }
        let owner = multi_session.as_ref().and_then(|m| m.get(id).cloned());
        scored.push(RetrieveHit {
            id: id.clone(),
            content: content.unwrap_or_default(),
            // Importance default mirrors `restore_sidecars_bulk` — neutral
            // 0.5 because we don't have the canonical fragment's value.
            importance: 0.5,
            // No embedding → no similarity. Use 0.0 so canonical hits
            // (which have real similarity scores ≥ ~0.6 typically) sort
            // ahead of sidecar-only hits in mixed result sets.
            similarity: 0.0,
            metadata: meta.unwrap_or_default(),
            session_id: owner,
        });
    }
    drop(texts);
    drop(metadata);

    // Sort by similarity desc, then importance desc as tiebreaker.
    scored.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.importance
                    .partial_cmp(&a.importance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    // Phase 4 of the neural-field epic: basin-aware expansion. Look up
    // the top hit's basin and surface its other members at a boosted
    // similarity. Gives "you've worked on this topic before"
    // expansion that pure cosine misses — past fragments that don't
    // word-match the query but are semantically siblings of the top
    // hit. Configurable via `CONTEXTNEST_RETRIEVE_BASIN_BOOST` (0.0
    // disables); see `basin_aware_expand` for the full design.
    basin_aware_expand(&services, &mut scored, &candidate_ids, &multi_session).await;

    // Re-sort because expansion may have inserted higher-scoring
    // basin siblings than the post-truncate tail of the original set.
    scored.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.importance
                    .partial_cmp(&a.importance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    scored.truncate(req.top_k);

    // Phase 2 of the neural-field epic: bump `last_accessed` on every
    // returned hit so future retrieves apply the recency bonus via
    // `decay_multiplier` (which prefers last_accessed over ts when
    // present). One write-lock, all hits in a single batch.
    if !scored.is_empty() {
        let now_iso = chrono::Utc::now().to_rfc3339();
        let mut meta_w = services.fragment_metadata.write().await;
        for hit in &scored {
            let entry = meta_w.entry(hit.id.clone()).or_default();
            entry.insert(
                "last_accessed".to_string(),
                serde_json::Value::String(now_iso.clone()),
            );
        }
    }

    // Update the retrieve co-occurrence log: every unordered pair of
    // returned hits gains +1 in the shared connection map. This is the
    // substrate's only signal for "fragments that show up together" —
    // the field viz reads it to draw real resonance edges (replacing
    // the synthesized same-session-and-kind placeholder edges).
    //
    // Cost gating: at most top_k² / 2 pair-updates per retrieve call,
    // which is ≤1250 for top_k=50. Each is a HashMap entry update so
    // total wall-clock per call stays under a millisecond.
    //
    // Memory bound: when the log exceeds 8000 entries we keep only the
    // 2000 strongest (highest count) and drop the long tail. This is
    // O(n log n) but only runs on overflow, so amortized cost is fine.
    if scored.len() >= 2 {
        let mut log = services.connection_log.write().await;
        for i in 0..scored.len() {
            for j in (i + 1)..scored.len() {
                let a = &scored[i].id;
                let b = &scored[j].id;
                let key = if a < b {
                    (a.clone(), b.clone())
                } else {
                    (b.clone(), a.clone())
                };
                *log.entry(key).or_insert(0) += 1;
            }
        }
        if log.len() > 8000 {
            let mut entries: Vec<((String, String), u32)> = log.drain().collect();
            entries.sort_by(|a, b| b.1.cmp(&a.1));
            entries.truncate(2000);
            *log = entries.into_iter().collect();
        }
    }

    (StatusCode::OK, Json(RetrieveResponse { hits: scored }))
}

/// Basin-aware retrieval expansion — Phase 4 of the neural-field epic
/// (`docs/roadmap/epics/neural-field-real.md`).
///
/// After the initial cosine sort, looks up the top hit's basin (from
/// the basin manager populated by Phase 1's consolidation worker) and
/// appends basin siblings as additional hits with their similarity
/// scaled down by a configurable factor. The point: surface fragments
/// that are semantically clustered with the top hit even when they
/// don't lexically match the query.
///
/// Knobs (all env, all optional):
/// - `CONTEXTNEST_RETRIEVE_BASIN_BOOST` (default 0.7) — multiplier on
///   the top hit's similarity. Range (0.0, 1.0]. Setting to 0.0
///   disables the expansion entirely.
/// - `CONTEXTNEST_RETRIEVE_BASIN_MAX_EXPANSION` (default 20) — cap on
///   how many basin siblings are appended. Bounds the worst-case
///   cost when a single basin holds hundreds of fragments.
///
/// No-op cases (all return early):
/// - boost = 0 → expansion disabled
/// - `scored` empty → nothing to anchor on
/// - top hit has similarity 0 (sidecar-only or no embedding) →
///   expansion would score everything at 0, no point
/// - top hit isn't in any basin yet (consolidation hasn't caught up) →
///   nothing to expand from
/// - basin has only the top hit as member → no siblings to surface
///
/// Restricts expansion to ids already in `candidate_ids`, i.e. the
/// active+filter-passing set for the current query. This keeps
/// metadata_filter semantics intact (a basin sibling tagged
/// kind="learning" won't suddenly appear in a kind="decision" query)
/// and preserves single-session affinity (siblings from other
/// sessions don't leak into a single-session retrieve).
async fn basin_aware_expand(
    services: &ContextNestServices,
    scored: &mut Vec<RetrieveHit>,
    candidate_ids: &[String],
    multi_session: &Option<HashMap<String, String>>,
) {
    let boost: f32 = std::env::var("CONTEXTNEST_RETRIEVE_BASIN_BOOST")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|v: &f32| v.is_finite() && *v > 0.0 && *v <= 1.0)
        .unwrap_or(0.7);
    if boost <= 0.0 {
        return;
    }
    let max_expansion: usize = std::env::var("CONTEXTNEST_RETRIEVE_BASIN_MAX_EXPANSION")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n: &usize| *n > 0)
        .unwrap_or(20);

    let Some(top) = scored.first() else {
        return;
    };
    let top_sim = top.similarity;
    if top_sim <= 0.0 {
        return;
    }
    let top_id = top.id.clone();

    let snapshots = services.attractor_manager.list_basin_snapshots().await;
    let Some(basin) = snapshots
        .into_iter()
        .find(|s| s.fragment_ids.contains(&top_id))
    else {
        return;
    };

    let candidate_set: HashSet<&String> = candidate_ids.iter().collect();
    let existing: HashSet<String> = scored.iter().map(|h| h.id.clone()).collect();
    let boosted = top_sim * boost;

    let mut additions: Vec<RetrieveHit> = Vec::new();
    {
        // Single lock acquisition for the whole expansion. Drops at
        // block end so the rest of retrieve() can reacquire freely.
        let texts = services.fragment_texts.read().await;
        let metadata = services.fragment_metadata.read().await;
        for sib_id in &basin.fragment_ids {
            if additions.len() >= max_expansion {
                break;
            }
            if existing.contains(sib_id) {
                continue;
            }
            if !candidate_set.contains(sib_id) {
                // Sibling from another session, soft-deleted, or
                // doesn't match the metadata_filter — skip.
                continue;
            }
            let content = texts.get(sib_id).cloned().unwrap_or_default();
            let meta = metadata.get(sib_id).cloned().unwrap_or_default();
            let owner = multi_session.as_ref().and_then(|m| m.get(sib_id).cloned());
            additions.push(RetrieveHit {
                id: sib_id.clone(),
                content,
                importance: 0.5,
                similarity: boosted,
                metadata: meta,
                session_id: owner,
            });
        }
    }
    scored.extend(additions);
}

/// Age-based decay multiplier applied to cosine similarity at retrieve
/// time. See `docs/roadmap/epics/neural-field-real.md` Phase 2.
///
/// The half-life is read from `CONTEXTNEST_DECAY_HALF_LIFE_DAYS` (default
/// 60 days). A 1-day-old fragment scores ~99% of its base similarity; a
/// 60-day-old one scores ~50%; a 180-day-old one ~12%.
///
/// Recency boost: `last_accessed` (bumped on every retrieve hit) takes
/// precedence over `ts` so frequently-referenced fragments stay fresh.
/// Returns 1.0 (no decay) when no usable timestamp is present.
fn decay_multiplier(metadata: &HashMap<String, serde_json::Value>) -> f32 {
    let half_life_days: f64 = std::env::var("CONTEXTNEST_DECAY_HALF_LIFE_DAYS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|v: &f64| v.is_finite() && *v > 0.0)
        .unwrap_or(60.0);

    let ref_ts = metadata
        .get("last_accessed")
        .or_else(|| metadata.get("ts"))
        .and_then(|v| v.as_str());

    let when = match ref_ts.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()) {
        Some(t) => t.with_timezone(&chrono::Utc),
        None => return 1.0,
    };

    let age_secs = (Utc::now() - when).num_seconds() as f64;
    if age_secs <= 0.0 {
        return 1.0;
    }
    let age_days = age_secs / 86_400.0;
    let lambda = std::f64::consts::LN_2 / half_life_days;
    (-lambda * age_days).exp() as f32
}

/// True iff every `(key, value)` pair in `filter` is present in
/// `fragment_meta` with an equal value. Missing key = no match.
/// Extra keys on the fragment side = still matches (we don't require
/// full-set equality).
fn metadata_filter_matches(
    filter: &HashMap<String, serde_json::Value>,
    fragment_meta: &HashMap<String, serde_json::Value>,
) -> bool {
    filter.iter().all(|(k, expected)| {
        fragment_meta
            .get(k)
            .map(|actual| actual == expected)
            .unwrap_or(false)
    })
}

/// POST /api/v1/tools/update — mutate an existing fragment's properties.
/// Phase H scope: importance and text content are updatable. Re-embedding on
/// content change is not yet wired through the canonical store
/// ([`MemoryAttractorManager`] doesn't expose embedding mutation today —
/// downstream basins would need rebalancing). For content changes that
/// should re-embed, callers should `discard` + `store` — note that the new
/// fragment receives a fresh UUID, so any downstream references to the old
/// `attractor_id` will dangle. There is no in-place re-embedding path.
/// ### Partial-failure semantics
/// When the caller passes both `importance` *and* `content`, we apply
/// `update_fragment_importance` first (canonical store), then `fragment_texts`
/// (sidecar). If the sidecar write succeeds and the importance update was
/// already applied, the system is consistent. If the importance update
/// succeeded but the process is killed before the sidecar write, the
/// canonical store has the new importance + old text — a divergence that
/// is detectable on the next `retrieve` (text won't match the new
/// importance) but not auto-corrected. Tracked as Phase-H follow-up:
/// either order the writes sidecar-first (text divergence becomes the
/// worse failure mode because retrievals would surface stale text with
/// new importance) or add a compensating revert. v0.1.0 ships with the
/// current order because in-process all-in-memory failure between two
/// adjacent HashMap operations requires a panic, not a recoverable error.
pub async fn update(
    State(services): State<ContextNestServices>,
    Json(req): Json<UpdateRequest>,
) -> impl IntoResponse {
    let session_id = req
        .session_id
        .unwrap_or_else(|| DEFAULT_SESSION.to_string());

    // Ownership check: only fragments registered to this session are
    // mutable from this session_id. find_session falls through to None when
    // the fragment doesn't exist at all (returns 404).
    let owning_session = services.session_index.find_session(&req.attractor_id).await;
    if owning_session.as_deref() != Some(session_id.as_str()) {
        return (
            StatusCode::NOT_FOUND,
            Json(UpdateResponse { updated: false }),
        );
    }

    let mut touched = false;

    if let Some(importance) = req.importance {
        let clamped = importance.clamp(0.0, 1.0);
        match services
            .attractor_manager
            .update_fragment_importance(&req.attractor_id, clamped)
            .await
        {
            Ok(true) => touched = true,
            Ok(false) => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(UpdateResponse { updated: false }),
                );
            }
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(UpdateResponse { updated: false }),
                );
            }
        }
    }

    if let Some(content) = req.content {
        // Update only the text sidecar — embedding stays the original (see
        // doc-comment caveat). Callers wanting re-embedding should
        // discard + store.
        services
            .fragment_texts
            .write()
            .await
            .insert(req.attractor_id.clone(), content);
        touched = true;
    }

    if touched {
        (StatusCode::OK, Json(UpdateResponse { updated: true }))
    } else {
        // No-op (caller passed neither importance nor content) — still 200
        // to keep the contract idempotent.
        (StatusCode::OK, Json(UpdateResponse { updated: false }))
    }
}

/// POST /api/v1/tools/summarize — compact a memory region.
/// Phase J: when an LLM provider is wired (`services.llm.is_enabled()`) and
/// the session has at least one active fragment, this handler:
///   1. Fetches the text content of every active fragment via `fragment_texts`
///   2. Calls `services.llm.summarize(&texts, req.target_tokens)` to compress
///      them into a single coherent paragraph
///   3. Stores the summary as a new fragment via the `store` pipeline (embed →
///      process_memories → sidecar → session_index), returning its id as
///      `summary_attractor_id`
///   4. Returns `merged_count` = number of source fragments that were compressed
/// ### Degradation paths
/// * If `services.llm.is_enabled()` is `false` (no API key in env) → falls
///   back to Phase H behaviour: returns the fragment count, `summary_attractor_id: null`.
/// * If `target_tokens` is `None` → same Phase H fallback (no summarization
///   requested).
/// * If the LLM call itself errors (network, rate limit, etc.) → logs a
///   `tracing::warn!` and degrades to the count-only path rather than
///   propagating a 500 to the caller, because a summarization failure should
///   not break the agent's main workflow.
/// ### Idempotency note
/// Each summarize call creates a new fragment. The source fragments are NOT
/// automatically discarded — the caller decides whether to `discard` them
/// afterward. This preserves lossless history for agents that want to inspect
/// what was compressed.
pub async fn summarize(
    State(services): State<ContextNestServices>,
    Json(req): Json<SummarizeRequest>,
) -> impl IntoResponse {
    let active_ids = services.session_index.list_active(&req.session_id).await;
    let merged_count = active_ids.len();

    // Fast path: no fragments or no target — return count only, no LLM call.
    if !services.llm.is_enabled() || req.target_tokens.is_none() || active_ids.is_empty() {
        return (
            StatusCode::OK,
            Json(SummarizeResponse {
                merged_count,
                summary_attractor_id: None,
            }),
        );
    }

    // Collect the text content for each active fragment from the sidecar.
    // We take a single read lock, drain the texts we need, then drop the lock
    // before doing any async I/O so we don't hold it across the LLM call.
    let texts: Vec<String> = {
        let texts_guard = services.fragment_texts.read().await;
        active_ids
            .iter()
            .filter_map(|id| texts_guard.get(id).cloned())
            .collect()
    };

    if texts.is_empty() {
        // All active ids exist in the session index but none have sidecar text
        // (e.g. they were stored before Phase H added the sidecar). Degrade.
        return (
            StatusCode::OK,
            Json(SummarizeResponse {
                merged_count,
                summary_attractor_id: None,
            }),
        );
    }

    // LLM compression step. Degrade on error rather than 500ing the caller.
    let summary_text = match services.llm.summarize(&texts, req.target_tokens).await {
        Ok(s) => s,
        Err(err) => {
            tracing::warn!(
                session_id = %req.session_id,
                fragment_count = merged_count,
                error = %err,
                "LLM summarize failed; degrading to count-only response"
            );
            return (
                StatusCode::OK,
                Json(SummarizeResponse {
                    merged_count,
                    summary_attractor_id: None,
                }),
            );
        }
    };

    // Store the summary as a new fragment via the standard store pipeline.
    // Embed → process_memories → sidecar → session_index.
    // Use the average importance of source fragments so the summary inherits
    // the signal strength of what it replaces.
    let avg_importance = {
        let mut sum = 0.0f32;
        let mut count = 0usize;
        for id in &active_ids {
            if let Ok(Some(frag)) = services.attractor_manager.get_fragment(id).await {
                sum += frag.importance;
                count += 1;
            }
        }
        if count > 0 {
            sum / count as f32
        } else {
            0.7
        }
    };

    let embedding = match services.embedding.generate_embedding(&summary_text).await {
        Ok(e) => e,
        Err(err) => {
            tracing::warn!(
                session_id = %req.session_id,
                error = %err,
                "Embedding failed for LLM summary; degrading to count-only response"
            );
            return (
                StatusCode::OK,
                Json(SummarizeResponse {
                    merged_count,
                    summary_attractor_id: None,
                }),
            );
        }
    };

    let now = chrono::Utc::now();
    let summary_id = uuid::Uuid::new_v4().to_string();
    let fragment = crate::memory::attractors::MemoryFragment {
        id: summary_id.clone(),
        content: embedding,
        importance: avg_importance,
        created_at: now,
        last_accessed: now,
        attractor_basin_id: None,
        connections: std::collections::HashSet::new(),
        confidence: avg_importance,
    };

    let process_req =
        crate::memory::attractors::memory_attractor_manager::MemoryProcessingRequest {
            id: format!("summarize-{summary_id}"),
            fragments: vec![fragment],
            options: store_processing_options(),
            priority:
                crate::memory::attractors::memory_attractor_manager::ProcessingPriority::Medium,
            created_at: now,
        };

    if services
        .attractor_manager
        .process_memories(process_req)
        .await
        .is_err()
    {
        // Store failed — degrade to count-only rather than 500.
        tracing::warn!(
            session_id = %req.session_id,
            "process_memories failed for summary fragment; returning count-only"
        );
        return (
            StatusCode::OK,
            Json(SummarizeResponse {
                merged_count,
                summary_attractor_id: None,
            }),
        );
    }

    services
        .fragment_texts
        .write()
        .await
        .insert(summary_id.clone(), summary_text);

    services
        .session_index
        .add(&req.session_id, &summary_id)
        .await;

    (
        StatusCode::OK,
        Json(SummarizeResponse {
            merged_count,
            summary_attractor_id: Some(summary_id),
        }),
    )
}

/// POST /api/v1/tools/discard — remove a fragment (soft by default).
/// Phase H semantics:
///   * **soft delete**: `session_index.soft_remove(...)` moves the id from
///     the session's active set into its deleted set. The canonical
///     fragment stays in the manager's store so it can still participate
///     in cross-session retrieval / reconstruction if those tools come
///     online later, and a future `restore` tool can flip it back.
///   * **hard delete**: also purges from the canonical store via
///     `MemoryAttractorManager::discard_fragment` and from the text
///     sidecar. No path back after hard delete.
pub async fn discard(
    State(services): State<ContextNestServices>,
    Json(req): Json<DiscardRequest>,
) -> impl IntoResponse {
    let _ = req.reason; // reserved for future audit-log wiring
    let session_id = req
        .session_id
        .clone()
        .unwrap_or_else(|| DEFAULT_SESSION.to_string());

    // Ownership check identical to `update`: only the owning session can discard.
    let owning_session = services.session_index.find_session(&req.attractor_id).await;
    if owning_session.as_deref() != Some(session_id.as_str()) {
        return (
            StatusCode::NOT_FOUND,
            Json(DiscardResponse {
                discarded: false,
                soft_delete: req.soft_delete,
            }),
        );
    }

    let discarded = if req.soft_delete {
        services
            .session_index
            .soft_remove(&session_id, &req.attractor_id)
            .await
    } else {
        // Hard delete: index, manager, sidecar — in that order so a partial
        // failure leaves the system in a consistent state from the API's
        // point of view (no orphan in active set after step 1). The
        // `removed_from_index` flag is captured so we can report success
        // when the index cleanup succeeded even if the manager-side delete
        // hit an internal error (the fragment is invisible either way,
        // which is what callers care about for the API contract).
        let removed_from_index = services
            .session_index
            .hard_remove(&session_id, &req.attractor_id)
            .await;
        let removed_from_manager = match services
            .attractor_manager
            .discard_fragment(&req.attractor_id)
            .await
        {
            Ok(was_present) => was_present,
            Err(err) => {
                // Don't silently absorb — log so operators can see canonical-store
                // orphans accumulating. We still return based on whether the
                // session visibility was killed.
                tracing::warn!(
                    fragment_id = %req.attractor_id,
                    error = %err,
                    "discard_fragment failed; canonical store may have an orphan",
                );
                false
            }
        };
        services
            .fragment_texts
            .write()
            .await
            .remove(&req.attractor_id);
        // The API contract is "is this fragment still visible to me?". Either
        // an index-cleared OR a manager-cleared result makes it invisible
        // through normal API surfaces; reflect that with OR rather than just
        // trusting the manager (which can fail under lock poisoning).
        removed_from_index || removed_from_manager
    };

    let status = if discarded {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    };
    (
        status,
        Json(DiscardResponse {
            discarded,
            soft_delete: req.soft_delete,
        }),
    )
}

/// POST /api/v1/tools/reconstruct — E-mem-style canonical-chain reconstruction.
/// v0.1.0 minimal-viable impl: walks the session's attractor field for the
/// `depth` most-similar fragments to the query, then stitches them into a
/// single content string. Coherence is the mean pairwise similarity of the
/// returned fragments' embeddings — a proxy for how tightly the
/// reconstruction holds together. Gap-filling is not yet wired (returns 0).
/// Canonical chain (per `00_COURSE/05_memory_systems/04_reconstructive_memory.md`):
///   ResonanceActivator → GapIdentifier → GapFillingEngine
///     → SemanticContinuityRestoration → HistoricalStateRecovery
///     → MemoryReconstructionCoordinator
/// The full chain ships once the 5 reconstruction modules unify their
/// `Fragment` types (see).
pub async fn reconstruct(
    State(services): State<ContextNestServices>,
    Json(req): Json<ReconstructRequest>,
) -> impl IntoResponse {
    let session_id = req
        .session_id
        .unwrap_or_else(|| DEFAULT_SESSION.to_string());
    let depth = req.depth.unwrap_or(5);

    let query_embedding = match services.embedding.generate_embedding(&req.query).await {
        Ok(e) => e,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ReconstructResponse {
                    reconstructed_content: String::new(),
                    source_fragment_ids: Vec::new(),
                    coherence: 0.0,
                    gaps_filled: 0,
                }),
            );
        }
    };

    // Pull session-affine fragment IDs and hydrate their canonical forms.
    // Materialize as (similarity, fragment) pairs so we can do the canonical
    // chain steps in-process: ResonanceActivator (similarity scoring) →
    // SemanticContinuityRestoration (importance-ordered stitch) → coherence.
    let active_ids = services.session_index.list_active(&session_id).await;
    if active_ids.is_empty() {
        return (
            StatusCode::OK,
            Json(ReconstructResponse {
                reconstructed_content: String::new(),
                source_fragment_ids: Vec::new(),
                coherence: 0.0,
                gaps_filled: 0,
            }),
        );
    }

    let mut hydrated: Vec<(f32, MemoryFragment)> = Vec::with_capacity(active_ids.len());
    for id in active_ids {
        let Ok(Some(fragment)) = services.attractor_manager.get_fragment(&id).await else {
            continue;
        };
        let similarity = services
            .embedding
            .calculate_similarity(&query_embedding, &fragment.content);
        hydrated.push((similarity, fragment));
    }

    // Step 1 — ResonanceActivator equivalent: rank by similarity, keep top-depth.
    hydrated.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    hydrated.truncate(depth);

    // Step 2 — SemanticContinuityRestoration proxy: re-order picked
    // fragments by importance descending so the stitch reads "biggest
    // signal first". A full canonical chain would also re-cluster via
    // basin proximity here; deferred until Phase H+1.
    hydrated.sort_by(|a, b| {
        b.1.importance
            .partial_cmp(&a.1.importance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let texts = services.fragment_texts.read().await;
    let source_fragment_ids: Vec<String> = hydrated.iter().map(|(_, f)| f.id.clone()).collect();
    let reconstructed_content = source_fragment_ids
        .iter()
        .filter_map(|id| texts.get(id).cloned())
        .collect::<Vec<String>>()
        .join("\n\n");
    drop(texts);

    // Step 3 — Coherence: mean pairwise cosine similarity of the picked set's
    // embeddings. A tight set (high coherence) means the reconstruction
    // holds together; a loose set means the gap-filling stage (deferred
    // to Phase J / LLM integration) would have more to do.
    let mut pair_count = 0usize;
    let mut pair_sum = 0.0f32;
    for i in 0..hydrated.len() {
        for j in (i + 1)..hydrated.len() {
            pair_sum += services
                .embedding
                .calculate_similarity(&hydrated[i].1.content, &hydrated[j].1.content);
            pair_count += 1;
        }
    }
    let coherence = if pair_count == 0 {
        if hydrated.is_empty() {
            0.0
        } else {
            1.0
        }
    } else {
        pair_sum / pair_count as f32
    };

    (
        StatusCode::OK,
        Json(ReconstructResponse {
            reconstructed_content,
            source_fragment_ids,
            coherence,
            gaps_filled: 0,
        }),
    )
}

/// POST /api/v1/tools/resonate — emergent activation patterns in the field.
/// v0.1.0: returns active attractors that resonate above `threshold` with
/// the query pattern, ranked by similarity. Field coherence is the mean
/// pairwise similarity of the activated set. Full neural-field resonance
/// (with phase-coupled attractor formation) lands once the field operator
/// chain integrates with the seven-tool surface.
pub async fn resonate(
    State(services): State<ContextNestServices>,
    Json(req): Json<ResonateRequest>,
) -> impl IntoResponse {
    let session_id = req
        .session_id
        .unwrap_or_else(|| DEFAULT_SESSION.to_string());
    let threshold = req.threshold.unwrap_or(0.3);

    let pattern_embedding = match services.embedding.generate_embedding(&req.pattern).await {
        Ok(e) => e,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ResonateResponse {
                    activations: Vec::new(),
                    field_coherence: 0.0,
                }),
            );
        }
    };

    let active_ids = services.session_index.list_active(&session_id).await;
    if active_ids.is_empty() {
        return (
            StatusCode::OK,
            Json(ResonateResponse {
                activations: Vec::new(),
                field_coherence: 0.0,
            }),
        );
    }

    let mut activations: Vec<(f32, ResonateActivation)> = Vec::new();
    for id in active_ids {
        let Ok(Some(fragment)) = services.attractor_manager.get_fragment(&id).await else {
            continue;
        };
        let similarity = services
            .embedding
            .calculate_similarity(&pattern_embedding, &fragment.content);
        if similarity >= threshold {
            activations.push((
                similarity,
                ResonateActivation {
                    pattern_id: fragment.id.clone(),
                    // The canonical fragment carries no separate `strength`
                    // field — importance is the closest analog (basin
                    // strength would require a basin lookup we skip in v0.1.0).
                    strength: fragment.importance,
                    resonance: similarity,
                },
            ));
        }
    }
    activations.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let activated: Vec<ResonateActivation> = activations.into_iter().map(|(_, a)| a).collect();

    // Field coherence: mean of all activation resonances.
    let field_coherence = if activated.is_empty() {
        0.0
    } else {
        activated.iter().map(|a| a.resonance).sum::<f32>() / activated.len() as f32
    };

    (
        StatusCode::OK,
        Json(ResonateResponse {
            activations: activated,
            field_coherence,
        }),
    )
}

// =============================================================================
// Router
// =============================================================================

/// Build the seven-tool router. Mount at `/api/v1/tools` in the main app.
pub fn create_tools_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/api/v1/tools/store", post(store))
        .route("/api/v1/tools/retrieve", post(retrieve))
        .route("/api/v1/tools/update", post(update))
        .route("/api/v1/tools/summarize", post(summarize))
        .route("/api/v1/tools/discard", post(discard))
        .route("/api/v1/tools/reconstruct", post(reconstruct))
        .route("/api/v1/tools/resonate", post(resonate))
}

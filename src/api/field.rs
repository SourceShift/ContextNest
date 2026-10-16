//! Field-view backend — three endpoints powering the /field UI:
//!
//! - `GET /api/v1/fragments` — bulk fragment listing, optionally with
//!   embeddings, used by the client-side PCA layout.
//! - `GET /api/v1/field/basins` — basin centroids, ranked by
//!   scope-filtered mass and capped by `limit` (default 40). Real
//!   centroids come from [`MemoryAttractorManager`]; when it's empty
//!   (post-sidecar replay), we fall back to project-derived centroids
//!   computed as the mean of contributing fragment embeddings. See
//!   [`BasinsQuery`] for the `min_mass` / `limit` contract.
//! - `GET /api/v1/connections` — retrieve co-occurrence edges. The
//!   ConnectionLog in services accumulates fragment-pair counts every
//!   time a `/retrieve` call returns multiple hits; this endpoint
//!   surfaces the top-K co-occurrence pairs for visualization.
//!
//! ## Design note: embeddings on sidecar-only fragments
//!
//! After a sidecars-only WAL replay (the default), most fragments
//! exist in `fragment_texts` / `fragment_metadata` / `session_index`
//! but NOT in `MemoryAttractorManager` (the canonical store with
//! embeddings). To still serve embeddings to the field client, we
//! re-embed the sidecar text on-demand via `EmbeddingService`. This
//! is cheap because the embedding service caches by content hash —
//! repeated requests don't re-compute.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::memory::attractors::attractor_basin::BasinSnapshot;
use crate::services::ContextNestServices;

// How many fragments we may embed in parallel.
//
// For a local TF-IDF embedder, 32+ is fine — the EmbeddingService's
// internal RwLock starts to dominate past that. For a network-backed
// embedder (DeepInfra / OpenAI), 16 keeps us under typical
// rate-limit ceilings while still getting a 10-15× speedup over the
// sequential baseline.
const EMBED_CONCURRENCY: usize = 16;

// =============================================================================
// /api/v1/fragments
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct FragmentsQuery {
    /// Required when `with_embedding=true` to bound the cost of re-embedding;
    /// optional otherwise — without it we walk every active fragment.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Filter by project — matched against the basename of
    /// `metadata.project_cwd`. So `project=researcher` matches both
    /// `/Volumes/.../researcher` and `~/code/researcher`. Use the
    /// `label` field of any `/api/v1/field/basins` entry as the value.
    #[serde(default)]
    pub project: Option<String>,
    /// Filter to a single metadata kind. Common values: `todo`, `learning`,
    /// `decision`, `user_action`, `goal_phase`. Omit for all kinds.
    #[serde(default)]
    pub kind: Option<String>,
    /// When true, the response includes a `Vec<f32>` per fragment (256
    /// dims by default). Fragments missing from the canonical store are
    /// re-embedded from their sidecar text — uses the embedding cache so
    /// the second request for the same content is free.
    #[serde(default)]
    pub with_embedding: bool,
    /// Hard cap on returned fragments. Default 250; ceiling 2000 because
    /// 256-d × float32 × 2000 ≈ 2MB which is the comfortable HTTP body
    /// size for snappy dashboard reloads.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    250
}

fn project_basename(cwd: &str) -> &str {
    cwd.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("unknown")
}

fn meta_matches_project(
    meta: Option<&HashMap<String, serde_json::Value>>,
    filter_project: &str,
) -> bool {
    meta.and_then(|m| m.get("project_cwd"))
        .and_then(|v| v.as_str())
        .map(|cwd| project_basename(cwd) == filter_project)
        .unwrap_or(false)
}

#[derive(Debug, Serialize)]
pub struct FragmentRow {
    pub id: String,
    pub session_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Importance value from canonical fragment, or `0.5` sidecar default.
    pub importance: f32,
    /// Embedding vector. Present iff caller passed `with_embedding=true`.
    /// Sidecar-only fragments get re-embedded on demand.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
}

#[derive(Debug, Serialize)]
pub struct FragmentsResponse {
    pub fragments: Vec<FragmentRow>,
    /// True when `limit` truncated the result set.
    pub truncated: bool,
}

pub async fn list_fragments(
    State(services): State<ContextNestServices>,
    Query(q): Query<FragmentsQuery>,
) -> Result<Json<FragmentsResponse>, StatusCode> {
    let limit = q.limit.min(2000).max(1);

    // === Phase 1: snapshot candidate set under each lock once ===
    //
    // We snapshot the data we need NOW so the parallel embedding work
    // below doesn't have to await the same locks repeatedly. The total
    // copy is bounded by `limit` (default 250, max 2000) so memory
    // pressure stays modest.
    let active = services.session_index.active_fragments_session_map().await;
    let metadata = services.fragment_metadata.read().await;
    let texts = services.fragment_texts.read().await;

    // Build candidate (frag_id, session_id, content, metadata) tuples.
    let mut candidates: Vec<(String, String, String, HashMap<String, serde_json::Value>)> =
        Vec::new();
    for (frag_id, session) in &active {
        if let Some(filter_session) = &q.session_id {
            if session != filter_session {
                continue;
            }
        }
        let meta = metadata.get(frag_id).cloned().unwrap_or_default();
        if let Some(filter_kind) = &q.kind {
            let kind_match = meta
                .get("kind")
                .and_then(|v| v.as_str())
                .map(|k| k == filter_kind)
                .unwrap_or(false);
            if !kind_match {
                continue;
            }
        }
        // Project filter: match against the BASENAME of `metadata.project_cwd`
        // so the same logical project clusters across path variants
        // (`/Volumes/.../researcher` ≡ `~/code/researcher`). The dropdown
        // in the dashboard's filter bar passes the label from
        // /api/v1/field/basins, which is also a basename, so the
        // comparison is symmetric.
        if let Some(filter_project) = &q.project {
            if !meta_matches_project(Some(&meta), filter_project) {
                continue;
            }
        }
        let content = texts.get(frag_id).cloned().unwrap_or_default();
        candidates.push((frag_id.clone(), session.clone(), content, meta));
    }
    drop(metadata);
    drop(texts);

    let total = candidates.len();
    let truncated = total > limit;
    candidates.truncate(limit);

    // === Phase 2: check the per-fragment-id embedding cache ===
    //
    // Avoids re-running embedding generation for fragments we've
    // already served. Survives across requests within the same server
    // process; lost on restart (cheap to repopulate as the dashboard
    // polls).
    let cached: HashMap<String, Vec<f32>> = if q.with_embedding {
        let cache = services.embeddings_by_id.read().await;
        candidates
            .iter()
            .filter_map(|(id, _, _, _)| cache.get(id).map(|v| (id.clone(), v.clone())))
            .collect()
    } else {
        HashMap::new()
    };

    // === Phase 3: parallel embed for the misses ===
    //
    // We build N futures each running on a separate task, then bound
    // the in-flight count with `buffer_unordered`. The order returned
    // matches submission order semantically because we re-key by
    // fragment id at the end.
    let with_embedding = q.with_embedding;
    let mut rows: Vec<FragmentRow> = stream::iter(candidates)
        .map(|(frag_id, session_id, content, meta)| {
            let services = services.clone();
            let cache_hit = cached.get(&frag_id).cloned();
            async move {
                // Importance: prefer canonical, fall back to neutral.
                // We only call get_fragment when with_embedding is set
                // AND the cache missed — saves lock churn on the
                // common cached path.
                let (importance, mut embedding) = if with_embedding {
                    if let Some(vec) = cache_hit {
                        (0.5, Some(vec))
                    } else {
                        match services.attractor_manager.get_fragment(&frag_id).await {
                            Ok(Some(frag)) => (frag.importance, Some(frag.content)),
                            _ => (0.5, None),
                        }
                    }
                } else {
                    (0.5, None)
                };

                // Cache miss → run the embedder. Local TF-IDF is
                // ~1ms; OpenAI would be ~200ms (and want a smaller
                // concurrency cap).
                if with_embedding && embedding.is_none() && !content.is_empty() {
                    if let Ok(vec) = services.embedding.generate_embedding(&content).await {
                        // Populate the per-id cache so the next call is free.
                        let mut cache = services.embeddings_by_id.write().await;
                        cache.insert(frag_id.clone(), vec.clone());
                        // Soft cap on cache size: drop oldest-ish entries
                        // when we exceed 20k. HashMap doesn't track insert
                        // order so we drop a deterministic slice — good
                        // enough for an LRU-ish approximation without
                        // pulling in a real LRU dep.
                        if cache.len() > 20_000 {
                            let drop_count = cache.len() - 16_000;
                            let to_drop: Vec<String> =
                                cache.keys().take(drop_count).cloned().collect();
                            for k in to_drop {
                                cache.remove(&k);
                            }
                        }
                        embedding = Some(vec);
                    }
                }

                FragmentRow {
                    id: frag_id,
                    session_id,
                    content,
                    metadata: meta,
                    importance,
                    embedding,
                }
            }
        })
        .buffer_unordered(EMBED_CONCURRENCY)
        .collect()
        .await;

    // buffer_unordered returns futures in completion order; resort by
    // fragment id so the response is deterministic across requests
    // (helps with caching at any HTTP intermediary).
    rows.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(Json(FragmentsResponse {
        fragments: rows,
        truncated,
    }))
}

// =============================================================================
// /api/v1/field/basins
// =============================================================================

#[derive(Debug, Serialize)]
pub struct BasinSummary {
    /// Stable id derived from project basename. Real attractor basins
    /// from `MemoryAttractorManager` would prefix with `basin-`; project
    /// fallbacks use `proj-`.
    pub id: String,
    pub label: String,
    pub source: BasinSource,
    /// Total ACTIVE fragments in this basin.
    pub mass: usize,
    /// Centroid in embedding space, computed as the mean of contributing
    /// fragment embeddings. Empty when no fragments have embeddings yet
    /// (cold start before any retrieve was called with `with_embedding`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub centroid: Vec<f32>,
    /// Histogram of `kind` metadata across the basin. Drives the
    /// dominant-color rendering on the basin disc.
    pub by_kind: HashMap<String, usize>,
    /// Session ids whose fragments contribute to this basin.
    pub sessions: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BasinSource {
    /// Centroid came from a real basin in [`MemoryAttractorManager`].
    Attractor,
    /// Centroid was synthesized from project_cwd grouping. This is the
    /// fallback when the canonical store hasn't formed real basins yet
    /// (e.g., post-sidecar-replay; live-hook ingest path is also
    /// sidecar-only so this state is the default in v0.1).
    Project,
}

/// Default cap on returned basins. The substrate forms one basin per
/// semantic cluster, so the cardinality is a function of how much has
/// been ingested — the operator's own corpus carried 291,244 basins
/// over 507,845 consolidated fragments as of 2026-10. A viz that maps
/// over all of them is unusable, and the centroids alone ran to 300+
/// MB of JSON, so the endpoint always ranks and truncates.
const BASIN_LIMIT_DEFAULT: usize = 40;
/// Ceiling for the `limit` query param.
const BASIN_LIMIT_CEILING: usize = 500;
/// Longest label we derive from a member fragment's text.
const BASIN_LABEL_MAX_CHARS: usize = 48;
/// A line shorter than this makes a useless label; fall through to the
/// dominant kind instead.
const BASIN_LABEL_MIN_CHARS: usize = 8;

fn default_basin_limit() -> usize {
    BASIN_LIMIT_DEFAULT
}

#[derive(Debug, Serialize)]
pub struct BasinsResponse {
    pub basins: Vec<BasinSummary>,
    /// Basins that passed the scope + mass filter, *before* `limit`
    /// truncated the list. Lets the client say "showing 40 of 1,426"
    /// instead of silently implying the substrate has 40 clusters.
    pub total_candidates: usize,
    /// True when `limit` dropped basins from the response.
    pub truncated: bool,
}

#[derive(Debug, Deserialize)]
pub struct BasinsQuery {
    /// Optional project basename filter, matching `metadata.project_cwd`.
    #[serde(default)]
    pub project: Option<String>,
    /// Optional session filter. When both project and session are set,
    /// both must match.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Minimum mass for a *real* attractor basin to be returned, where
    /// mass counts only the members that survive the scope filter.
    /// Defaults to 1 (no floor). Real basins are heavily skewed —
    /// median mass 1 across the corpus — so a floor is what separates
    /// "cluster" from "one fragment that happened to land alone".
    ///
    /// Deliberately NOT applied to the project-derived fallback: a
    /// one-fragment project is still a project.
    #[serde(default = "default_min_mass")]
    pub min_mass: usize,
    /// Cap on returned basins, ranked by scope-filtered mass. Default
    /// 40, ceiling [`BASIN_LIMIT_CEILING`].
    #[serde(default = "default_basin_limit")]
    pub limit: usize,
}

fn default_min_mass() -> usize {
    1
}

/// Reduce a fragment's text to a candidate basin label: the first
/// non-empty line, whitespace-collapsed, capped at
/// [`BASIN_LABEL_MAX_CHARS`] characters.
///
/// Returns the label plus its *pre-cap* length as a score, so a caller
/// comparing the members of one basin can prefer the most descriptive
/// among them without re-reading their texts. `None` when there is no
/// line worth labelling a cluster with (blank, or shorter than
/// [`BASIN_LABEL_MIN_CHARS`]).
///
/// The cap counts `char`s, never bytes: fragment text is arbitrary
/// UTF-8 and a byte slice would panic mid-codepoint.
fn label_candidate(text: &str) -> Option<(String, usize)> {
    let line: String = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let line_len = line.chars().count();
    if line_len < BASIN_LABEL_MIN_CHARS {
        return None;
    }
    let capped: String = line.chars().take(BASIN_LABEL_MAX_CHARS).collect();
    let label = if line_len > BASIN_LABEL_MAX_CHARS {
        format!("{capped}…")
    } else {
        capped
    };
    Some((label, line_len.min(BASIN_LABEL_MAX_CHARS)))
}

pub async fn list_basins(
    State(services): State<ContextNestServices>,
    Query(q): Query<BasinsQuery>,
) -> Result<Json<BasinsResponse>, StatusCode> {
    // Phase 3 of the neural-field epic
    // (docs/roadmap/epics/neural-field-real.md): prefer real basins
    // formed by the consolidation worker (Phase 1) over the
    // project-derived fallback. When the worker has caught up the
    // response is genuinely attractor-shaped; before it catches up the
    // project basins keep the frontend usable instead of going blank.
    let limit = q.limit.clamp(1, BASIN_LIMIT_CEILING);
    let min_mass = q.min_mass.max(1);

    let snapshots = services.attractor_manager.list_basin_snapshots().await;
    if !snapshots.is_empty() {
        let active = services.session_index.active_fragments_session_map().await;
        let metadata = services.fragment_metadata.read().await;

        let matches = |frag_id: &str| -> bool {
            // Only count fragments that are still active in some
            // session — discarded ones shouldn't inflate basin mass.
            let Some(session_id) = active.get(frag_id) else {
                return false;
            };
            if let Some(filter_session) = &q.session_id {
                if session_id != filter_session {
                    return false;
                }
            }
            if let Some(filter_project) = &q.project {
                return meta_matches_project(metadata.get(frag_id), filter_project);
            }
            true
        };

        // === Pass 1: scope-filtered mass per basin, nothing else ===
        //
        // Deliberately cheap. The substrate carries one basin per
        // semantic cluster (291k on the operator's corpus), so this
        // loop runs over ~0.5M member ids per request; building the
        // `by_kind` histogram, the `sessions` list and a 1024-float
        // centroid for every basin — 99% of which the `limit` then
        // discards — is what made this endpoint return 300 MB.
        let mut ranked: Vec<(&BasinSnapshot, usize)> = snapshots
            .iter()
            .map(|snap| {
                let mass = snap
                    .fragment_ids
                    .iter()
                    .filter(|fid| matches(fid.as_str()))
                    .count();
                (snap, mass)
            })
            .collect();
        // Basins with no surviving member (everything soft-deleted, or
        // filtered out of scope) are phantom centroids — drop them
        // rather than shipping them to the viz.
        //
        // `scope_matched_anything` distinguishes "the scope matched no
        // real basin at all" (→ project fallback, below) from "the
        // caller's `min_mass` excluded every basin" (→ honest empty
        // list). Without it, raising a threshold would silently swap
        // the response over to project-derived basins.
        let scope_matched_anything = ranked.iter().any(|(_, mass)| *mass > 0);
        ranked.retain(|(_, mass)| *mass >= min_mass);
        let total_candidates = ranked.len();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));
        let truncated = total_candidates > limit;
        ranked.truncate(limit);

        if !ranked.is_empty() {
            // === Pass 2: rich fields, for the survivors only ===
            //
            // Lock order matches `list_fragments`: metadata, then texts.
            let texts = services.fragment_texts.read().await;
            let mut basins: Vec<BasinSummary> = Vec::with_capacity(ranked.len());
            for (snap, mass) in ranked {
                let mut by_kind: HashMap<String, usize> = HashMap::new();
                let mut sessions: Vec<String> = Vec::new();
                let mut seen_sessions: HashSet<&str> = HashSet::new();
                // Best label candidate so far: (label, score).
                let mut label_member: Option<(String, usize)> = None;
                for fid in &snap.fragment_ids {
                    if !matches(fid.as_str()) {
                        continue;
                    }
                    // `matches` already proved the fragment is active.
                    let session_id = active.get(fid).expect("matches() checks active");
                    if seen_sessions.insert(session_id.as_str()) {
                        sessions.push(session_id.clone());
                    }
                    let meta = metadata.get(fid);
                    if let Some(kind) = meta.and_then(|m| m.get("kind")).and_then(|v| v.as_str()) {
                        *by_kind.entry(kind.to_string()).or_insert(0) += 1;
                    }
                    if let Some((candidate, score)) =
                        texts.get(fid).and_then(|text| label_candidate(text))
                    {
                        // Strictly better wins; ties keep the earliest
                        // member, so the label is stable across requests.
                        if label_member
                            .as_ref()
                            .map(|(_, s)| score > *s)
                            .unwrap_or(true)
                        {
                            label_member = Some((candidate, score));
                        }
                    }
                }
                // Deterministic dominant kind — HashMap iteration order
                // isn't, and the label below feeds a stable UI.
                let mut kinds: Vec<(&String, &usize)> = by_kind.iter().collect();
                kinds.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
                let dominant_kind = kinds.first().map(|(k, _)| (*k).clone());

                // Label = a headline lifted from the basin's most
                // descriptive member fragment, falling back to the
                // dominant kind, then to a short id slug. The snippet is
                // the only option that distinguishes two basins sharing
                // a kind — "accomplishment" labels 4,152 basins.
                let label = label_member
                    .map(|(candidate, _)| candidate)
                    .or(dominant_kind)
                    .unwrap_or_else(|| {
                        let short = snap.id.split('-').next().unwrap_or("basin");
                        format!("basin-{short}")
                    });

                basins.push(BasinSummary {
                    id: format!("basin-{}", snap.id),
                    label,
                    source: BasinSource::Attractor,
                    mass,
                    centroid: snap.center.clone(),
                    by_kind,
                    sessions,
                });
            }
            drop(texts);
            drop(metadata);
            return Ok(Json(BasinsResponse {
                basins,
                total_candidates,
                truncated,
            }));
        }
        drop(metadata);
        if scope_matched_anything {
            // The scope matches real basins, but none reached
            // `min_mass`. Report the empty set rather than presenting
            // project-derived basins as if they were attractors.
            return Ok(Json(BasinsResponse {
                basins: Vec::new(),
                total_candidates: 0,
                truncated: false,
            }));
        }
        // Every real basin's members were discarded or filtered out of
        // scope, so there is nothing attractor-shaped to show — drop
        // through to the project fallback rather than returning an
        // empty list, which keeps the viz useful while the worker
        // re-balances.
    }

    // Fallback: project-derived basins. Used when (a) consolidation
    // hasn't caught up so `list_basin_snapshots` is empty, or (b)
    // every real basin's members got soft-deleted.
    let active = services.session_index.active_fragments_session_map().await;
    let metadata = services.fragment_metadata.read().await;

    let mut by_project: HashMap<String, BasinSummary> = HashMap::new();
    for (frag_id, session_id) in &active {
        if let Some(filter_session) = &q.session_id {
            if session_id != filter_session {
                continue;
            }
        }
        let meta = metadata.get(frag_id);
        if let Some(filter_project) = &q.project {
            if !meta_matches_project(meta, filter_project) {
                continue;
            }
        }
        let project_raw = meta
            .and_then(|m| m.get("project_cwd"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        // Normalize to basename so e.g. `/Users/me/code/ratchet` and
        // `~/code/ratchet` cluster together.
        let label = project_basename(&project_raw).to_string();
        let id = format!("proj-{label}");
        let entry = by_project
            .entry(label.clone())
            .or_insert_with(|| BasinSummary {
                id: id.clone(),
                label: label.clone(),
                source: BasinSource::Project,
                mass: 0,
                centroid: Vec::new(),
                by_kind: HashMap::new(),
                sessions: Vec::new(),
            });
        entry.mass += 1;
        if let Some(meta) = meta {
            if let Some(kind) = meta.get("kind").and_then(|v| v.as_str()) {
                *entry.by_kind.entry(kind.to_string()).or_insert(0) += 1;
            }
        }
        if !entry.sessions.contains(session_id) {
            entry.sessions.push(session_id.clone());
        }
    }
    drop(metadata);

    let mut basins: Vec<BasinSummary> = by_project.into_values().collect();
    // Sort by mass desc so the heavy basins come first — frontends that
    // cap rendering at N get the most-important slots.
    basins.sort_by(|a, b| b.mass.cmp(&a.mass).then_with(|| a.label.cmp(&b.label)));
    // Same `limit` contract as the attractor path. `min_mass` is NOT
    // applied here: a one-fragment project is still a project.
    let total_candidates = basins.len();
    let truncated = total_candidates > limit;
    basins.truncate(limit);

    Ok(Json(BasinsResponse {
        basins,
        total_candidates,
        truncated,
    }))
}

// =============================================================================
// /api/v1/connections
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct ConnectionsQuery {
    /// Optional — limit to connections involving at least one fragment
    /// in this session.
    #[serde(default)]
    pub session_id: Option<String>,
    /// Optional project basename filter, matching `metadata.project_cwd`.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default = "default_conn_limit")]
    pub limit: usize,
}

fn default_conn_limit() -> usize {
    300
}

#[derive(Debug, Serialize)]
pub struct ConnectionRow {
    pub source: String,
    pub target: String,
    /// Number of times these two fragments were returned by the same
    /// retrieve call. Higher = stronger learned co-occurrence.
    pub count: u32,
}

#[derive(Debug, Serialize)]
pub struct ConnectionsResponse {
    pub connections: Vec<ConnectionRow>,
    pub total_known: usize,
}

pub async fn list_connections(
    State(services): State<ContextNestServices>,
    Query(q): Query<ConnectionsQuery>,
) -> Result<Json<ConnectionsResponse>, StatusCode> {
    let log = services.connection_log.read().await;
    let active = services.session_index.active_fragments_session_map().await;
    let metadata = services.fragment_metadata.read().await;

    // Filter by session/project if requested.
    let in_session = |frag_id: &str| -> bool {
        if let Some(sess) = &q.session_id {
            if !active.get(frag_id).map(|s| s == sess).unwrap_or(false) {
                return false;
            }
        }
        if let Some(project) = &q.project {
            return meta_matches_project(metadata.get(frag_id), project);
        }
        active.contains_key(frag_id)
    };

    let mut entries: Vec<ConnectionRow> = log
        .iter()
        .filter(|((a, b), _)| in_session(a) || in_session(b))
        .map(|((a, b), n)| ConnectionRow {
            source: a.clone(),
            target: b.clone(),
            count: *n,
        })
        .collect();
    let total_known = entries.len();
    // Highest co-occurrence first; ties broken by source id for stable order.
    entries.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.source.cmp(&b.source)));
    entries.truncate(q.limit.min(2000).max(1));

    Ok(Json(ConnectionsResponse {
        connections: entries,
        total_known,
    }))
}

// =============================================================================
// Router
// =============================================================================

pub fn create_field_router() -> Router<ContextNestServices> {
    Router::new()
        .route("/api/v1/fragments", get(list_fragments))
        .route("/api/v1/field/basins", get(list_basins))
        .route("/api/v1/connections", get(list_connections))
}

#[cfg(test)]
mod label_candidate_tests {
    use super::{label_candidate, BASIN_LABEL_MAX_CHARS};

    #[test]
    fn takes_the_first_non_empty_line_collapsed() {
        // Leading blank lines are skipped and internal runs of
        // whitespace collapse, so a multi-line fragment still yields a
        // one-line label.
        let (label, score) =
            label_candidate("\n\n  cargo   test --lib → 14 passed  \nanother line")
                .expect("a substantive first line is a valid label");
        assert_eq!(label, "cargo test --lib → 14 passed");
        assert_eq!(score, label.chars().count());
    }

    #[test]
    fn caps_at_the_budget_on_char_boundaries() {
        // Multi-byte text must not be sliced mid-codepoint — a byte
        // index here would panic.
        let long = "é".repeat(200);
        let (label, score) = label_candidate(&long).expect("long line is a valid label");
        assert_eq!(score, BASIN_LABEL_MAX_CHARS);
        assert_eq!(label.chars().count(), BASIN_LABEL_MAX_CHARS + 1);
        assert!(label.ends_with('…'));
    }

    #[test]
    fn rejects_text_too_thin_to_label_a_cluster() {
        assert!(label_candidate("").is_none());
        assert!(label_candidate("   \n\t\n  ").is_none());
        assert!(label_candidate("ok").is_none());
        assert!(label_candidate("a long enough line").is_some());
    }
}

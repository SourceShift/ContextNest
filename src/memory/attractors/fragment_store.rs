//! Canonical fragment store backed by the shared [`VectorArena`].
//!
//! Holds fragment metadata only; the vector lives once in the arena under a
//! [`Holder::Fragment`] claim, shared with the fragment's graph node.
//! Readers get a hydrated [`MemoryFragment`] (one vector copy, the same cost
//! as the `HashMap::get(..).cloned()` this replaces). Predicates and
//! in-place updates see [`FragmentMeta`], which has no vector field, so no
//! internal caller can mistake a missing vector for an empty one.

use super::vector_arena::{Holder, VectorArena};
use super::MemoryFragment;
use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Fragment metadata without its vector.
#[derive(Debug, Clone)]
pub struct FragmentMeta {
    pub id: String,
    pub importance: f32,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub attractor_basin_id: Option<String>,
    pub connections: HashSet<String>,
    pub confidence: f32,
}

/// One candidate's score against a query embedding.
///
/// No vector, no fragment — this is the retrieve hot path's batch
/// primitive. `canonical == false` means this store has no entry for the
/// id, so the caller must rebuild the hit from the sidecars (the
/// post-WAL-replay fallback path).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CandidateScore {
    pub similarity: f32,
    /// Canonical importance — the ranking tiebreaker behind similarity.
    pub importance: f32,
    pub canonical: bool,
}

#[derive(Debug)]
struct Entry {
    meta: FragmentMeta,
    /// Vector kept inline only when the arena rejected it (dimension differs
    /// from the arena's). Never set on the normal path.
    inline: Option<Vec<f32>>,
}

#[derive(Debug)]
pub struct FragmentStore {
    arena: Arc<VectorArena>,
    entries: HashMap<String, Entry>,
}

impl FragmentStore {
    pub fn new(arena: Arc<VectorArena>) -> Self {
        Self {
            arena,
            entries: HashMap::new(),
        }
    }

    /// Insert or replace a fragment. Replacing overwrites the arena row,
    /// which the fragment's graph node shares.
    pub fn insert(&mut self, fragment: MemoryFragment) {
        let MemoryFragment {
            id,
            content,
            importance,
            created_at,
            last_accessed,
            attractor_basin_id,
            connections,
            confidence,
        } = fragment;
        let inline = match self.arena.claim(&id, Holder::Fragment, &content) {
            Ok(_) => None,
            Err(error) => {
                // A replacement that no longer fits must not leave the old
                // vector claimed under this id.
                self.arena.release(&id, Holder::Fragment);
                tracing::debug!(fragment_id = %id, %error, "fragment vector kept inline");
                Some(content)
            }
        };
        self.entries.insert(
            id.clone(),
            Entry {
                meta: FragmentMeta {
                    id,
                    importance,
                    created_at,
                    last_accessed,
                    attractor_basin_id,
                    connections,
                    confidence,
                },
                inline,
            },
        );
    }

    /// Hydrated copy of the fragment.
    pub fn get(&self, id: &str) -> Option<MemoryFragment> {
        self.entries.get(id).map(|entry| self.hydrate(entry))
    }

    pub fn meta(&self, id: &str) -> Option<&FragmentMeta> {
        self.entries.get(id).map(|entry| &entry.meta)
    }

    /// Mutate metadata in place. `false` when `id` is absent.
    pub fn update(&mut self, id: &str, f: impl FnOnce(&mut FragmentMeta)) -> bool {
        match self.entries.get_mut(id) {
            Some(entry) => {
                f(&mut entry.meta);
                true
            }
            None => false,
        }
    }

    /// Remove a fragment and release its arena claim. `true` if present.
    pub fn remove(&mut self, id: &str) -> bool {
        let removed = self.entries.remove(id).is_some();
        if removed {
            self.arena.release(id, Holder::Fragment);
        }
        removed
    }

    /// Keep fragments for which `keep` returns true; returns removed count.
    pub fn retain(&mut self, mut keep: impl FnMut(&FragmentMeta) -> bool) -> usize {
        let doomed: Vec<String> = self
            .entries
            .values()
            .filter(|entry| !keep(&entry.meta))
            .map(|entry| entry.meta.id.clone())
            .collect();
        for id in &doomed {
            self.remove(id);
        }
        doomed.len()
    }

    pub fn contains_key(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    pub fn ids(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Hydrated copies of every fragment. Allocates one vector per
    /// fragment; reserved for whole-state snapshots (tenant persistence).
    pub fn all(&self) -> Vec<MemoryFragment> {
        self.entries.values().map(|e| self.hydrate(e)).collect()
    }

    /// Cosine between `query` and a fragment's vector, without copying it.
    pub fn cosine_to(&self, id: &str, query: &[f32]) -> Option<f32> {
        let entry = self.entries.get(id)?;
        match &entry.inline {
            Some(inline) => Some(super::utils::cosine_similarity(query, inline)),
            None => self.arena.cosine_to(id, query),
        }
    }

    /// Score every id in `ids` against `query`, positionally: `out[i]`
    /// belongs to `ids[i]`.
    ///
    /// Two whole-batch savings over calling [`Self::get`] (or
    /// [`Self::cosine_to`]) per candidate: one arena read lock, and one
    /// query-norm pass — see [`VectorArena::cosine_batch`]. The importance
    /// tiebreaker is read by borrow from [`Self::meta`], so nothing here
    /// allocates a vector or a connection set.
    ///
    /// What this replaces mattered at substrate scale: `get` → `hydrate`
    /// allocated a fresh `Vec<f32>` plus a cloned `HashSet<String>` of
    /// connections for *every* candidate, ~2.5 GB of churn on a global
    /// query, of which all but `top_k` results were discarded unread.
    ///
    /// `entries` stays the authority on canonical-ness rather than the arena:
    /// the arena is shared with the connection graph, so a live row is not by
    /// itself evidence of a fragment. This mirrors `get`, which returned
    /// `None` for an id the store does not hold.
    pub fn score_batch(&self, ids: &[String], query: &[f32]) -> Vec<CandidateScore> {
        let mut scores = self.arena.cosine_batch(ids, query);
        ids.iter()
            .zip(scores.iter_mut())
            .map(|(id, slot)| match self.entries.get(id) {
                Some(entry) => {
                    if let Some(inline) = &entry.inline {
                        // The arena rejected this vector (its dimension
                        // differs from the arena's), so it lives inline.
                        *slot = Some(super::utils::cosine_similarity(query, inline));
                    }
                    CandidateScore {
                        similarity: slot.unwrap_or(0.0),
                        importance: entry.meta.importance,
                        canonical: true,
                    }
                }
                None => CandidateScore {
                    similarity: 0.0,
                    // Neutral default, mirroring the retrieve handler's
                    // sidecar-only hits and `restore_sidecars_bulk`.
                    importance: 0.5,
                    canonical: false,
                },
            })
            .collect()
    }

    fn hydrate(&self, entry: &Entry) -> MemoryFragment {
        let content = match &entry.inline {
            Some(inline) => inline.clone(),
            None => self.arena.vector(&entry.meta.id).unwrap_or_default(),
        };
        let meta = entry.meta.clone();
        MemoryFragment {
            id: meta.id,
            content,
            importance: meta.importance,
            created_at: meta.created_at,
            last_accessed: meta.last_accessed,
            attractor_basin_id: meta.attractor_basin_id,
            connections: meta.connections,
            confidence: meta.confidence,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fragment(id: &str, content: Vec<f32>) -> MemoryFragment {
        MemoryFragment {
            id: id.to_string(),
            content,
            importance: 0.5,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            attractor_basin_id: None,
            connections: HashSet::new(),
            confidence: 0.9,
        }
    }

    #[test]
    fn score_batch_matches_cosine_to_positionally() {
        let arena = Arc::new(VectorArena::new());
        let mut store = FragmentStore::new(arena.clone());
        store.insert(fragment("a", vec![1.0, 0.0, 0.0]));
        store.insert(fragment("b", vec![0.0, 1.0, 0.0]));
        let query = vec![0.9, 0.1, 0.0];
        let ids = vec!["a".to_string(), "b".to_string(), "ghost".to_string()];
        let scored = store.score_batch(&ids, &query);

        assert_eq!(scored.len(), ids.len(), "one score per id, in order");
        // The batch path must not be a different formula — it is the same
        // cosine, with the query norm hoisted out of the loop.
        assert_eq!(scored[0].similarity, store.cosine_to("a", &query).unwrap());
        assert_eq!(scored[1].similarity, store.cosine_to("b", &query).unwrap());
        assert!(scored[0].similarity > scored[1].similarity);
        assert!(scored[0].canonical && scored[1].canonical);
        assert_eq!(scored[0].importance, 0.5);

        // "ghost" is the sidecar-only fallback: absent from the store means
        // absent, not zero-scored-canonical.
        assert!(!scored[2].canonical);
        assert_eq!(scored[2].similarity, 0.0);
    }

    #[test]
    fn score_batch_ignores_arena_rows_the_store_does_not_own() {
        let arena = Arc::new(VectorArena::new());
        let store = FragmentStore::new(arena.clone());
        // The arena is shared with the connection graph, so a live row is not
        // by itself evidence of a fragment — `entries` stays the authority,
        // exactly as `get` was.
        arena
            .claim("node-only", Holder::Node, &[1.0, 0.0, 0.0])
            .unwrap();
        let scored = store.score_batch(&["node-only".to_string()], &[1.0, 0.0, 0.0]);
        assert!(!scored[0].canonical);
    }

    #[test]
    fn round_trips_through_the_arena() {
        let arena = Arc::new(VectorArena::new());
        let mut store = FragmentStore::new(arena.clone());
        store.insert(fragment("f1", vec![1.0, 2.0, 3.0]));
        assert_eq!(store.get("f1").unwrap().content, vec![1.0, 2.0, 3.0]);
        assert!(arena.contains("f1"));
        assert!(store.remove("f1"));
        assert!(!arena.contains("f1"), "last holder released the row");
        assert!(store.get("f1").is_none());
    }

    #[test]
    fn mismatched_dimension_is_kept_inline_not_dropped() {
        let arena = Arc::new(VectorArena::new());
        let mut store = FragmentStore::new(arena.clone());
        store.insert(fragment("a", vec![1.0, 0.0]));
        store.insert(fragment("b", vec![1.0, 0.0, 0.0]));
        assert_eq!(store.get("b").unwrap().content, vec![1.0, 0.0, 0.0]);
        assert!(!arena.contains("b"));
        assert!((store.cosine_to("b", &[1.0, 0.0, 0.0]).unwrap() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn update_and_retain_see_metadata_only() {
        let arena = Arc::new(VectorArena::new());
        let mut store = FragmentStore::new(arena.clone());
        store.insert(fragment("keep", vec![1.0, 0.0]));
        store.insert(fragment("drop", vec![0.0, 1.0]));
        assert!(store.update("keep", |m| m.importance = 0.9));
        assert!(!store.update("missing", |m| m.importance = 0.1));
        assert_eq!(store.retain(|m| m.id == "keep"), 1);
        assert_eq!(store.get("keep").unwrap().importance, 0.9);
        assert!(!arena.contains("drop"));
    }
}

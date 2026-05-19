//! Session-to-fragment index for the seven-tool memory API.
//!
//! ## Why this exists
//!
//! The canonical [`MemoryAttractorManager`] is deliberately session-agnostic:
//! it tracks attractor basin dynamics and connection networks globally, not
//! per-caller. That is the right abstraction for the consolidation layer, but
//! the HTTP API layer needs to answer two questions the manager cannot:
//!
//! 1. "What fragment IDs belong to session X?" (for `retrieve` and `summarize`)
//! 2. "Which session owns fragment F?" (for cross-session audit and `discard`
//!    restore semantics)
//!
//! `SessionIndex` is that thin, session-aware overlay. It carries no attractor
//! physics — it is purely an in-memory routing table that keeps three
//! consistent hash maps across every mutating operation.
//!
//! ## Consistency contract
//! The three maps (`active`, `deleted`, `reverse`) are **always written
//! together** inside a single `write()` guard for any mutating operation. This
//! avoids the torn-read window you would get if the maps were locked
//! independently. Reads acquire only the map they need, so read concurrency is
//! still high.
//! ## Soft vs. hard removal
//! `discard(soft=true)` (the API default) must not break the reverse index:
//! other sub-systems may still ask "which session does fragment F come from?"
//! for audit purposes even after the fragment has been logically hidden from
//! `retrieve`. `soft_remove` therefore **keeps** the reverse entry and only
//! moves the id from `active` → `deleted`.
//! `hard_remove` is a full purge: active, deleted, and reverse entries are all
//! cleared. Use it only when the backing attractor has been permanently
//! destroyed (e.g., a `discard(soft=false)` call).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Lightweight session-to-fragment routing index.
/// All three maps are kept coherent: every mutating method acquires write
/// locks on all three before touching any of them so there is no window
/// where the maps disagree about a fragment's state.
/// The struct is cheaply `Clone`able because the inner data lives behind
/// `Arc`; clones share the same underlying maps and see each other's writes.
#[derive(Debug, Clone)]
pub struct SessionIndex {
    /// `session_id → set of currently-active fragment IDs`.
    /// "Active" means the fragment is visible to `retrieve` and `summarize`.
    /// After a soft-remove the id leaves this set and moves to `deleted`.
    active: Arc<RwLock<HashMap<String, HashSet<String>>>>,

    /// `session_id → set of soft-deleted fragment IDs`.
    /// Soft-deleted fragments are hidden from retrieval but their reverse
    /// entry is preserved so callers can still audit which session owns them.
    /// A subsequent `add` call restores them (removes from here, re-adds to
    /// `active`), implementing the `discard` + re-`store` round-trip.
    deleted: Arc<RwLock<HashMap<String, HashSet<String>>>>,

    /// `fragment_id → session_id` reverse lookup.
    /// Populated on `add` and cleared only on `hard_remove`. Intentionally
    /// kept alive across soft-removes so the session can always be resolved
    /// for a known fragment regardless of its active/deleted state.
    reverse: Arc<RwLock<HashMap<String, String>>>,
}

impl SessionIndex {
    /// Create a new, empty index.
    pub fn new() -> Self {
        Self {
            active: Arc::new(RwLock::new(HashMap::new())),
            deleted: Arc::new(RwLock::new(HashMap::new())),
            reverse: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register `fragment_id` as belonging to `session`.
    /// This operation is **idempotent**: calling it twice for the same pair
    /// is a no-op with no visible side effects.
    /// **Restore semantics**: if `fragment_id` was previously soft-deleted for
    /// this session (i.e. it is in `deleted[session]`), the call moves it back
    /// to `active[session]`, effectively undoing the soft-remove. This mirrors
    /// the expected behaviour when a caller re-stores a fragment that was
    /// previously discarded with `soft=true`.
    pub async fn add(&self, session: &str, fragment_id: &str) {
        // Acquire all three write guards up front to ensure we never expose a
        // partially-updated state to concurrent readers.
        let mut active = self.active.write().await;
        let mut deleted = self.deleted.write().await;
        let mut reverse = self.reverse.write().await;

        // If the fragment is currently soft-deleted for this session, lift the
        // deletion before re-inserting into the active set.
        if let Some(del_set) = deleted.get_mut(session) {
            del_set.remove(fragment_id);
        }

        active
            .entry(session.to_owned())
            .or_insert_with(HashSet::new)
            .insert(fragment_id.to_owned());

        reverse.insert(fragment_id.to_owned(), session.to_owned());
    }

    /// Move `fragment_id` from the active set to the deleted set for `session`.
    /// Returns `true` if the fragment was in the active set (and was therefore
    /// actually moved). Returns `false` if the fragment was not active — e.g.
    /// it was already soft-deleted or never registered — in which case the
    /// index is unchanged.
    /// The reverse-index entry is **preserved** so cross-session audit calls
    /// (`find_session`) continue to work after a soft-remove.
    pub async fn soft_remove(&self, session: &str, fragment_id: &str) -> bool {
        let mut active = self.active.write().await;
        let mut deleted = self.deleted.write().await;
        // Note: reverse is NOT touched here — that is the whole point of the
        // soft/hard distinction.

        let was_active = active
            .get_mut(session)
            .map(|s| s.remove(fragment_id))
            .unwrap_or(false);

        if was_active {
            deleted
                .entry(session.to_owned())
                .or_insert_with(HashSet::new)
                .insert(fragment_id.to_owned());
        }

        was_active
    }

    /// Permanently remove `fragment_id` from all three maps for `session`.
    /// Unlike `soft_remove`, this also clears the reverse-index entry, so
    /// `find_session` will return `None` afterwards. Use this only when the
    /// backing attractor fragment has been irreversibly destroyed.
    /// Returns `true` if anything was actually removed from any of the three
    /// maps (useful for callers that want to distinguish "already gone" from
    /// a genuine deletion).
    pub async fn hard_remove(&self, session: &str, fragment_id: &str) -> bool {
        let mut active = self.active.write().await;
        let mut deleted = self.deleted.write().await;
        let mut reverse = self.reverse.write().await;

        let from_active = active
            .get_mut(session)
            .map(|s| s.remove(fragment_id))
            .unwrap_or(false);

        let from_deleted = deleted
            .get_mut(session)
            .map(|s| s.remove(fragment_id))
            .unwrap_or(false);

        let from_reverse = reverse.remove(fragment_id).is_some();

        from_active || from_deleted || from_reverse
    }

    /// Return all currently-active fragment IDs for `session`.
    /// The returned `Vec` is a snapshot clone; the caller owns it and
    /// subsequent index mutations do not affect it. Returns an empty `Vec`
    /// for an unknown session rather than an error.
    pub async fn list_active(&self, session: &str) -> Vec<String> {
        let active = self.active.read().await;
        active
            .get(session)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Return all soft-deleted fragment IDs for `session`.
    /// Same ownership and empty-default semantics as [`list_active`].
    pub async fn list_deleted(&self, session: &str) -> Vec<String> {
        let deleted = self.deleted.read().await;
        deleted
            .get(session)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Return `true` if `fragment_id` is in the **active** set for `session`.
    /// This is the hot-path membership test used by the `retrieve` handler to
    /// filter fragments before returning them to the caller. It intentionally
    /// returns `false` for soft-deleted fragments even though their reverse
    /// entry still exists.
    pub async fn is_active(&self, session: &str, fragment_id: &str) -> bool {
        let active = self.active.read().await;
        active
            .get(session)
            .map(|s| s.contains(fragment_id))
            .unwrap_or(false)
    }

    /// Resolve the owning session for a known `fragment_id`.
    /// Returns `Some(session_id)` whether the fragment is currently active or
    /// soft-deleted. Returns `None` only after a `hard_remove` or if the
    /// fragment was never registered.
    pub async fn find_session(&self, fragment_id: &str) -> Option<String> {
        let reverse = self.reverse.read().await;
        reverse.get(fragment_id).cloned()
    }
}

impl Default for SessionIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // 1. add + list_active round-trip
    #[tokio::test]
    async fn test_add_and_list_active() {
        let idx = SessionIndex::new();
        idx.add("sess-1", "frag-a").await;
        idx.add("sess-1", "frag-b").await;

        let mut active = idx.list_active("sess-1").await;
        active.sort();
        assert_eq!(active, vec!["frag-a", "frag-b"]);
    }

    // 2. add is idempotent — inserting the same pair twice must not duplicate
    #[tokio::test]
    async fn test_add_is_idempotent() {
        let idx = SessionIndex::new();
        idx.add("sess-1", "frag-a").await;
        idx.add("sess-1", "frag-a").await;

        let active = idx.list_active("sess-1").await;
        assert_eq!(
            active.len(),
            1,
            "duplicate add must not grow the active set"
        );
    }

    // 3. soft_remove moves id from active → deleted; list_deleted sees it
    #[tokio::test]
    async fn test_soft_remove_moves_to_deleted() {
        let idx = SessionIndex::new();
        idx.add("sess-1", "frag-a").await;
        idx.add("sess-1", "frag-b").await;

        let removed = idx.soft_remove("sess-1", "frag-a").await;
        assert!(
            removed,
            "soft_remove should return true when fragment was active"
        );

        // frag-a gone from active
        let active = idx.list_active("sess-1").await;
        assert!(!active.contains(&"frag-a".to_string()));
        assert!(active.contains(&"frag-b".to_string()));

        // frag-a visible in deleted
        let deleted = idx.list_deleted("sess-1").await;
        assert!(deleted.contains(&"frag-a".to_string()));
    }

    // 4. add after soft_remove restores id to active (and removes from deleted)
    #[tokio::test]
    async fn test_add_after_soft_remove_restores() {
        let idx = SessionIndex::new();
        idx.add("sess-1", "frag-a").await;
        idx.soft_remove("sess-1", "frag-a").await;

        // sanity: currently deleted
        assert!(idx
            .list_deleted("sess-1")
            .await
            .contains(&"frag-a".to_string()));

        // re-add (simulating a re-store)
        idx.add("sess-1", "frag-a").await;

        // now active again
        assert!(idx
            .list_active("sess-1")
            .await
            .contains(&"frag-a".to_string()));
        // and no longer in deleted
        assert!(
            idx.list_deleted("sess-1").await.is_empty(),
            "deleted set should be empty after restore"
        );
    }

    // 5. hard_remove purges from all three maps
    #[tokio::test]
    async fn test_hard_remove_purges_all() {
        let idx = SessionIndex::new();
        idx.add("sess-1", "frag-a").await;
        idx.soft_remove("sess-1", "frag-a").await;

        // frag-a is now in deleted + reverse
        assert!(idx.find_session("frag-a").await.is_some());

        let removed = idx.hard_remove("sess-1", "frag-a").await;
        assert!(
            removed,
            "hard_remove should return true when something was removed"
        );

        assert!(idx.list_active("sess-1").await.is_empty());
        assert!(idx.list_deleted("sess-1").await.is_empty());
        assert!(
            idx.find_session("frag-a").await.is_none(),
            "reverse entry must be gone after hard_remove"
        );
    }

    // 6. find_session returns correct session_id
    #[tokio::test]
    async fn test_find_session_reverse_lookup() {
        let idx = SessionIndex::new();
        idx.add("sess-alpha", "frag-x").await;
        idx.add("sess-beta", "frag-y").await;

        assert_eq!(
            idx.find_session("frag-x").await.as_deref(),
            Some("sess-alpha")
        );
        assert_eq!(
            idx.find_session("frag-y").await.as_deref(),
            Some("sess-beta")
        );
        assert_eq!(idx.find_session("frag-unknown").await, None);
    }

    // 7. is_active returns true only for currently-active fragments
    #[tokio::test]
    async fn test_is_active_membership() {
        let idx = SessionIndex::new();
        idx.add("sess-1", "frag-a").await;
        idx.add("sess-1", "frag-b").await;

        // both active initially
        assert!(idx.is_active("sess-1", "frag-a").await);
        assert!(idx.is_active("sess-1", "frag-b").await);

        // soft-delete frag-a
        idx.soft_remove("sess-1", "frag-a").await;

        // frag-a no longer active…
        assert!(!idx.is_active("sess-1", "frag-a").await);
        // …but frag-b is still active
        assert!(idx.is_active("sess-1", "frag-b").await);

        // unknown fragment always false
        assert!(!idx.is_active("sess-1", "frag-z").await);
        // unknown session always false
        assert!(!idx.is_active("sess-unknown", "frag-a").await);
    }
}

//! Single contiguous store for canonical memory vectors.
//!
//! Before this arena every fragment vector was held ~3.5 times: in the
//! fragment store, again in its connection-graph node, again in the
//! `embeddings_by_id` sidecar. Each copy was its own 4 KB heap allocation,
//! and similarity scans walked them through `HashMap` iteration — scattered
//! reads that forced the kernel to decompress the whole swapped heap during
//! consolidation bursts (see `docs/roadmap/epics/disk-first-substrate.md`).
//!
//! The arena stores each vector once, in row-major order, keyed by id.
//! Holders (fragment store, graph) *claim* a row; the row is freed when the
//! last holder releases it. Graph node indices are arena rows, so a graph
//! scan is a linear pass over one contiguous buffer.
//!
//! Locking: one `std::sync::RwLock`, never held across `.await`. Callers
//! that also hold the graph lock must take the graph lock first. While an
//! [`ArenaView`] is alive, use only the view — calling any other arena
//! method on the same thread re-enters the lock and can deadlock behind a
//! queued writer.

use crate::services::exact;
use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockReadGuard};

/// Who holds a claim on an arena row. A row lives while any holder does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    Fragment,
    Node,
}

impl Holder {
    fn bit(self) -> u8 {
        match self {
            Holder::Fragment => 1,
            Holder::Node => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArenaError {
    /// Empty, non-finite or zero-norm vector.
    InvalidVector,
    /// Vector length differs from the arena's established dimension.
    DimensionMismatch { expected: usize, actual: usize },
    /// Row count would exceed `u32` addressing.
    Full,
    /// File-backed storage could not grow.
    Storage(String),
}

impl std::fmt::Display for ArenaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArenaError::InvalidVector => write!(f, "invalid memory vector"),
            ArenaError::DimensionMismatch { expected, actual } => write!(
                f,
                "memory vector dimension mismatch (expected {expected}, got {actual})"
            ),
            ArenaError::Full => write!(f, "vector arena row space exhausted"),
            ArenaError::Storage(e) => write!(f, "vector arena storage: {e}"),
        }
    }
}

impl std::error::Error for ArenaError {}

/// Row storage. Heap by default; file-backed when the operator configures
/// an arena directory (see [`VectorArena::attach_file`]).
#[derive(Debug)]
enum Rows {
    Heap(Vec<f32>),
    File(super::vector_arena_file::FileRows),
}

impl Rows {
    fn floats(&self) -> &[f32] {
        match self {
            Rows::Heap(v) => v,
            Rows::File(f) => f.floats(),
        }
    }

    fn floats_mut(&mut self) -> &mut [f32] {
        match self {
            Rows::Heap(v) => v,
            Rows::File(f) => f.floats_mut(),
        }
    }

    /// Ensure room for `floats` values, growing geometrically.
    fn reserve_floats(&mut self, floats: usize) -> Result<(), ArenaError> {
        match self {
            // Grow length exactly; `Vec`'s capacity doubling amortises the
            // reallocations and untouched capacity is never dirtied.
            Rows::Heap(v) => {
                if v.len() < floats {
                    v.resize(floats, 0.0);
                }
                Ok(())
            }
            Rows::File(f) => f.reserve_floats(floats).map_err(ArenaError::Storage),
        }
    }
}

#[derive(Debug)]
struct Inner {
    /// 0 until the first vector establishes the dimension.
    dim: usize,
    rows: Rows,
    norms: Vec<f32>,
    ids: Vec<Option<Arc<str>>>,
    holders: Vec<u8>,
    index: HashMap<Arc<str>, u32>,
    free: Vec<u32>,
}

impl Inner {
    fn new(rows: Rows) -> Self {
        Self {
            dim: 0,
            rows,
            norms: Vec::new(),
            ids: Vec::new(),
            holders: Vec::new(),
            index: HashMap::new(),
            free: Vec::new(),
        }
    }

    fn validate(&self, vector: &[f32]) -> Result<f32, ArenaError> {
        let norm = exact::norm(vector).ok_or(ArenaError::InvalidVector)?;
        if self.dim != 0 && vector.len() != self.dim {
            return Err(ArenaError::DimensionMismatch {
                expected: self.dim,
                actual: vector.len(),
            });
        }
        Ok(norm)
    }

    fn write_row(&mut self, row: u32, vector: &[f32], norm: f32) {
        let dim = self.dim;
        let start = row as usize * dim;
        self.rows.floats_mut()[start..start + dim].copy_from_slice(vector);
        self.norms[row as usize] = norm;
    }

    fn allocate_row(&mut self, id: &str) -> Result<u32, ArenaError> {
        let row = match self.free.pop() {
            Some(row) => row,
            None => {
                let row = u32::try_from(self.ids.len()).map_err(|_| ArenaError::Full)?;
                if row == u32::MAX {
                    return Err(ArenaError::Full);
                }
                self.rows.reserve_floats((row as usize + 1) * self.dim)?;
                self.norms.push(0.0);
                self.ids.push(None);
                self.holders.push(0);
                row
            }
        };
        let id: Arc<str> = Arc::from(id);
        self.ids[row as usize] = Some(id.clone());
        self.index.insert(id, row);
        Ok(row)
    }

    fn row_slice(&self, row: u32) -> &[f32] {
        let start = row as usize * self.dim;
        &self.rows.floats()[start..start + self.dim]
    }
}

/// Contiguous, holder-counted vector store shared by the fragment store and
/// the connection graph.
#[derive(Debug)]
pub struct VectorArena {
    inner: RwLock<Inner>,
}

impl Default for VectorArena {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorArena {
    /// Heap-backed arena.
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner::new(Rows::Heap(Vec::new()))),
        }
    }

    /// Move row storage into a file-backed mapping under `dir`. The file is
    /// derived state: it is truncated here and holds only this process's
    /// rows. Existing rows are copied across. Call before bulk restore.
    pub fn attach_file(&self, dir: &std::path::Path, name: &str) -> Result<(), ArenaError> {
        let mut inner = self.inner.write().unwrap();
        let used = inner.ids.len() * inner.dim;
        let mut file = super::vector_arena_file::FileRows::create(dir, name, used)
            .map_err(ArenaError::Storage)?;
        file.floats_mut()[..used].copy_from_slice(&inner.rows.floats()[..used]);
        inner.rows = Rows::File(file);
        Ok(())
    }

    /// Store or overwrite `id`'s vector and record `holder`'s claim.
    /// Returns the row. Overwriting keeps the row; other holders see the
    /// new vector (they always shared the same id).
    pub fn claim(&self, id: &str, holder: Holder, vector: &[f32]) -> Result<u32, ArenaError> {
        let mut inner = self.inner.write().unwrap();
        let norm = inner.validate(vector)?;
        if inner.dim == 0 {
            inner.dim = vector.len();
        }
        let row = match inner.index.get(id) {
            Some(&row) => row,
            None => inner.allocate_row(id)?,
        };
        inner.write_row(row, vector, norm);
        inner.holders[row as usize] |= holder.bit();
        Ok(row)
    }

    /// Add `holder`'s claim to an existing row without rewriting it.
    /// `None` when `id` has no row.
    pub fn claim_existing(&self, id: &str, holder: Holder) -> Option<u32> {
        let mut inner = self.inner.write().unwrap();
        let row = *inner.index.get(id)?;
        inner.holders[row as usize] |= holder.bit();
        Some(row)
    }

    /// Drop `holder`'s claim. Frees the row when no holder remains.
    /// Returns `true` when the row was freed.
    pub fn release(&self, id: &str, holder: Holder) -> bool {
        let mut inner = self.inner.write().unwrap();
        let Some(&row) = inner.index.get(id) else {
            return false;
        };
        let slot = row as usize;
        inner.holders[slot] &= !holder.bit();
        if inner.holders[slot] != 0 {
            return false;
        }
        inner.index.remove(id);
        inner.ids[slot] = None;
        inner.norms[slot] = 0.0;
        inner.free.push(row);
        true
    }

    pub fn row(&self, id: &str) -> Option<u32> {
        self.inner.read().unwrap().index.get(id).copied()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.inner.read().unwrap().index.contains_key(id)
    }

    /// Owned copy of `id`'s vector.
    pub fn vector(&self, id: &str) -> Option<Vec<f32>> {
        let inner = self.inner.read().unwrap();
        let row = *inner.index.get(id)?;
        Some(inner.row_slice(row).to_vec())
    }

    /// Borrow `id`'s vector and norm without copying.
    pub fn with_vector<R>(&self, id: &str, f: impl FnOnce(&[f32], f32) -> R) -> Option<R> {
        let inner = self.inner.read().unwrap();
        let row = *inner.index.get(id)?;
        Some(f(inner.row_slice(row), inner.norms[row as usize]))
    }

    /// Cosine between `query` and `id`'s stored vector.
    pub fn cosine_to(&self, id: &str, query: &[f32]) -> Option<f32> {
        let query_norm = exact::norm(query)?;
        self.with_vector(id, |v, n| cosine_rows(query, query_norm, v, n))
            .flatten()
    }

    /// Established dimension, `None` before the first vector.
    pub fn dim(&self) -> Option<usize> {
        let dim = self.inner.read().unwrap().dim;
        (dim != 0).then_some(dim)
    }

    /// Live rows.
    pub fn len(&self) -> usize {
        self.inner.read().unwrap().index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Validate a vector against the arena without storing it.
    pub fn validate(&self, vector: &[f32]) -> Result<(), ArenaError> {
        self.inner.read().unwrap().validate(vector).map(|_| ())
    }

    /// Read view for multi-row scans under a single lock acquisition.
    pub fn read(&self) -> ArenaView<'_> {
        ArenaView {
            inner: self.inner.read().unwrap(),
        }
    }

    pub fn stats(&self) -> ArenaStats {
        let inner = self.inner.read().unwrap();
        ArenaStats {
            live_rows: inner.index.len(),
            allocated_rows: inner.ids.len(),
            dim: inner.dim,
            file_backed: matches!(inner.rows, Rows::File(_)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ArenaStats {
    pub live_rows: usize,
    pub allocated_rows: usize,
    pub dim: usize,
    pub file_backed: bool,
}

/// Borrowed read view: row access and scans without per-row locking.
pub struct ArenaView<'a> {
    inner: RwLockReadGuard<'a, Inner>,
}

impl ArenaView<'_> {
    pub fn row_of(&self, id: &str) -> Option<u32> {
        self.inner.index.get(id).copied()
    }

    /// Id of a live row.
    pub fn id(&self, row: u32) -> Option<&Arc<str>> {
        self.inner.ids.get(row as usize)?.as_ref()
    }

    /// Vector of a live row.
    pub fn vector(&self, row: u32) -> Option<&[f32]> {
        self.id(row)?;
        Some(self.inner.row_slice(row))
    }

    pub fn norm(&self, row: u32) -> Option<f32> {
        self.id(row)?;
        Some(self.inner.norms[row as usize])
    }

    /// Exact top-`k` rows by cosine to `query` among live rows accepted by
    /// `filter`, keeping only scores strictly above `min_score`. Descending
    /// score, ascending id on ties.
    pub fn top_k(
        &self,
        query: &[f32],
        k: usize,
        min_score: f32,
        filter: impl Fn(u32) -> bool,
    ) -> Vec<(String, f32)> {
        let Some(query_norm) = exact::norm(query) else {
            return Vec::new();
        };
        exact::top_k(
            self.scores(query, query_norm, filter)
                .filter(|(_, score)| *score > min_score),
            k,
        )
    }

    /// Every live row accepted by `filter` scoring at least `min_score`,
    /// unsorted.
    pub fn all_at_least(
        &self,
        query: &[f32],
        min_score: f32,
        filter: impl Fn(u32) -> bool,
    ) -> Vec<(String, f32)> {
        let Some(query_norm) = exact::norm(query) else {
            return Vec::new();
        };
        self.scores(query, query_norm, filter)
            .filter(|(_, score)| *score >= min_score)
            .map(|(id, score)| (id.to_owned(), score))
            .collect()
    }

    fn scores<'s>(
        &'s self,
        query: &'s [f32],
        query_norm: f32,
        filter: impl Fn(u32) -> bool + 's,
    ) -> impl Iterator<Item = (&'s str, f32)> + 's {
        let dim = self.inner.dim;
        let floats = self.inner.rows.floats();
        self.inner
            .ids
            .iter()
            .enumerate()
            .filter_map(move |(row, id)| {
                let id = id.as_deref()?;
                let row = row as u32;
                if dim != query.len() || !filter(row) {
                    return None;
                }
                let start = row as usize * dim;
                let score = cosine_rows(
                    query,
                    query_norm,
                    &floats[start..start + dim],
                    self.inner.norms[row as usize],
                )?;
                Some((id, score))
            })
    }
}

/// Cosine with a lane-parallel dot product. Sixteen independent accumulators
/// over fixed-size chunks (no per-element bounds checks) compile to four
/// 128-bit multiply/add streams on aarch64; the summation order differs
/// from a sequential fold by rounding only (~1e-7), never by ranking in
/// practice.
pub fn cosine_rows(a: &[f32], an: f32, b: &[f32], bn: f32) -> Option<f32> {
    if a.len() != b.len() || an <= 0.0 || bn <= 0.0 {
        return None;
    }
    let score = dot(a, b) / (an * bn);
    score.is_finite().then_some(score.clamp(-1.0, 1.0))
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    const LANES: usize = 16;
    let mut lanes = [0.0f32; LANES];
    let chunks_a = a.chunks_exact(LANES);
    let chunks_b = b.chunks_exact(LANES);
    let tail: f32 = chunks_a
        .remainder()
        .iter()
        .zip(chunks_b.remainder())
        .map(|(x, y)| x * y)
        .sum();
    for (ca, cb) in chunks_a.zip(chunks_b) {
        let ca: &[f32; LANES] = ca.try_into().expect("chunks_exact yields LANES");
        let cb: &[f32; LANES] = cb.try_into().expect("chunks_exact yields LANES");
        for ((lane, x), y) in lanes.iter_mut().zip(ca).zip(cb) {
            *lane += x * y;
        }
    }
    lanes.iter().sum::<f32>() + tail
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(seed: f32, dim: usize) -> Vec<f32> {
        (0..dim).map(|i| seed + i as f32 * 0.01).collect()
    }

    #[test]
    fn claim_shares_one_row_across_holders() {
        let arena = VectorArena::new();
        let a = arena.claim("f1", Holder::Fragment, &v(1.0, 16)).unwrap();
        let b = arena.claim("f1", Holder::Node, &v(1.0, 16)).unwrap();
        assert_eq!(a, b);
        assert_eq!(arena.len(), 1);
        assert!(
            !arena.release("f1", Holder::Fragment),
            "node still holds it"
        );
        assert!(arena.contains("f1"));
        assert!(arena.release("f1", Holder::Node));
        assert!(!arena.contains("f1"));
    }

    #[test]
    fn freed_rows_are_reused() {
        let arena = VectorArena::new();
        let first = arena.claim("a", Holder::Fragment, &v(1.0, 8)).unwrap();
        arena.claim("b", Holder::Fragment, &v(2.0, 8)).unwrap();
        arena.release("a", Holder::Fragment);
        let reused = arena.claim("c", Holder::Fragment, &v(3.0, 8)).unwrap();
        assert_eq!(first, reused);
        assert_eq!(arena.vector("c").unwrap(), v(3.0, 8));
        assert!(arena.vector("a").is_none());
        assert_eq!(arena.stats().allocated_rows, 2);
    }

    #[test]
    fn rejects_invalid_and_mismatched_vectors() {
        let arena = VectorArena::new();
        assert_eq!(
            arena.claim("z", Holder::Node, &[0.0; 4]),
            Err(ArenaError::InvalidVector)
        );
        assert_eq!(
            arena.claim("n", Holder::Node, &[f32::NAN, 1.0]),
            Err(ArenaError::InvalidVector)
        );
        arena.claim("a", Holder::Node, &v(1.0, 4)).unwrap();
        assert_eq!(
            arena.claim("b", Holder::Node, &v(1.0, 5)),
            Err(ArenaError::DimensionMismatch {
                expected: 4,
                actual: 5
            })
        );
    }

    #[test]
    fn overwrite_keeps_row_and_updates_vector() {
        let arena = VectorArena::new();
        let row = arena.claim("a", Holder::Fragment, &v(1.0, 8)).unwrap();
        let again = arena.claim("a", Holder::Fragment, &v(5.0, 8)).unwrap();
        assert_eq!(row, again);
        assert_eq!(arena.vector("a").unwrap(), v(5.0, 8));
    }

    #[test]
    fn top_k_matches_reference_cosine_and_orders_ties_by_id() {
        let arena = VectorArena::new();
        let dim = 37; // exercises the non-multiple-of-8 tail
        let mut reference = Vec::new();
        for i in 0..50 {
            // Distinct per id, so the only exact ties are the ones added below.
            let vector: Vec<f32> = (0..dim)
                .map(|j| (i as f32 * 1.3 + j as f32 * 0.7).sin())
                .collect();
            let id = format!("id{i:02}");
            arena.claim(&id, Holder::Node, &vector).unwrap();
            reference.push((id, vector));
        }
        arena.claim("tie-b", Holder::Node, &reference[3].1).unwrap();
        arena.claim("tie-a", Holder::Node, &reference[3].1).unwrap();
        let query = reference[3].1.clone();
        let view = arena.read();
        let got = view.top_k(&query, 3, -1.0, |_| true);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].0, "id03");
        assert_eq!(got[1].0, "tie-a");
        assert_eq!(got[2].0, "tie-b");
        for (id, vector) in &reference {
            let expected = crate::memory::attractors::utils::cosine_similarity(&query, vector);
            let actual = arena.cosine_to(id, &query).unwrap();
            assert!(
                (expected - actual).abs() < 1e-5,
                "{id}: {expected} vs {actual}"
            );
        }
    }

    #[test]
    fn filter_and_threshold_are_respected() {
        let arena = VectorArena::new();
        let a = arena.claim("a", Holder::Node, &[1.0, 0.0]).unwrap();
        arena.claim("b", Holder::Node, &[0.0, 1.0]).unwrap();
        arena.claim("c", Holder::Node, &[1.0, 0.1]).unwrap();
        let view = arena.read();
        let hits = view.top_k(&[1.0, 0.0], 10, 0.5, |row| row != a);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "c");
    }
}

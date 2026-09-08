//! Exact cosine search with cached norms and O(K) candidate storage.
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

pub fn norm(vector: &[f32]) -> Option<f32> {
    if vector.is_empty() || vector.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let n = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    (n.is_finite() && n > 0.0).then_some(n)
}

pub fn cosine(a: &[f32], an: f32, b: &[f32], bn: f32) -> Option<f32> {
    if a.len() != b.len() || an <= 0.0 || bn <= 0.0 {
        return None;
    }
    let score = a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() / (an * bn);
    score.is_finite().then_some(score.clamp(-1.0, 1.0))
}

#[derive(Debug, PartialEq)]
struct Candidate<'a> {
    id: &'a str,
    score: f32,
}
impl Eq for Candidate<'_> {}
impl Ord for Candidate<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.id.cmp(self.id))
    }
}
impl PartialOrd for Candidate<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Descending score, ascending ID breaks ties deterministically. IDs are only
/// cloned for the final K results, never for every scored candidate.
pub fn top_k<'a>(scores: impl Iterator<Item = (&'a str, f32)>, k: usize) -> Vec<(String, f32)> {
    if k == 0 {
        return Vec::new();
    }
    let mut heap = BinaryHeap::with_capacity(k + 1);
    for (id, score) in scores.filter(|(_, score)| score.is_finite()) {
        heap.push(Reverse(Candidate { id, score }));
        if heap.len() > k {
            heap.pop();
        }
    }
    let mut result: Vec<_> = heap.into_iter().map(|Reverse(c)| c).collect();
    result.sort_unstable_by(|a, b| b.cmp(a));
    result
        .into_iter()
        .map(|c| (c.id.to_owned(), c.score))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_matches_full_sort_with_ties_and_invalid_vectors() {
        assert!(norm(&[0.0, 0.0]).is_none());
        assert!(norm(&[f32::NAN]).is_none());
        assert!(cosine(&[1.0], 1.0, &[1.0, 0.0], 1.0).is_none());
        let scores = [
            ("z", 0.9),
            ("a", 0.9),
            ("x", 0.1),
            ("b", 0.8),
            ("bad", f32::NAN),
        ];
        assert_eq!(
            top_k(scores.into_iter(), 3),
            vec![("a".into(), 0.9), ("z".into(), 0.9), ("b".into(), 0.8)]
        );
        for i in 1..100 {
            let a = [i as f32, 2.0, -0.5];
            let b = [0.3, 1.0, 2.0];
            let expected = crate::memory::attractors::utils::cosine_similarity(&a, &b);
            assert!(
                (cosine(&a, norm(&a).unwrap(), &b, norm(&b).unwrap()).unwrap() - expected).abs()
                    < 1e-6
            );
        }
    }
}

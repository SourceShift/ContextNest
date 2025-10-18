//! Fragment-type bridges between the canonical memory subsystem and the
//! context-side reconstruction modules.
//! The substrate has five distinct fragment shapes, each fit for a stage
//! in the memory lifecycle:
//! | Type | Where | Stage |
//! |------|-------|-------|
//! | [`crate::memory::attractors::MemoryFragment`] | canonical IP (canon Module 05) | storage / attractor formation |
//! | [`crate::context::resonance_activation::MemoryFragment`] | context-side | resonance scoring |
//! | [`crate::context::gap_identification::ReconstructedFragment`] | context-side | gap-fill output |
//! | [`crate::context::memory_reconstruction::ReconstructionFragment`] | context-side | reconstruction input/output |
//! | [`crate::context::memory::MemoryFragmentInfo`] | context-side meta | memory-cell bookkeeping |
//! Phase C makes them interoperable: canonical fragments flow into the
//! context-side reconstruction pipeline via `From` conversions, and any
//! context-side fragment that needs to land back in the canonical store
//! can be promoted with [`canonical_from_reconstruction`].
//! ### Semantic mismatch worth knowing
//! Canonical [`memory::attractors::MemoryFragment.content`] is `Vec<f32>`
//! (a semantic embedding); the context-side fragments split *text*
//! (`content: String`) from *embedding* (`embedding: Vec<f32>`). The
//! canonical type does not carry textual content because in canon Module 05
//! fragments are pure embedding tokens — text reconstruction is a
//! downstream gap-fill concern. When we bridge canonical → context-side
//! we synthesize a placeholder `content` string of the form `"frag-<id>"`
//! so the receiving module has a stable handle; callers that need real
//! text should look it up from the originating store before bridging.

use crate::context::memory_reconstruction::{ReconstructionFragment, TemporalInfo};
use crate::context::resonance_activation::MemoryFragment as ResonanceFragment;
use crate::memory::attractors::MemoryFragment as CanonicalFragment;

/// Bridge a canonical fragment into the resonance-activation pipeline.
/// Lossy in one direction: `strength` is initialized from `importance`
/// because the canonical fragment has no separate strength field
/// (canon Module 05 treats strength as a runtime property of the
/// attractor basin, not the fragment); `content` is synthesized.
impl From<&CanonicalFragment> for ResonanceFragment {
    fn from(c: &CanonicalFragment) -> Self {
        Self {
            id: c.id.clone(),
            embedding: c.content.clone(),
            content: format!("frag-{}", c.id),
            strength: c.importance,
            importance: c.importance,
            connections: c.connections.iter().cloned().collect(),
            last_accessed: c.last_accessed,
            access_count: 0, // canonical fragment doesn't carry access counters
        }
    }
}

/// Bridge a canonical fragment into the memory-reconstruction pipeline.
/// `source_attractor_id` is populated from `attractor_basin_id` when
/// available (canonical fragments may have already been clustered into
/// a basin), falling back to the fragment's own id when unanchored.
impl From<&CanonicalFragment> for ReconstructionFragment {
    fn from(c: &CanonicalFragment) -> Self {
        Self {
            id: c.id.clone(),
            source_attractor_id: c.attractor_basin_id.clone().unwrap_or_else(|| c.id.clone()),
            content: format!("frag-{}", c.id),
            embedding: c.content.clone(),
            strength: c.importance,
            confidence: c.confidence,
            position: None,
            connections: c.connections.iter().cloned().collect(),
            temporal_info: TemporalInfo {
                created_at: c.created_at,
                sequence_position: None,
                temporal_relationships: Vec::new(),
            },
        }
    }
}

/// Promote a reconstruction-pipeline fragment back into the canonical store.
/// Free function rather than `From` because we discard the text `content`
/// field — canonical fragments use `content: Vec<f32>` for the embedding,
/// not the text. Callers who need the text should retain it separately
/// before promoting.
pub fn canonical_from_reconstruction(r: &ReconstructionFragment) -> CanonicalFragment {
    let now = chrono::Utc::now();
    CanonicalFragment {
        id: r.id.clone(),
        content: r.embedding.clone(),
        importance: r.strength,
        created_at: r.temporal_info.created_at,
        last_accessed: now,
        attractor_basin_id: if r.source_attractor_id == r.id {
            None
        } else {
            Some(r.source_attractor_id.clone())
        },
        connections: r.connections.iter().cloned().collect(),
        confidence: r.confidence,
    }
}

/// Promote a resonance-pipeline fragment back into the canonical store.
/// Same caveat as [`canonical_from_reconstruction`]: text is discarded.
pub fn canonical_from_resonance(r: &ResonanceFragment) -> CanonicalFragment {
    let now = chrono::Utc::now();
    CanonicalFragment {
        id: r.id.clone(),
        content: r.embedding.clone(),
        importance: r.importance,
        created_at: now,
        last_accessed: r.last_accessed,
        attractor_basin_id: None,
        connections: r.connections.iter().cloned().collect(),
        confidence: r.strength, // resonance.strength is the closest analog to canonical.confidence
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn canonical_sample() -> CanonicalFragment {
        let mut connections = HashSet::new();
        connections.insert("c1".to_string());
        connections.insert("c2".to_string());
        CanonicalFragment {
            id: "frag-test".to_string(),
            content: vec![0.1, 0.2, 0.3, 0.4],
            importance: 0.75,
            created_at: chrono::Utc::now(),
            last_accessed: chrono::Utc::now(),
            attractor_basin_id: Some("basin-1".to_string()),
            connections,
            confidence: 0.9,
        }
    }

    #[test]
    fn canonical_to_resonance_roundtrip_preserves_identity() {
        let original = canonical_sample();
        let resonance: ResonanceFragment = (&original).into();
        let restored = canonical_from_resonance(&resonance);

        assert_eq!(original.id, restored.id);
        assert_eq!(original.content, restored.content);
        assert!((original.importance - restored.importance).abs() < f32::EPSILON);
        // `connections` is HashSet — order-insensitive equality
        assert_eq!(original.connections, restored.connections);
        // basin_id is lost in the round trip because ResonanceFragment has
        // no equivalent — documented in `canonical_from_resonance`.
        assert!(restored.attractor_basin_id.is_none());
    }

    #[test]
    fn canonical_to_reconstruction_preserves_basin() {
        let original = canonical_sample();
        let recon: ReconstructionFragment = (&original).into();
        assert_eq!(recon.source_attractor_id, "basin-1");
        assert_eq!(recon.embedding, original.content);
        assert_eq!(recon.confidence, original.confidence);
        assert_eq!(recon.temporal_info.created_at, original.created_at);
    }

    #[test]
    fn reconstruction_without_basin_keeps_own_id_as_source() {
        let mut original = canonical_sample();
        original.attractor_basin_id = None;
        let recon: ReconstructionFragment = (&original).into();
        // Falls back to the fragment's own id (documented behavior).
        assert_eq!(recon.source_attractor_id, original.id);
    }

    #[test]
    fn reconstruction_roundtrip_preserves_embedding_and_strength() {
        let original = canonical_sample();
        let recon: ReconstructionFragment = (&original).into();
        let restored = canonical_from_reconstruction(&recon);

        assert_eq!(original.id, restored.id);
        assert_eq!(original.content, restored.content); // embedding survives
        assert!((original.importance - restored.importance).abs() < f32::EPSILON);
        assert_eq!(original.attractor_basin_id, restored.attractor_basin_id);
    }
}

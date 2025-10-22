//! Gap Identification for Memory Reconstruction
//! This module detects missing information in reconstructed memories by identifying
//! different types of gaps: temporal sequences, causal chains, semantic bridges,
//! contextual details, emotional content, and procedural steps.

use crate::error::ContextNestResult;
use crate::{ContextNestError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Gap identifier for detecting missing information
#[derive(Debug, Clone)]
pub struct GapIdentifier {
    /// Minimum importance threshold for gap identification
    pub importance_threshold: f32,
    /// Gap detection strategies
    pub detection_strategies: GapDetectionStrategies,
    /// Detected gaps history
    pub detected_gaps: Vec<MemoryGap>,
}

/// Strategies for detecting different types of gaps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapDetectionStrategies {
    /// Enable temporal sequence gap detection
    pub detect_temporal: bool,
    /// Enable causal chain gap detection
    pub detect_causal: bool,
    /// Enable semantic bridge gap detection
    pub detect_semantic: bool,
    /// Enable contextual detail gap detection
    pub detect_contextual: bool,
    /// Enable emotional content gap detection
    pub detect_emotional: bool,
    /// Enable procedural step gap detection
    pub detect_procedural: bool,
}

impl Default for GapDetectionStrategies {
    fn default() -> Self {
        Self {
            detect_temporal: true,
            detect_causal: true,
            detect_semantic: true,
            detect_contextual: true,
            detect_emotional: true,
            detect_procedural: true,
        }
    }
}

/// Types of gaps that can be detected
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum GapType {
    /// Missing time sequence between events
    TemporalSequence,
    /// Missing cause-effect relationship
    CausalChain,
    /// Missing semantic connection between concepts
    SemanticBridge,
    /// Missing contextual information
    ContextualDetail,
    /// Missing emotional or affective content
    EmotionalContent,
    /// Missing procedural step in a process
    ProceduralStep,
}

/// Detected memory gap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGap {
    /// Gap unique identifier
    pub id: String,
    /// Type of gap
    pub gap_type: GapType,
    /// Fragments before the gap
    pub before_fragments: Vec<String>,
    /// Fragments after the gap
    pub after_fragments: Vec<String>,
    /// Gap description
    pub description: String,
    /// Importance of filling this gap
    pub fill_importance: f32,
    /// Context relevance score
    pub context_relevance: f32,
    /// Estimated difficulty of filling
    pub fill_difficulty: f32,
    /// Priority score (importance * relevance / difficulty)
    pub priority_score: f32,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
    /// Detected timestamp
    pub detected_at: chrono::DateTime<chrono::Utc>,
}

/// Fragment information for gap detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructedFragment {
    pub id: String,
    pub content: String,
    pub embedding: Vec<f32>,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub fragment_type: FragmentType,
    pub connections: Vec<String>,
    pub coherence_score: f32,
}

/// Types of reconstructed fragments
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FragmentType {
    Event,
    Concept,
    Procedure,
    Emotion,
    Context,
    Unknown,
}

/// Result of gap identification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapIdentificationResult {
    pub total_gaps: usize,
    pub gaps_by_type: HashMap<GapType, usize>,
    pub high_priority_gaps: Vec<MemoryGap>,
    pub all_gaps: Vec<MemoryGap>,
    pub average_priority: f32,
}

impl GapIdentifier {
    /// Create new gap identifier with default settings
    pub fn new() -> Self {
        Self {
            importance_threshold: 0.5,
            detection_strategies: GapDetectionStrategies::default(),
            detected_gaps: Vec::new(),
        }
    }

    /// Identify gaps in reconstructed memory
    pub fn identify_gaps(
        &mut self,
        fragments: &[ReconstructedFragment],
    ) -> ContextNestResult<GapIdentificationResult> {
        self.detected_gaps.clear();

        // Detect different types of gaps
        if self.detection_strategies.detect_temporal {
            self.detect_temporal_gaps(fragments)?;
        }

        if self.detection_strategies.detect_causal {
            self.detect_causal_gaps(fragments)?;
        }

        if self.detection_strategies.detect_semantic {
            self.detect_semantic_gaps(fragments)?;
        }

        if self.detection_strategies.detect_contextual {
            self.detect_contextual_gaps(fragments)?;
        }

        if self.detection_strategies.detect_emotional {
            self.detect_emotional_gaps(fragments)?;
        }

        if self.detection_strategies.detect_procedural {
            self.detect_procedural_gaps(fragments)?;
        }

        // Sort gaps by priority
        self.detected_gaps
            .sort_by(|a, b| b.priority_score.partial_cmp(&a.priority_score).unwrap());

        // Compile results
        self.compile_results()
    }

    /// Detect temporal sequence gaps
    fn detect_temporal_gaps(
        &mut self,
        fragments: &[ReconstructedFragment],
    ) -> ContextNestResult<()> {
        let mut temporal_fragments: Vec<_> =
            fragments.iter().filter(|f| f.timestamp.is_some()).collect();

        temporal_fragments.sort_by_key(|f| f.timestamp.unwrap());

        for i in 0..temporal_fragments.len().saturating_sub(1) {
            let current = temporal_fragments[i];
            let next = temporal_fragments[i + 1];

            let time_diff = (next.timestamp.unwrap() - current.timestamp.unwrap()).num_minutes();

            // Detect significant time gaps (>30 minutes)
            if time_diff > 30 {
                let gap = MemoryGap {
                    id: uuid::Uuid::new_v4().to_string(),
                    gap_type: GapType::TemporalSequence,
                    before_fragments: vec![current.id.clone()],
                    after_fragments: vec![next.id.clone()],
                    description: format!("Temporal gap of {} minutes", time_diff),
                    fill_importance: self.calculate_temporal_importance(time_diff),
                    context_relevance: (current.coherence_score + next.coherence_score) / 2.0,
                    fill_difficulty: 0.6,
                    priority_score: 0.0, // Will be calculated
                    metadata: HashMap::from([(
                        "time_diff_minutes".to_string(),
                        time_diff.to_string(),
                    )]),
                    detected_at: chrono::Utc::now(),
                };

                self.add_gap(gap);
            }
        }

        Ok(())
    }

    /// Detect causal chain gaps
    fn detect_causal_gaps(&mut self, fragments: &[ReconstructedFragment]) -> ContextNestResult<()> {
        // Look for event sequences without clear causation
        for i in 0..fragments.len().saturating_sub(1) {
            let current = &fragments[i];
            let next = &fragments[i + 1];

            if current.fragment_type == FragmentType::Event
                && next.fragment_type == FragmentType::Event
            {
                // Check if fragments are connected
                if !current.connections.contains(&next.id)
                    && !next.connections.contains(&current.id)
                {
                    // Check semantic distance
                    let semantic_distance =
                        self.calculate_semantic_distance(&current.embedding, &next.embedding);

                    if semantic_distance > 0.5 {
                        let gap = MemoryGap {
                            id: uuid::Uuid::new_v4().to_string(),
                            gap_type: GapType::CausalChain,
                            before_fragments: vec![current.id.clone()],
                            after_fragments: vec![next.id.clone()],
                            description: "Missing causal link between events".to_string(),
                            fill_importance: 0.8,
                            context_relevance: (current.coherence_score + next.coherence_score)
                                / 2.0,
                            fill_difficulty: 0.7,
                            priority_score: 0.0,
                            metadata: HashMap::from([(
                                "semantic_distance".to_string(),
                                semantic_distance.to_string(),
                            )]),
                            detected_at: chrono::Utc::now(),
                        };

                        self.add_gap(gap);
                    }
                }
            }
        }

        Ok(())
    }

    /// Detect semantic bridge gaps
    fn detect_semantic_gaps(
        &mut self,
        fragments: &[ReconstructedFragment],
    ) -> ContextNestResult<()> {
        for i in 0..fragments.len() {
            for j in (i + 1)..fragments.len() {
                let frag_a = &fragments[i];
                let frag_b = &fragments[j];

                let semantic_distance =
                    self.calculate_semantic_distance(&frag_a.embedding, &frag_b.embedding);

                // Significant semantic gap (>0.6) without direct connection
                if semantic_distance > 0.6
                    && !frag_a.connections.contains(&frag_b.id)
                    && !frag_b.connections.contains(&frag_a.id)
                {
                    let gap = MemoryGap {
                        id: uuid::Uuid::new_v4().to_string(),
                        gap_type: GapType::SemanticBridge,
                        before_fragments: vec![frag_a.id.clone()],
                        after_fragments: vec![frag_b.id.clone()],
                        description: "Missing semantic connection between concepts".to_string(),
                        fill_importance: 0.6,
                        context_relevance: (frag_a.coherence_score + frag_b.coherence_score) / 2.0,
                        fill_difficulty: 0.5,
                        priority_score: 0.0,
                        metadata: HashMap::from([(
                            "semantic_distance".to_string(),
                            semantic_distance.to_string(),
                        )]),
                        detected_at: chrono::Utc::now(),
                    };

                    self.add_gap(gap);
                }
            }
        }

        Ok(())
    }

    /// Detect contextual detail gaps
    fn detect_contextual_gaps(
        &mut self,
        fragments: &[ReconstructedFragment],
    ) -> ContextNestResult<()> {
        for fragment in fragments {
            // Low coherence suggests missing context
            if fragment.coherence_score < 0.5 && fragment.fragment_type != FragmentType::Context {
                let gap = MemoryGap {
                    id: uuid::Uuid::new_v4().to_string(),
                    gap_type: GapType::ContextualDetail,
                    before_fragments: vec![fragment.id.clone()],
                    after_fragments: vec![],
                    description: "Missing contextual information".to_string(),
                    fill_importance: 0.7,
                    context_relevance: fragment.coherence_score,
                    fill_difficulty: 0.4,
                    priority_score: 0.0,
                    metadata: HashMap::from([(
                        "coherence_score".to_string(),
                        fragment.coherence_score.to_string(),
                    )]),
                    detected_at: chrono::Utc::now(),
                };

                self.add_gap(gap);
            }
        }

        Ok(())
    }

    /// Detect emotional content gaps
    fn detect_emotional_gaps(
        &mut self,
        fragments: &[ReconstructedFragment],
    ) -> ContextNestResult<()> {
        let has_emotion = fragments
            .iter()
            .any(|f| f.fragment_type == FragmentType::Emotion);

        // If we have events but no emotional content, that's a gap
        if !has_emotion
            && fragments
                .iter()
                .any(|f| f.fragment_type == FragmentType::Event)
        {
            let event_ids: Vec<_> = fragments
                .iter()
                .filter(|f| f.fragment_type == FragmentType::Event)
                .map(|f| f.id.clone())
                .collect();

            if !event_ids.is_empty() {
                let gap = MemoryGap {
                    id: uuid::Uuid::new_v4().to_string(),
                    gap_type: GapType::EmotionalContent,
                    before_fragments: event_ids,
                    after_fragments: vec![],
                    description: "Missing emotional context for events".to_string(),
                    fill_importance: 0.5,
                    context_relevance: 0.6,
                    fill_difficulty: 0.8, // Emotions are hard to infer
                    priority_score: 0.0,
                    metadata: HashMap::new(),
                    detected_at: chrono::Utc::now(),
                };

                self.add_gap(gap);
            }
        }

        Ok(())
    }

    /// Detect procedural step gaps
    fn detect_procedural_gaps(
        &mut self,
        fragments: &[ReconstructedFragment],
    ) -> ContextNestResult<()> {
        let procedural_fragments: Vec<_> = fragments
            .iter()
            .filter(|f| f.fragment_type == FragmentType::Procedure)
            .collect();

        // Look for disconnected procedure steps
        for i in 0..procedural_fragments.len().saturating_sub(1) {
            let current = procedural_fragments[i];
            let next = procedural_fragments[i + 1];

            if !current.connections.contains(&next.id) {
                let gap = MemoryGap {
                    id: uuid::Uuid::new_v4().to_string(),
                    gap_type: GapType::ProceduralStep,
                    before_fragments: vec![current.id.clone()],
                    after_fragments: vec![next.id.clone()],
                    description: "Missing procedural step".to_string(),
                    fill_importance: 0.9, // Procedural gaps are critical
                    context_relevance: (current.coherence_score + next.coherence_score) / 2.0,
                    fill_difficulty: 0.6,
                    priority_score: 0.0,
                    metadata: HashMap::new(),
                    detected_at: chrono::Utc::now(),
                };

                self.add_gap(gap);
            }
        }

        Ok(())
    }

    /// Add gap and calculate priority
    fn add_gap(&mut self, mut gap: MemoryGap) {
        // Calculate priority: importance * relevance / difficulty
        gap.priority_score = if gap.fill_difficulty > 0.0 {
            (gap.fill_importance * gap.context_relevance) / gap.fill_difficulty
        } else {
            gap.fill_importance * gap.context_relevance * 10.0 // Very easy to fill
        };

        self.detected_gaps.push(gap);
    }

    /// Calculate temporal importance based on time difference
    fn calculate_temporal_importance(&self, time_diff_minutes: i64) -> f32 {
        // Longer gaps are more important to fill
        let hours = time_diff_minutes as f32 / 60.0;
        (hours / 24.0).min(1.0)
    }

    /// Calculate semantic distance between embeddings
    fn calculate_semantic_distance(&self, emb_a: &[f32], emb_b: &[f32]) -> f32 {
        if emb_a.len() != emb_b.len() || emb_a.is_empty() {
            return 1.0; // Maximum distance
        }

        // Use 1 - cosine_similarity as distance
        let similarity = cosine_similarity(emb_a, emb_b);
        1.0 - similarity
    }

    /// Compile identification results
    fn compile_results(&self) -> ContextNestResult<GapIdentificationResult> {
        let mut gaps_by_type: HashMap<GapType, usize> = HashMap::new();

        for gap in &self.detected_gaps {
            *gaps_by_type.entry(gap.gap_type.clone()).or_insert(0) += 1;
        }

        let high_priority_gaps: Vec<_> = self
            .detected_gaps
            .iter()
            .filter(|g| g.priority_score > 0.5)
            .cloned()
            .collect();

        let average_priority = if !self.detected_gaps.is_empty() {
            self.detected_gaps
                .iter()
                .map(|g| g.priority_score)
                .sum::<f32>()
                / self.detected_gaps.len() as f32
        } else {
            0.0
        };

        Ok(GapIdentificationResult {
            total_gaps: self.detected_gaps.len(),
            gaps_by_type,
            high_priority_gaps,
            all_gaps: self.detected_gaps.clone(),
            average_priority,
        })
    }

    /// Get gaps of specific type
    pub fn get_gaps_by_type(&self, gap_type: GapType) -> Vec<&MemoryGap> {
        self.detected_gaps
            .iter()
            .filter(|g| g.gap_type == gap_type)
            .collect()
    }

    /// Clear detected gaps
    pub fn clear_gaps(&mut self) {
        self.detected_gaps.clear();
    }
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        (dot_product / (norm_a * norm_b)).max(-1.0).min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gap_identifier_creation() {
        let identifier = GapIdentifier::new();
        assert_eq!(identifier.importance_threshold, 0.5);
        assert!(identifier.detection_strategies.detect_temporal);
    }

    #[test]
    fn test_temporal_gap_detection() {
        let mut identifier = GapIdentifier::new();

        let fragments = vec![
            ReconstructedFragment {
                id: "frag1".to_string(),
                content: "First event".to_string(),
                embedding: vec![1.0, 0.0, 0.0],
                timestamp: Some(chrono::Utc::now()),
                fragment_type: FragmentType::Event,
                connections: vec![],
                coherence_score: 0.8,
            },
            ReconstructedFragment {
                id: "frag2".to_string(),
                content: "Second event".to_string(),
                embedding: vec![0.9, 0.1, 0.0],
                timestamp: Some(chrono::Utc::now() + chrono::Duration::hours(2)),
                fragment_type: FragmentType::Event,
                connections: vec![],
                coherence_score: 0.8,
            },
        ];

        let result = identifier.identify_gaps(&fragments).unwrap();
        let _ = result;
    }
}

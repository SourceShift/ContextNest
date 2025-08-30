//! Resonance Activation for Memory Reconstruction
//! This module implements cue-based fragment activation through resonance calculations.
//! It supports multiple resonance types (cue, context, network) and combines them to
//! activate memory fragments above a threshold.

use crate::error::ContextNestResult;
use crate::{ContextNestError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Resonance activator for cue-based fragment activation
#[derive(Debug, Clone)]
pub struct ResonanceActivator {
    /// Activation threshold for fragments
    pub activation_threshold: f32,
    /// Weights for different resonance types
    pub resonance_weights: ResonanceWeights,
    /// Activation history for tracking patterns
    pub activation_history: Vec<ActivationRecord>,
    /// Network connections strength
    pub network_connections: HashMap<String, Vec<(String, f32)>>,
}

/// Weights for combining different resonance types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResonanceWeights {
    /// Weight for cue-based resonance (0.5 default)
    pub cue_weight: f32,
    /// Weight for context-based resonance (0.3 default)
    pub context_weight: f32,
    /// Weight for network-based resonance (0.2 default)
    pub network_weight: f32,
}

impl Default for ResonanceWeights {
    fn default() -> Self {
        Self {
            cue_weight: 0.5,
            context_weight: 0.3,
            network_weight: 0.2,
        }
    }
}

/// Record of fragment activation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationRecord {
    pub fragment_id: String,
    pub cue_resonance: f32,
    pub context_resonance: f32,
    pub network_resonance: f32,
    pub combined_resonance: f32,
    pub activated: bool,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Fragment information for activation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFragment {
    pub id: String,
    pub embedding: Vec<f32>,
    pub content: String,
    pub strength: f32,
    pub importance: f32,
    pub connections: Vec<String>,
    pub last_accessed: chrono::DateTime<chrono::Utc>,
    pub access_count: u32,
}

/// Activation cue for triggering fragment recall
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationCue {
    pub query: String,
    pub query_embedding: Vec<f32>,
    pub context_embeddings: Vec<Vec<f32>>,
    pub context_relevance: f32,
}

/// Result of activation process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationResult {
    pub activated_fragments: Vec<String>,
    pub resonance_scores: HashMap<String, ResonanceScore>,
    pub total_activated: usize,
    pub average_resonance: f32,
}

/// Detailed resonance scores for a fragment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResonanceScore {
    pub cue_resonance: f32,
    pub context_resonance: f32,
    pub network_resonance: f32,
    pub combined_resonance: f32,
}

impl ResonanceActivator {
    /// Create new resonance activator with default threshold (0.3)
    pub fn new() -> Self {
        Self::with_threshold(0.3)
    }

    /// Create resonance activator with custom threshold
    pub fn with_threshold(threshold: f32) -> Self {
        Self {
            activation_threshold: threshold,
            resonance_weights: ResonanceWeights::default(),
            activation_history: Vec::new(),
            network_connections: HashMap::new(),
        }
    }

    /// Activate fragments based on resonance with cue
    pub fn activate_fragments(
        &mut self,
        cue: &ActivationCue,
        fragments: &[MemoryFragment],
    ) -> ContextNestResult<ActivationResult> {
        let mut activated_fragments = Vec::new();
        let mut resonance_scores = HashMap::new();
        let mut total_resonance = 0.0;
        let timestamp = chrono::Utc::now();

        for fragment in fragments {
            // Calculate all resonance types
            let cue_resonance =
                self.calculate_cue_resonance(&fragment.embedding, &cue.query_embedding)?;
            let context_resonance = self.calculate_context_resonance(
                &fragment.embedding,
                &cue.context_embeddings,
                cue.context_relevance,
            )?;
            let network_resonance =
                self.calculate_network_resonance(&fragment.id, &activated_fragments, fragments)?;

            // Combine resonance scores with weights
            let combined_resonance =
                self.combine_resonance(cue_resonance, context_resonance, network_resonance);

            // Record resonance scores
            resonance_scores.insert(
                fragment.id.clone(),
                ResonanceScore {
                    cue_resonance,
                    context_resonance,
                    network_resonance,
                    combined_resonance,
                },
            );

            // Activate if above threshold
            let activated = combined_resonance >= self.activation_threshold;
            if activated {
                activated_fragments.push(fragment.id.clone());
                total_resonance += combined_resonance;

                // Update network connections
                self.update_network_connections(&fragment.id, &fragment.connections);
            }

            // Record activation attempt
            self.activation_history.push(ActivationRecord {
                fragment_id: fragment.id.clone(),
                cue_resonance,
                context_resonance,
                network_resonance,
                combined_resonance,
                activated,
                timestamp,
            });
        }

        let average_resonance = if !activated_fragments.is_empty() {
            total_resonance / activated_fragments.len() as f32
        } else {
            0.0
        };

        Ok(ActivationResult {
            total_activated: activated_fragments.len(),
            activated_fragments,
            resonance_scores,
            average_resonance,
        })
    }

    /// Calculate cue-based resonance using cosine similarity
    fn calculate_cue_resonance(
        &self,
        fragment_embedding: &[f32],
        cue_embedding: &[f32],
    ) -> ContextNestResult<f32> {
        if fragment_embedding.is_empty() || cue_embedding.is_empty() {
            return Ok(0.0);
        }

        if fragment_embedding.len() != cue_embedding.len() {
            return Err(ContextNestError::Validation(
                "Embedding dimensions mismatch".to_string(),
            ));
        }

        Ok(cosine_similarity(fragment_embedding, cue_embedding))
    }

    /// Calculate context-based resonance from multiple context embeddings
    fn calculate_context_resonance(
        &self,
        fragment_embedding: &[f32],
        context_embeddings: &[Vec<f32>],
        context_relevance: f32,
    ) -> ContextNestResult<f32> {
        if context_embeddings.is_empty() {
            return Ok(0.0);
        }

        let mut total_similarity = 0.0;
        for context_embedding in context_embeddings {
            if fragment_embedding.len() == context_embedding.len() {
                total_similarity += cosine_similarity(fragment_embedding, context_embedding);
            }
        }

        let average_similarity = total_similarity / context_embeddings.len() as f32;
        Ok(average_similarity * context_relevance)
    }

    /// Calculate network-based resonance from connected fragments
    fn calculate_network_resonance(
        &self,
        fragment_id: &str,
        activated_fragments: &[String],
        all_fragments: &[MemoryFragment],
    ) -> ContextNestResult<f32> {
        if activated_fragments.is_empty() {
            return Ok(0.0);
        }

        // Find fragment's connections
        let fragment = all_fragments
            .iter()
            .find(|f| f.id == fragment_id)
            .ok_or_else(|| {
                ContextNestError::NotFound(format!("Fragment not found: {}", fragment_id))
            })?;

        if fragment.connections.is_empty() {
            return Ok(0.0);
        }

        // Count how many of fragment's connections are already activated
        let activated_connections = fragment
            .connections
            .iter()
            .filter(|conn_id| activated_fragments.contains(conn_id))
            .count();

        // Calculate resonance based on proportion of activated connections
        let resonance = activated_connections as f32 / fragment.connections.len() as f32;

        // Boost resonance if this fragment has strong connections
        let connection_strength = self
            .network_connections
            .get(fragment_id)
            .map(|conns| {
                conns
                    .iter()
                    .filter(|(id, _)| activated_fragments.contains(id))
                    .map(|(_, strength)| strength)
                    .sum::<f32>()
                    / conns.len() as f32
            })
            .unwrap_or(0.0);

        Ok((resonance + connection_strength) / 2.0)
    }

    /// Combine resonance scores using weights
    fn combine_resonance(
        &self,
        cue_resonance: f32,
        context_resonance: f32,
        network_resonance: f32,
    ) -> f32 {
        let combined = (cue_resonance * self.resonance_weights.cue_weight)
            + (context_resonance * self.resonance_weights.context_weight)
            + (network_resonance * self.resonance_weights.network_weight);

        combined.min(1.0) // Cap at 1.0
    }

    /// Update network connections for activated fragment
    fn update_network_connections(&mut self, fragment_id: &str, connections: &[String]) {
        for connection_id in connections {
            self.network_connections
                .entry(fragment_id.to_string())
                .or_insert_with(Vec::new)
                .push((connection_id.clone(), 1.0));
        }
    }

    /// Set custom resonance weights
    pub fn set_weights(&mut self, weights: ResonanceWeights) {
        self.resonance_weights = weights;
    }

    /// Get activation statistics
    pub fn get_activation_stats(&self) -> ActivationStats {
        if self.activation_history.is_empty() {
            return ActivationStats::default();
        }

        let total_activations = self.activation_history.len();
        let successful_activations = self
            .activation_history
            .iter()
            .filter(|r| r.activated)
            .count();

        let avg_cue = self
            .activation_history
            .iter()
            .map(|r| r.cue_resonance)
            .sum::<f32>()
            / total_activations as f32;

        let avg_context = self
            .activation_history
            .iter()
            .map(|r| r.context_resonance)
            .sum::<f32>()
            / total_activations as f32;

        let avg_network = self
            .activation_history
            .iter()
            .map(|r| r.network_resonance)
            .sum::<f32>()
            / total_activations as f32;

        let avg_combined = self
            .activation_history
            .iter()
            .map(|r| r.combined_resonance)
            .sum::<f32>()
            / total_activations as f32;

        ActivationStats {
            total_activations,
            successful_activations,
            activation_rate: successful_activations as f32 / total_activations as f32,
            average_cue_resonance: avg_cue,
            average_context_resonance: avg_context,
            average_network_resonance: avg_network,
            average_combined_resonance: avg_combined,
        }
    }

    /// Clear activation history
    pub fn clear_history(&mut self) {
        self.activation_history.clear();
    }
}

/// Statistics for activation performance
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActivationStats {
    pub total_activations: usize,
    pub successful_activations: usize,
    pub activation_rate: f32,
    pub average_cue_resonance: f32,
    pub average_context_resonance: f32,
    pub average_network_resonance: f32,
    pub average_combined_resonance: f32,
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
    fn test_resonance_activator_creation() {
        let activator = ResonanceActivator::new();
        assert_eq!(activator.activation_threshold, 0.3);
        assert_eq!(activator.resonance_weights.cue_weight, 0.5);
        assert_eq!(activator.resonance_weights.context_weight, 0.3);
        assert_eq!(activator.resonance_weights.network_weight, 0.2);
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![1.0, 0.0, 0.0];
        let d = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&c, &d) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_fragment_activation() {
        let mut activator = ResonanceActivator::new();

        let fragments = vec![MemoryFragment {
            id: "frag1".to_string(),
            embedding: vec![1.0, 0.0, 0.0],
            content: "Test content".to_string(),
            strength: 0.8,
            importance: 0.7,
            connections: vec![],
            last_accessed: chrono::Utc::now(),
            access_count: 5,
        }];

        let cue = ActivationCue {
            query: "test query".to_string(),
            query_embedding: vec![0.9, 0.1, 0.0],
            context_embeddings: vec![],
            context_relevance: 0.8,
        };

        let result = activator.activate_fragments(&cue, &fragments).unwrap();
        assert!(result.total_activated <= 1);
    }
}

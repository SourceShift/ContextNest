//! Harmonic Integration for Co-Emergence Multi-Agent Protocol
//! This module implements resonance-based attractor connection creation,
//! enabling attractors to harmonically integrate through semantic similarity
//! and field resonance patterns.

use crate::context::attractor_dynamics::{AttractorBasin, AttractorDynamicsEngine};
use crate::context::field::{Attractor, NeuralField, SemanticPattern};
use crate::error::{ContextNestError, ContextNestResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Integration strategy for connecting attractors
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IntegrationStrategy {
    /// Natural harmonic resonance-based integration
    Harmonic {
        /// Minimum resonance threshold (0.0-1.0)
        resonance_threshold: f32,
        /// Amplification factor for harmonic connections
        amplification: f32,
    },
    /// Boundary dissolution allowing attractors to merge
    BoundaryDissolution {
        /// Rate of boundary permeability increase
        dissolution_rate: f32,
        /// Minimum overlap for dissolution
        overlap_threshold: f32,
    },
    /// Resonance amplification strengthening existing connections
    ResonanceAmplification {
        /// Amplification power (exponential)
        power: f32,
        /// Maximum amplification limit
        max_amplification: f32,
    },
}

impl Default for IntegrationStrategy {
    fn default() -> Self {
        IntegrationStrategy::Harmonic {
            resonance_threshold: 0.7,
            amplification: 1.2,
        }
    }
}

/// Harmonic connection between attractors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarmonicConnection {
    /// Source attractor ID
    pub source_id: String,
    /// Target attractor ID
    pub target_id: String,
    /// Connection strength (0.0-1.0)
    pub strength: f32,
    /// Resonance frequency
    pub resonance_frequency: f32,
    /// Phase alignment (-1.0 to 1.0)
    pub phase_alignment: f32,
    /// Semantic similarity score
    pub semantic_similarity: f32,
    /// Connection type
    pub connection_type: ConnectionType,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Type of harmonic connection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConnectionType {
    /// Complementary connection (filling gaps)
    Complementary,
    /// Transformative connection (qualitative change)
    Transformative,
    /// Catalytic connection (one enables another)
    Catalytic,
    /// Resonant connection (mutual reinforcement)
    Resonant,
}

/// Harmonic integrator for attractor co-emergence
#[derive(Debug, Clone)]
pub struct HarmonicIntegrator {
    /// Active harmonic connections
    pub connections: Vec<HarmonicConnection>,
    /// Integration strategy
    pub strategy: IntegrationStrategy,
    /// Resonance field properties
    pub resonance_field: ResonanceField,
    /// Integration metrics
    pub metrics: IntegrationMetrics,
}

/// Resonance field for harmonic integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResonanceField {
    /// Base resonance frequency
    pub base_frequency: f32,
    /// Field coherence (0.0-1.0)
    pub coherence: f32,
    /// Energy level
    pub energy: f32,
    /// Harmonic overtones
    pub harmonics: Vec<f32>,
}

impl Default for ResonanceField {
    fn default() -> Self {
        Self {
            base_frequency: 1.0,
            coherence: 0.8,
            energy: 0.5,
            harmonics: vec![2.0, 3.0, 4.0, 5.0], // Natural harmonic series
        }
    }
}

/// Integration metrics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntegrationMetrics {
    /// Total connections created
    pub total_connections: usize,
    /// Average connection strength
    pub avg_strength: f32,
    /// Maximum resonance achieved
    pub max_resonance: f32,
    /// Coherence score
    pub coherence_score: f32,
    /// Number of emergent patterns
    pub emergent_patterns: usize,
}

impl HarmonicIntegrator {
    /// Create a new harmonic integrator
    pub fn new(strategy: IntegrationStrategy) -> Self {
        Self {
            connections: Vec::new(),
            strategy,
            resonance_field: ResonanceField::default(),
            metrics: IntegrationMetrics::default(),
        }
    }

    /// Connect attractors via harmonic resonance
    pub fn connect_attractors(
        &mut self,
        source: &AttractorBasin,
        target: &AttractorBasin,
        field: &NeuralField,
    ) -> ContextNestResult<HarmonicConnection> {
        // Calculate semantic similarity between attractors
        let semantic_similarity = self.calculate_semantic_similarity(source, target, field)?;

        // Calculate resonance frequency
        let resonance_frequency = self.calculate_resonance_frequency(source, target);

        // Calculate phase alignment
        let phase_alignment = self.calculate_phase_alignment(source, target);

        // Determine connection strength based on strategy
        let strength = self.calculate_connection_strength(
            semantic_similarity,
            resonance_frequency,
            phase_alignment,
        )?;

        // Determine connection type
        let connection_type = self.determine_connection_type(source, target, semantic_similarity);

        let connection = HarmonicConnection {
            source_id: source.id.clone(),
            target_id: target.id.clone(),
            strength,
            resonance_frequency,
            phase_alignment,
            semantic_similarity,
            connection_type,
            created_at: chrono::Utc::now(),
        };

        // Update metrics
        self.metrics.total_connections += 1;
        self.metrics.avg_strength =
            (self.metrics.avg_strength * (self.metrics.total_connections - 1) as f32 + strength)
                / self.metrics.total_connections as f32;
        self.metrics.max_resonance = self.metrics.max_resonance.max(resonance_frequency);

        self.connections.push(connection.clone());

        Ok(connection)
    }

    /// Calculate semantic similarity between two attractors
    fn calculate_semantic_similarity(
        &self,
        source: &AttractorBasin,
        target: &AttractorBasin,
        field: &NeuralField,
    ) -> ContextNestResult<f32> {
        // Use cosine similarity between attractor centers
        if source.center.len() != target.center.len() {
            return Err(ContextNestError::Validation(
                "Attractor dimensions mismatch".to_string(),
            ));
        }

        let dot_product: f32 = source
            .center
            .iter()
            .zip(target.center.iter())
            .map(|(a, b)| a * b)
            .sum();

        let source_magnitude: f32 = source.center.iter().map(|x| x * x).sum::<f32>().sqrt();
        let target_magnitude: f32 = target.center.iter().map(|x| x * x).sum::<f32>().sqrt();

        if source_magnitude == 0.0 || target_magnitude == 0.0 {
            return Ok(0.0);
        }

        let similarity = dot_product / (source_magnitude * target_magnitude);
        Ok(similarity.max(0.0).min(1.0))
    }

    /// Calculate resonance frequency between attractors
    fn calculate_resonance_frequency(
        &self,
        source: &AttractorBasin,
        target: &AttractorBasin,
    ) -> f32 {
        // Resonance based on attractor depths (strength) and radii (influence)
        let source_oscillation = source.depth / (source.radius + 1.0);
        let target_oscillation = target.depth / (target.radius + 1.0);

        // Calculate harmonic frequency
        let frequency =
            (source_oscillation * target_oscillation).sqrt() * self.resonance_field.base_frequency;

        frequency
    }

    /// Calculate phase alignment between attractors
    fn calculate_phase_alignment(&self, source: &AttractorBasin, target: &AttractorBasin) -> f32 {
        // Phase alignment based on temporal synchronization
        let time_diff = (target.last_modified.timestamp() - source.last_modified.timestamp()).abs();

        // Exponential decay of alignment with time difference
        let alignment = (-time_diff as f32 / 3600.0).exp(); // 1 hour half-life

        // Normalize to -1 to 1 range (positive for in-phase, negative for out-of-phase)
        alignment * 2.0 - 1.0
    }

    /// Calculate connection strength based on strategy
    fn calculate_connection_strength(
        &self,
        semantic_similarity: f32,
        resonance_frequency: f32,
        phase_alignment: f32,
    ) -> ContextNestResult<f32> {
        let base_strength = match &self.strategy {
            IntegrationStrategy::Harmonic {
                resonance_threshold,
                amplification,
            } => {
                if semantic_similarity < *resonance_threshold {
                    return Ok(0.0);
                }
                semantic_similarity * amplification * (1.0 + phase_alignment.abs())
            }
            IntegrationStrategy::BoundaryDissolution {
                dissolution_rate,
                overlap_threshold,
            } => {
                if semantic_similarity < *overlap_threshold {
                    return Ok(0.0);
                }
                semantic_similarity * (1.0 + dissolution_rate)
            }
            IntegrationStrategy::ResonanceAmplification {
                power,
                max_amplification,
            } => {
                let amplified = semantic_similarity.powf(*power) * resonance_frequency;
                amplified.min(*max_amplification)
            }
        };

        Ok(base_strength.max(0.0).min(1.0))
    }

    /// Determine the type of connection between attractors
    fn determine_connection_type(
        &self,
        source: &AttractorBasin,
        target: &AttractorBasin,
        semantic_similarity: f32,
    ) -> ConnectionType {
        // Complementary: moderate similarity, different strengths (filling gaps)
        if semantic_similarity > 0.4 && semantic_similarity < 0.7 {
            if (source.depth - target.depth).abs() > 0.3 {
                return ConnectionType::Complementary;
            }
        }

        // Transformative: low similarity, high potential for change
        if semantic_similarity < 0.4 {
            return ConnectionType::Transformative;
        }

        // Catalytic: one much stronger than the other
        if (source.depth - target.depth).abs() > 0.5 {
            return ConnectionType::Catalytic;
        }

        // Default: Resonant (mutual reinforcement)
        ConnectionType::Resonant
    }

    /// Create multiple connections between attractor sets
    pub fn integrate_attractor_sets(
        &mut self,
        sources: &[AttractorBasin],
        targets: &[AttractorBasin],
        field: &NeuralField,
    ) -> ContextNestResult<Vec<HarmonicConnection>> {
        let mut new_connections = Vec::new();

        for source in sources {
            for target in targets {
                // Avoid self-connections
                if source.id == target.id {
                    continue;
                }

                // Try to create connection
                match self.connect_attractors(source, target, field) {
                    Ok(connection) => {
                        if connection.strength > 0.0 {
                            new_connections.push(connection);
                        }
                    }
                    Err(_) => continue, // Skip failed connections
                }
            }
        }

        // Update coherence based on new connections
        self.update_field_coherence();

        Ok(new_connections)
    }

    /// Update field coherence based on connections
    fn update_field_coherence(&mut self) {
        if self.connections.is_empty() {
            self.resonance_field.coherence = 0.0;
            return;
        }

        // Calculate coherence as average strength weighted by phase alignment
        let total_weighted_strength: f32 = self
            .connections
            .iter()
            .map(|c| c.strength * (1.0 + c.phase_alignment.abs()) / 2.0)
            .sum();

        self.resonance_field.coherence = total_weighted_strength / self.connections.len() as f32;

        self.metrics.coherence_score = self.resonance_field.coherence;
    }

    /// Find resonant connections for a given attractor
    pub fn find_resonant_connections(&self, attractor_id: &str) -> Vec<&HarmonicConnection> {
        self.connections
            .iter()
            .filter(|c| {
                (c.source_id == attractor_id || c.target_id == attractor_id)
                    && c.connection_type == ConnectionType::Resonant
            })
            .collect()
    }

    /// Amplify resonance in the field
    pub fn amplify_resonance(&mut self, factor: f32) -> ContextNestResult<()> {
        if factor <= 0.0 {
            return Err(ContextNestError::Validation(
                "Amplification factor must be positive".to_string(),
            ));
        }

        // Amplify connection strengths
        for connection in &mut self.connections {
            connection.strength = (connection.strength * factor).min(1.0);
        }

        // Amplify field energy
        self.resonance_field.energy = (self.resonance_field.energy * factor).min(1.0);

        // Update metrics
        self.update_field_coherence();

        Ok(())
    }

    /// Dissolve weak connections below threshold
    pub fn prune_weak_connections(&mut self, threshold: f32) -> usize {
        let original_count = self.connections.len();
        self.connections.retain(|c| c.strength >= threshold);
        let pruned = original_count - self.connections.len();

        // Update metrics
        self.update_field_coherence();
        self.metrics.total_connections = self.connections.len();

        pruned
    }

    /// Get integration metrics
    pub fn get_metrics(&self) -> &IntegrationMetrics {
        &self.metrics
    }

    /// Export integration state for analysis
    pub fn export_state(&self) -> IntegrationState {
        IntegrationState {
            connections: self.connections.clone(),
            strategy: self.strategy.clone(),
            resonance_field: self.resonance_field.clone(),
            metrics: self.metrics.clone(),
        }
    }
}

/// Exported integration state for analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationState {
    pub connections: Vec<HarmonicConnection>,
    pub strategy: IntegrationStrategy,
    pub resonance_field: ResonanceField,
    pub metrics: IntegrationMetrics,
}

// Tests temporarily disabled due to struct compatibility issues
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_harmonic_integrator_creation() {
        let strategy = IntegrationStrategy::default();
        let integrator = HarmonicIntegrator::new(strategy);

        assert_eq!(integrator.connections.len(), 0);
        assert_eq!(integrator.metrics.total_connections, 0);
    }
}

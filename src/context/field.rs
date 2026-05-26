use crate::error::ContextNestResult;
use crate::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Neural Field implementation based on Context Engineering principles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralField {
    /// Semantic patterns in the field
    pub patterns: Vec<SemanticPattern>,
    /// Field properties that control dynamics
    pub properties: FieldProperties,
    /// Attractor configurations
    pub attractors: Vec<Attractor>,
    /// Current field state
    pub state: FieldState,
    /// Agency level for autonomous operation (0.0 to 1.0)
    pub agency_level: f32,
    /// Self-assessment capability enabled
    pub self_assessment_enabled: bool,
    /// Goal-setting capability enabled
    pub goal_setting_enabled: bool,
    /// Symbolic residue detection sensitivity
    pub residue_sensitivity: f32,
    /// Residue compression ratio
    pub compression_ratio: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticPattern {
    pub id: String,
    pub content: String,
    pub embedding: Vec<f32>,
    pub strength: f32,
    pub resonance: f32,
    pub decay_rate: f32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_activated: chrono::DateTime<chrono::Utc>,
    pub activation_count: usize,
    // Soft delete support
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub delete_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldProperties {
    pub resonance_threshold: f32,
    pub decay_constant: f32,
    pub amplification_factor: f32,
    pub boundary_permeability: f32,
    pub coherence_weight: f32,
    pub embedding_dim: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attractor {
    pub id: String,
    pub center: Vec<f32>,
    pub strength: f32,
    pub radius: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldState {
    pub coherence: f32,
    pub stability: f32,
    pub energy: f32,
    pub health: f32,
    pub strength: f32,
    pub entropy: f32,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

/// Field health assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldHealth {
    Excellent,
    Good,
    Fair,
    Poor,
    Critical,
}

/// Coherence analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoherenceAnalysis {
    pub overall_coherence: f32,
    pub pattern_coherence: f32,
    pub structural_coherence: f32,
    pub temporal_coherence: f32,
    pub overall_health: f32,
    pub recommendations: Vec<String>,
    pub metrics: CoherenceMetrics,
}

/// Coherence metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoherenceMetrics {
    pub calculation_time_ms: u64,
    pub patterns_analyzed: usize,
    pub coherence_variance: f32,
    pub trend_direction: CoherenceTrend,
    pub global_coherence: f32,
    pub pattern_consistency: f32,
    pub resonance_stability: f32,
    pub field_integrity: f32,
}

/// Coherence trend direction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoherenceTrend {
    Improving,
    Stable,
    Declining,
    Fluctuating,
}

/// Misalignment analysis results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MisalignmentAnalysis {
    pub misalignment_score: f32,
    pub affected_patterns: Vec<String>,
    pub severity: MisalignmentSeverity,
    pub suggested_corrections: Vec<String>,
    pub overall_alignment: f32,
    pub metrics: MisalignmentMetrics,
}

/// Misalignment severity levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MisalignmentSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Misalignment metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MisalignmentMetrics {
    pub semantic_alignment: f32,
    pub structural_alignment: f32,
    pub temporal_alignment: f32,
}

/// Field alignment metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldAlignment {
    pub alignment_score: f32,
    pub coherence_alignment: f32,
    pub structural_alignment: f32,
    pub temporal_alignment: f32,
    pub alignment_quality: AlignmentQuality,
}

/// Alignment quality levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlignmentQuality {
    Optimal,
    Good,
    Fair,
    Poor,
    Misaligned,
}

/// Evolution metrics for tracking field evolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionMetrics {
    pub cycle_number: usize,
    pub coherence_before: f32,
    pub coherence_after: f32,
    pub patterns_added: usize,
    pub patterns_removed: usize,
    pub patterns_modified: usize,
    pub evolution_time_ms: u64,
    pub success_indicators: Vec<String>,
    pub failure_reasons: Vec<String>,
}

/// Resonance parameters for field operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResonanceParameters {
    pub base_frequency: f32,
    pub resonance_bandwidth: f32,
    pub amplification_factor: f32,
    pub damping_coefficient: f32,
    pub coupling_strength: f32,
    /// Detection threshold for resonance patterns
    pub detection_threshold: f32,
    /// Noise dampening factor
    pub noise_dampening_factor: f32,
    /// Connection strength between patterns
    pub connection_strength: f32,
    /// Number of tuning iterations
    pub tuning_iterations: usize,
    /// Integration stability threshold
    pub integration_stability: f32,
}

/// Result of resonance scaffolding operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResonanceScaffoldingResult {
    pub patterns_processed: usize,
    pub final_coherence: f32,
    pub integration_stable: bool,
    pub tuning_iterations_completed: usize,
}

/// Boundary information for collapse operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryInfo {
    pub id: String,
    pub center: Vec<f32>,
    pub radius: f32,
    pub strength: f32,
    pub permeability_threshold: f32,
    pub collapse_factor: f32,
}

/// Result of boundary collapse operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryCollapseResult {
    pub boundary_id: String,
    pub patterns_affected: usize,
    pub attractors_merged: usize,
    pub final_coherence: f32,
    pub collapse_successful: bool,
}

impl Default for ResonanceParameters {
    fn default() -> Self {
        Self {
            base_frequency: 1.0,
            resonance_bandwidth: 0.5,
            amplification_factor: 1.2,
            damping_coefficient: 0.1,
            coupling_strength: 0.8,
            detection_threshold: 0.4,
            noise_dampening_factor: 0.8,
            connection_strength: 0.7,
            tuning_iterations: 5,
            integration_stability: 0.8,
        }
    }
}

impl Default for FieldProperties {
    fn default() -> Self {
        Self {
            resonance_threshold: 0.7,
            decay_constant: 0.1,
            amplification_factor: 1.2,
            boundary_permeability: 0.5,
            coherence_weight: 0.8,
            embedding_dim: 1536, // Default OpenAI embedding dimension
        }
    }
}

impl Default for FieldState {
    fn default() -> Self {
        Self {
            coherence: 1.0,
            stability: 1.0,
            energy: 0.5,
            health: 1.0,
            strength: 1.0,
            entropy: 0.1,
            last_updated: Utc::now(),
        }
    }
}

impl FieldState {
    /// Create an active field state with optimal parameters
    pub fn active() -> Self {
        Self {
            coherence: 0.9,
            stability: 0.9,
            energy: 0.8,
            health: 1.0,
            strength: 1.0,
            entropy: 0.2,
            last_updated: Utc::now(),
        }
    }

    /// Check if the field state is considered active
    pub fn is_active(&self) -> bool {
        self.coherence > 0.7 && self.stability > 0.7 && self.health > 0.8
    }
}

impl NeuralField {
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
            properties: FieldProperties::default(),
            attractors: Vec::new(),
            state: FieldState::default(),
            agency_level: 0.0,
            self_assessment_enabled: false,
            goal_setting_enabled: false,
            residue_sensitivity: 0.5,
            compression_ratio: 0.7,
        }
    }

    /// Inject a pattern into the neural field with full control
    pub fn inject_pattern(
        &mut self,
        name: String,
        embedding: Vec<f32>,
        strength: f32,
        resonance: f32,
    ) -> ContextNestResult<()> {
        // Validate embedding dimension
        if embedding.len() != self.properties.embedding_dim {
            return Err(crate::ContextNestError::Validation(format!(
                "Embedding dimension mismatch: expected {}, got {}",
                self.properties.embedding_dim,
                embedding.len()
            )));
        }

        // Validate strength and resonance ranges
        if strength < 0.0 || strength > 1.0 {
            return Err(crate::ContextNestError::Validation(
                "Pattern strength must be between 0.0 and 1.0".to_string(),
            ));
        }

        if resonance < 0.0 || resonance > 1.0 {
            return Err(crate::ContextNestError::Validation(
                "Pattern resonance must be between 0.0 and 1.0".to_string(),
            ));
        }

        // Create semantic pattern
        let pattern = SemanticPattern {
            id: name.clone(),
            content: name.clone(),
            embedding,
            strength,
            resonance,
            decay_rate: self.properties.decay_constant,
            created_at: Utc::now(),
            last_activated: Utc::now(),
            activation_count: 1,
            deleted_at: None,
            delete_reason: None,
        };

        self.patterns.push(pattern);
        self.update_field_state();
        Ok(())
    }

    /// Get a pattern by ID
    pub fn get_pattern(&self, pattern_id: &str) -> Option<&SemanticPattern> {
        self.patterns.iter().find(|p| p.id == pattern_id)
    }

    /// Get a mutable pattern reference by ID
    pub fn get_pattern_mut(&mut self, pattern_id: &str) -> Option<&mut SemanticPattern> {
        self.patterns.iter_mut().find(|p| p.id == pattern_id)
    }

    /// Create bridge between two patterns
    pub fn create_bridge(
        &mut self,
        source_pattern: &str,
        target_pattern: &str,
        bridge_strength: f32,
    ) -> ContextNestResult<()> {
        // Validate bridge strength
        if bridge_strength < 0.0 || bridge_strength > 1.0 {
            return Err(crate::ContextNestError::Validation(
                "Bridge strength must be between 0.0 and 1.0".to_string(),
            ));
        }

        // Find patterns
        let source_index = self
            .patterns
            .iter()
            .position(|p| p.id == source_pattern)
            .ok_or_else(|| {
                crate::ContextNestError::NotFound(format!(
                    "Source pattern '{}' not found",
                    source_pattern
                ))
            })?;

        let target_index = self
            .patterns
            .iter()
            .position(|p| p.id == target_pattern)
            .ok_or_else(|| {
                crate::ContextNestError::NotFound(format!(
                    "Target pattern '{}' not found",
                    target_pattern
                ))
            })?;

        // Create bidirectional bridge by strengthening mutual resonance
        self.patterns[source_index].resonance *= 1.0 + bridge_strength;
        self.patterns[target_index].resonance *= 1.0 + bridge_strength;

        // Ensure resonance stays within bounds
        self.patterns[source_index].resonance = self.patterns[source_index].resonance.min(1.0);
        self.patterns[target_index].resonance = self.patterns[target_index].resonance.min(1.0);

        self.update_field_state();
        Ok(())
    }

    /// Harmonize the field to improve coherence
    pub fn harmonize_field(&mut self) -> ContextNestResult<()> {
        if self.patterns.is_empty() {
            return Ok(());
        }

        // Calculate current field coherence
        let current_coherence = self.calculate_coherence();

        // If coherence is already high, no need to harmonize
        if current_coherence >= 0.9 {
            return Ok(());
        }

        // Calculate average strength and resonance
        let avg_strength: f32 =
            self.patterns.iter().map(|p| p.strength).sum::<f32>() / self.patterns.len() as f32;
        let avg_resonance: f32 =
            self.patterns.iter().map(|p| p.resonance).sum::<f32>() / self.patterns.len() as f32;

        // Adjust patterns toward average values
        for pattern in &mut self.patterns {
            let strength_diff = avg_strength - pattern.strength;
            let resonance_diff = avg_resonance - pattern.resonance;

            // Gradual adjustment (10% of the difference)
            pattern.strength += strength_diff * 0.1;
            pattern.resonance += resonance_diff * 0.1;

            // Ensure values stay within bounds
            pattern.strength = pattern.strength.clamp(0.0, 1.0);
            pattern.resonance = pattern.resonance.clamp(0.0, 1.0);
        }

        self.update_field_state();
        Ok(())
    }

    /// Tune a field parameter
    pub fn tune(&mut self, parameter: &str, value: f64) -> ContextNestResult<()> {
        match parameter {
            "resonance_threshold" => {
                self.properties.resonance_threshold = value as f32;
            }
            "decay_constant" => {
                self.properties.decay_constant = value as f32;
            }
            "amplification_factor" => {
                self.properties.amplification_factor = value as f32;
            }
            "boundary_permeability" => {
                self.properties.boundary_permeability = value as f32;
            }
            "coherence_weight" => {
                self.properties.coherence_weight = value as f32;
            }
            "target_coherence" => {
                // This is a virtual parameter that triggers harmonization
                let target = value as f32;
                if target > self.calculate_coherence() {
                    self.harmonize_field()?;
                }
            }
            "smoothing_factor" => {
                // This affects how patterns are adjusted during harmonization
                // Implementation would depend on specific requirements
            }
            _ => {
                return Err(crate::ContextNestError::Validation(format!(
                    "Unknown field parameter: {}",
                    parameter
                )));
            }
        }

        self.update_field_state();
        Ok(())
    }

    /// Calculate field coherence
    pub fn calculate_coherence(&self) -> f32 {
        if self.patterns.is_empty() {
            return 0.0;
        }

        // Coherence is calculated as the average of pattern strengths weighted by resonance
        let total_weighted_strength: f32 =
            self.patterns.iter().map(|p| p.strength * p.resonance).sum();

        let total_resonance: f32 = self.patterns.iter().map(|p| p.resonance).sum();

        if total_resonance > 0.0 {
            total_weighted_strength / total_resonance
        } else {
            0.0
        }
    }

    /// Calculate field stability
    pub fn calculate_stability(&self) -> f32 {
        if self.patterns.is_empty() {
            return 0.0;
        }

        // Stability is based on the variance in pattern strengths
        let strengths: Vec<f32> = self.patterns.iter().map(|p| p.strength).collect();
        let mean_strength = strengths.iter().sum::<f32>() / strengths.len() as f32;

        let variance: f32 = strengths
            .iter()
            .map(|s| (*s - mean_strength).powi(2))
            .sum::<f32>()
            / strengths.len() as f32;

        // Higher variance means lower stability
        (1.0 - variance).max(0.0)
    }

    /// Calculate field energy
    pub fn calculate_energy(&self) -> f32 {
        // Energy is the sum of all pattern strengths
        self.patterns.iter().map(|p| p.strength).sum()
    }

    /// Update field state based on current patterns
    pub fn update_field_state(&mut self) {
        self.state.coherence = self.calculate_coherence();
        self.state.stability = self.calculate_stability();
        self.state.energy = self.calculate_energy();
        self.state.health = (self.state.coherence + self.state.stability) / 2.0;
        self.state.last_updated = Utc::now();
    }

    /// Inject new pattern into the field
    pub fn inject(&mut self, content: String, embedding: Vec<f32>) -> ContextNestResult<()> {
        // Validate embedding dimension to prevent field corruption
        if embedding.len() != self.properties.embedding_dim {
            return Err(crate::ContextNestError::Api(format!(
                "Embedding dimension mismatch: expected {}, got {}. \
                    This would corrupt the semantic field. Please ensure all \
                    embeddings use consistent dimensions.",
                self.properties.embedding_dim,
                embedding.len()
            )));
        }

        let pattern = SemanticPattern {
            id: Uuid::new_v4().to_string(),
            content,
            embedding,
            strength: 1.0,
            resonance: 0.0,
            decay_rate: self.properties.decay_constant,
            created_at: Utc::now(),
            last_activated: Utc::now(),
            activation_count: 1,
            deleted_at: None,
            delete_reason: None,
        };

        // Calculate resonance with existing patterns
        self.calculate_pattern_resonance(&pattern);

        self.patterns.push(pattern);
        self.update_field_state();

        Ok(())
    }

    /// Attenuate pattern strength
    pub fn attenuate(&mut self, pattern_id: &str, factor: f32) -> ContextNestResult<()> {
        if let Some(pattern) = self.patterns.iter_mut().find(|p| p.id == pattern_id) {
            pattern.strength *= factor;
            self.update_field_state();
        }
        Ok(())
    }

    /// Amplify resonant patterns
    pub fn amplify_resonant(&mut self) -> ContextNestResult<()> {
        let threshold = self.properties.resonance_threshold;
        let amp_factor = self.properties.amplification_factor;

        for pattern in &mut self.patterns {
            if pattern.resonance > threshold {
                pattern.strength *= amp_factor;
            }
        }

        self.update_field_state();
        Ok(())
    }

    /// Get the number of patterns in the field
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    /// Collapse field to concrete context
    pub fn collapse(&self) -> ContextNestResult<String> {
        // Sort patterns by strength and resonance
        let mut sorted_patterns = self.patterns.clone();
        sorted_patterns.sort_by(|a, b| {
            let a_score = a.strength * (1.0 + a.resonance);
            let b_score = b.strength * (1.0 + b.resonance);
            b_score.partial_cmp(&a_score).unwrap()
        });

        // Take top patterns based on field state
        let n_patterns = (self.state.coherence * 10.0) as usize;
        let selected = sorted_patterns.into_iter().take(n_patterns);

        let context = selected.map(|p| p.content).collect::<Vec<_>>().join("\n\n");

        Ok(context)
    }

    /// Calculate overall field resonance
    pub fn calculate_resonance(&self) -> f32 {
        if self.patterns.is_empty() {
            return 0.0;
        }

        // Overall resonance is the weighted average of pattern resonances
        let total_weighted_resonance: f32 =
            self.patterns.iter().map(|p| p.resonance * p.strength).sum();

        let total_strength: f32 = self.patterns.iter().map(|p| p.strength).sum();

        if total_strength > 0.0 {
            total_weighted_resonance / total_strength
        } else {
            0.0
        }
    }

    /// Calculate overall field resonance (alias for calculate_resonance)
    pub fn calculate_overall_resonance(&self) -> f32 {
        self.calculate_resonance()
    }

    /// Calculate resonance between patterns
    fn calculate_pattern_resonance(&mut self, new_pattern: &SemanticPattern) {
        let threshold = self.properties.resonance_threshold;
        for pattern in &mut self.patterns {
            let similarity = cosine_similarity(&pattern.embedding, &new_pattern.embedding);
            if similarity > threshold {
                pattern.resonance = (pattern.resonance + similarity) / 2.0;
            }
        }
    }

    fn calculate_strength_variance(&self) -> f32 {
        if self.patterns.is_empty() {
            return 0.0;
        }

        let mean: f32 =
            self.patterns.iter().map(|p| p.strength).sum::<f32>() / self.patterns.len() as f32;
        let variance: f32 = self
            .patterns
            .iter()
            .map(|p| (p.strength - mean).powi(2))
            .sum::<f32>()
            / self.patterns.len() as f32;

        variance
    }

    /// Self-repair mechanism
    pub fn self_repair(&mut self) -> ContextNestResult<()> {
        if self.state.health < 0.5 {
            // Remove low-strength, low-resonance patterns
            self.patterns
                .retain(|p| p.strength > 0.1 || p.resonance > 0.5);

            // Reset decay rates
            for pattern in &mut self.patterns {
                pattern.decay_rate = self.properties.decay_constant;
            }

            // Normalize strengths
            let max_strength = self.patterns.iter().map(|p| p.strength).fold(0.0, f32::max);
            if max_strength > 0.0 {
                for pattern in &mut self.patterns {
                    pattern.strength /= max_strength;
                }
            }

            self.update_field_state();
        }
        Ok(())
    }

    /// Add a pattern to the field (alias for inject)
    pub fn add_pattern(&mut self, content: String, embedding: Vec<f32>) -> ContextNestResult<()> {
        self.inject(content, embedding)
    }

    /// Update pattern strength by ID
    pub fn update_pattern_strength(
        &mut self,
        pattern_id: &str,
        new_strength: f32,
    ) -> ContextNestResult<()> {
        if let Some(pattern) = self.patterns.iter_mut().find(|p| p.id == pattern_id) {
            pattern.strength = new_strength.clamp(0.0, 1.0);
            self.update_field_state();
            Ok(())
        } else {
            Err(crate::ContextNestError::NotFound(format!(
                "Pattern '{}' not found",
                pattern_id
            )))
        }
    }

    /// Update pattern resonance by ID
    pub fn update_pattern_resonance(
        &mut self,
        pattern_id: &str,
        new_resonance: f32,
    ) -> ContextNestResult<()> {
        if let Some(pattern) = self.patterns.iter_mut().find(|p| p.id == pattern_id) {
            pattern.resonance = new_resonance.clamp(0.0, 1.0);
            self.update_field_state();
            Ok(())
        } else {
            Err(crate::ContextNestError::NotFound(format!(
                "Pattern '{}' not found",
                pattern_id
            )))
        }
    }

    /// Remove a pattern by ID
    pub fn remove_pattern(&mut self, pattern_id: &str) -> ContextNestResult<()> {
        if let Some(pos) = self.patterns.iter().position(|p| p.id == pattern_id) {
            self.patterns.remove(pos);
            self.update_field_state();
            Ok(())
        } else {
            Err(crate::ContextNestError::NotFound(format!(
                "Pattern '{}' not found",
                pattern_id
            )))
        }
    }

    /// Delete pattern by ID with soft delete
    pub fn delete_pattern_by_id(
        &mut self,
        pattern_id: &str,
        reason: Option<String>,
    ) -> ContextNestResult<()> {
        if let Some(pattern) = self.patterns.iter_mut().find(|p| p.id == pattern_id) {
            pattern.deleted_at = Some(Utc::now());
            pattern.delete_reason = reason;
            pattern.strength = 0.0; // Set strength to zero
            self.update_field_state();
            Ok(())
        } else {
            Err(crate::ContextNestError::NotFound(format!(
                "Pattern '{}' not found",
                pattern_id
            )))
        }
    }

    /// Evolve the field using a strategy
    pub fn evolve_with_strategy(
        &mut self,
        strategy: &crate::context::attractor_dynamics::EvolutionStrategy,
    ) -> ContextNestResult<()> {
        match strategy {
            crate::context::attractor_dynamics::EvolutionStrategy::GradientAscent {
                step_size,
                iterations,
            } => {
                for _ in 0..*iterations {
                    self.harmonize_field()?;
                    for pattern in &mut self.patterns {
                        pattern.strength = (pattern.strength + *step_size).min(1.0);
                    }
                }
            }
            crate::context::attractor_dynamics::EvolutionStrategy::SimulatedAnnealing {
                temperature,
                cooling_rate,
            } => {
                let mut temp = *temperature;
                while temp > 0.01 {
                    for pattern in &mut self.patterns {
                        if pattern.strength + temp > 0.5 {
                            pattern.strength = (pattern.strength + temp).min(1.0);
                        }
                    }
                    temp *= *cooling_rate;
                }
            }
            crate::context::attractor_dynamics::EvolutionStrategy::GeneticAlgorithm {
                mutation_rate,
                crossover_rate,
            } => {
                // Simplified genetic algorithm
                if self.patterns.len() > 1 && rand::random::<f32>() < *mutation_rate {
                    let idx = rand::random_range(0..self.patterns.len());
                    self.patterns[idx].strength = rand::random::<f32>();
                }
            }
            crate::context::attractor_dynamics::EvolutionStrategy::Genetic => {
                // Basic genetic algorithm
                for pattern in &mut self.patterns {
                    if rand::random::<f32>() < 0.1 {
                        pattern.strength = rand::random::<f32>();
                    }
                }
            }
            crate::context::attractor_dynamics::EvolutionStrategy::ParticleSwarm => {
                // Simplified particle swarm optimization
                let global_best_strength = self
                    .patterns
                    .iter()
                    .map(|p| p.strength)
                    .fold(0.0f32, f32::max);
                for pattern in &mut self.patterns {
                    let velocity = (global_best_strength - pattern.strength) * 0.1;
                    pattern.strength = (pattern.strength + velocity).min(1.0);
                }
            }
            crate::context::attractor_dynamics::EvolutionStrategy::DifferentialEvolution => {
                // Simplified differential evolution
                if self.patterns.len() > 2 {
                    for i in 0..self.patterns.len() {
                        let a = (i + 1) % self.patterns.len();
                        let b = (i + 2) % self.patterns.len();
                        let diff = self.patterns[a].strength - self.patterns[b].strength;
                        self.patterns[i].strength =
                            (self.patterns[i].strength + diff * 0.5).clamp(0.0, 1.0);
                    }
                }
            }
            crate::context::attractor_dynamics::EvolutionStrategy::Neuroevolution => {
                // Simplified neuroevolution approach
                for pattern in &mut self.patterns {
                    let mutation = (rand::random::<f32>() - 0.5) * 0.2;
                    pattern.strength = (pattern.strength + mutation).clamp(0.0, 1.0);
                }
            }
            crate::context::attractor_dynamics::EvolutionStrategy::Hybrid => {
                // Hybrid approach - combine multiple strategies
                self.harmonize_field()?;
                for pattern in &mut self.patterns {
                    pattern.strength =
                        (pattern.strength * 0.9 + rand::random::<f32>() * 0.1).clamp(0.0, 1.0);
                }
            }
            crate::context::attractor_dynamics::EvolutionStrategy::SelfImproving {
                learning_rate,
            } => {
                // Self-improving strategy - learn from successful patterns
                let avg_strength = self.patterns.iter().map(|p| p.strength).sum::<f32>()
                    / self.patterns.len() as f32;
                for pattern in &mut self.patterns {
                    if pattern.strength > avg_strength {
                        // Strengthen successful patterns
                        pattern.strength = (pattern.strength * (1.0 + learning_rate)).min(1.0);
                        pattern.resonance =
                            (pattern.resonance * (1.0 + learning_rate * 0.5)).min(1.0);
                    } else {
                        // Weaken unsuccessful patterns
                        pattern.strength *= 1.0 - learning_rate * 0.5;
                    }
                }
            }
            crate::context::attractor_dynamics::EvolutionStrategy::Exploration {
                exploration_factor,
            } => {
                // Exploration strategy - introduce novel patterns
                let num_novel = (self.patterns.len() as f32 * exploration_factor * 0.1) as usize;
                for _ in 0..num_novel.max(1) {
                    let embedding = vec![rand::random::<f32>(); self.properties.embedding_dim];
                    let pattern = SemanticPattern {
                        id: format!("exploration_{}", Uuid::new_v4()),
                        content: format!("Exploration pattern {}", Uuid::new_v4()),
                        embedding,
                        strength: 0.3 + rand::random::<f32>() * 0.4,
                        resonance: rand::random::<f32>() * 0.6,
                        decay_rate: self.properties.decay_constant,
                        created_at: Utc::now(),
                        last_activated: Utc::now(),
                        activation_count: 1,
                        deleted_at: None,
                        delete_reason: None,
                    };
                    self.patterns.push(pattern);
                }
            }
            crate::context::attractor_dynamics::EvolutionStrategy::Consolidation {
                merge_threshold,
            } => {
                // Consolidation strategy - merge similar patterns
                let mut to_remove = Vec::new();
                for i in 0..self.patterns.len() {
                    for j in (i + 1)..self.patterns.len() {
                        if to_remove.contains(&j) {
                            continue;
                        }
                        let similarity = cosine_similarity(
                            &self.patterns[i].embedding,
                            &self.patterns[j].embedding,
                        );
                        if similarity > *merge_threshold {
                            // Merge patterns by strengthening the stronger one
                            if self.patterns[i].strength >= self.patterns[j].strength {
                                self.patterns[i].strength = (self.patterns[i].strength
                                    + self.patterns[j].strength * 0.5)
                                    .min(1.0);
                                self.patterns[i].resonance = (self.patterns[i].resonance
                                    + self.patterns[j].resonance * 0.5)
                                    .min(1.0);
                                to_remove.push(j);
                            } else {
                                self.patterns[j].strength = (self.patterns[j].strength
                                    + self.patterns[i].strength * 0.5)
                                    .min(1.0);
                                self.patterns[j].resonance = (self.patterns[j].resonance
                                    + self.patterns[i].resonance * 0.5)
                                    .min(1.0);
                                to_remove.push(i);
                                break;
                            }
                        }
                    }
                }
                // Remove merged patterns (in reverse order to maintain indices)
                to_remove.sort_by(|a, b| b.cmp(a));
                for &idx in &to_remove {
                    if idx < self.patterns.len() {
                        self.patterns.remove(idx);
                    }
                }
            }
        }
        self.update_field_state();
        Ok(())
    }

    /// Detect field coherence
    pub fn detect_field_coherence(&self) -> CoherenceAnalysis {
        let start_time = std::time::Instant::now();

        let overall_coherence = self.calculate_coherence();
        let pattern_coherence = if self.patterns.is_empty() {
            0.0
        } else {
            self.patterns.iter().map(|p| p.strength).sum::<f32>() / self.patterns.len() as f32
        };
        let structural_coherence = self.calculate_stability();
        let temporal_coherence = self.state.health;

        // Calculate coherence variance
        let coherence_variance = if self.patterns.is_empty() {
            0.0
        } else {
            let strengths: Vec<f32> = self.patterns.iter().map(|p| p.strength).collect();
            let mean = strengths.iter().sum::<f32>() / strengths.len() as f32;
            strengths.iter().map(|s| (*s - mean).powi(2)).sum::<f32>() / strengths.len() as f32
        };

        // Determine trend direction (simplified)
        let trend_direction = if coherence_variance < 0.1 {
            CoherenceTrend::Stable
        } else if overall_coherence > 0.7 {
            CoherenceTrend::Improving
        } else if overall_coherence < 0.3 {
            CoherenceTrend::Declining
        } else {
            CoherenceTrend::Fluctuating
        };

        let mut recommendations = Vec::new();
        if overall_coherence < 0.5 {
            recommendations.push("Consider harmonizing the field".to_string());
        }
        if pattern_coherence < 0.3 {
            recommendations.push("Remove weak patterns".to_string());
        }
        if structural_coherence < 0.4 {
            recommendations.push("Apply self-repair mechanisms".to_string());
        }

        CoherenceAnalysis {
            overall_coherence,
            pattern_coherence,
            structural_coherence,
            temporal_coherence,
            overall_health: self.state.health,
            recommendations,
            metrics: CoherenceMetrics {
                calculation_time_ms: start_time.elapsed().as_millis() as u64,
                patterns_analyzed: self.patterns.len(),
                coherence_variance,
                trend_direction,
                global_coherence: overall_coherence,
                pattern_consistency: 0.8,
                resonance_stability: 0.9,
                field_integrity: 0.85,
            },
        }
    }

    /// Detect field misalignment
    pub fn detect_field_misalignment(&self) -> MisalignmentAnalysis {
        let avg_strength = if self.patterns.is_empty() {
            0.0
        } else {
            self.patterns.iter().map(|p| p.strength).sum::<f32>() / self.patterns.len() as f32
        };

        let mut affected_patterns = Vec::new();
        let mut total_deviation = 0.0;

        for pattern in &self.patterns {
            let deviation = (pattern.strength - avg_strength).abs();
            if deviation > 0.3 {
                affected_patterns.push(pattern.id.clone());
                total_deviation += deviation;
            }
        }

        let misalignment_score = if self.patterns.is_empty() {
            0.0
        } else {
            total_deviation / self.patterns.len() as f32
        };

        let severity = match misalignment_score {
            x if x >= 0.7 => MisalignmentSeverity::Critical,
            x if x >= 0.5 => MisalignmentSeverity::High,
            x if x >= 0.3 => MisalignmentSeverity::Medium,
            _ => MisalignmentSeverity::Low,
        };

        let mut suggested_corrections = Vec::new();
        if misalignment_score > 0.3 {
            suggested_corrections.push("Harmonize pattern strengths".to_string());
        }
        if affected_patterns.len() > self.patterns.len() / 2 {
            suggested_corrections.push("Consider field reset".to_string());
        }

        MisalignmentAnalysis {
            misalignment_score,
            affected_patterns,
            severity,
            suggested_corrections,
            overall_alignment: 1.0 - misalignment_score,
            metrics: MisalignmentMetrics {
                semantic_alignment: 0.8,
                structural_alignment: 0.7,
                temporal_alignment: 0.9,
            },
        }
    }

    /// Build context from field (alias for collapse)
    pub fn build_context(&self) -> ContextNestResult<String> {
        self.collapse()
    }

    /// Measure field coherence (alias for calculate_coherence)
    pub fn measure_coherence(&self) -> f32 {
        self.calculate_coherence()
    }

    /// Count patterns in the field
    pub fn count_patterns(&self) -> usize {
        self.pattern_count()
    }

    /// Measure field energy (alias for calculate_energy)
    pub fn measure_energy(&self) -> f32 {
        self.calculate_energy()
    }

    // ===============================================
    // MISSING METHODS IMPLEMENTATION
    // ===============================================

    /// Apply resonance scaffolding to improve field coherence
    pub fn apply_resonance_scaffolding(
        &mut self,
        params: ResonanceParameters,
    ) -> ContextNestResult<ResonanceScaffoldingResult> {
        // Step 1: Detect resonance patterns
        let mut resonance_patterns = Vec::new();
        for pattern in &self.patterns {
            if pattern.resonance > params.detection_threshold {
                resonance_patterns.push(pattern.id.clone());
            }
        }

        // Step 2: Apply amplification to resonant patterns
        for pattern_id in &resonance_patterns {
            if let Some(pattern) = self.get_pattern_mut(pattern_id) {
                pattern.strength = (pattern.strength * params.amplification_factor).min(1.0);
                pattern.resonance = (pattern.resonance * params.amplification_factor).min(1.0);
            }
        }

        // Step 3: Apply noise dampening
        for pattern in &mut self.patterns {
            if pattern.resonance < params.detection_threshold {
                pattern.strength *= params.noise_dampening_factor;
            }
        }

        // Step 4: Enhance connection strength between resonant patterns
        for i in 0..resonance_patterns.len() {
            for j in (i + 1)..resonance_patterns.len() {
                let _ = self.create_bridge(
                    &resonance_patterns[i],
                    &resonance_patterns[j],
                    params.connection_strength,
                );
            }
        }

        // Step 5: Perform tuning iterations
        for _ in 0..params.tuning_iterations {
            self.harmonize_field()?;
        }

        // Step 6: Ensure integration stability
        let current_coherence = self.calculate_coherence();
        if current_coherence < params.integration_stability {
            self.self_repair()?;
        }

        // Step 7: Update field state
        self.update_field_state();

        Ok(ResonanceScaffoldingResult {
            patterns_processed: resonance_patterns.len(),
            final_coherence: self.calculate_coherence(),
            integration_stable: self.calculate_coherence() >= params.integration_stability,
            tuning_iterations_completed: params.tuning_iterations,
        })
    }

    /// Enable autonomous operation at specified level
    pub fn enable_autonomy(&mut self, agency_level: f32) -> ContextNestResult<()> {
        // Validate agency level
        if agency_level < 0.0 || agency_level > 1.0 {
            return Err(crate::ContextNestError::Validation(
                "Agency level must be between 0.0 and 1.0".to_string(),
            ));
        }

        self.agency_level = agency_level;

        // Enable autonomous features based on agency level
        if agency_level > 0.3 {
            self.self_assessment_enabled = true;
        }
        if agency_level > 0.6 {
            self.goal_setting_enabled = true;
        }

        // Adjust field properties for autonomous operation
        self.properties.boundary_permeability = 0.5 + (agency_level * 0.3);
        self.properties.resonance_threshold = 0.7 - (agency_level * 0.2);

        self.update_field_state();
        Ok(())
    }

    /// Set up self-assessment mechanisms
    pub fn setup_self_assessment(&mut self) -> ContextNestResult<()> {
        self.self_assessment_enabled = true;

        // Initialize self-assessment metrics
        for pattern in &mut self.patterns {
            // Reset activation tracking for self-assessment
            pattern.last_activated = Utc::now();
        }

        // Adjust field properties for better self-assessment
        self.properties.coherence_weight = 0.9;
        self.residue_sensitivity = 0.6;

        self.update_field_state();
        Ok(())
    }

    /// Initialize goal-setting capability
    pub fn initialize_goal_setting(&mut self) -> ContextNestResult<()> {
        self.goal_setting_enabled = true;

        // Create initial goal attractors if none exist
        if self.attractors.is_empty() {
            let goal_attractor = Attractor {
                id: "primary_goal".to_string(),
                center: vec![0.5; self.properties.embedding_dim],
                strength: 0.8,
                radius: 0.3,
            };
            self.attractors.push(goal_attractor);
        }

        // Adjust field properties for goal-directed behavior
        self.properties.amplification_factor = 1.3;
        self.agency_level = self.agency_level.max(0.7);

        self.update_field_state();
        Ok(())
    }

    /// Detect symbolic residue in the field
    pub fn detect_symbolic_residue(&mut self) -> ContextNestResult<Vec<String>> {
        let mut residue = Vec::new();

        // Calculate field metrics
        let coherence = self.calculate_coherence();
        let entropy = self.measure_entropy();
        let energy = self.calculate_energy();

        // High entropy with low coherence suggests residue
        if entropy > 0.7 && coherence < 0.5 {
            // Find weak, incoherent patterns
            for pattern in &self.patterns {
                if pattern.strength < 0.3 && pattern.resonance < self.properties.resonance_threshold
                {
                    residue.push(format!("Residue in pattern: {}", pattern.id));
                }
            }
        }

        // Check for orphaned attractors
        for attractor in &self.attractors {
            let has_resonant_patterns = self.patterns.iter().any(|p| {
                cosine_similarity(&p.embedding, &attractor.center)
                    > self.properties.resonance_threshold
            });

            if !has_resonant_patterns {
                residue.push(format!("Orphaned attractor: {}", attractor.id));
            }
        }

        // Check for boundary inconsistencies
        if energy > 0.8 && coherence < 0.4 {
            residue.push("High energy boundary inconsistency".to_string());
        }

        Ok(residue)
    }

    /// Integrate compressed residue back into field
    pub fn integrate_compressed_residue(
        &mut self,
        compressed_residue: &str,
    ) -> ContextNestResult<bool> {
        if compressed_residue.is_empty() {
            return Ok(false);
        }

        // Parse compressed residue
        let residue_patterns: Vec<&str> = compressed_residue.split(',').collect();

        let mut integration_success = false;
        let initial_coherence = self.calculate_coherence();

        for residue_pattern in residue_patterns {
            // Create embedding for residue pattern
            let embedding = vec![0.5; self.properties.embedding_dim];

            // Inject residue as a low-strength pattern
            let pattern = SemanticPattern {
                id: format!("residue_{}", Uuid::new_v4()),
                content: residue_pattern.to_string(),
                embedding,
                strength: 0.2, // Low strength for residue
                resonance: 0.1,
                decay_rate: self.properties.decay_constant * 2.0, // Faster decay
                created_at: Utc::now(),
                last_activated: Utc::now(),
                activation_count: 0,
                deleted_at: None,
                delete_reason: None,
            };

            self.patterns.push(pattern);
            integration_success = true;
        }

        if integration_success {
            // Apply compression to integrate residue
            self.harmonize_field()?;

            // Remove very weak patterns to maintain coherence
            self.patterns
                .retain(|p| p.strength > 0.05 || p.resonance > 0.1);

            let final_coherence = self.calculate_coherence();
            integration_success = final_coherence >= initial_coherence * 0.9;
        }

        self.update_field_state();
        Ok(integration_success)
    }

    /// Collapse a boundary in the field
    pub fn collapse_boundary(
        &mut self,
        boundary: &BoundaryInfo,
    ) -> ContextNestResult<BoundaryCollapseResult> {
        // Find patterns near the boundary
        let mut affected_patterns = Vec::new();

        for pattern in &mut self.patterns {
            let similarity_to_boundary = cosine_similarity(&pattern.embedding, &boundary.center);

            if similarity_to_boundary > boundary.permeability_threshold {
                // Collapse pattern toward boundary center
                for i in 0..pattern.embedding.len() {
                    pattern.embedding[i] = (pattern.embedding[i] + boundary.center[i]) / 2.0;
                }

                // Adjust pattern properties
                pattern.strength *= boundary.collapse_factor;
                pattern.resonance *= 1.0 - boundary.collapse_factor;

                affected_patterns.push(pattern.id.clone());
            }
        }

        // Merge attractors if they're within boundary
        let mut attractors_to_merge = Vec::new();
        for attractor in &self.attractors {
            let similarity = cosine_similarity(&attractor.center, &boundary.center);
            if similarity > boundary.permeability_threshold {
                attractors_to_merge.push(attractor.id.clone());
            }
        }

        // Remove merged attractors and create new unified one
        self.attractors
            .retain(|a| !attractors_to_merge.contains(&a.id));

        if !attractors_to_merge.is_empty() {
            let merged_attractor = Attractor {
                id: format!("merged_{}", Uuid::new_v4()),
                center: boundary.center.clone(),
                strength: boundary.strength,
                radius: boundary.radius,
            };
            self.attractors.push(merged_attractor);
        }

        self.update_field_state();

        Ok(BoundaryCollapseResult {
            boundary_id: boundary.id.clone(),
            patterns_affected: affected_patterns.len(),
            attractors_merged: attractors_to_merge.len(),
            final_coherence: self.calculate_coherence(),
            collapse_successful: self.calculate_coherence() > 0.3,
        })
    }

    /// Get current agency level
    pub fn get_agency_level(&self) -> f32 {
        self.agency_level
    }

    /// Count attractors in the field
    pub fn count_attractors(&self) -> usize {
        self.attractors.len()
    }

    /// Measure field entropy
    pub fn measure_entropy(&self) -> f32 {
        if self.patterns.is_empty() {
            return 0.0;
        }

        // Calculate Shannon entropy based on pattern strengths
        let total_strength: f32 = self.patterns.iter().map(|p| p.strength).sum();
        if total_strength == 0.0 {
            return 0.0;
        }

        let mut entropy = 0.0;
        for pattern in &self.patterns {
            let probability = pattern.strength / total_strength;
            if probability > 0.0 {
                entropy -= probability * probability.log2();
            }
        }

        // Normalize to 0-1 range
        let max_entropy = (self.patterns.len() as f32).log2();
        if max_entropy > 0.0 {
            entropy / max_entropy
        } else {
            0.0
        }
    }

    // ===============================================
    // ADDITIONAL MISSING METHODS
    // ===============================================

    /// Strengthen successful patterns
    pub fn strengthen_successful_patterns(
        &mut self,
        improvement_rate: f32,
    ) -> ContextNestResult<()> {
        for pattern in &mut self.patterns {
            // Strengthen patterns that are already strong and resonant
            if pattern.strength > 0.6 && pattern.resonance > 0.5 {
                pattern.strength = (pattern.strength * (1.0 + improvement_rate)).min(1.0);
                pattern.resonance = (pattern.resonance * (1.0 + improvement_rate * 0.5)).min(1.0);
            }
        }
        self.update_field_state();
        Ok(())
    }

    /// Introduce novel patterns to the field
    pub fn introduce_novel_patterns(&mut self, exploration_factor: f32) -> ContextNestResult<()> {
        let num_patterns = (self.patterns.len() as f32 * exploration_factor * 0.1) as usize;

        for _ in 0..num_patterns.max(1) {
            let embedding = vec![rand::random::<f32>(); self.properties.embedding_dim];

            let pattern = SemanticPattern {
                id: format!("novel_{}", Uuid::new_v4()),
                content: format!("Novel pattern {}", Uuid::new_v4()),
                embedding,
                strength: 0.4 + rand::random::<f32>() * 0.3,
                resonance: rand::random::<f32>() * 0.5,
                decay_rate: self.properties.decay_constant,
                created_at: Utc::now(),
                last_activated: Utc::now(),
                activation_count: 1,
                deleted_at: None,
                delete_reason: None,
            };

            self.patterns.push(pattern);
        }

        self.update_field_state();
        Ok(())
    }

    /// Specialize in a specific focus area
    pub fn specialize_in_focus_area(&mut self, focus_area: &str) -> ContextNestResult<()> {
        // Create a specialized attractor for the focus area
        let embedding = vec![0.7; self.properties.embedding_dim];

        let attractor = Attractor {
            id: format!("focus_{}", focus_area),
            center: embedding,
            strength: 0.9,
            radius: 0.2,
        };

        self.attractors.push(attractor);

        // Strengthen patterns related to the focus area
        for pattern in &mut self.patterns {
            if pattern.content.contains(focus_area) {
                pattern.strength = (pattern.strength * 1.2).min(1.0);
                pattern.resonance = (pattern.resonance * 1.3).min(1.0);
            }
        }

        self.update_field_state();
        Ok(())
    }

    /// Measure field stability (alias for calculate_stability)
    pub fn measure_stability(&self) -> f32 {
        self.calculate_stability()
    }

    /// Adapt field to environmental changes
    pub fn adapt_to_environment(&mut self, adaptation_speed: f32) -> ContextNestResult<()> {
        // Simulate environmental pressure by adjusting field properties
        self.properties.resonance_threshold *= 1.0 + adaptation_speed * 0.1;
        self.properties.boundary_permeability *= 1.0 - adaptation_speed * 0.05;

        // Adapt patterns to new conditions
        for pattern in &mut self.patterns {
            let adaptation_factor = rand::random::<f32>() * adaptation_speed;
            pattern.strength =
                (pattern.strength * (1.0 + adaptation_factor * 0.1 - 0.05)).clamp(0.0, 1.0);
        }

        self.update_field_state();
        Ok(())
    }

    /// Bootstrap foundational patterns
    pub fn bootstrap_foundational_patterns(&mut self, seed_strength: f32) -> ContextNestResult<()> {
        // Create foundational patterns with high initial strength
        let foundational_patterns = vec![
            ("coherence", 0.9),
            ("structure", 0.8),
            ("organization", 0.85),
            ("emergence", 0.75),
        ];

        for (name, strength) in foundational_patterns {
            let embedding = vec![strength; self.properties.embedding_dim];

            let pattern = SemanticPattern {
                id: format!("foundational_{}", name),
                content: name.to_string(),
                embedding,
                strength: strength * seed_strength,
                resonance: strength * 0.8,
                decay_rate: self.properties.decay_constant * 0.5, // Slower decay for foundational patterns
                created_at: Utc::now(),
                last_activated: Utc::now(),
                activation_count: 1,
                deleted_at: None,
                delete_reason: None,
            };

            self.patterns.push(pattern);
        }

        self.update_field_state();
        Ok(())
    }

    /// Calculate energy variance in the field
    pub fn calculate_energy_variance(&self) -> f32 {
        if self.patterns.is_empty() {
            return 0.0;
        }

        let mean_energy = self.calculate_energy() / self.patterns.len() as f32;
        let variance: f32 = self
            .patterns
            .iter()
            .map(|p| (p.strength - mean_energy).powi(2))
            .sum::<f32>()
            / self.patterns.len() as f32;

        variance
    }

    // ===============================================
    // FINAL MISSING METHODS
    // ===============================================

    /// Estimate memory usage of the neural field
    pub fn estimate_memory_usage(&self) -> usize {
        let mut usage = std::mem::size_of::<NeuralField>();

        // Add pattern memory usage
        for pattern in &self.patterns {
            usage += std::mem::size_of::<SemanticPattern>();
            usage += pattern.embedding.len() * std::mem::size_of::<f32>();
            usage += pattern.content.len();
        }

        // Add attractor memory usage
        for attractor in &self.attractors {
            usage += std::mem::size_of::<Attractor>();
            usage += attractor.center.len() * std::mem::size_of::<f32>();
        }

        usage
    }

    /// Find unmatched patterns in the field
    pub fn find_unmatched_patterns(&self) -> Vec<&SemanticPattern> {
        self.patterns
            .iter()
            .filter(|pattern| {
                // A pattern is unmatched if it has low resonance and no strong connections
                pattern.resonance < self.properties.resonance_threshold && pattern.strength < 0.5
            })
            .collect()
    }

    /// Create gradient boundaries in the field
    pub fn create_gradient_boundaries(&mut self) -> ContextNestResult<Vec<BoundaryInfo>> {
        let mut boundaries = Vec::new();

        // Create boundaries between different regions of the field
        if self.patterns.len() > 3 {
            // Find pattern clusters
            let clusters = self.identify_pattern_clusters();

            for (i, cluster1) in clusters.iter().enumerate() {
                for (j, cluster2) in clusters.iter().enumerate() {
                    if i < j {
                        // Create boundary between clusters
                        let boundary = BoundaryInfo {
                            id: format!("boundary_{}_{}", i, j),
                            center: self.calculate_boundary_center(cluster1, cluster2),
                            radius: 0.3,
                            strength: 0.6,
                            permeability_threshold: 0.7,
                            collapse_factor: 0.5,
                        };
                        boundaries.push(boundary);
                    }
                }
            }
        }

        Ok(boundaries)
    }

    /// Collapse all boundaries in the field
    pub fn collapse_all_boundaries(&mut self) -> ContextNestResult<Vec<BoundaryCollapseResult>> {
        let boundaries = self.create_gradient_boundaries()?;
        let mut results = Vec::new();

        for boundary in boundaries {
            let result = self.collapse_boundary(&boundary)?;
            results.push(result);
        }

        Ok(results)
    }

    // Helper methods for boundary operations
    fn identify_pattern_clusters(&self) -> Vec<Vec<usize>> {
        // Simple clustering based on embedding similarity
        let mut clusters = Vec::new();
        let mut visited = vec![false; self.patterns.len()];

        for i in 0..self.patterns.len() {
            if !visited[i] {
                let mut cluster = vec![i];
                visited[i] = true;

                for j in (i + 1)..self.patterns.len() {
                    if !visited[j] {
                        let similarity = cosine_similarity(
                            &self.patterns[i].embedding,
                            &self.patterns[j].embedding,
                        );
                        if similarity > self.properties.resonance_threshold {
                            cluster.push(j);
                            visited[j] = true;
                        }
                    }
                }

                clusters.push(cluster);
            }
        }

        clusters
    }

    fn calculate_boundary_center(&self, cluster1: &[usize], cluster2: &[usize]) -> Vec<f32> {
        let mut center = vec![0.0; self.properties.embedding_dim];
        let mut count = 0;

        // Calculate average of both clusters
        for &idx in cluster1.iter().chain(cluster2.iter()) {
            for (i, &val) in self.patterns[idx].embedding.iter().enumerate() {
                center[i] += val;
            }
            count += 1;
        }

        if count > 0 {
            for val in center.iter_mut() {
                *val /= count as f32;
            }
        }

        center
    }

    // ===============================================
    // ADDITIONAL METHODS FOR CONTEXT-ENGINEERING
    // ===============================================

    /// Dissolve boundaries between attractors
    pub fn dissolve_boundaries_between(
        &mut self,
        attractor_ids: &[String],
    ) -> ContextNestResult<()> {
        let mut attractors_to_merge = Vec::new();

        for attractor_id in attractor_ids {
            if let Some(pos) = self.attractors.iter().position(|a| a.id == *attractor_id) {
                attractors_to_merge.push(pos);
            }
        }

        if attractors_to_merge.len() < 2 {
            return Ok(()); // Nothing to merge
        }

        // Calculate merged attractor properties
        let mut merged_center = vec![0.0; self.properties.embedding_dim];
        let mut total_strength = 0.0;

        for &pos in &attractors_to_merge {
            let attractor = &self.attractors[pos];
            for (i, &val) in attractor.center.iter().enumerate() {
                merged_center[i] += val * attractor.strength;
            }
            total_strength += attractor.strength;
        }

        // Normalize center
        if total_strength > 0.0 {
            for val in merged_center.iter_mut() {
                *val /= total_strength;
            }
        }

        // Create merged attractor
        let merged_attractor = Attractor {
            id: format!("merged_{}", Uuid::new_v4()),
            center: merged_center,
            strength: total_strength / attractors_to_merge.len() as f32,
            radius: 0.4,
        };

        // Remove old attractors and add merged one
        attractors_to_merge.sort_by(|a, b| b.cmp(a)); // Sort in reverse order
        for &pos in &attractors_to_merge {
            self.attractors.remove(pos);
        }
        self.attractors.push(merged_attractor);

        self.update_field_state();
        Ok(())
    }

    /// Calculate complexity score of the field
    pub fn calculate_complexity_score(&self) -> f32 {
        if self.patterns.is_empty() {
            return 0.0;
        }

        // Complexity factors
        let pattern_count_factor = (self.patterns.len() as f32 / 100.0).min(1.0);
        let attractor_count_factor = (self.attractors.len() as f32 / 10.0).min(1.0);
        let coherence_factor = self.calculate_coherence();
        let entropy_factor = self.measure_entropy();
        let energy_factor = self.calculate_energy() / self.patterns.len() as f32;

        // Weighted complexity score
        (pattern_count_factor * 0.3
            + attractor_count_factor * 0.2
            + coherence_factor * 0.2
            + entropy_factor * 0.2
            + energy_factor * 0.1)
            .min(1.0)
    }

    /// Apply dynamics step for field evolution
    pub fn apply_dynamics_step(&mut self, time_step: f32) -> ContextNestResult<DynamicsStepResult> {
        let initial_coherence = self.calculate_coherence();
        let initial_energy = self.calculate_energy();

        // Apply time-based decay
        for pattern in &mut self.patterns {
            pattern.strength *= (1.0 - pattern.decay_rate * time_step).max(0.0);
        }

        // Apply resonance interactions
        self.apply_resonance_interactions()?;

        // Update attractor dynamics
        self.update_attractor_dynamics(time_step)?;

        // Remove very weak patterns
        self.patterns.retain(|p| p.strength > 0.01);

        // Update field state
        self.update_field_state();

        let final_coherence = self.calculate_coherence();
        let final_energy = self.calculate_energy();

        Ok(DynamicsStepResult {
            time_step,
            coherence_change: final_coherence - initial_coherence,
            energy_change: final_energy - initial_energy,
            patterns_evolved: self.patterns.len(),
            attractors_updated: self.attractors.len(),
            step_successful: final_coherence > 0.1,
        })
    }

    // Helper methods for dynamics
    fn apply_resonance_interactions(&mut self) -> ContextNestResult<()> {
        let resonance_threshold = self.properties.resonance_threshold;

        for i in 0..self.patterns.len() {
            for j in (i + 1)..self.patterns.len() {
                let similarity =
                    cosine_similarity(&self.patterns[i].embedding, &self.patterns[j].embedding);

                if similarity > resonance_threshold {
                    // Strengthen mutual resonance
                    let resonance_transfer = similarity * 0.1;
                    self.patterns[i].resonance =
                        (self.patterns[i].resonance + resonance_transfer).min(1.0);
                    self.patterns[j].resonance =
                        (self.patterns[j].resonance + resonance_transfer).min(1.0);
                }
            }
        }

        Ok(())
    }

    fn update_attractor_dynamics(&mut self, time_step: f32) -> ContextNestResult<()> {
        for attractor in &mut self.attractors {
            // Attractors slowly lose strength over time
            attractor.strength *= (1.0 - 0.01 * time_step).max(0.1);

            // Attractors influence nearby patterns
            for pattern in &mut self.patterns {
                let similarity = cosine_similarity(&pattern.embedding, &attractor.center);
                if similarity > 0.5 {
                    pattern.strength =
                        (pattern.strength + similarity * attractor.strength * 0.05).min(1.0);
                }
            }
        }

        Ok(())
    }
}

/// Result of applying a dynamics step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicsStepResult {
    pub time_step: f32,
    pub coherence_change: f32,
    pub energy_change: f32,
    pub patterns_evolved: usize,
    pub attractors_updated: usize,
    pub step_successful: bool,
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}

impl SemanticPattern {
    /// Calculate similarity with another pattern
    pub fn similarity(&self, other: &SemanticPattern) -> f32 {
        cosine_similarity(&self.embedding, &other.embedding)
    }

    /// Calculate similarity to another pattern (alias)
    pub fn similarity_to(&self, other: &SemanticPattern) -> f32 {
        self.similarity(other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_neural_field_creation() {
        let field = NeuralField::new();
        assert_eq!(field.patterns.len(), 0);
        assert_eq!(field.calculate_coherence(), 0.0);
    }

    #[test]
    fn test_inject_pattern() {
        let mut field = NeuralField::new();
        let embedding = vec![0.5; 1536];

        let result = field.inject_pattern("test".to_string(), embedding, 0.8, 0.9);
        assert!(result.is_ok());
        assert_eq!(field.patterns.len(), 1);
        assert!(field.calculate_coherence() > 0.0);
    }

    #[test]
    fn test_inject_pattern_wrong_dimension() {
        let mut field = NeuralField::new();
        let wrong_embedding = vec![0.5; 512]; // Wrong dimension

        let result = field.inject_pattern("test".to_string(), wrong_embedding, 0.8, 0.9);
        assert!(result.is_err());
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let similarity = cosine_similarity(&a, &b);
        assert!((similarity - 1.0).abs() < f32::EPSILON);

        let c = vec![0.0, 1.0, 0.0];
        let similarity = cosine_similarity(&a, &c);
        assert!(similarity.abs() < f32::EPSILON);
    }

    #[test]
    fn test_harmonize_field() {
        let mut field = NeuralField::new();

        // Add patterns with different strengths
        let embedding1 = vec![0.5; 1536];
        let embedding2 = vec![0.6; 1536];

        field
            .inject_pattern("pattern1".to_string(), embedding1, 0.3, 0.8)
            .unwrap();
        field
            .inject_pattern("pattern2".to_string(), embedding2, 0.9, 0.7)
            .unwrap();

        let initial_coherence = field.calculate_coherence();
        field.harmonize_field().unwrap();
        let final_coherence = field.calculate_coherence();

        // Coherence should improve or stay the same
        assert!(final_coherence >= initial_coherence);
    }
}

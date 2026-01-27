//! Field Pattern Recognition Module
//! Provides pattern recognition capabilities for field-based data structures
//! and neural field activations.

use crate::error::ContextNestResult;
use crate::error::{ContextNestError, Result};
use serde::{Deserialize, Serialize};

/// Field pattern descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldPattern {
    /// Pattern name
    pub name: String,
    /// Pattern type
    pub pattern_type: FieldPatternType,
    /// Pattern strength (0.0 to 1.0)
    pub strength: f32,
    /// Pattern resonance level
    pub resonance: f32,
    /// Pattern metadata
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

/// Types of field patterns
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FieldPatternType {
    /// Semantic patterns based on meaning
    Semantic,
    /// Structural patterns based on data organization
    Structural,
    /// Temporal patterns based on time sequences
    Temporal,
    /// Spatial patterns based on position/distribution
    Spatial,
    /// Frequency patterns based on occurrence
    Frequency,
}

/// Field pattern extractor
pub struct FieldPatternExtractor {
    /// Minimum pattern strength threshold
    min_strength: f32,
    /// Maximum number of patterns to extract
    max_patterns: usize,
    /// Pattern types to extract
    enabled_types: std::collections::HashSet<FieldPatternType>,
}

impl FieldPatternExtractor {
    /// Create a new field pattern extractor
    pub fn new() -> Self {
        Self {
            min_strength: 0.1,
            max_patterns: 10,
            enabled_types: std::collections::HashSet::from([
                FieldPatternType::Semantic,
                FieldPatternType::Structural,
                FieldPatternType::Temporal,
            ]),
        }
    }

    /// Extract patterns from field data
    pub fn extract_patterns(&self, field_data: &[f32]) -> ContextNestResult<Vec<FieldPattern>> {
        if field_data.is_empty() {
            return Ok(Vec::new());
        }

        let mut patterns = Vec::new();

        // Extract different pattern types based on enabled types
        if self.enabled_types.contains(&FieldPatternType::Semantic) {
            patterns.extend(self.extract_semantic_patterns(field_data)?);
        }

        if self.enabled_types.contains(&FieldPatternType::Structural) {
            patterns.extend(self.extract_structural_patterns(field_data)?);
        }

        if self.enabled_types.contains(&FieldPatternType::Temporal) {
            patterns.extend(self.extract_temporal_patterns(field_data)?);
        }

        // Filter by strength and limit by max_patterns
        patterns.sort_by(|a, b| b.strength.partial_cmp(&a.strength).unwrap());
        patterns.retain(|p| p.strength >= self.min_strength);
        patterns.truncate(self.max_patterns);

        Ok(patterns)
    }

    /// Extract semantic patterns
    fn extract_semantic_patterns(
        &self,
        field_data: &[f32],
    ) -> ContextNestResult<Vec<FieldPattern>> {
        let mut patterns = Vec::new();

        // Simple semantic pattern detection based on activation clusters
        let clusters = self.find_activation_clusters(field_data, 0.5);
        for (i, cluster) in clusters.iter().enumerate() {
            if cluster.len() >= 3 {
                // Calculate mean activation from actual field values
                let mean = cluster
                    .iter()
                    .filter_map(|&idx| field_data.get(idx))
                    .sum::<f32>()
                    / cluster.len() as f32;

                patterns.push(FieldPattern {
                    name: format!("semantic_cluster_{}", i),
                    pattern_type: FieldPatternType::Semantic,
                    strength: cluster.len() as f32 / field_data.len() as f32,
                    resonance: self.calculate_cluster_coherence(cluster),
                    metadata: std::collections::HashMap::from([
                        (
                            "cluster_size".to_string(),
                            serde_json::Value::Number(cluster.len().into()),
                        ),
                        (
                            "mean_activation".to_string(),
                            serde_json::Value::Number(
                                serde_json::Number::from_f64(mean as f64).ok_or_else(|| {
                                    ContextNestError::Validation("Invalid mean value".into())
                                })?,
                            ),
                        ),
                    ]),
                });
            }
        }

        Ok(patterns)
    }

    /// Extract structural patterns
    fn extract_structural_patterns(
        &self,
        field_data: &[f32],
    ) -> ContextNestResult<Vec<FieldPattern>> {
        let mut patterns = Vec::new();

        // Detect periodic structures
        let periodicity = self.detect_periodicity(field_data);
        if periodicity > 0 {
            patterns.push(FieldPattern {
                name: "periodic_structure".to_string(),
                pattern_type: FieldPatternType::Structural,
                strength: 0.7,
                resonance: periodicity as f32, // Cast usize to f32
                metadata: std::collections::HashMap::from([(
                    "period".to_string(),
                    serde_json::Value::Number(periodicity.into()),
                )]),
            });
        }

        // Detect symmetry patterns
        let symmetry_score = self.calculate_symmetry_score(field_data);
        if symmetry_score > 0.5 {
            patterns.push(FieldPattern {
                name: "symmetry_pattern".to_string(),
                pattern_type: FieldPatternType::Structural,
                strength: symmetry_score,
                resonance: symmetry_score * 0.8,
                metadata: std::collections::HashMap::from([(
                    "symmetry_score".to_string(),
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(symmetry_score as f64).unwrap(),
                    ),
                )]),
            });
        }

        Ok(patterns)
    }

    /// Extract temporal patterns
    fn extract_temporal_patterns(
        &self,
        field_data: &[f32],
    ) -> ContextNestResult<Vec<FieldPattern>> {
        let mut patterns = Vec::new();

        // Detect trends and patterns in the sequence
        if field_data.len() >= 5 {
            let trend = self.calculate_trend(field_data);
            patterns.push(FieldPattern {
                name: "temporal_trend".to_string(),
                pattern_type: FieldPatternType::Temporal,
                strength: trend.abs(),
                resonance: 0.6,
                metadata: std::collections::HashMap::from([
                    (
                        "trend_direction".to_string(),
                        serde_json::Value::String(if trend > 0.0 {
                            "increasing".to_string()
                        } else {
                            "decreasing".to_string()
                        }),
                    ),
                    (
                        "trend_strength".to_string(),
                        serde_json::Value::Number(
                            serde_json::Number::from_f64(trend as f64).unwrap(),
                        ),
                    ),
                ]),
            });
        }

        Ok(patterns)
    }

    /// Helper methods for pattern extraction
    fn find_activation_clusters(&self, data: &[f32], threshold: f32) -> Vec<Vec<usize>> {
        let mut clusters = Vec::new();
        let mut current_cluster = Vec::new();

        for (i, &value) in data.iter().enumerate() {
            if value >= threshold {
                current_cluster.push(i);
            } else if !current_cluster.is_empty() {
                clusters.push(current_cluster.clone());
                current_cluster.clear();
            }
        }

        if !current_cluster.is_empty() {
            clusters.push(current_cluster);
        }

        clusters
    }

    fn calculate_cluster_coherence(&self, cluster: &[usize]) -> f32 {
        if cluster.len() < 2 {
            return 1.0;
        }

        // Simple coherence based on cluster density
        let span = cluster[cluster.len() - 1] - cluster[0] + 1;
        cluster.len() as f32 / span as f32
    }

    fn detect_periodicity(&self, data: &[f32]) -> usize {
        if data.len() < 6 {
            return 0;
        }

        let mut best_period = 0;
        let mut best_correlation = 0.0;

        for period in 2..data.len() / 2 {
            let mut correlation = 0.0;
            let mut count = 0;

            for i in period..data.len() {
                correlation += (data[i] - data[i - period]).abs();
                count += 1;
            }

            if count > 0 {
                correlation = 1.0 / (1.0 + correlation / count as f32);
                if correlation > best_correlation {
                    best_correlation = correlation;
                    best_period = period;
                }
            }
        }

        if best_correlation > 0.7 {
            best_period
        } else {
            0
        }
    }

    fn calculate_symmetry_score(&self, data: &[f32]) -> f32 {
        let n = data.len();
        if n < 2 {
            return 0.0;
        }

        let mut symmetry_sum = 0.0;
        for i in 0..n / 2 {
            symmetry_sum += 1.0 - (data[i] - data[n - 1 - i]).abs();
        }

        symmetry_sum / (n / 2) as f32
    }

    fn calculate_trend(&self, data: &[f32]) -> f32 {
        if data.len() < 2 {
            return 0.0;
        }

        let n = data.len() as f32;
        let sum_x: f32 = (0..data.len()).map(|i| i as f32).sum();
        let sum_y: f32 = data.iter().sum();
        let sum_xy: f32 = data.iter().enumerate().map(|(i, &y)| i as f32 * y).sum();
        let sum_x2: f32 = (0..data.len()).map(|i| (i as f32).powi(2)).sum();

        let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_x2 - sum_x.powi(2));
        slope
    }

    /// Set minimum pattern strength
    pub fn set_min_strength(&mut self, min_strength: f32) {
        self.min_strength = min_strength.clamp(0.0, 1.0);
    }

    /// Set maximum number of patterns
    pub fn set_max_patterns(&mut self, max_patterns: usize) {
        self.max_patterns = max_patterns;
    }

    /// Enable specific pattern types
    pub fn enable_pattern_type(&mut self, pattern_type: FieldPatternType) {
        self.enabled_types.insert(pattern_type);
    }

    /// Disable specific pattern types
    pub fn disable_pattern_type(&mut self, pattern_type: FieldPatternType) {
        self.enabled_types.remove(&pattern_type);
    }
}

impl Default for FieldPatternExtractor {
    fn default() -> Self {
        Self::new()
    }
}

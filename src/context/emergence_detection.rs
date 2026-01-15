/// Emergence Detection implementation for Recursive Emergence Protocol
/// This module implements pattern detection for emergent behaviors including
/// recursive capabilities, novel concepts, and self-improvement patterns.
use crate::context::field::{NeuralField, SemanticPattern};
use crate::error::{ContextNestError, ContextNestResult};
use crate::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Types of emergent patterns that can be detected
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EmergenceType {
    /// System developing recursive/self-referential capabilities
    RecursiveCapability { depth: usize, complexity: f32 },
    /// Novel concepts not present in training/initial patterns
    NovelConcept {
        novelty_score: f32,
        semantic_distance: f32,
    },
    /// Self-improvement patterns (meta-learning)
    SelfImprovement {
        improvement_rate: f32,
        metric: String,
    },
    /// Unexpected pattern combinations
    SyntheticPattern {
        source_patterns: Vec<String>,
        synthesis_score: f32,
    },
    /// Cross-domain concept transfer
    ConceptTransfer {
        source_domain: String,
        target_domain: String,
        transfer_strength: f32,
    },
    /// Higher-order abstraction formation
    AbstractionFormation {
        abstraction_level: usize,
        generalization_score: f32,
    },
}

/// Detected emergence event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergenceEvent {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub emergence_type: EmergenceType,
    pub confidence: f32,
    pub evidence: Vec<String>,
    pub affected_patterns: Vec<String>,
    pub implications: Vec<String>,
}

/// Configuration for emergence detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergenceDetectionConfig {
    /// Sensitivity for detection (0.0 = low, 1.0 = high)
    pub sensitivity: f32,
    /// Minimum confidence threshold for reporting
    pub min_confidence: f32,
    /// Enable recursive capability detection
    pub detect_recursive: bool,
    /// Enable novel concept detection
    pub detect_novel: bool,
    /// Enable self-improvement detection
    pub detect_self_improvement: bool,
    /// Window size for temporal analysis
    pub temporal_window_size: usize,
}

impl Default for EmergenceDetectionConfig {
    fn default() -> Self {
        Self {
            sensitivity: 0.7,
            min_confidence: 0.6,
            detect_recursive: true,
            detect_novel: true,
            detect_self_improvement: true,
            temporal_window_size: 10,
        }
    }
}

/// Metrics for emergence detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergenceMetrics {
    pub total_scans: usize,
    pub emergences_detected: usize,
    pub by_type: HashMap<String, usize>,
    pub avg_confidence: f32,
    pub false_positives: usize,
    pub confirmed_emergences: usize,
}

impl Default for EmergenceMetrics {
    fn default() -> Self {
        Self {
            total_scans: 0,
            emergences_detected: 0,
            by_type: HashMap::new(),
            avg_confidence: 0.0,
            false_positives: 0,
            confirmed_emergences: 0,
        }
    }
}

/// Trait for emergence detection strategies
pub trait EmergenceDetector {
    /// Scan the field for emergent patterns
    fn scan(
        &self,
        field: &NeuralField,
        history: &[FieldSnapshot],
    ) -> ContextNestResult<Vec<EmergenceEvent>>;

    /// Analyze a specific pattern for emergence
    fn analyze_pattern(
        &self,
        pattern: &SemanticPattern,
        field: &NeuralField,
    ) -> ContextNestResult<Option<EmergenceEvent>>;

    /// Get detector name
    fn name(&self) -> &str;

    /// Get detector sensitivity
    fn sensitivity(&self) -> f32;
}

/// Snapshot of field state for temporal analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSnapshot {
    pub timestamp: DateTime<Utc>,
    pub pattern_count: usize,
    pub avg_coherence: f32,
    pub avg_strength: f32,
    pub pattern_ids: HashSet<String>,
}

impl From<&NeuralField> for FieldSnapshot {
    fn from(field: &NeuralField) -> Self {
        let pattern_count = field.patterns.len();
        let avg_strength = if pattern_count > 0 {
            field.patterns.iter().map(|p| p.strength).sum::<f32>() / pattern_count as f32
        } else {
            0.0
        };

        Self {
            timestamp: Utc::now(),
            pattern_count,
            avg_coherence: field.state.coherence,
            avg_strength,
            pattern_ids: field.patterns.iter().map(|p| p.id.clone()).collect(),
        }
    }
}

/// Detector for recursive capability emergence
pub struct RecursiveCapabilityDetector {
    sensitivity: f32,
    min_depth: usize,
}

impl RecursiveCapabilityDetector {
    pub fn new(sensitivity: f32) -> Self {
        Self {
            sensitivity,
            min_depth: 2,
        }
    }

    /// Detect self-referential patterns
    fn detect_self_reference(&self, pattern: &SemanticPattern, field: &NeuralField) -> f32 {
        let mut self_ref_score = 0.0;

        // Check for patterns that refer to themselves or similar patterns
        let content_lower = pattern.content.to_lowercase();

        // Keywords indicating self-reference
        let self_ref_keywords = [
            "self",
            "itself",
            "recursive",
            "meta",
            "own",
            "introspect",
            "reflect",
            "internal",
            "auto",
            "self-",
        ];

        for keyword in &self_ref_keywords {
            if content_lower.contains(keyword) {
                self_ref_score += 0.1;
            }
        }

        // Check for semantic similarity to other patterns (circular reference)
        let similar_count = field
            .patterns
            .iter()
            .filter(|p| p.id != pattern.id)
            .filter(|p| self.calculate_similarity(&pattern.embedding, &p.embedding) > 0.8)
            .count();

        if similar_count > 0 {
            self_ref_score += 0.2 * (similar_count as f32).min(3.0) / 3.0;
        }

        self_ref_score.min(1.0)
    }

    /// Calculate cosine similarity between embeddings
    fn calculate_similarity(&self, emb1: &[f32], emb2: &[f32]) -> f32 {
        if emb1.len() != emb2.len() {
            return 0.0;
        }

        let dot: f32 = emb1.iter().zip(emb2).map(|(a, b)| a * b).sum();
        let mag1: f32 = emb1.iter().map(|x| x * x).sum::<f32>().sqrt();
        let mag2: f32 = emb2.iter().map(|x| x * x).sum::<f32>().sqrt();

        if mag1 == 0.0 || mag2 == 0.0 {
            return 0.0;
        }

        (dot / (mag1 * mag2)).max(0.0).min(1.0)
    }
}

impl EmergenceDetector for RecursiveCapabilityDetector {
    fn scan(
        &self,
        field: &NeuralField,
        _history: &[FieldSnapshot],
    ) -> ContextNestResult<Vec<EmergenceEvent>> {
        let mut events = Vec::new();

        for pattern in &field.patterns {
            if let Some(event) = self.analyze_pattern(pattern, field)? {
                events.push(event);
            }
        }

        Ok(events)
    }

    fn analyze_pattern(
        &self,
        pattern: &SemanticPattern,
        field: &NeuralField,
    ) -> ContextNestResult<Option<EmergenceEvent>> {
        let self_ref_score = self.detect_self_reference(pattern, field);

        if self_ref_score >= self.sensitivity {
            let confidence = self_ref_score;
            let depth = ((self_ref_score * 5.0) as usize).max(self.min_depth);

            Ok(Some(EmergenceEvent {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: Utc::now(),
                emergence_type: EmergenceType::RecursiveCapability {
                    depth,
                    complexity: self_ref_score,
                },
                confidence,
                evidence: vec![
                    format!(
                        "Pattern '{}' exhibits self-referential characteristics",
                        pattern.id
                    ),
                    format!("Self-reference score: {:.2}", self_ref_score),
                ],
                affected_patterns: vec![pattern.id.clone()],
                implications: vec![
                    "System may be developing meta-cognitive capabilities".to_string(),
                    "Recursive processing patterns emerging".to_string(),
                ],
            }))
        } else {
            Ok(None)
        }
    }

    fn name(&self) -> &str {
        "RecursiveCapabilityDetector"
    }

    fn sensitivity(&self) -> f32 {
        self.sensitivity
    }
}

/// Detector for novel concept emergence
pub struct NovelConceptDetector {
    sensitivity: f32,
    baseline_patterns: Vec<Vec<f32>>,
}

impl NovelConceptDetector {
    pub fn new(sensitivity: f32) -> Self {
        Self {
            sensitivity,
            baseline_patterns: Vec::new(),
        }
    }

    /// Set baseline patterns for novelty comparison
    pub fn set_baseline(&mut self, patterns: Vec<Vec<f32>>) {
        self.baseline_patterns = patterns;
    }

    /// Calculate novelty score
    fn calculate_novelty(&self, embedding: &[f32]) -> f32 {
        if self.baseline_patterns.is_empty() {
            return 0.5; // Unknown novelty without baseline
        }

        // Find maximum similarity to any baseline pattern
        let max_similarity = self
            .baseline_patterns
            .iter()
            .map(|baseline| self.calculate_similarity(embedding, baseline))
            .fold(0.0f32, f32::max);

        // Novelty is inverse of similarity
        1.0 - max_similarity
    }

    /// Calculate cosine similarity
    fn calculate_similarity(&self, emb1: &[f32], emb2: &[f32]) -> f32 {
        if emb1.len() != emb2.len() {
            return 0.0;
        }

        let dot: f32 = emb1.iter().zip(emb2).map(|(a, b)| a * b).sum();
        let mag1: f32 = emb1.iter().map(|x| x * x).sum::<f32>().sqrt();
        let mag2: f32 = emb2.iter().map(|x| x * x).sum::<f32>().sqrt();

        if mag1 == 0.0 || mag2 == 0.0 {
            return 0.0;
        }

        (dot / (mag1 * mag2)).max(0.0).min(1.0)
    }
}

impl EmergenceDetector for NovelConceptDetector {
    fn scan(
        &self,
        field: &NeuralField,
        _history: &[FieldSnapshot],
    ) -> ContextNestResult<Vec<EmergenceEvent>> {
        let mut events = Vec::new();

        for pattern in &field.patterns {
            if let Some(event) = self.analyze_pattern(pattern, field)? {
                events.push(event);
            }
        }

        Ok(events)
    }

    fn analyze_pattern(
        &self,
        pattern: &SemanticPattern,
        field: &NeuralField,
    ) -> ContextNestResult<Option<EmergenceEvent>> {
        let novelty_score = self.calculate_novelty(&pattern.embedding);

        if novelty_score >= self.sensitivity {
            // Calculate semantic distance to nearest pattern
            let semantic_distance = field
                .patterns
                .iter()
                .filter(|p| p.id != pattern.id)
                .map(|p| 1.0 - self.calculate_similarity(&pattern.embedding, &p.embedding))
                .fold(f32::INFINITY, f32::min);

            Ok(Some(EmergenceEvent {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: Utc::now(),
                emergence_type: EmergenceType::NovelConcept {
                    novelty_score,
                    semantic_distance: semantic_distance.min(1.0),
                },
                confidence: novelty_score,
                evidence: vec![
                    format!(
                        "Pattern '{}' shows high novelty: {:.2}",
                        pattern.id, novelty_score
                    ),
                    format!(
                        "Semantic distance from existing patterns: {:.2}",
                        semantic_distance
                    ),
                ],
                affected_patterns: vec![pattern.id.clone()],
                implications: vec![
                    "New conceptual space being explored".to_string(),
                    "Potential for expanded capabilities".to_string(),
                ],
            }))
        } else {
            Ok(None)
        }
    }

    fn name(&self) -> &str {
        "NovelConceptDetector"
    }

    fn sensitivity(&self) -> f32 {
        self.sensitivity
    }
}

/// Detector for self-improvement emergence
pub struct SelfImprovementDetector {
    sensitivity: f32,
    metric_history: HashMap<String, Vec<(DateTime<Utc>, f32)>>,
}

impl SelfImprovementDetector {
    pub fn new(sensitivity: f32) -> Self {
        Self {
            sensitivity,
            metric_history: HashMap::new(),
        }
    }

    /// Record a metric value
    pub fn record_metric(&mut self, metric: &str, value: f32) {
        let entry = self
            .metric_history
            .entry(metric.to_string())
            .or_insert_with(Vec::new);
        entry.push((Utc::now(), value));

        // Keep only recent history (last 100 entries)
        if entry.len() > 100 {
            entry.remove(0);
        }
    }

    /// Calculate improvement rate for a metric
    fn calculate_improvement_rate(&self, metric: &str) -> Option<f32> {
        let history = self.metric_history.get(metric)?;

        if history.len() < 2 {
            return None;
        }

        // Calculate linear regression slope
        let n = history.len() as f32;
        let sum_x: f32 = (0..history.len()).map(|i| i as f32).sum();
        let sum_y: f32 = history.iter().map(|(_, v)| v).sum();
        let sum_xy: f32 = history
            .iter()
            .enumerate()
            .map(|(i, (_, v))| i as f32 * v)
            .sum();
        let sum_xx: f32 = (0..history.len()).map(|i| (i as f32).powi(2)).sum();

        let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_xx - sum_x.powi(2));

        Some(slope)
    }
}

impl EmergenceDetector for SelfImprovementDetector {
    fn scan(
        &self,
        _field: &NeuralField,
        history: &[FieldSnapshot],
    ) -> ContextNestResult<Vec<EmergenceEvent>> {
        let mut events = Vec::new();

        // Check coherence improvement
        if history.len() >= 5 {
            let coherence_values: Vec<f32> = history.iter().map(|s| s.avg_coherence).collect();

            if let Some(trend) = self.calculate_trend(&coherence_values) {
                if trend > self.sensitivity {
                    events.push(EmergenceEvent {
                        id: uuid::Uuid::new_v4().to_string(),
                        timestamp: Utc::now(),
                        emergence_type: EmergenceType::SelfImprovement {
                            improvement_rate: trend,
                            metric: "coherence".to_string(),
                        },
                        confidence: trend,
                        evidence: vec![
                            format!(
                                "Coherence improved by {:.1}% over recent history",
                                trend * 100.0
                            ),
                            format!("Consistent upward trend detected"),
                        ],
                        affected_patterns: vec![],
                        implications: vec![
                            "System is self-optimizing field coherence".to_string(),
                            "Meta-learning mechanisms active".to_string(),
                        ],
                    });
                }
            }
        }

        Ok(events)
    }

    fn analyze_pattern(
        &self,
        _pattern: &SemanticPattern,
        _field: &NeuralField,
    ) -> ContextNestResult<Option<EmergenceEvent>> {
        // Self-improvement is analyzed at field level, not pattern level
        Ok(None)
    }

    fn name(&self) -> &str {
        "SelfImprovementDetector"
    }

    fn sensitivity(&self) -> f32 {
        self.sensitivity
    }
}

impl SelfImprovementDetector {
    /// Calculate trend in values
    fn calculate_trend(&self, values: &[f32]) -> Option<f32> {
        if values.len() < 2 {
            return None;
        }

        let first_half = &values[..values.len() / 2];
        let second_half = &values[values.len() / 2..];

        let avg_first: f32 = first_half.iter().sum::<f32>() / first_half.len() as f32;
        let avg_second: f32 = second_half.iter().sum::<f32>() / second_half.len() as f32;

        Some((avg_second - avg_first) / avg_first.max(0.01))
    }
}

/// Main emergence detection manager
pub struct EmergenceDetectionSystem {
    config: EmergenceDetectionConfig,
    detectors: Vec<Box<dyn EmergenceDetector + Send + Sync>>,
    history: Vec<FieldSnapshot>,
    detected_events: Vec<EmergenceEvent>,
    metrics: EmergenceMetrics,
}

impl EmergenceDetectionSystem {
    /// Create new emergence detection system
    pub fn new(config: EmergenceDetectionConfig) -> Self {
        let mut system = Self {
            config,
            detectors: Vec::new(),
            history: Vec::new(),
            detected_events: Vec::new(),
            metrics: EmergenceMetrics::default(),
        };

        system.initialize_detectors();
        system
    }

    /// Initialize default detectors
    fn initialize_detectors(&mut self) {
        if self.config.detect_recursive {
            self.detectors
                .push(Box::new(RecursiveCapabilityDetector::new(
                    self.config.sensitivity,
                )));
        }

        if self.config.detect_novel {
            self.detectors
                .push(Box::new(NovelConceptDetector::new(self.config.sensitivity)));
        }

        if self.config.detect_self_improvement {
            self.detectors.push(Box::new(SelfImprovementDetector::new(
                self.config.sensitivity,
            )));
        }
    }

    /// Scan field for emergent patterns
    pub fn scan_field(&mut self, field: &NeuralField) -> ContextNestResult<Vec<EmergenceEvent>> {
        // Add current state to history
        let snapshot = FieldSnapshot::from(field);
        self.history.push(snapshot);

        // Trim history to window size
        if self.history.len() > self.config.temporal_window_size {
            self.history.remove(0);
        }

        let mut all_events = Vec::new();

        // Run each detector
        for detector in &self.detectors {
            let events = detector.scan(field, &self.history)?;

            // Filter by confidence
            let filtered: Vec<_> = events
                .into_iter()
                .filter(|e| e.confidence >= self.config.min_confidence)
                .collect();

            all_events.extend(filtered);
        }

        // Update metrics
        self.metrics.total_scans += 1;
        self.metrics.emergences_detected += all_events.len();

        for event in &all_events {
            let type_name = format!("{:?}", event.emergence_type);
            *self.metrics.by_type.entry(type_name).or_insert(0) += 1;

            // Update average confidence
            let total = self.metrics.emergences_detected as f32;
            self.metrics.avg_confidence =
                (self.metrics.avg_confidence * (total - 1.0) + event.confidence) / total;
        }

        self.detected_events.extend(all_events.clone());

        Ok(all_events)
    }

    /// Get detection metrics
    pub fn get_metrics(&self) -> &EmergenceMetrics {
        &self.metrics
    }

    /// Get all detected events
    pub fn get_events(&self) -> &[EmergenceEvent] {
        &self.detected_events
    }

    /// Tune sensitivity
    pub fn set_sensitivity(&mut self, sensitivity: f32) {
        self.config.sensitivity = sensitivity.max(0.0).min(1.0);
        // Reinitialize detectors with new sensitivity
        self.detectors.clear();
        self.initialize_detectors();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emergence_detection_creation() {
        let config = EmergenceDetectionConfig::default();
        let system = EmergenceDetectionSystem::new(config);
        assert!(system.detectors.len() > 0);
    }

    #[test]
    fn test_field_snapshot() {
        let field = NeuralField::new();
        let snapshot = FieldSnapshot::from(&field);
        assert_eq!(snapshot.pattern_count, 0);
    }

    #[test]
    fn test_recursive_detector() {
        let detector = RecursiveCapabilityDetector::new(0.7);
        assert_eq!(detector.name(), "RecursiveCapabilityDetector");
        assert_eq!(detector.sensitivity(), 0.7);
    }
}

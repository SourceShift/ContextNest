//! Semantic Pattern Recognition Module
//! Provides semantic pattern recognition capabilities for understanding
//! meaning and relationships in data.

use crate::error::ContextNestResult;
use crate::error::{ContextNestError, Result};
use serde::{Deserialize, Serialize};

/// Semantic pattern descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticPattern {
    /// Pattern identifier
    pub id: String,
    /// Pattern description
    pub description: String,
    /// Semantic category
    pub category: String,
    /// Confidence score
    pub confidence: f32,
    /// Related concepts
    pub related_concepts: Vec<String>,
    /// Pattern metadata
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

/// Semantic pattern recognizer
pub struct SemanticPatternRecognizer {
    /// Known patterns
    patterns: Vec<SemanticPattern>,
    /// Similarity threshold
    similarity_threshold: f32,
}

impl SemanticPatternRecognizer {
    /// Create a new semantic pattern recognizer
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
            similarity_threshold: 0.7,
        }
    }

    /// Add a semantic pattern
    pub fn add_pattern(&mut self, pattern: SemanticPattern) {
        self.patterns.push(pattern);
    }

    /// Recognize patterns in input text
    pub fn recognize_patterns(&self, input: &str) -> ContextNestResult<Vec<(String, f32)>> {
        let mut matches = Vec::new();

        for pattern in &self.patterns {
            let similarity = self.calculate_semantic_similarity(input, &pattern.description);
            if similarity >= self.similarity_threshold {
                matches.push((pattern.id.clone(), similarity));
            }
        }

        matches.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        Ok(matches)
    }

    /// Calculate semantic similarity between two texts
    fn calculate_semantic_similarity(&self, text1: &str, text2: &str) -> f32 {
        // Simple word overlap similarity - in real implementation would use embeddings
        let words1: std::collections::HashSet<String> = text1
            .split_whitespace()
            .map(|w| {
                w.to_lowercase()
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_string()
            })
            .filter(|w| !w.is_empty())
            .collect();

        let words2: std::collections::HashSet<String> = text2
            .split_whitespace()
            .map(|w| {
                w.to_lowercase()
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_string()
            })
            .filter(|w| !w.is_empty())
            .collect();

        if words1.is_empty() && words2.is_empty() {
            return 1.0;
        }

        if words1.is_empty() || words2.is_empty() {
            return 0.0;
        }

        let intersection = words1.intersection(&words2).count();
        let union = words1.union(&words2).count();

        intersection as f32 / union as f32
    }

    /// Get patterns by category
    pub fn get_patterns_by_category(&self, category: &str) -> Vec<&SemanticPattern> {
        self.patterns
            .iter()
            .filter(|p| p.category == category)
            .collect()
    }

    /// Set similarity threshold
    pub fn set_similarity_threshold(&mut self, threshold: f32) {
        self.similarity_threshold = threshold.clamp(0.0, 1.0);
    }
}

impl Default for SemanticPatternRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

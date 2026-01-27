//! Cross-Domain Pattern Recognition Module
//! Provides cross-domain pattern recognition capabilities for finding
//! patterns that span across different domains and contexts.

use crate::error::ContextNestResult;
use crate::error::{ContextNestError, Result};
use serde::{Deserialize, Serialize};

/// Cross-domain pattern descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossDomainPattern {
    /// Pattern identifier
    pub id: String,
    /// Pattern name
    pub name: String,
    /// Source domains involved
    pub source_domains: Vec<String>,
    /// Target domains where pattern applies
    pub target_domains: Vec<String>,
    /// Pattern strength
    pub strength: f32,
    /// Transferability score
    pub transferability: f32,
    /// Pattern metadata
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

/// Cross-domain pattern analyzer
pub struct CrossDomainPatternAnalyzer {
    /// Known cross-domain patterns
    patterns: Vec<CrossDomainPattern>,
    /// Minimum strength threshold
    min_strength: f32,
    /// Domain similarity cache
    similarity_cache: std::collections::HashMap<(String, String), f32>,
}

impl CrossDomainPatternAnalyzer {
    /// Create a new cross-domain pattern analyzer
    pub fn new() -> Self {
        Self {
            patterns: Vec::new(),
            min_strength: 0.4,
            similarity_cache: std::collections::HashMap::new(),
        }
    }

    /// Add a cross-domain pattern
    pub fn add_pattern(&mut self, pattern: CrossDomainPattern) {
        self.patterns.push(pattern);
    }

    /// Analyze patterns across domains
    pub fn analyze_cross_domain_patterns(
        &mut self,
        domain_a: &str,
        domain_b: &str,
        patterns_a: &[String],
        patterns_b: &[String],
    ) -> ContextNestResult<Vec<CrossDomainPattern>> {
        let mut cross_domain_patterns = Vec::new();

        // Calculate domain similarity
        let similarity = self.calculate_domain_similarity(domain_a, domain_b)?;

        if similarity >= self.min_strength {
            // Find patterns that appear in both domains
            let common_patterns: Vec<String> = patterns_a
                .iter()
                .filter(|p| patterns_b.contains(p))
                .cloned()
                .collect();

            for pattern_name in common_patterns {
                cross_domain_patterns.push(CrossDomainPattern {
                    id: format!("cross_{}_{}_{}", domain_a, domain_b, pattern_name),
                    name: pattern_name.clone(),
                    source_domains: vec![domain_a.to_string()],
                    target_domains: vec![domain_b.to_string()],
                    strength: similarity,
                    transferability: similarity * 0.8,
                    metadata: std::collections::HashMap::from([
                        (
                            "pattern_name".to_string(),
                            serde_json::Value::String(pattern_name),
                        ),
                        (
                            "domain_similarity".to_string(),
                            serde_json::Value::Number(
                                serde_json::Number::from_f64(similarity as f64).unwrap(),
                            ),
                        ),
                    ]),
                });
            }
        }

        // Store discovered patterns
        for pattern in &cross_domain_patterns {
            self.patterns.push(pattern.clone());
        }

        Ok(cross_domain_patterns)
    }

    /// Calculate domain similarity
    fn calculate_domain_similarity(
        &mut self,
        domain_a: &str,
        domain_b: &str,
    ) -> ContextNestResult<f32> {
        let key = (domain_a.to_string(), domain_b.to_string());

        if let Some(&similarity) = self.similarity_cache.get(&key) {
            return Ok(similarity);
        }

        // Simple similarity based on string similarity
        let similarity = self.string_similarity(domain_a, domain_b);

        // Cache the result
        self.similarity_cache.insert(key.clone(), similarity);
        self.similarity_cache.insert((key.1, key.0), similarity);

        Ok(similarity)
    }

    /// Calculate string similarity (Levenshtein distance based)
    fn string_similarity(&self, s1: &str, s2: &str) -> f32 {
        let len1 = s1.chars().count();
        let len2 = s2.chars().count();

        if len1 == 0 && len2 == 0 {
            return 1.0;
        }

        if len1 == 0 || len2 == 0 {
            return 0.0;
        }

        let distance = self.levenshtein_distance(s1, s2);
        let max_len = len1.max(len2);

        1.0 - (distance as f32 / max_len as f32)
    }

    /// Calculate Levenshtein distance
    fn levenshtein_distance(&self, s1: &str, s2: &str) -> usize {
        let chars1: Vec<char> = s1.chars().collect();
        let chars2: Vec<char> = s2.chars().collect();
        let len1 = chars1.len();
        let len2 = chars2.len();

        if len1 == 0 {
            return len2;
        }
        if len2 == 0 {
            return len1;
        }

        let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];

        for i in 0..=len1 {
            matrix[i][0] = i;
        }
        for j in 0..=len2 {
            matrix[0][j] = j;
        }

        for i in 1..=len1 {
            for j in 1..=len2 {
                let cost = if chars1[i - 1] == chars2[j - 1] { 0 } else { 1 };
                matrix[i][j] = (matrix[i - 1][j] + 1)
                    .min(matrix[i][j - 1] + 1)
                    .min(matrix[i - 1][j - 1] + cost);
            }
        }

        matrix[len1][len2]
    }

    /// Get patterns by source domain
    pub fn get_patterns_by_source_domain(&self, domain: &str) -> Vec<&CrossDomainPattern> {
        self.patterns
            .iter()
            .filter(|p| p.source_domains.contains(&domain.to_string()))
            .collect()
    }

    /// Get patterns by target domain
    pub fn get_patterns_by_target_domain(&self, domain: &str) -> Vec<&CrossDomainPattern> {
        self.patterns
            .iter()
            .filter(|p| p.target_domains.contains(&domain.to_string()))
            .collect()
    }

    /// Set minimum strength threshold
    pub fn set_min_strength(&mut self, min_strength: f32) {
        self.min_strength = min_strength.clamp(0.0, 1.0);
    }

    /// Clear similarity cache
    pub fn clear_cache(&mut self) {
        self.similarity_cache.clear();
    }
}

impl Default for CrossDomainPatternAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

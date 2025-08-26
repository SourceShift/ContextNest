//! Multi-Attractor Coordination for Co-Emergence Protocol
//! This module implements coordination between multiple attractors,
//! including attractor scanning, residue surfacing, field auditing,
//! and boundary collapse mechanisms.

use crate::context::attractor_dynamics::{
    AttractorBasin, AttractorDynamicsEngine, CoEmergenceOpportunity, CoEmergenceType,
};
use crate::context::field::{NeuralField, SemanticPattern};
use crate::context::harmonic_integration::{HarmonicConnection, HarmonicIntegrator};
use crate::error::{ContextNestError, ContextNestResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Multi-attractor coordinator for managing complex attractor interactions
#[derive(Debug, Clone)]
pub struct MultiAttractorCoordinator {
    /// Harmonic integrator for connections
    pub integrator: HarmonicIntegrator,
    /// Attractor scan results
    pub scan_results: Vec<AttractorScanResult>,
    /// Field audit results
    pub audit_results: Vec<FieldAuditResult>,
    /// Residue tracking
    pub residues: Vec<SymbolicResidue>,
    /// Coordination metrics
    pub metrics: CoordinationMetrics,
}

/// Result of attractor scanning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttractorScanResult {
    /// Scanned attractor ID
    pub attractor_id: String,
    /// Strength of attractor
    pub strength: f32,
    /// Number of connections
    pub connection_count: usize,
    /// Influence radius
    pub influence_radius: f32,
    /// Detected resonances
    pub resonances: Vec<ResonanceDetection>,
    /// Scan timestamp
    pub scanned_at: chrono::DateTime<chrono::Utc>,
}

/// Detected resonance during scanning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResonanceDetection {
    /// Resonance frequency
    pub frequency: f32,
    /// Amplitude
    pub amplitude: f32,
    /// Connected attractor ID
    pub connected_to: Option<String>,
}

/// Symbolic residue (fragments that can form connections)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolicResidue {
    /// Residue ID
    pub id: String,
    /// Symbolic content
    pub content: String,
    /// Embedding vector
    pub embedding: Vec<f32>,
    /// Source attractor ID
    pub source_attractor: String,
    /// Strength
    pub strength: f32,
    /// Potential for connection
    pub connection_potential: f32,
    /// Created timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Field audit result for detecting new basins
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldAuditResult {
    /// Audit ID
    pub id: String,
    /// New basins detected
    pub new_basins_detected: Vec<NewBasinCandidate>,
    /// Boundary conditions
    pub boundary_conditions: Vec<BoundaryCondition>,
    /// Field coherence score
    pub field_coherence: f32,
    /// Emergence indicators
    pub emergence_indicators: Vec<EmergenceIndicator>,
    /// Audit timestamp
    pub audited_at: chrono::DateTime<chrono::Utc>,
}

/// Candidate for new attractor basin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewBasinCandidate {
    /// Candidate location in embedding space
    pub location: Vec<f32>,
    /// Estimated strength
    pub estimated_strength: f32,
    /// Confidence score
    pub confidence: f32,
    /// Supporting evidence (pattern IDs)
    pub evidence: Vec<String>,
}

/// Boundary condition in the field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryCondition {
    /// Boundary type
    pub boundary_type: BoundaryType,
    /// Location
    pub location: Vec<f32>,
    /// Permeability (0.0-1.0)
    pub permeability: f32,
    /// Gradient strength
    pub gradient_strength: f32,
}

/// Type of boundary
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BoundaryType {
    /// Hard boundary (low permeability)
    Hard,
    /// Soft boundary (high permeability)
    Soft,
    /// Permeable boundary (variable)
    Permeable,
    /// Dissolving boundary
    Dissolving,
}

/// Indicator of emergence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergenceIndicator {
    /// Type of emergence
    pub emergence_type: String,
    /// Strength
    pub strength: f32,
    /// Location in field
    pub location: Vec<f32>,
    /// Contributing attractors
    pub contributors: Vec<String>,
}

/// Coordination metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoordinationMetrics {
    /// Total attractors coordinated
    pub total_attractors: usize,
    /// Active connections
    pub active_connections: usize,
    /// Residues surfaced
    pub residues_surfaced: usize,
    /// New basins detected
    pub new_basins_detected: usize,
    /// Average field coherence
    pub avg_field_coherence: f32,
    /// Emergence events
    pub emergence_events: usize,
}

impl MultiAttractorCoordinator {
    /// Create a new multi-attractor coordinator
    pub fn new(integrator: HarmonicIntegrator) -> Self {
        Self {
            integrator,
            scan_results: Vec::new(),
            audit_results: Vec::new(),
            residues: Vec::new(),
            metrics: CoordinationMetrics::default(),
        }
    }

    /// Scan attractors with strength filtering
    pub fn scan_attractors(
        &mut self,
        engine: &AttractorDynamicsEngine,
        min_strength: f32,
    ) -> ContextNestResult<Vec<AttractorScanResult>> {
        let mut results = Vec::new();

        for basin in &engine.attractor_basins {
            // Filter by strength
            if basin.depth < min_strength {
                continue;
            }

            // Count connections to this attractor
            let connection_count = self
                .integrator
                .connections
                .iter()
                .filter(|c| c.source_id == basin.id || c.target_id == basin.id)
                .count();

            // Detect resonances
            let resonances = self.detect_resonances(basin, engine)?;

            let scan_result = AttractorScanResult {
                attractor_id: basin.id.clone(),
                strength: basin.depth,
                connection_count,
                influence_radius: basin.radius,
                resonances,
                scanned_at: chrono::Utc::now(),
            };

            results.push(scan_result);
        }

        self.scan_results.extend(results.clone());
        self.metrics.total_attractors = self.scan_results.len();
        self.metrics.active_connections = self.integrator.connections.len();

        Ok(results)
    }

    /// Detect resonances for an attractor
    fn detect_resonances(
        &self,
        basin: &AttractorBasin,
        engine: &AttractorDynamicsEngine,
    ) -> ContextNestResult<Vec<ResonanceDetection>> {
        let mut resonances = Vec::new();

        // Find connections involving this attractor
        for connection in &self.integrator.connections {
            if connection.source_id == basin.id {
                resonances.push(ResonanceDetection {
                    frequency: connection.resonance_frequency,
                    amplitude: connection.strength,
                    connected_to: Some(connection.target_id.clone()),
                });
            } else if connection.target_id == basin.id {
                resonances.push(ResonanceDetection {
                    frequency: connection.resonance_frequency,
                    amplitude: connection.strength,
                    connected_to: Some(connection.source_id.clone()),
                });
            }
        }

        // Detect self-resonance (attractor's own oscillation)
        let self_frequency = basin.depth / (basin.radius + 1.0);
        resonances.push(ResonanceDetection {
            frequency: self_frequency,
            amplitude: basin.depth,
            connected_to: None,
        });

        Ok(resonances)
    }

    /// Surface residues (symbolic fragments) that can form connections
    pub fn surface_residues(
        &mut self,
        engine: &AttractorDynamicsEngine,
        field: &NeuralField,
    ) -> ContextNestResult<Vec<SymbolicResidue>> {
        let mut new_residues = Vec::new();

        // Examine patterns not strongly attracted to any basin
        for pattern in &field.patterns {
            // Skip deleted patterns
            if pattern.deleted_at.is_some() {
                continue;
            }

            // Find strongest attraction
            let mut max_attraction = 0.0;
            let mut attracting_basin: Option<&AttractorBasin> = None;

            for basin in &engine.attractor_basins {
                let attraction = self.calculate_attraction(pattern, basin)?;
                if attraction > max_attraction {
                    max_attraction = attraction;
                    attracting_basin = Some(basin);
                }
            }

            // If attraction is moderate (0.3-0.6), create residue
            if max_attraction > 0.3 && max_attraction < 0.6 {
                if let Some(basin) = attracting_basin {
                    let residue = SymbolicResidue {
                        id: format!("residue_{}_{}", pattern.id, basin.id),
                        content: pattern.content.clone(),
                        embedding: pattern.embedding.clone(),
                        source_attractor: basin.id.clone(),
                        strength: pattern.strength,
                        connection_potential: 1.0 - max_attraction, // Higher potential when less attracted
                        created_at: chrono::Utc::now(),
                    };

                    new_residues.push(residue);
                }
            }
        }

        self.residues.extend(new_residues.clone());
        self.metrics.residues_surfaced = self.residues.len();

        Ok(new_residues)
    }

    /// Calculate attraction between pattern and basin
    fn calculate_attraction(
        &self,
        pattern: &SemanticPattern,
        basin: &AttractorBasin,
    ) -> ContextNestResult<f32> {
        if pattern.embedding.len() != basin.center.len() {
            return Ok(0.0);
        }

        // Calculate distance from pattern to basin center
        let distance: f32 = pattern
            .embedding
            .iter()
            .zip(basin.center.iter())
            .map(|(p, b)| (p - b).powi(2))
            .sum::<f32>()
            .sqrt();

        // Attraction inversely proportional to distance, scaled by basin depth
        let attraction = if distance < basin.radius {
            basin.depth * (1.0 - distance / basin.radius)
        } else {
            0.0
        };

        Ok(attraction.max(0.0).min(1.0))
    }

    /// Audit field for new attractor basins
    pub fn audit_field(
        &mut self,
        engine: &AttractorDynamicsEngine,
        field: &NeuralField,
    ) -> ContextNestResult<FieldAuditResult> {
        let audit_id = uuid::Uuid::new_v4().to_string();

        // Detect new basin candidates
        let new_basins = self.detect_new_basin_candidates(engine, field)?;

        // Analyze boundary conditions
        let boundary_conditions = self.analyze_boundaries(engine, field)?;

        // Calculate field coherence
        let field_coherence = self.calculate_field_coherence(engine, field)?;

        // Detect emergence indicators
        let emergence_indicators = self.detect_emergence_indicators(engine, field)?;

        let audit_result = FieldAuditResult {
            id: audit_id,
            new_basins_detected: new_basins,
            boundary_conditions,
            field_coherence,
            emergence_indicators,
            audited_at: chrono::Utc::now(),
        };

        self.metrics.new_basins_detected += audit_result.new_basins_detected.len();
        self.metrics.emergence_events += audit_result.emergence_indicators.len();
        self.metrics.avg_field_coherence =
            (self.metrics.avg_field_coherence + field_coherence) / 2.0;

        self.audit_results.push(audit_result.clone());

        Ok(audit_result)
    }

    /// Detect new basin candidates
    fn detect_new_basin_candidates(
        &self,
        engine: &AttractorDynamicsEngine,
        field: &NeuralField,
    ) -> ContextNestResult<Vec<NewBasinCandidate>> {
        let mut candidates = Vec::new();

        // Group patterns by region in embedding space
        let mut region_patterns: HashMap<Vec<usize>, Vec<&SemanticPattern>> = HashMap::new();

        for pattern in &field.patterns {
            if pattern.deleted_at.is_some() {
                continue;
            }

            // Discretize pattern location (for clustering)
            let region_key: Vec<usize> = pattern
                .embedding
                .iter()
                .map(|&x| ((x + 1.0) * 5.0) as usize) // Map to discrete regions
                .collect();

            region_patterns
                .entry(region_key)
                .or_insert_with(Vec::new)
                .push(pattern);
        }

        // Find regions with high pattern density (potential new basins)
        for (region, patterns) in region_patterns.iter() {
            if patterns.len() >= 3 {
                // At least 3 patterns to form a basin
                // Check if region is far from existing basins
                let region_center = self.calculate_region_center(patterns);
                let min_distance = engine
                    .attractor_basins
                    .iter()
                    .map(|b| self.euclidean_distance(&region_center, &b.center))
                    .min_by(|a, b| a.partial_cmp(b).unwrap())
                    .unwrap_or(f32::MAX);

                if min_distance > 0.5 {
                    // Far enough from existing basins
                    let candidate = NewBasinCandidate {
                        location: region_center,
                        estimated_strength: patterns.iter().map(|p| p.strength).sum::<f32>()
                            / patterns.len() as f32,
                        confidence: (patterns.len() as f32 / 10.0).min(1.0),
                        evidence: patterns.iter().map(|p| p.id.clone()).collect(),
                    };

                    candidates.push(candidate);
                }
            }
        }

        Ok(candidates)
    }

    /// Calculate center of a pattern region
    fn calculate_region_center(&self, patterns: &[&SemanticPattern]) -> Vec<f32> {
        if patterns.is_empty() {
            return Vec::new();
        }

        let dim = patterns[0].embedding.len();
        let mut center = vec![0.0; dim];

        for pattern in patterns {
            for (i, &val) in pattern.embedding.iter().enumerate() {
                center[i] += val;
            }
        }

        center.iter_mut().for_each(|x| *x /= patterns.len() as f32);
        center
    }

    /// Euclidean distance between two vectors
    fn euclidean_distance(&self, a: &[f32], b: &[f32]) -> f32 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    /// Analyze boundary conditions in the field
    fn analyze_boundaries(
        &self,
        engine: &AttractorDynamicsEngine,
        field: &NeuralField,
    ) -> ContextNestResult<Vec<BoundaryCondition>> {
        let mut boundaries = Vec::new();

        // Analyze boundaries between attractor pairs
        for i in 0..engine.attractor_basins.len() {
            for j in (i + 1)..engine.attractor_basins.len() {
                let basin1 = &engine.attractor_basins[i];
                let basin2 = &engine.attractor_basins[j];

                // Find boundary location (midpoint)
                let boundary_location: Vec<f32> = basin1
                    .center
                    .iter()
                    .zip(basin2.center.iter())
                    .map(|(a, b)| (a + b) / 2.0)
                    .collect();

                // Calculate permeability based on basin depths
                let depth_diff = (basin1.depth - basin2.depth).abs();
                let permeability = 1.0 - depth_diff; // More similar depths = higher permeability

                // Calculate gradient strength
                let distance = self.euclidean_distance(&basin1.center, &basin2.center);
                let gradient_strength = if distance > 0.0 {
                    (basin1.depth + basin2.depth) / distance
                } else {
                    0.0
                };

                // Determine boundary type
                let boundary_type = if permeability > 0.7 {
                    BoundaryType::Dissolving
                } else if permeability > 0.5 {
                    BoundaryType::Permeable
                } else if permeability > 0.3 {
                    BoundaryType::Soft
                } else {
                    BoundaryType::Hard
                };

                boundaries.push(BoundaryCondition {
                    boundary_type,
                    location: boundary_location,
                    permeability,
                    gradient_strength,
                });
            }
        }

        Ok(boundaries)
    }

    /// Calculate overall field coherence
    fn calculate_field_coherence(
        &self,
        engine: &AttractorDynamicsEngine,
        field: &NeuralField,
    ) -> ContextNestResult<f32> {
        if engine.attractor_basins.is_empty() {
            return Ok(0.0);
        }

        // Coherence based on:
        // 1. Average connection strength
        // 2. Basin health
        // 3. Pattern coverage

        let avg_connection_strength = if !self.integrator.connections.is_empty() {
            self.integrator
                .connections
                .iter()
                .map(|c| c.strength)
                .sum::<f32>()
                / self.integrator.connections.len() as f32
        } else {
            0.0
        };

        let avg_basin_health = engine
            .attractor_basins
            .iter()
            .map(|b| b.health.stability)
            .sum::<f32>()
            / engine.attractor_basins.len() as f32;

        let pattern_coverage = self.calculate_pattern_coverage(engine, field)?;

        let coherence = (avg_connection_strength + avg_basin_health + pattern_coverage) / 3.0;

        Ok(coherence)
    }

    /// Calculate what fraction of patterns are well-covered by attractors
    fn calculate_pattern_coverage(
        &self,
        engine: &AttractorDynamicsEngine,
        field: &NeuralField,
    ) -> ContextNestResult<f32> {
        if field.patterns.is_empty() {
            return Ok(1.0);
        }

        let mut covered_count = 0;

        for pattern in &field.patterns {
            if pattern.deleted_at.is_some() {
                continue;
            }

            // Check if pattern is well-attracted to any basin
            for basin in &engine.attractor_basins {
                let attraction = self.calculate_attraction(pattern, basin)?;
                if attraction > 0.6 {
                    // Well-covered
                    covered_count += 1;
                    break;
                }
            }
        }

        let active_patterns = field
            .patterns
            .iter()
            .filter(|p| p.deleted_at.is_none())
            .count();
        if active_patterns == 0 {
            return Ok(1.0);
        }

        Ok(covered_count as f32 / active_patterns as f32)
    }

    /// Detect emergence indicators
    fn detect_emergence_indicators(
        &self,
        engine: &AttractorDynamicsEngine,
        field: &NeuralField,
    ) -> ContextNestResult<Vec<EmergenceIndicator>> {
        let mut indicators = Vec::new();

        // Detect co-emergence opportunities as indicators
        let opportunities = engine.detect_co_emergence_opportunities();

        for opportunity in opportunities {
            let source_basin = engine
                .attractor_basins
                .iter()
                .find(|b| b.id == opportunity.source_id);
            let target_basin = engine
                .attractor_basins
                .iter()
                .find(|b| b.id == opportunity.target_id);

            if let (Some(source), Some(target)) = (source_basin, target_basin) {
                // Calculate emergence location (between attractors)
                let location: Vec<f32> = source
                    .center
                    .iter()
                    .zip(target.center.iter())
                    .map(|(a, b)| (a + b) / 2.0)
                    .collect();

                indicators.push(EmergenceIndicator {
                    emergence_type: format!("{:?}", opportunity.emergence_type),
                    strength: opportunity.potential_strength,
                    location,
                    contributors: vec![opportunity.source_id, opportunity.target_id],
                });
            }
        }

        Ok(indicators)
    }

    /// Collapse boundary between two attractors
    pub fn collapse_boundary(
        &mut self,
        engine: &mut AttractorDynamicsEngine,
        attractor1_id: &str,
        attractor2_id: &str,
    ) -> ContextNestResult<BoundaryCollapseResult> {
        // Find the boundary condition between these attractors
        let boundary = self
            .audit_results
            .last()
            .and_then(|audit| {
                audit.boundary_conditions.iter().find(|bc| {
                    // Check if this boundary is between our two attractors
                    bc.boundary_type == BoundaryType::Dissolving
                        || bc.boundary_type == BoundaryType::Permeable
                })
            })
            .cloned();

        if boundary.is_none() {
            return Err(ContextNestError::Validation(
                "No collapsible boundary found".to_string(),
            ));
        }

        let boundary = boundary.unwrap();

        // Increase permeability of connections between these attractors
        for connection in &mut self.integrator.connections {
            if (connection.source_id == attractor1_id && connection.target_id == attractor2_id)
                || (connection.source_id == attractor2_id && connection.target_id == attractor1_id)
            {
                connection.strength *= 1.5; // Strengthen connection
                connection.strength = connection.strength.min(1.0);
            }
        }

        Ok(BoundaryCollapseResult {
            attractor1_id: attractor1_id.to_string(),
            attractor2_id: attractor2_id.to_string(),
            new_permeability: boundary.permeability * 1.5,
            connections_strengthened: 1,
            timestamp: chrono::Utc::now(),
        })
    }

    /// Get coordination metrics
    pub fn get_metrics(&self) -> &CoordinationMetrics {
        &self.metrics
    }
}

/// Result of boundary collapse
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryCollapseResult {
    pub attractor1_id: String,
    pub attractor2_id: String,
    pub new_permeability: f32,
    pub connections_strengthened: usize,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::harmonic_integration::IntegrationStrategy;

    #[test]
    fn test_multi_attractor_coordinator_creation() {
        let integrator = HarmonicIntegrator::new(IntegrationStrategy::default());
        let coordinator = MultiAttractorCoordinator::new(integrator);

        assert_eq!(coordinator.scan_results.len(), 0);
        assert_eq!(coordinator.metrics.total_attractors, 0);
    }

    #[test]
    fn test_boundary_type_classification() {
        let boundary = BoundaryCondition {
            boundary_type: BoundaryType::Permeable,
            location: vec![0.5, 0.5],
            permeability: 0.7,
            gradient_strength: 0.3,
        };

        assert_eq!(boundary.boundary_type, BoundaryType::Permeable);
        assert!(boundary.permeability > 0.5);
    }
}

use crate::context::emergence_detection::{EmergenceEvent, EmergenceType};
/// Self-Organizing Pattern Emergence implementation
/// This module implements autonomous pattern discovery, organization, and emergence
/// capabilities that enable the system to spontaneously generate and structure
/// knowledge patterns without external supervision.
use crate::context::field::{Attractor, NeuralField, SemanticPattern};
use crate::context::memory::AttractorField;
use crate::error::{ContextNestError, ContextNestResult};
use crate::Result;
use chrono::{DateTime, Utc};
use rand::seq::IteratorRandom;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Configuration for self-organizing pattern emergence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfOrganizingConfig {
    /// Minimum similarity for patterns to group together
    pub similarity_threshold: f32,
    /// Minimum cluster size to consider stable
    pub min_cluster_size: usize,
    /// Maximum distance for pattern influence
    pub influence_radius: f32,
    /// Spontaneous pattern generation probability
    pub spontaneous_generation_rate: f32,
    /// Pattern complexity threshold for emergence
    pub complexity_threshold: f32,
    /// Enable autonomous pattern discovery
    pub enable_discovery: bool,
    /// Enable pattern self-organization
    pub enable_organization: bool,
    /// Enable spontaneous pattern generation
    pub enable_spontaneous_generation: bool,
}

impl Default for SelfOrganizingConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.7,
            min_cluster_size: 3,
            influence_radius: 0.5,
            spontaneous_generation_rate: 0.1,
            complexity_threshold: 0.6,
            enable_discovery: true,
            enable_organization: true,
            enable_spontaneous_generation: false, // Conservative default
        }
    }
}

/// Self-organizing pattern emergence manager
pub struct SelfOrganizingEmergence {
    config: SelfOrganizingConfig,
    pattern_clusters: HashMap<String, PatternCluster>,
    emergence_history: Vec<EmergenceEvent>,
    organization_state: OrganizationState,
    metrics: SelfOrganizingMetrics,
}

/// State of pattern organization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationState {
    /// Current organization phase
    pub phase: OrganizationPhase,
    /// Pattern connectivity graph
    pub connectivity: PatternConnectivity,
    /// Field coherence trajectory
    pub coherence_trajectory: Vec<f32>,
    /// Last organization timestamp
    pub last_organization: DateTime<Utc>,
    /// Organization cycles completed
    pub organization_cycles: usize,
}

/// Phases of self-organization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OrganizationPhase {
    /// Initial disordered state
    Dispersed,
    /// Patterns forming local connections
    LocalClustering,
    /// Global structure emerging
    GlobalCoherence,
    /// Stable organized state
    Organized,
    /// Reorganization due to perturbation
    Reorganizing,
}

/// Pattern connectivity information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternConnectivity {
    /// Adjacency matrix of pattern similarities
    pub adjacency_matrix: HashMap<String, HashMap<String, f32>>,
    /// Cluster assignments
    pub clusters: HashMap<String, String>,
    /// Bridge patterns connecting clusters
    pub bridge_patterns: HashSet<String>,
    /// Centrality scores for patterns
    pub centrality: HashMap<String, f32>,
}

/// Cluster of related patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternCluster {
    /// Cluster identifier
    pub id: String,
    /// Member pattern IDs
    pub members: Vec<String>,
    /// Cluster center point (in embedding space)
    pub center: Vec<f32>,
    /// Cluster cohesion measure
    pub cohesion: f32,
    /// Cluster separation from other clusters
    pub separation: f32,
    /// Cluster complexity
    pub complexity: f32,
    /// Emergence potential
    pub emergence_potential: f32,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
}

/// Metrics for self-organizing emergence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfOrganizingMetrics {
    /// Total patterns discovered
    pub patterns_discovered: usize,
    /// Clusters formed
    pub clusters_formed: usize,
    /// Spontaneous generations
    pub spontaneous_generations: usize,
    /// Organization cycles completed
    pub organization_cycles: usize,
    /// Average cluster cohesion
    pub avg_cohesion: f32,
    /// Global organization quality
    pub organization_quality: f32,
    /// Emergence events detected
    pub emergence_events: usize,
    /// Last updated timestamp
    pub last_updated: DateTime<Utc>,
}

impl Default for SelfOrganizingMetrics {
    fn default() -> Self {
        Self {
            patterns_discovered: 0,
            clusters_formed: 0,
            spontaneous_generations: 0,
            organization_cycles: 0,
            avg_cohesion: 0.0,
            organization_quality: 0.0,
            emergence_events: 0,
            last_updated: Utc::now(),
        }
    }
}

/// Result of self-organization process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationResult {
    /// New patterns discovered
    pub discovered_patterns: Vec<SemanticPattern>,
    /// Clusters formed or updated
    pub updated_clusters: Vec<PatternCluster>,
    /// Emergence events detected
    pub emergence_events: Vec<EmergenceEvent>,
    /// Organization quality improvement
    pub quality_improvement: f32,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
    /// New phase entered
    pub new_phase: Option<OrganizationPhase>,
}

impl SelfOrganizingEmergence {
    /// Create new self-organizing emergence manager
    pub fn new(config: SelfOrganizingConfig) -> Self {
        Self {
            config,
            pattern_clusters: HashMap::new(),
            emergence_history: Vec::new(),
            organization_state: OrganizationState {
                phase: OrganizationPhase::Dispersed,
                connectivity: PatternConnectivity {
                    adjacency_matrix: HashMap::new(),
                    clusters: HashMap::new(),
                    bridge_patterns: HashSet::new(),
                    centrality: HashMap::new(),
                },
                coherence_trajectory: Vec::new(),
                last_organization: Utc::now(),
                organization_cycles: 0,
            },
            metrics: SelfOrganizingMetrics::default(),
        }
    }

    /// Execute one cycle of self-organization
    pub fn organize_patterns(
        &mut self,
        field: &mut NeuralField,
        memory: &mut AttractorField,
    ) -> ContextNestResult<OrganizationResult> {
        let start_time = std::time::Instant::now();
        let mut result = OrganizationResult {
            discovered_patterns: Vec::new(),
            updated_clusters: Vec::new(),
            emergence_events: Vec::new(),
            quality_improvement: 0.0,
            processing_time_ms: 0,
            new_phase: None,
        };

        // Step 1: Discover new patterns
        if self.config.enable_discovery {
            let discovered = self.discover_patterns(field, memory)?;
            result.discovered_patterns = discovered;

            // Add discovered patterns to field
            for pattern in &result.discovered_patterns {
                field.inject(pattern.content.clone(), pattern.embedding.clone())?;
            }
            self.metrics.patterns_discovered += result.discovered_patterns.len();
        }

        // Step 2: Update pattern connectivity
        self.update_connectivity(field)?;

        // Step 3: Form or update clusters
        if self.config.enable_organization {
            let clusters_updated = self.update_pattern_clusters(field)?;
            result.updated_clusters = clusters_updated;
            self.metrics.clusters_formed = self.pattern_clusters.len();
        }

        // Step 4: Detect emergence events
        let emergence_events = self.detect_emergence_events(field)?;
        result.emergence_events = emergence_events.clone();
        self.metrics.emergence_events += emergence_events.len();
        self.emergence_history.extend(emergence_events);

        // Step 5: Spontaneous pattern generation
        if self.config.enable_spontaneous_generation {
            let spontaneous = self.generate_spontaneous_patterns(field)?;
            result.discovered_patterns.extend(spontaneous);
            self.metrics.spontaneous_generations += result
                .discovered_patterns
                .len()
                .checked_sub(self.metrics.patterns_discovered)
                .unwrap_or(0);
        }

        // Step 6: Update organization phase
        let old_phase = self.organization_state.phase.clone();
        self.update_organization_phase(field)?;
        if old_phase != self.organization_state.phase {
            result.new_phase = Some(self.organization_state.phase.clone());
        }

        // Step 7: Update metrics
        self.update_metrics(field)?;
        self.organization_state.organization_cycles += 1;
        self.organization_state.last_organization = Utc::now();

        result.processing_time_ms = start_time.elapsed().as_millis() as u64;
        Ok(result)
    }

    /// Discover new patterns from field state
    fn discover_patterns(
        &self,
        field: &NeuralField,
        memory: &AttractorField,
    ) -> ContextNestResult<Vec<SemanticPattern>> {
        let mut discovered = Vec::new();

        // Analyze gaps in pattern space
        let gaps = self.identify_pattern_gaps(field, memory)?;
        for gap in gaps {
            if gap.potential > self.config.complexity_threshold {
                let pattern = self.synthesize_pattern_from_gap(field, &gap)?;
                discovered.push(pattern);
            }
        }

        // Discover implicit patterns from attractor interactions
        let implicit_patterns = self.discover_implicit_patterns(field, memory)?;
        discovered.extend(implicit_patterns);

        Ok(discovered)
    }

    /// Identify gaps in pattern space that could be filled
    fn identify_pattern_gaps(
        &self,
        field: &NeuralField,
        _memory: &AttractorField,
    ) -> ContextNestResult<Vec<PatternGap>> {
        let mut gaps = Vec::new();

        if field.patterns.len() < 2 {
            return Ok(gaps);
        }

        // Find regions with low pattern density
        for attractor in &field.attractors {
            // Check if attractor region is underpopulated
            let nearby_patterns = field
                .patterns
                .iter()
                .filter(|p| {
                    let distance = Self::calculate_distance(&p.embedding, &attractor.center);
                    distance < attractor.radius
                })
                .count();

            if nearby_patterns < self.config.min_cluster_size {
                let gap = PatternGap {
                    id: uuid::Uuid::new_v4().to_string(),
                    center: attractor.center.clone(),
                    radius: attractor.radius,
                    potential: attractor.strength * 0.8, // Attractor strength indicates potential
                    gap_type: GapType::UnderpopulatedRegion,
                    related_patterns: field
                        .patterns
                        .iter()
                        .filter(|p| {
                            let distance =
                                Self::calculate_distance(&p.embedding, &attractor.center);
                            distance < attractor.radius * 2.0
                        })
                        .map(|p| p.id.clone())
                        .collect(),
                };
                gaps.push(gap);
            }
        }

        // Find conceptual gaps between existing patterns
        for (i, pattern1) in field.patterns.iter().enumerate() {
            for pattern2 in field.patterns.iter().skip(i + 1) {
                let distance = Self::calculate_distance(&pattern1.embedding, &pattern2.embedding);

                // If patterns are moderately distant, there might be a conceptual gap
                if 0.5 < distance && distance < 0.8 {
                    let mid_point =
                        Self::calculate_midpoint(&pattern1.embedding, &pattern2.embedding);

                    let gap = PatternGap {
                        id: uuid::Uuid::new_v4().to_string(),
                        center: mid_point,
                        radius: distance * 0.3,
                        potential: (1.0 - distance) * 0.7,
                        gap_type: GapType::ConceptualBridge,
                        related_patterns: vec![pattern1.id.clone(), pattern2.id.clone()],
                    };
                    gaps.push(gap);
                }
            }
        }

        Ok(gaps)
    }

    /// Synthesize a pattern to fill an identified gap
    fn synthesize_pattern_from_gap(
        &self,
        field: &NeuralField,
        gap: &PatternGap,
    ) -> ContextNestResult<SemanticPattern> {
        // Create pattern based on gap type
        let content = match gap.gap_type {
            GapType::UnderpopulatedRegion => {
                format!("Pattern for underpopulated region near attractor")
            }
            GapType::ConceptualBridge => {
                if gap.related_patterns.len() >= 2 {
                    format!("Bridge concept connecting patterns in region")
                } else {
                    format!("Concept bridging existing patterns")
                }
            }
        };

        // Calculate pattern strength based on gap potential and related patterns
        let base_strength = gap.potential;
        let related_strength = if !gap.related_patterns.is_empty() {
            let related_strengths: Vec<f32> = gap
                .related_patterns
                .iter()
                .filter_map(|id| field.patterns.iter().find(|p| &p.id == id))
                .map(|p| p.strength)
                .collect();

            if !related_strengths.is_empty() {
                related_strengths.iter().sum::<f32>() / related_strengths.len() as f32
            } else {
                0.5
            }
        } else {
            0.5
        };

        let strength = (base_strength + related_strength) / 2.0;

        Ok(SemanticPattern {
            id: gap.id.clone(),
            content,
            embedding: gap.center.clone(),
            strength,
            resonance: 0.7,
            decay_rate: 0.01,
            activation_count: 0,
            created_at: Utc::now(),
            last_activated: Utc::now(),
            deleted_at: None,
            delete_reason: None,
        })
    }

    /// Discover implicit patterns from attractor interactions
    fn discover_implicit_patterns(
        &self,
        field: &NeuralField,
        _memory: &AttractorField,
    ) -> ContextNestResult<Vec<SemanticPattern>> {
        let mut implicit = Vec::new();

        // Look for attractor overlaps that suggest implicit concepts
        for (i, attractor1) in field.attractors.iter().enumerate() {
            for attractor2 in field.attractors.iter().skip(i + 1) {
                let overlap = Self::calculate_attractor_overlap(attractor1, attractor2);

                if overlap > 0.3 {
                    // Create implicit pattern from overlap
                    let center = Self::calculate_midpoint(&attractor1.center, &attractor2.center);

                    let pattern = SemanticPattern {
                        id: uuid::Uuid::new_v4().to_string(),
                        content: format!("Implicit pattern from attractor interaction"),
                        embedding: center,
                        strength: overlap * attractor1.strength * attractor2.strength,
                        resonance: 0.6,
                        decay_rate: 0.01,
                        activation_count: 0,
                        created_at: Utc::now(),
                        last_activated: Utc::now(),
                        deleted_at: None,
                        delete_reason: None,
                    };

                    implicit.push(pattern);
                }
            }
        }

        Ok(implicit)
    }

    /// Update pattern connectivity graph
    fn update_connectivity(&mut self, field: &NeuralField) -> ContextNestResult<()> {
        let mut adjacency_matrix = HashMap::new();
        let mut centrality = HashMap::new();

        // Build adjacency matrix based on similarity
        for (i, pattern1) in field.patterns.iter().enumerate() {
            let mut connections = HashMap::new();
            let mut total_similarity = 0.0f32;

            for pattern2 in field.patterns.iter() {
                if pattern1.id != pattern2.id {
                    let similarity =
                        Self::calculate_similarity(&pattern1.embedding, &pattern2.embedding);

                    if similarity > self.config.similarity_threshold {
                        connections.insert(pattern2.id.clone(), similarity);
                        total_similarity += similarity;
                    }
                }
            }

            adjacency_matrix.insert(pattern1.id.clone(), connections);
            centrality.insert(pattern1.id.clone(), total_similarity);
        }

        // Update connectivity state
        self.organization_state.connectivity.adjacency_matrix = adjacency_matrix;
        self.organization_state.connectivity.centrality = centrality;

        // Identify bridge patterns
        self.identify_bridge_patterns(field)?;

        Ok(())
    }

    /// Identify patterns that connect different clusters
    fn identify_bridge_patterns(&mut self, field: &NeuralField) -> ContextNestResult<()> {
        let mut bridge_patterns = HashSet::new();

        // Simple heuristic: patterns with diverse connections are bridges
        for (pattern_id, connections) in &self.organization_state.connectivity.adjacency_matrix {
            if connections.len() >= 3 {
                // Check if connections are to diverse patterns
                let connected_clusters: HashSet<_> = connections
                    .iter()
                    .filter_map(|(id, _)| self.organization_state.connectivity.clusters.get(id))
                    .collect();

                if connected_clusters.len() >= 2 {
                    bridge_patterns.insert(pattern_id.clone());
                }
            }
        }

        self.organization_state.connectivity.bridge_patterns = bridge_patterns;
        Ok(())
    }

    /// Update pattern clusters based on connectivity
    fn update_pattern_clusters(
        &mut self,
        field: &NeuralField,
    ) -> ContextNestResult<Vec<PatternCluster>> {
        let mut updated_clusters = Vec::new();
        let mut new_clusters = HashMap::new();

        // Simple clustering based on connectivity
        let mut visited = HashSet::new();

        for pattern in &field.patterns {
            if visited.contains(&pattern.id) {
                continue;
            }

            // Start new cluster
            let cluster_id = format!(
                "cluster_{}",
                uuid::Uuid::new_v4().to_string()[..8].to_string()
            );
            let mut cluster_members = Vec::new();
            let mut to_visit = vec![pattern.id.clone()];

            // Depth-first search to find connected components
            while let Some(current_id) = to_visit.pop() {
                if visited.contains(&current_id) {
                    continue;
                }

                visited.insert(current_id.clone());

                if let Some(pattern) = field.patterns.iter().find(|p| p.id == current_id) {
                    cluster_members.push(pattern.id.clone());

                    // Add connected patterns
                    if let Some(connections) = self
                        .organization_state
                        .connectivity
                        .adjacency_matrix
                        .get(&current_id)
                    {
                        for connected_id in connections.keys() {
                            if !visited.contains(connected_id) {
                                to_visit.push(connected_id.clone());
                            }
                        }
                    }
                }
            }

            // Create cluster if it meets minimum size
            if cluster_members.len() >= self.config.min_cluster_size {
                let cluster =
                    self.create_cluster_from_members(&cluster_id, &cluster_members, field)?;
                updated_clusters.push(cluster.clone());
                new_clusters.insert(cluster_id.clone(), cluster);
            }
        }

        // Update pattern clusters
        self.pattern_clusters = new_clusters;

        // Update cluster assignments in connectivity
        for (cluster_id, cluster) in &self.pattern_clusters {
            for member_id in &cluster.members {
                self.organization_state
                    .connectivity
                    .clusters
                    .insert(member_id.clone(), cluster_id.clone());
            }
        }

        Ok(updated_clusters)
    }

    /// Create a cluster from member patterns
    fn create_cluster_from_members(
        &self,
        cluster_id: &str,
        members: &[String],
        field: &NeuralField,
    ) -> ContextNestResult<PatternCluster> {
        // Calculate cluster center (centroid of member embeddings)
        let mut center = None;
        let mut total_strength = 0.0f32;

        for member_id in members {
            if let Some(pattern) = field.patterns.iter().find(|p| p.id == *member_id) {
                total_strength += pattern.strength;

                if center.is_none() {
                    center = Some(pattern.embedding.clone());
                } else {
                    // Add to center (will average later)
                    let center_ref = center.as_mut().unwrap();
                    for (i, val) in pattern.embedding.iter().enumerate() {
                        center_ref[i] += val;
                    }
                }
            }
        }

        // Average the center
        if let Some(mut center_vec) = center {
            let member_count = members.len() as f32;
            for val in &mut center_vec {
                *val /= member_count;
            }

            // Calculate cohesion (average similarity within cluster)
            let mut total_similarity = 0.0f32;
            let mut similarity_count = 0;

            for (i, member_id) in members.iter().enumerate() {
                if let Some(pattern1) = field.patterns.iter().find(|p| p.id == *member_id) {
                    for other_id in members.iter().skip(i + 1) {
                        if let Some(pattern2) = field.patterns.iter().find(|p| p.id == *other_id) {
                            let similarity = Self::calculate_similarity(
                                &pattern1.embedding,
                                &pattern2.embedding,
                            );
                            total_similarity += similarity;
                            similarity_count += 1;
                        }
                    }
                }
            }

            let cohesion = if similarity_count > 0 {
                total_similarity / similarity_count as f32
            } else {
                0.0
            };

            // Calculate complexity based on member diversity
            let complexity = self.calculate_cluster_complexity(members, field);

            Ok(PatternCluster {
                id: cluster_id.to_string(),
                members: members.to_vec(),
                center: center_vec,
                cohesion,
                separation: 0.0, // Will be calculated later
                complexity,
                emergence_potential: cohesion * complexity * total_strength,
                created_at: Utc::now(),
            })
        } else {
            Err(crate::ContextNestError::Api(
                "No valid patterns for cluster".to_string(),
            ))
        }
    }

    /// Calculate cluster complexity based on member diversity
    fn calculate_cluster_complexity(&self, members: &[String], field: &NeuralField) -> f32 {
        if members.len() < 2 {
            return 0.0;
        }

        // Calculate average pairwise distance as complexity measure
        let mut total_distance = 0.0f32;
        let mut pair_count = 0;

        for (i, member_id) in members.iter().enumerate() {
            if let Some(pattern1) = field.patterns.iter().find(|p| p.id == *member_id) {
                for other_id in members.iter().skip(i + 1) {
                    if let Some(pattern2) = field.patterns.iter().find(|p| p.id == *other_id) {
                        let distance =
                            Self::calculate_distance(&pattern1.embedding, &pattern2.embedding);
                        total_distance += distance;
                        pair_count += 1;
                    }
                }
            }
        }

        if pair_count > 0 {
            total_distance / pair_count as f32
        } else {
            0.0
        }
    }

    /// Detect emergence events in current field state
    fn detect_emergence_events(
        &self,
        field: &NeuralField,
    ) -> ContextNestResult<Vec<EmergenceEvent>> {
        let mut events = Vec::new();

        // Detect cluster emergence
        for cluster in self.pattern_clusters.values() {
            if cluster.cohesion > 0.8 && cluster.complexity > self.config.complexity_threshold {
                events.push(EmergenceEvent {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    emergence_type: EmergenceType::NovelConcept {
                        novelty_score: cluster.complexity,
                        semantic_distance: 1.0 - cluster.cohesion,
                    },
                    confidence: cluster.cohesion,
                    evidence: vec![
                        format!(
                            "Cluster '{}' with high cohesion: {:.2}",
                            cluster.id, cluster.cohesion
                        ),
                        format!("Complexity: {:.2}", cluster.complexity),
                    ],
                    affected_patterns: cluster.members.clone(),
                    implications: vec![
                        "Emergent conceptual structure detected".to_string(),
                        "Potential for higher-level abstraction".to_string(),
                    ],
                });
            }
        }

        // Detect global organization emergence
        let organization_quality = self.calculate_organization_quality();
        if organization_quality > 0.8 {
            events.push(EmergenceEvent {
                id: uuid::Uuid::new_v4().to_string(),
                timestamp: Utc::now(),
                emergence_type: EmergenceType::AbstractionFormation {
                    abstraction_level: self.pattern_clusters.len(),
                    generalization_score: organization_quality,
                },
                confidence: organization_quality,
                evidence: vec![
                    format!("Global organization quality: {:.2}", organization_quality),
                    format!("Phase: {:?}", self.organization_state.phase),
                ],
                affected_patterns: field.patterns.iter().map(|p| p.id.clone()).collect(),
                implications: vec![
                    "System achieving global coherence".to_string(),
                    "Emergent self-organization capabilities".to_string(),
                ],
            });
        }

        Ok(events)
    }

    /// Generate spontaneous patterns
    fn generate_spontaneous_patterns(
        &self,
        field: &NeuralField,
    ) -> ContextNestResult<Vec<SemanticPattern>> {
        let mut spontaneous = Vec::new();

        // Random chance for spontaneous generation
        if rand::random::<f32>() < self.config.spontaneous_generation_rate {
            // Generate pattern in region of high energy/uncertainty
            if let Some(hotspot) = self.find_creative_hotspot(field) {
                let pattern = SemanticPattern {
                    id: uuid::Uuid::new_v4().to_string(),
                    content: "Spontaneously generated pattern".to_string(),
                    embedding: hotspot,
                    strength: 0.6,
                    resonance: 0.5,
                    decay_rate: 0.01,
                    activation_count: 0,
                    created_at: Utc::now(),
                    last_activated: Utc::now(),
                    deleted_at: None,
                    delete_reason: None,
                };
                spontaneous.push(pattern);
            }
        }

        Ok(spontaneous)
    }

    /// Find creative hotspot for spontaneous generation
    fn find_creative_hotspot(&self, field: &NeuralField) -> Option<Vec<f32>> {
        if field.patterns.is_empty() {
            return None;
        }

        // Find region with moderate pattern density and high energy
        let mut best_spot = None;
        let mut best_score = 0.0f32;

        // Sample random points in embedding space
        for _ in 0..10 {
            let mut point = vec![0.0f32; field.properties.embedding_dim];

            // Generate random point based on existing patterns
            if let Some(ref_pattern) = field.patterns.iter().choose(&mut rand::rng()) {
                for (i, val) in ref_pattern.embedding.iter().enumerate() {
                    point[i] = val + (rand::random::<f32>() - 0.5) * 0.5;
                }
            }

            // Calculate score based on local properties
            let density = self.calculate_local_density(&point, field);
            let energy = field.state.energy;

            // Prefer moderate density with high energy
            let score = (1.0 - (density - 0.5).abs()) * energy;

            if score > best_score {
                best_score = score;
                best_spot = Some(point);
            }
        }

        best_spot
    }

    /// Calculate local pattern density at a point
    fn calculate_local_density(&self, point: &[f32], field: &NeuralField) -> f32 {
        let mut nearby_count = 0;
        let radius = self.config.influence_radius;

        for pattern in &field.patterns {
            let distance = Self::calculate_distance(point, &pattern.embedding);
            if distance < radius {
                nearby_count += 1;
            }
        }

        nearby_count as f32 / field.patterns.len() as f32
    }

    /// Update organization phase based on current state
    fn update_organization_phase(&mut self, field: &NeuralField) -> ContextNestResult<()> {
        let organization_quality = self.calculate_organization_quality();
        let coherence = field.state.coherence;
        let cluster_count = self.pattern_clusters.len();

        let new_phase = match (organization_quality, coherence, cluster_count) {
            (q, c, _) if q < 0.3 || c < 0.4 => OrganizationPhase::Dispersed,
            (q, _, _) if q < 0.6 => OrganizationPhase::LocalClustering,
            (q, c, _) if q < 0.8 || c < 0.7 => OrganizationPhase::GlobalCoherence,
            (q, c, clusters) if q >= 0.8 && c >= 0.7 && clusters > 0 => {
                if self.organization_state.phase == OrganizationPhase::Organized
                    && (organization_quality < self.metrics.organization_quality - 0.1)
                {
                    OrganizationPhase::Reorganizing
                } else {
                    OrganizationPhase::Organized
                }
            }
            _ => self.organization_state.phase.clone(),
        };

        self.organization_state.phase = new_phase;
        self.organization_state.coherence_trajectory.push(coherence);

        // Keep trajectory manageable
        if self.organization_state.coherence_trajectory.len() > 100 {
            self.organization_state.coherence_trajectory.remove(0);
        }

        Ok(())
    }

    /// Calculate overall organization quality
    fn calculate_organization_quality(&self) -> f32 {
        if self.pattern_clusters.is_empty() {
            return 0.0;
        }

        // Average cluster cohesion
        let avg_cohesion: f32 = self
            .pattern_clusters
            .values()
            .map(|c| c.cohesion)
            .sum::<f32>()
            / self.pattern_clusters.len() as f32;

        // Pattern connectivity (average centrality)
        let avg_centrality: f32 = self
            .organization_state
            .connectivity
            .centrality
            .values()
            .sum::<f32>()
            / self.organization_state.connectivity.centrality.len() as f32;

        // Phase progression bonus
        let phase_bonus = match self.organization_state.phase {
            OrganizationPhase::Dispersed => 0.0,
            OrganizationPhase::LocalClustering => 0.25,
            OrganizationPhase::GlobalCoherence => 0.5,
            OrganizationPhase::Organized => 1.0,
            OrganizationPhase::Reorganizing => 0.75,
        };

        (avg_cohesion + avg_centrality + phase_bonus) / 3.0
    }

    /// Update self-organizing metrics
    fn update_metrics(&mut self, field: &NeuralField) -> ContextNestResult<()> {
        self.metrics.organization_cycles = self.organization_state.organization_cycles;
        self.metrics.organization_quality = self.calculate_organization_quality();

        if !self.pattern_clusters.is_empty() {
            self.metrics.avg_cohesion = self
                .pattern_clusters
                .values()
                .map(|c| c.cohesion)
                .sum::<f32>()
                / self.pattern_clusters.len() as f32;
        }

        self.metrics.last_updated = Utc::now();
        Ok(())
    }

    // Helper methods

    /// Calculate cosine similarity between embeddings
    fn calculate_similarity(embedding1: &[f32], embedding2: &[f32]) -> f32 {
        if embedding1.len() != embedding2.len() {
            return 0.0;
        }

        let dot_product: f32 = embedding1
            .iter()
            .zip(embedding2.iter())
            .map(|(a, b)| a * b)
            .sum();

        let magnitude1: f32 = embedding1.iter().map(|x| x * x).sum::<f32>().sqrt();
        let magnitude2: f32 = embedding2.iter().map(|x| x * x).sum::<f32>().sqrt();

        if magnitude1 == 0.0 || magnitude2 == 0.0 {
            return 0.0;
        }

        (dot_product / (magnitude1 * magnitude2)).max(0.0).min(1.0)
    }

    /// Calculate Euclidean distance between embeddings
    fn calculate_distance(embedding1: &[f32], embedding2: &[f32]) -> f32 {
        if embedding1.len() != embedding2.len() {
            return f32::INFINITY;
        }

        embedding1
            .iter()
            .zip(embedding2.iter())
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    /// Calculate midpoint between two embeddings
    fn calculate_midpoint(embedding1: &[f32], embedding2: &[f32]) -> Vec<f32> {
        if embedding1.len() != embedding2.len() {
            return Vec::new();
        }

        embedding1
            .iter()
            .zip(embedding2.iter())
            .map(|(a, b)| (a + b) / 2.0)
            .collect()
    }

    /// Calculate overlap between two attractors
    fn calculate_attractor_overlap(attractor1: &Attractor, attractor2: &Attractor) -> f32 {
        let distance = Self::calculate_distance(&attractor1.center, &attractor2.center);
        let combined_radius = attractor1.radius + attractor2.radius;

        if distance >= combined_radius {
            0.0
        } else {
            let overlap = 1.0 - (distance / combined_radius);
            overlap * attractor1.strength * attractor2.strength
        }
    }

    /// Get current organization state
    pub fn get_organization_state(&self) -> &OrganizationState {
        &self.organization_state
    }

    /// Get current metrics
    pub fn get_metrics(&self) -> &SelfOrganizingMetrics {
        &self.metrics
    }

    /// Get pattern clusters
    pub fn get_pattern_clusters(&self) -> &HashMap<String, PatternCluster> {
        &self.pattern_clusters
    }

    /// Get emergence history
    pub fn get_emergence_history(&self) -> &[EmergenceEvent] {
        &self.emergence_history
    }
}

// Supporting data structures

/// Gap in pattern space that could be filled
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternGap {
    pub id: String,
    pub center: Vec<f32>,
    pub radius: f32,
    pub potential: f32,
    pub gap_type: GapType,
    pub related_patterns: Vec<String>,
}

/// Type of pattern gap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GapType {
    UnderpopulatedRegion,
    ConceptualBridge,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_self_organizing_creation() {
        let config = SelfOrganizingConfig::default();
        let so = SelfOrganizingEmergence::new(config);
        assert!(matches!(
            so.organization_state.phase,
            OrganizationPhase::Dispersed
        ));
    }

    #[test]
    fn test_pattern_cluster_creation() {
        let cluster = PatternCluster {
            id: "test".to_string(),
            members: vec!["pattern1".to_string(), "pattern2".to_string()],
            center: vec![0.5, 0.5],
            cohesion: 0.8,
            separation: 0.3,
            complexity: 0.6,
            emergence_potential: 0.7,
            created_at: Utc::now(),
        };
        assert_eq!(cluster.members.len(), 2);
        assert!(cluster.cohesion > 0.7);
    }

    #[test]
    fn test_organization_quality_calculation() {
        let config = SelfOrganizingConfig::default();
        let mut so = SelfOrganizingEmergence::new(config);

        // Should be 0 with no clusters
        assert_eq!(so.calculate_organization_quality(), 0.0);
    }
}

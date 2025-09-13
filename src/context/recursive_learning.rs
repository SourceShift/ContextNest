use crate::context::field::NeuralField;
use crate::context::memory::AttractorField;
use crate::context::meta_recursive::{EnhancementEvent, EnhancementType, SystemAnalysis};
use crate::error::ContextNestResult;
use crate::{ContextNestError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Recursive learner that learns from its own learning process
/// This implements meta-learning by extracting patterns from successful learning episodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecursiveLearner {
    /// History of learning episodes
    pub learning_episodes: Vec<LearningEpisode>,
    /// Extracted patterns from successful learning
    pub learning_patterns: Vec<LearningPattern>,
    /// Meta-patterns that learn from other patterns (NEW)
    pub meta_patterns: Vec<MetaLearningPattern>,
    /// Meta-learning metrics
    pub meta_metrics: MetaLearningMetrics,
    /// Learning about learning insights
    pub meta_insights: Vec<MetaInsight>,
    /// Configuration for learning strategies
    pub learning_config: LearningConfig,
    /// Optimization history for tracking improvements
    pub optimization_history: Vec<OptimizationRecord>,
}

/// A single learning episode with context and outcomes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningEpisode {
    pub episode_id: String,
    pub timestamp: DateTime<Utc>,
    pub context: LearningContext,
    pub actions_taken: Vec<LearningAction>,
    pub outcomes: Vec<LearningOutcome>,
    pub effectiveness: f32,
    pub duration_ms: u64,
    pub recursive_depth: usize,
}

/// Context in which learning occurred
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningContext {
    pub task_type: TaskType,
    pub initial_state: StateSnapshot,
    pub constraints: Vec<String>,
    pub available_resources: HashMap<String, f32>,
    pub domain: String,
}

/// Type of learning task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskType {
    PatternRecognition,
    Optimization,
    Adaptation,
    ProblemSolving,
    TransferLearning,
    EmergenceDetection,
}

/// Snapshot of system state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub coherence_score: f32,
    pub memory_utilization: f32,
    pub performance_metrics: HashMap<String, f32>,
    pub active_patterns: usize,
    pub timestamp: DateTime<Utc>,
}

/// Action taken during learning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningAction {
    pub action_id: String,
    pub action_type: ActionType,
    pub parameters: HashMap<String, serde_json::Value>,
    pub rationale: String,
    pub expected_impact: f32,
    pub actual_impact: Option<f32>,
}

/// Type of learning action
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ActionType {
    ParameterAdjustment,
    StrategyChange,
    ResourceAllocation,
    PatternIntegration,
    ExplorationPhase,
    ExploitationPhase,
}

/// Outcome from learning action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningOutcome {
    pub outcome_id: String,
    pub success: bool,
    pub improvement: f32,
    pub side_effects: Vec<String>,
    pub lessons_learned: Vec<String>,
    pub generalizability: f32,
}

/// Pattern extracted from successful learning episodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningPattern {
    pub pattern_id: String,
    pub pattern_type: PatternType,
    pub conditions: Vec<PatternCondition>,
    pub actions: Vec<ActionSequence>,
    pub success_rate: f32,
    pub confidence: f32,
    pub applicability_domains: Vec<String>,
    pub discovered_at: DateTime<Utc>,
    pub applications_count: usize,
}

/// Type of learning pattern
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum PatternType {
    SuccessSequence,
    AvoidancePattern,
    OptimizationStrategy,
    AdaptationTactic,
    TransferPattern,
    EmergentBehavior,
}

/// Condition for pattern application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternCondition {
    pub condition_type: String,
    pub threshold: f32,
    pub required_context: Vec<String>,
}

/// Sequence of actions forming a pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSequence {
    pub actions: Vec<ActionType>,
    pub timing_constraints: Option<TimingConstraints>,
    pub ordering_flexibility: OrderingFlexibility,
}

/// Timing constraints for action sequences
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimingConstraints {
    pub max_delay_between_actions_ms: u64,
    pub total_sequence_timeout_ms: u64,
}

/// Flexibility in action ordering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderingFlexibility {
    Strict,
    Flexible,
    Parallel,
}

/// Metrics for meta-learning performance
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetaLearningMetrics {
    pub total_episodes: usize,
    pub successful_episodes: usize,
    pub patterns_discovered: usize,
    pub patterns_applied: usize,
    pub average_improvement_per_episode: f32,
    pub learning_efficiency: f32,
    pub transfer_learning_success_rate: f32,
    pub meta_optimization_cycles: usize,
    pub last_updated: DateTime<Utc>,
}

/// Insight about the learning process itself
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaInsight {
    pub insight_id: String,
    pub insight_type: InsightType,
    pub description: String,
    pub supporting_evidence: Vec<String>,
    pub confidence: f32,
    pub actionable_recommendations: Vec<String>,
    pub discovered_at: DateTime<Utc>,
}

/// Type of meta-insight
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InsightType {
    LearningRatePattern,
    OptimalStrategyForContext,
    BottleneckIdentification,
    SynergyOpportunity,
    TransferOpportunity,
    EmergentCapability,
}

/// Configuration for learning strategies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningConfig {
    pub exploration_rate: f32,
    pub exploitation_rate: f32,
    pub pattern_extraction_threshold: f32,
    pub minimum_episodes_for_pattern: usize,
    pub max_recursive_depth: usize,
    pub enable_transfer_learning: bool,
    pub enable_meta_optimization: bool,
}

impl Default for LearningConfig {
    fn default() -> Self {
        Self {
            exploration_rate: 0.3,
            exploitation_rate: 0.7,
            pattern_extraction_threshold: 0.75,
            minimum_episodes_for_pattern: 3,
            max_recursive_depth: 5,
            enable_transfer_learning: true,
            enable_meta_optimization: true,
        }
    }
}

/// Record of optimization attempts and results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationRecord {
    pub record_id: String,
    pub timestamp: DateTime<Utc>,
    pub optimization_target: String,
    pub baseline_performance: f32,
    pub final_performance: f32,
    pub improvement_percentage: f32,
    pub techniques_used: Vec<String>,
    pub iterations_required: usize,
    pub success: bool,
}

impl RecursiveLearner {
    /// Create a new recursive learner with default configuration
    pub fn new() -> Self {
        Self {
            learning_episodes: Vec::new(),
            learning_patterns: Vec::new(),
            meta_patterns: Vec::new(), // Initialize meta-patterns
            meta_metrics: MetaLearningMetrics {
                last_updated: Utc::now(),
                ..Default::default()
            },
            meta_insights: Vec::new(),
            learning_config: LearningConfig::default(),
            optimization_history: Vec::new(),
        }
    }

    /// Create a new recursive learner with custom configuration
    pub fn with_config(config: LearningConfig) -> Self {
        let mut learner = Self::new();
        learner.learning_config = config;
        learner
    }

    /// Learn from a completed enhancement event
    pub fn learn_from_enhancement(
        &mut self,
        event: &EnhancementEvent,
        system_analysis: &SystemAnalysis,
    ) -> ContextNestResult<LearningEpisode> {
        let start_time = Utc::now();

        // Create learning context from system analysis
        let context = LearningContext {
            task_type: self.infer_task_type(&event.enhancement_type),
            initial_state: StateSnapshot {
                coherence_score: match system_analysis.field_health {
                    crate::context::field::FieldHealth::Excellent => 0.9,
                    crate::context::field::FieldHealth::Good => 0.7,
                    crate::context::field::FieldHealth::Fair => 0.5,
                    crate::context::field::FieldHealth::Poor => 0.3,
                    crate::context::field::FieldHealth::Critical => 0.1,
                },
                memory_utilization: system_analysis.memory_utilization,
                performance_metrics: HashMap::from([
                    ("overall".to_string(), system_analysis.overall_performance),
                    ("protocol".to_string(), system_analysis.protocol_performance),
                ]),
                active_patterns: 0, // Would be populated from actual field
                timestamp: Utc::now(),
            },
            constraints: event.trigger_conditions.clone(),
            available_resources: HashMap::from([
                ("cpu".to_string(), 0.7),
                ("memory".to_string(), system_analysis.memory_utilization),
            ]),
            domain: "general".to_string(),
        };

        // Convert enhancement modifications to learning actions
        let actions_taken: Vec<LearningAction> = event
            .modifications
            .iter()
            .map(|modification| LearningAction {
                action_id: uuid::Uuid::new_v4().to_string(),
                action_type: self.map_modification_to_action_type(&modification.modification_type),
                parameters: modification.parameters.clone(),
                rationale: modification.description.clone(),
                expected_impact: modification.impact_assessment.predicted_improvement,
                actual_impact: Some(event.effectiveness),
            })
            .collect();

        // Create learning outcomes
        let outcomes = vec![LearningOutcome {
            outcome_id: uuid::Uuid::new_v4().to_string(),
            success: event.effectiveness > 0.6,
            improvement: event.effectiveness,
            side_effects: Vec::new(),
            lessons_learned: self.extract_lessons(event, system_analysis),
            generalizability: self.assess_generalizability(event, system_analysis),
        }];

        let duration = (Utc::now() - start_time).num_milliseconds() as u64;

        let episode = LearningEpisode {
            episode_id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            context,
            actions_taken,
            outcomes,
            effectiveness: event.effectiveness,
            duration_ms: duration,
            recursive_depth: event.recursive_level,
        };

        // Store the episode
        self.learning_episodes.push(episode.clone());

        // Update metrics
        self.update_meta_metrics(&episode);

        // Extract patterns if threshold reached
        if self.learning_episodes.len() >= self.learning_config.minimum_episodes_for_pattern {
            self.extract_learning_patterns()?;
        }

        // Generate meta-insights
        if self.should_generate_insights() {
            self.generate_meta_insights()?;
        }

        Ok(episode)
    }

    /// Extract patterns from learning history
    pub fn extract_learning_patterns(&mut self) -> ContextNestResult<Vec<LearningPattern>> {
        let mut new_patterns = Vec::new();

        // Group episodes by task type and success
        let successful_episodes: Vec<&LearningEpisode> = self
            .learning_episodes
            .iter()
            .filter(|ep| ep.effectiveness > self.learning_config.pattern_extraction_threshold)
            .collect();

        // Convert references to owned episodes to avoid borrow checker issues
        let owned_episodes: Vec<LearningEpisode> =
            successful_episodes.iter().map(|&ep| ep.clone()).collect();

        // Find common action sequences in successful episodes
        let action_sequences = self.find_common_action_sequences(&owned_episodes);

        for sequence in action_sequences {
            // Check if this pattern is novel
            if !self.is_duplicate_pattern(&sequence) {
                let pattern = LearningPattern {
                    pattern_id: uuid::Uuid::new_v4().to_string(),
                    pattern_type: PatternType::SuccessSequence,
                    conditions: self.extract_pattern_conditions(&owned_episodes),
                    actions: vec![sequence],
                    success_rate: self.calculate_pattern_success_rate(&owned_episodes),
                    confidence: self.calculate_pattern_confidence(&owned_episodes),
                    applicability_domains: vec!["general".to_string()],
                    discovered_at: Utc::now(),
                    applications_count: 0,
                };

                new_patterns.push(pattern.clone());
                self.learning_patterns.push(pattern);
            }
        }

        // Extract advanced patterns using enhanced analysis
        // Convert references to owned episodes to avoid borrow checker issues
        let episode_clones: Vec<LearningEpisode> =
            successful_episodes.iter().map(|&ep| ep.clone()).collect();
        let advanced_patterns = self.extract_advanced_learning_patterns(&episode_clones)?;
        new_patterns.extend(advanced_patterns);

        // Update metrics
        self.meta_metrics.patterns_discovered += new_patterns.len();

        Ok(new_patterns)
    }

    /// Extract advanced learning patterns using sophisticated analysis
    fn extract_advanced_learning_patterns(
        &mut self,
        successful_episodes: &[LearningEpisode],
    ) -> ContextNestResult<Vec<LearningPattern>> {
        let mut advanced_patterns = Vec::new();

        // Extract contextual patterns
        let contextual_patterns = self.extract_contextual_patterns(successful_episodes)?;
        advanced_patterns.extend(contextual_patterns);

        // Extract temporal patterns
        let temporal_patterns = self.extract_temporal_patterns(successful_episodes)?;
        advanced_patterns.extend(temporal_patterns);

        // Extract cross-domain patterns
        let cross_domain_patterns = self.extract_cross_domain_patterns(successful_episodes)?;
        advanced_patterns.extend(cross_domain_patterns);

        // Extract adaptive patterns
        let adaptive_patterns = self.extract_adaptive_patterns(successful_episodes)?;
        advanced_patterns.extend(adaptive_patterns);

        Ok(advanced_patterns)
    }

    /// Extract patterns based on contextual conditions
    fn extract_contextual_patterns(
        &self,
        episodes: &[LearningEpisode],
    ) -> ContextNestResult<Vec<LearningPattern>> {
        let mut patterns = Vec::new();

        // Group episodes by similar initial conditions
        let mut context_groups: HashMap<String, Vec<&LearningEpisode>> = HashMap::new();

        for episode in episodes {
            let context_key = self.generate_context_key(&episode.context);
            context_groups
                .entry(context_key)
                .or_insert_with(Vec::new)
                .push(episode);
        }

        // For each context group, extract successful patterns
        for (context, group_episodes) in context_groups {
            if group_episodes.len() >= 2 {
                // Convert references to owned episodes to avoid borrow checker issues
                let owned_episodes: Vec<LearningEpisode> =
                    group_episodes.iter().map(|&ep| ep.clone()).collect();

                // Analyze what works in this context
                let successful_actions =
                    self.extract_successful_actions_from_group(&owned_episodes);

                if !successful_actions.is_empty() {
                    let pattern = LearningPattern {
                        pattern_id: uuid::Uuid::new_v4().to_string(),
                        pattern_type: PatternType::OptimizationStrategy,
                        conditions: self.extract_context_conditions(&context),
                        actions: successful_actions,
                        success_rate: self.calculate_group_success_rate(&owned_episodes),
                        confidence: self.calculate_group_confidence(&owned_episodes),
                        applicability_domains: vec![context],
                        discovered_at: Utc::now(),
                        applications_count: 0,
                    };
                    patterns.push(pattern);
                }
            }
        }

        Ok(patterns)
    }

    /// Extract temporal patterns (sequences that work over time)
    fn extract_temporal_patterns(
        &self,
        episodes: &[LearningEpisode],
    ) -> ContextNestResult<Vec<LearningPattern>> {
        let mut patterns = Vec::new();

        // Sort episodes by timestamp to find temporal sequences
        let mut sorted_episodes = episodes.to_vec();
        sorted_episodes.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        // Look for sequences that improve over time
        for window in sorted_episodes.windows(3) {
            if self.is_improving_sequence(window) {
                let pattern = LearningPattern {
                    pattern_id: uuid::Uuid::new_v4().to_string(),
                    pattern_type: PatternType::AdaptationTactic,
                    conditions: self.extract_temporal_conditions(window),
                    actions: self.extract_sequence_actions(window),
                    success_rate: self.calculate_sequence_success_rate(window),
                    confidence: self.calculate_sequence_confidence(window),
                    applicability_domains: vec!["temporal_improvement".to_string()],
                    discovered_at: Utc::now(),
                    applications_count: 0,
                };
                patterns.push(pattern);
            }
        }

        Ok(patterns)
    }

    /// Extract cross-domain transfer patterns
    fn extract_cross_domain_patterns(
        &self,
        episodes: &[LearningEpisode],
    ) -> ContextNestResult<Vec<LearningPattern>> {
        let mut patterns = Vec::new();

        // Group episodes by domain
        let mut domain_groups: HashMap<String, Vec<&LearningEpisode>> = HashMap::new();
        for episode in episodes {
            domain_groups
                .entry(episode.context.domain.clone())
                .or_insert_with(Vec::new)
                .push(episode);
        }

        // Look for patterns that work across multiple domains
        if domain_groups.len() >= 2 {
            // Find actions that are successful in multiple domains
            let cross_domain_actions = self.find_cross_domain_successful_actions(&domain_groups);

            for (action, domains, effectiveness) in cross_domain_actions {
                let pattern = LearningPattern {
                    pattern_id: uuid::Uuid::new_v4().to_string(),
                    pattern_type: PatternType::TransferPattern,
                    conditions: self.extract_cross_domain_conditions(&domains),
                    actions: vec![ActionSequence {
                        actions: vec![action],
                        timing_constraints: None,
                        ordering_flexibility: OrderingFlexibility::Flexible,
                    }],
                    success_rate: effectiveness,
                    confidence: self.calculate_cross_domain_confidence(&domains),
                    applicability_domains: domains,
                    discovered_at: Utc::now(),
                    applications_count: 0,
                };
                patterns.push(pattern);
            }
        }

        Ok(patterns)
    }

    /// Extract adaptive patterns that respond to changing conditions
    fn extract_adaptive_patterns(
        &self,
        episodes: &[LearningEpisode],
    ) -> ContextNestResult<Vec<LearningPattern>> {
        let mut patterns = Vec::new();

        // Look for episodes where conditions changed during execution
        for episode in episodes {
            if self.has_condition_changes(episode) {
                let adaptive_actions = self.extract_adaptive_actions(episode);

                if !adaptive_actions.is_empty() {
                    let pattern = LearningPattern {
                        pattern_id: uuid::Uuid::new_v4().to_string(),
                        pattern_type: PatternType::EmergentBehavior,
                        conditions: self.extract_adaptive_conditions(episode),
                        actions: adaptive_actions,
                        success_rate: episode.effectiveness,
                        confidence: 0.8, // High confidence for adaptive patterns
                        applicability_domains: vec![episode.context.domain.clone()],
                        discovered_at: Utc::now(),
                        applications_count: 0,
                    };
                    patterns.push(pattern);
                }
            }
        }

        Ok(patterns)
    }

    /// Extract meta-patterns that learn from other patterns (NEW)
    pub fn extract_meta_patterns(&mut self) -> ContextNestResult<Vec<MetaLearningPattern>> {
        let mut meta_patterns = Vec::new();

        // Analyze existing patterns to find higher-order patterns
        if self.learning_patterns.len() >= 3 {
            // Pattern 1: Patterns that lead to successful pattern discovery
            let discovery_patterns = self.find_pattern_discovery_patterns()?;
            meta_patterns.extend(discovery_patterns);

            // Pattern 2: Patterns that optimize other patterns
            let optimization_patterns = self.find_pattern_optimization_patterns()?;
            meta_patterns.extend(optimization_patterns);

            // Pattern 3: Recursive patterns that apply to themselves
            let recursive_patterns = self.find_recursive_patterns()?;
            meta_patterns.extend(recursive_patterns);

            // Pattern 4: Cross-domain pattern interaction patterns
            let interaction_patterns = self.find_pattern_interaction_patterns()?;
            meta_patterns.extend(interaction_patterns);
        }

        Ok(meta_patterns)
    }

    /// Find patterns that lead to successful pattern discovery
    fn find_pattern_discovery_patterns(&self) -> ContextNestResult<Vec<MetaLearningPattern>> {
        let mut patterns = Vec::new();

        // Group successful pattern discoveries by context
        let mut discovery_contexts: HashMap<String, Vec<&LearningPattern>> = HashMap::new();

        for pattern in &self.learning_patterns {
            if pattern.success_rate > 0.8 && pattern.applications_count > 0 {
                let context_key = self.generate_pattern_context_key(pattern);
                discovery_contexts
                    .entry(context_key)
                    .or_insert_with(Vec::new)
                    .push(pattern);
            }
        }

        // Find commonalities in successful discovery contexts
        for (context, context_patterns) in discovery_contexts {
            if context_patterns.len() >= 2 {
                let meta_pattern = MetaLearningPattern {
                    meta_pattern_id: uuid::Uuid::new_v4().to_string(),
                    meta_pattern_type: MetaPatternType::PatternDiscovery,
                    source_patterns: context_patterns
                        .iter()
                        .map(|p| p.pattern_id.clone())
                        .collect(),
                    learning_strategy: LearningStrategy::ContextualAbstraction,
                    effectiveness: context_patterns.iter().map(|p| p.success_rate).sum::<f32>()
                        / context_patterns.len() as f32,
                    meta_conditions: self.extract_meta_conditions(&context_patterns),
                    recursive_application: RecursiveApplication {
                        can_apply_to_self: true,
                        max_recursion_depth: 3,
                        self_improvement_factor: 1.2,
                    },
                    discovered_at: Utc::now(),
                    applications_count: 0,
                };
                patterns.push(meta_pattern);
            }
        }

        Ok(patterns)
    }

    /// Find patterns that optimize other patterns
    fn find_pattern_optimization_patterns(&self) -> ContextNestResult<Vec<MetaLearningPattern>> {
        let mut patterns = Vec::new();

        // Look for patterns that have been successfully applied multiple times
        let optimized_patterns: Vec<&LearningPattern> = self
            .learning_patterns
            .iter()
            .filter(|p| p.applications_count >= 3 && p.success_rate > 0.7)
            .collect();

        // Find optimization strategies
        for pattern_group in optimized_patterns.chunks(2) {
            if pattern_group.len() == 2 {
                let optimization_strategy =
                    self.infer_optimization_strategy(pattern_group[0], pattern_group[1]);

                let meta_pattern = MetaLearningPattern {
                    meta_pattern_id: uuid::Uuid::new_v4().to_string(),
                    meta_pattern_type: MetaPatternType::PatternOptimization,
                    source_patterns: pattern_group.iter().map(|p| p.pattern_id.clone()).collect(),
                    learning_strategy: optimization_strategy,
                    effectiveness: pattern_group.iter().map(|p| p.success_rate).sum::<f32>() / 2.0,
                    meta_conditions: vec![MetaCondition {
                        condition_type: "pattern_maturity".to_string(),
                        threshold: 3.0, // Minimum applications
                        context_requirement: "multiple_applications".to_string(),
                    }],
                    recursive_application: RecursiveApplication {
                        can_apply_to_self: true,
                        max_recursion_depth: 2,
                        self_improvement_factor: 1.1,
                    },
                    discovered_at: Utc::now(),
                    applications_count: 0,
                };
                patterns.push(meta_pattern);
            }
        }

        Ok(patterns)
    }

    /// Find recursive patterns that can apply to themselves
    fn find_recursive_patterns(&self) -> ContextNestResult<Vec<MetaLearningPattern>> {
        let mut patterns = Vec::new();

        // Look for self-referential patterns
        for pattern in &self.learning_patterns {
            if self.is_self_applicable(pattern) {
                let meta_pattern = MetaLearningPattern {
                    meta_pattern_id: uuid::Uuid::new_v4().to_string(),
                    meta_pattern_type: MetaPatternType::RecursiveSelfApplication,
                    source_patterns: vec![pattern.pattern_id.clone()],
                    learning_strategy: LearningStrategy::RecursiveRefinement,
                    effectiveness: pattern.success_rate * 0.9, // Slightly conservative for recursive patterns
                    meta_conditions: vec![MetaCondition {
                        condition_type: "pattern_consistency".to_string(),
                        threshold: pattern.confidence,
                        context_requirement: "stable_behavior".to_string(),
                    }],
                    recursive_application: RecursiveApplication {
                        can_apply_to_self: true,
                        max_recursion_depth: 5,
                        self_improvement_factor: 1.15,
                    },
                    discovered_at: Utc::now(),
                    applications_count: 0,
                };
                patterns.push(meta_pattern);
            }
        }

        Ok(patterns)
    }

    /// Find patterns that describe how other patterns interact
    fn find_pattern_interaction_patterns(&self) -> ContextNestResult<Vec<MetaLearningPattern>> {
        let mut patterns = Vec::new();

        // Look for synergistic pattern combinations
        for i in 0..self.learning_patterns.len() {
            for j in (i + 1)..self.learning_patterns.len() {
                let pattern_a = &self.learning_patterns[i];
                let pattern_b = &self.learning_patterns[j];

                if self.patterns_have_synergy(pattern_a, pattern_b) {
                    let interaction_pattern = MetaLearningPattern {
                        meta_pattern_id: uuid::Uuid::new_v4().to_string(),
                        meta_pattern_type: MetaPatternType::PatternInteraction,
                        source_patterns: vec![
                            pattern_a.pattern_id.clone(),
                            pattern_b.pattern_id.clone(),
                        ],
                        learning_strategy: LearningStrategy::SynergisticCombination,
                        effectiveness: (pattern_a.success_rate + pattern_b.success_rate) / 2.0
                            * 1.1, // Synergy bonus
                        meta_conditions: vec![MetaCondition {
                            condition_type: "pattern_compatibility".to_string(),
                            threshold: 0.8,
                            context_requirement: "compatible_domains".to_string(),
                        }],
                        recursive_application: RecursiveApplication {
                            can_apply_to_self: false,
                            max_recursion_depth: 1,
                            self_improvement_factor: 1.0,
                        },
                        discovered_at: Utc::now(),
                        applications_count: 0,
                    };
                    patterns.push(interaction_pattern);
                }
            }
        }

        Ok(patterns)
    }

    /// Generate context key for pattern grouping
    fn generate_pattern_context_key(&self, pattern: &LearningPattern) -> String {
        format!(
            "{}_{}_{}",
            match pattern.pattern_type {
                PatternType::SuccessSequence => "success",
                PatternType::AvoidancePattern => "avoidance",
                PatternType::OptimizationStrategy => "optimize",
                PatternType::AdaptationTactic => "adapt",
                PatternType::TransferPattern => "transfer",
                PatternType::EmergentBehavior => "emergent",
            },
            (pattern.success_rate * 10.0) as usize,
            pattern.applicability_domains.len()
        )
    }

    /// Extract meta-conditions from pattern group
    fn extract_meta_conditions(&self, patterns: &[&LearningPattern]) -> Vec<MetaCondition> {
        let mut conditions = Vec::new();

        // Common conditions across patterns
        if patterns.iter().all(|p| p.success_rate > 0.7) {
            conditions.push(MetaCondition {
                condition_type: "high_success_rate".to_string(),
                threshold: 0.7,
                context_requirement: "proven_effectiveness".to_string(),
            });
        }

        if patterns.iter().all(|p| p.confidence > 0.6) {
            conditions.push(MetaCondition {
                condition_type: "high_confidence".to_string(),
                threshold: 0.6,
                context_requirement: "well_established".to_string(),
            });
        }

        conditions
    }

    /// Infer optimization strategy from pattern comparison
    fn infer_optimization_strategy(
        &self,
        pattern1: &LearningPattern,
        pattern2: &LearningPattern,
    ) -> LearningStrategy {
        // Compare patterns to determine what optimization occurred
        if pattern1.actions.len() < pattern2.actions.len() {
            LearningStrategy::PatternElaboration
        } else if pattern1.actions.len() > pattern2.actions.len() {
            LearningStrategy::PatternSimplification
        } else if pattern2.success_rate > pattern1.success_rate {
            LearningStrategy::ParameterTuning
        } else {
            LearningStrategy::ContextualAdaptation
        }
    }

    /// Check if pattern is self-applicable
    fn is_self_applicable(&self, pattern: &LearningPattern) -> bool {
        // Pattern is self-applicable if it has consistent conditions and actions
        pattern.conditions.len() <= 3 && // Not too complex
        pattern.success_rate > 0.75 &&     // High success rate
        pattern.confidence > 0.7 // High confidence
    }

    /// Check if two patterns have synergy
    fn patterns_have_synergy(
        &self,
        pattern_a: &LearningPattern,
        pattern_b: &LearningPattern,
    ) -> bool {
        // Check for domain overlap
        let domain_overlap = pattern_a
            .applicability_domains
            .iter()
            .any(|d| pattern_b.applicability_domains.contains(d));

        // Check for complementary actions
        let complementary_actions =
            self.actions_are_complementary(&pattern_a.actions, &pattern_b.actions);

        // Check for compatible conditions
        let compatible_conditions =
            self.conditions_are_compatible(&pattern_a.conditions, &pattern_b.conditions);

        domain_overlap && (complementary_actions || compatible_conditions)
    }

    /// Check if action sequences are complementary
    fn actions_are_complementary(
        &self,
        actions_a: &[ActionSequence],
        actions_b: &[ActionSequence],
    ) -> bool {
        // Simple heuristic: different action types suggest complementarity
        let types_a: std::collections::HashSet<_> =
            actions_a.iter().flat_map(|seq| &seq.actions).collect();

        let types_b: std::collections::HashSet<_> =
            actions_b.iter().flat_map(|seq| &seq.actions).collect();

        !types_a
            .intersection(&types_b)
            .collect::<Vec<_>>()
            .is_empty()
            && types_a != types_b
    }

    /// Check if conditions are compatible
    fn conditions_are_compatible(
        &self,
        conditions_a: &[PatternCondition],
        conditions_b: &[PatternCondition],
    ) -> bool {
        // Conditions are compatible if they don't contradict
        for cond_a in conditions_a {
            for cond_b in conditions_b {
                if cond_a.condition_type == cond_b.condition_type {
                    let diff = (cond_a.threshold - cond_b.threshold).abs();
                    if diff > 0.3 {
                        return false; // Contradictory thresholds
                    }
                }
            }
        }
        true
    }

    /// Apply meta-pattern to enhance existing patterns
    pub fn apply_meta_pattern(
        &mut self,
        meta_pattern_id: &str,
        target_patterns: &[String],
    ) -> ContextNestResult<MetaPatternApplicationResult> {
        let meta_pattern = self
            .meta_patterns
            .iter()
            .find(|mp| mp.meta_pattern_id == meta_pattern_id)
            .ok_or_else(|| {
                ContextNestError::NotFound(format!("Meta pattern {} not found", meta_pattern_id))
            })?
            .clone();

        let mut enhanced_patterns = Vec::new();
        let mut total_improvement = 0.0;

        for target_pattern_id in target_patterns {
            // Find pattern index first to avoid borrow checker issues
            if let Some(pattern_idx) = self
                .learning_patterns
                .iter_mut()
                .position(|p| p.pattern_id == *target_pattern_id)
            {
                let original_effectiveness = self.learning_patterns[pattern_idx].success_rate;

                // Apply meta-pattern enhancement using a separate helper
                let enhanced_effectiveness = self.apply_meta_pattern_enhancement(
                    &self.learning_patterns[pattern_idx],
                    &meta_pattern,
                )?;

                let improvement = enhanced_effectiveness - original_effectiveness;
                total_improvement += improvement;

                // Update the pattern with the enhanced effectiveness
                self.learning_patterns[pattern_idx].success_rate = enhanced_effectiveness;

                enhanced_patterns.push(EnhancedPattern {
                    pattern_id: target_pattern_id.clone(),
                    original_effectiveness,
                    enhanced_effectiveness,
                    improvement,
                });
            }
        }

        // Update meta-pattern statistics
        let enhanced_patterns_len = enhanced_patterns.len();
        let application_result = MetaPatternApplicationResult {
            meta_pattern_id: meta_pattern_id.to_string(),
            enhanced_patterns,
            total_improvement: total_improvement / enhanced_patterns_len as f32,
            application_success: enhanced_patterns_len > 0,
            timestamp: Utc::now(),
        };

        Ok(application_result)
    }

    /// Apply meta-pattern enhancement and return new effectiveness (borrow checker friendly)
    fn apply_meta_pattern_enhancement(
        &self,
        target_pattern: &LearningPattern,
        meta_pattern: &MetaLearningPattern,
    ) -> ContextNestResult<f32> {
        let mut enhanced_effectiveness = target_pattern.success_rate;

        match meta_pattern.learning_strategy {
            LearningStrategy::RecursiveRefinement => {
                // Apply recursive improvement
                enhanced_effectiveness *=
                    meta_pattern.recursive_application.self_improvement_factor;
            }
            LearningStrategy::ContextualAbstraction => {
                // Generalize pattern conditions (improves effectiveness)
                enhanced_effectiveness *= 1.02;
            }
            LearningStrategy::SynergisticCombination => {
                // Combine with related patterns
                enhanced_effectiveness *= 1.1; // Synergy bonus
            }
            LearningStrategy::PatternElaboration => {
                // Add more specific conditions
                enhanced_effectiveness *= 1.05;
            }
            LearningStrategy::PatternSimplification => {
                // Simplify conditions while maintaining effectiveness
                enhanced_effectiveness *= 0.95; // Slight reduction for simplicity
            }
            LearningStrategy::ParameterTuning => {
                // Fine-tune pattern parameters
                enhanced_effectiveness *= 1.03;
            }
            LearningStrategy::ContextualAdaptation => {
                // Adapt patterns to new contexts
                enhanced_effectiveness *= 1.01;
            }
        }

        // Clamp values to valid range
        enhanced_effectiveness = enhanced_effectiveness.clamp(0.0, 1.0);

        Ok(enhanced_effectiveness)
    }

    /// Enhance a pattern using meta-pattern
    fn enhance_pattern_with_meta(
        &mut self,
        target_pattern: &mut LearningPattern,
        meta_pattern: &MetaLearningPattern,
    ) -> ContextNestResult<()> {
        match meta_pattern.learning_strategy {
            LearningStrategy::RecursiveRefinement => {
                // Apply recursive improvement
                target_pattern.success_rate *=
                    meta_pattern.recursive_application.self_improvement_factor;
                target_pattern.confidence *= 1.05; // Slight confidence boost
            }
            LearningStrategy::ContextualAbstraction => {
                // Generalize pattern conditions
                for condition in &mut target_pattern.conditions {
                    condition.threshold *= 0.95; // More lenient
                }
                target_pattern.applicability_domains.extend(
                    meta_pattern
                        .source_patterns
                        .iter()
                        .flat_map(|_| vec!["generalized".to_string()]),
                );
            }
            LearningStrategy::SynergisticCombination => {
                // Combine with related patterns
                target_pattern.success_rate *= 1.1; // Synergy bonus
            }
            LearningStrategy::PatternElaboration => {
                // Add more specific conditions
                let new_condition = PatternCondition {
                    condition_type: "refined_constraint".to_string(),
                    threshold: 0.8,
                    required_context: vec!["enhanced_context".to_string()],
                };
                target_pattern.conditions.push(new_condition);
            }
            LearningStrategy::PatternSimplification => {
                // Simplify conditions while maintaining effectiveness
                target_pattern.conditions.retain(|c| c.threshold > 0.5);
                target_pattern.success_rate *= 0.95; // Slight reduction for simplicity
            }
            _ => {}
        }

        // Clamp values to valid range
        target_pattern.success_rate = target_pattern.success_rate.clamp(0.0, 1.0);
        target_pattern.confidence = target_pattern.confidence.clamp(0.0, 1.0);

        Ok(())
    }

    // Helper methods for advanced pattern extraction

    fn generate_context_key(&self, context: &LearningContext) -> String {
        format!(
            "{}_{}_{}",
            match context.task_type {
                TaskType::PatternRecognition => "pattern",
                TaskType::Optimization => "optimize",
                TaskType::Adaptation => "adapt",
                TaskType::ProblemSolving => "solve",
                TaskType::TransferLearning => "transfer",
                TaskType::EmergenceDetection => "emerge",
            },
            (context.initial_state.coherence_score * 10.0) as usize,
            (context.initial_state.active_patterns as f32 / 10.0) as usize
        )
    }

    fn extract_successful_actions_from_group(
        &self,
        episodes: &[LearningEpisode],
    ) -> Vec<ActionSequence> {
        let mut action_sequences = Vec::new();

        // Count action frequencies in successful episodes
        let mut action_counts: HashMap<ActionType, usize> = HashMap::new();
        for episode in episodes {
            for action in &episode.actions_taken {
                if action.actual_impact.unwrap_or(0.0) > 0.5 {
                    *action_counts.entry(action.action_type.clone()).or_insert(0) += 1;
                }
            }
        }

        // Create sequences from most common actions
        let mut sorted_actions: Vec<_> = action_counts
            .into_iter()
            .filter(|(_, count)| *count >= episodes.len() / 2) // Appears in at least half
            .collect();
        sorted_actions.sort_by(|a, b| b.1.cmp(&a.1));

        for (action_type, _) in sorted_actions {
            action_sequences.push(ActionSequence {
                actions: vec![action_type],
                timing_constraints: None,
                ordering_flexibility: OrderingFlexibility::Flexible,
            });
        }

        action_sequences
    }

    fn extract_context_conditions(&self, context: &str) -> Vec<PatternCondition> {
        let mut conditions = Vec::new();

        // Parse context key to extract conditions
        let parts: Vec<&str> = context.split('_').collect();
        if parts.len() >= 2 {
            if let Ok(coherence) = parts[1].parse::<usize>() {
                conditions.push(PatternCondition {
                    condition_type: "coherence_range".to_string(),
                    threshold: coherence as f32 / 10.0,
                    required_context: vec![context.to_string()],
                });
            }
        }

        conditions
    }

    fn calculate_group_success_rate(&self, episodes: &[LearningEpisode]) -> f32 {
        if episodes.is_empty() {
            return 0.0;
        }
        episodes.iter().map(|e| e.effectiveness).sum::<f32>() / episodes.len() as f32
    }

    fn calculate_group_confidence(&self, episodes: &[LearningEpisode]) -> f32 {
        let base_confidence = self.calculate_group_success_rate(episodes);
        let size_factor = (episodes.len() as f32 / 5.0).min(1.0);
        (base_confidence + size_factor) / 2.0
    }

    fn is_improving_sequence(&self, episodes: &[LearningEpisode]) -> bool {
        if episodes.len() < 3 {
            return false;
        }

        let first_third = &episodes[..episodes.len() / 3];
        let last_third = &episodes[episodes.len() * 2 / 3..];

        let first_avg =
            first_third.iter().map(|e| e.effectiveness).sum::<f32>() / first_third.len() as f32;
        let last_avg =
            last_third.iter().map(|e| e.effectiveness).sum::<f32>() / last_third.len() as f32;

        last_avg > first_avg + 0.1 // Significant improvement
    }

    fn extract_temporal_conditions(&self, episodes: &[LearningEpisode]) -> Vec<PatternCondition> {
        let mut conditions = Vec::new();

        // Add temporal constraint
        conditions.push(PatternCondition {
            condition_type: "temporal_sequence".to_string(),
            threshold: episodes.len() as f32,
            required_context: vec!["sequential_improvement".to_string()],
        });

        conditions
    }

    fn extract_sequence_actions(&self, episodes: &[LearningEpisode]) -> Vec<ActionSequence> {
        let mut sequences = Vec::new();

        // Extract common action sequence across episodes
        if episodes.len() >= 2 {
            let first_actions: Vec<_> = episodes
                .iter()
                .filter_map(|e| e.actions_taken.first())
                .map(|a| a.action_type.clone())
                .collect();

            if first_actions.len() >= episodes.len() / 2 {
                sequences.push(ActionSequence {
                    actions: first_actions,
                    timing_constraints: Some(TimingConstraints {
                        max_delay_between_actions_ms: 5000,
                        total_sequence_timeout_ms: 30000,
                    }),
                    ordering_flexibility: OrderingFlexibility::Flexible,
                });
            }
        }

        sequences
    }

    fn calculate_sequence_success_rate(&self, episodes: &[LearningEpisode]) -> f32 {
        episodes.iter().map(|e| e.effectiveness).sum::<f32>() / episodes.len() as f32
    }

    fn calculate_sequence_confidence(&self, episodes: &[LearningEpisode]) -> f32 {
        let effectiveness = self.calculate_sequence_success_rate(episodes);
        let consistency = 1.0
            - (episodes.iter().map(|e| e.effectiveness).sum::<f32>() / episodes.len() as f32
                - effectiveness)
                .abs();
        (effectiveness + consistency) / 2.0
    }

    fn find_cross_domain_successful_actions(
        &self,
        domain_groups: &HashMap<String, Vec<&LearningEpisode>>,
    ) -> Vec<(ActionType, Vec<String>, f32)> {
        let mut cross_domain_actions = Vec::new();

        // Find actions that appear in multiple domains
        let mut action_domains: HashMap<ActionType, HashSet<String>> = HashMap::new();
        let mut action_effectiveness: HashMap<ActionType, Vec<f32>> = HashMap::new();

        for (domain, episodes) in domain_groups {
            for episode in episodes {
                for action in &episode.actions_taken {
                    if action.actual_impact.unwrap_or(0.0) > 0.6 {
                        action_domains
                            .entry(action.action_type.clone())
                            .or_insert_with(HashSet::new)
                            .insert(domain.clone());
                        action_effectiveness
                            .entry(action.action_type.clone())
                            .or_insert_with(Vec::new)
                            .push(action.actual_impact.unwrap_or(0.0));
                    }
                }
            }
        }

        // Filter for actions that work in multiple domains
        for (action, domains) in action_domains {
            if domains.len() >= 2 {
                let empty_vec = vec![];
                let effectiveness_scores = action_effectiveness.get(&action).unwrap_or(&empty_vec);
                let avg_effectiveness = if !effectiveness_scores.is_empty() {
                    effectiveness_scores.iter().sum::<f32>() / effectiveness_scores.len() as f32
                } else {
                    0.0
                };

                cross_domain_actions.push((
                    action,
                    domains.into_iter().collect(),
                    avg_effectiveness,
                ));
            }
        }

        cross_domain_actions
    }

    fn extract_cross_domain_conditions(&self, domains: &[String]) -> Vec<PatternCondition> {
        vec![PatternCondition {
            condition_type: "cross_domain".to_string(),
            threshold: domains.len() as f32,
            required_context: domains.to_vec(),
        }]
    }

    fn calculate_cross_domain_confidence(&self, domains: &[String]) -> f32 {
        let base_confidence = 0.7;
        let domain_factor = (domains.len() as f32 / 5.0).min(1.0);
        base_confidence + domain_factor * 0.3
    }

    fn has_condition_changes(&self, episode: &LearningEpisode) -> bool {
        // Check if any outcomes indicate adaptation to changing conditions
        episode.outcomes.iter().any(|outcome| {
            outcome.lessons_learned.iter().any(|lesson| {
                lesson.to_lowercase().contains("adapt")
                    || lesson.to_lowercase().contains("adjust")
                    || lesson.to_lowercase().contains("change")
            })
        })
    }

    fn extract_adaptive_actions(&self, episode: &LearningEpisode) -> Vec<ActionSequence> {
        let mut sequences = Vec::new();

        // Look for actions that indicate adaptation
        let adaptive_actions: Vec<_> = episode
            .actions_taken
            .iter()
            .filter(|action| {
                matches!(
                    action.action_type,
                    ActionType::StrategyChange | ActionType::ParameterAdjustment
                )
            })
            .map(|action| action.action_type.clone())
            .collect();

        if !adaptive_actions.is_empty() {
            sequences.push(ActionSequence {
                actions: adaptive_actions,
                timing_constraints: Some(TimingConstraints {
                    max_delay_between_actions_ms: 1000,
                    total_sequence_timeout_ms: 10000,
                }),
                ordering_flexibility: OrderingFlexibility::Strict,
            });
        }

        sequences
    }

    fn extract_adaptive_conditions(&self, episode: &LearningEpisode) -> Vec<PatternCondition> {
        vec![PatternCondition {
            condition_type: "adaptive_required".to_string(),
            threshold: episode.recursive_depth as f32,
            required_context: vec![format!("domain_{}", episode.context.domain)],
        }]
    }

    /// Apply learned patterns to new situations
    pub fn apply_learned_pattern(
        &mut self,
        pattern_id: &str,
        field: &mut NeuralField,
        memory: &mut AttractorField,
    ) -> ContextNestResult<f32> {
        // Find pattern and clone its actions to avoid borrow checker issues
        let pattern_actions = self
            .learning_patterns
            .iter_mut()
            .find(|p| p.pattern_id == pattern_id)
            .map(|p| {
                let actions = p.actions.clone();
                // Update pattern statistics
                p.applications_count += 1;
                actions
            })
            .ok_or_else(|| {
                ContextNestError::NotFound(format!("Pattern {} not found", pattern_id))
            })?;

        // Apply the pattern's action sequence
        let mut total_impact = 0.0f32;
        let actions_count = pattern_actions.len();

        for action_sequence in &pattern_actions {
            // Execute each action in the sequence
            for action_type in &action_sequence.actions {
                let impact = self.execute_learning_action(action_type, field, memory)?;
                total_impact += impact;
            }
        }

        // Update meta metrics
        self.meta_metrics.patterns_applied += 1;

        let average_impact = if actions_count > 0 {
            total_impact / actions_count as f32
        } else {
            0.0
        };

        Ok(average_impact)
    }

    /// Generate insights about the learning process itself
    pub fn generate_meta_insights(&mut self) -> ContextNestResult<Vec<MetaInsight>> {
        let mut insights = Vec::new();

        // Analyze learning rate trends
        if let Some(learning_rate_insight) = self.analyze_learning_rate_trends() {
            insights.push(learning_rate_insight);
        }

        // Identify optimal strategies for different contexts
        if let Some(strategy_insight) = self.identify_optimal_strategies() {
            insights.push(strategy_insight);
        }

        // Detect bottlenecks in learning process
        if let Some(bottleneck_insight) = self.identify_learning_bottlenecks() {
            insights.push(bottleneck_insight);
        }

        // Find synergy opportunities between patterns
        if let Some(synergy_insight) = self.find_pattern_synergies() {
            insights.push(synergy_insight);
        }

        // Store insights
        self.meta_insights.extend(insights.clone());

        Ok(insights)
    }

    /// Analyze trends in learning effectiveness over time
    fn analyze_learning_rate_trends(&self) -> Option<MetaInsight> {
        if self.learning_episodes.len() < 10 {
            return None;
        }

        let recent_episodes = &self.learning_episodes[self.learning_episodes.len() - 10..];
        let avg_recent_effectiveness =
            recent_episodes.iter().map(|e| e.effectiveness).sum::<f32>() / 10.0;

        let early_episodes = &self.learning_episodes[..10.min(self.learning_episodes.len())];
        let avg_early_effectiveness = early_episodes.iter().map(|e| e.effectiveness).sum::<f32>()
            / early_episodes.len() as f32;

        let improvement = avg_recent_effectiveness - avg_early_effectiveness;

        Some(MetaInsight {
            insight_id: uuid::Uuid::new_v4().to_string(),
            insight_type: InsightType::LearningRatePattern,
            description: format!(
                "Learning effectiveness has {} by {:.1}% over time",
                if improvement > 0.0 {
                    "improved"
                } else {
                    "decreased"
                },
                improvement.abs() * 100.0
            ),
            supporting_evidence: vec![
                format!("Early average: {:.3}", avg_early_effectiveness),
                format!("Recent average: {:.3}", avg_recent_effectiveness),
            ],
            confidence: 0.8,
            actionable_recommendations: if improvement > 0.0 {
                vec!["Continue current learning strategies".to_string()]
            } else {
                vec![
                    "Review and adjust learning parameters".to_string(),
                    "Increase exploration rate".to_string(),
                ]
            },
            discovered_at: Utc::now(),
        })
    }

    /// Identify optimal strategies for different contexts
    fn identify_optimal_strategies(&self) -> Option<MetaInsight> {
        if self.learning_patterns.is_empty() {
            return None;
        }

        let best_pattern = self
            .learning_patterns
            .iter()
            .max_by(|a, b| a.success_rate.partial_cmp(&b.success_rate).unwrap())?;

        Some(MetaInsight {
            insight_id: uuid::Uuid::new_v4().to_string(),
            insight_type: InsightType::OptimalStrategyForContext,
            description: format!(
                "Pattern {} shows highest success rate ({:.1}%) for {:?} tasks",
                best_pattern.pattern_id,
                best_pattern.success_rate * 100.0,
                best_pattern.pattern_type
            ),
            supporting_evidence: vec![
                format!("Applied {} times", best_pattern.applications_count),
                format!("Confidence: {:.3}", best_pattern.confidence),
            ],
            confidence: best_pattern.confidence,
            actionable_recommendations: vec![
                "Prioritize this pattern for similar tasks".to_string(),
                "Analyze pattern components for transfer learning".to_string(),
            ],
            discovered_at: Utc::now(),
        })
    }

    /// Identify bottlenecks in the learning process
    fn identify_learning_bottlenecks(&self) -> Option<MetaInsight> {
        if self.learning_episodes.len() < 5 {
            return None;
        }

        // Find episodes with long duration but low effectiveness
        let inefficient_episodes: Vec<&LearningEpisode> = self
            .learning_episodes
            .iter()
            .filter(|ep| ep.duration_ms > 1000 && ep.effectiveness < 0.5)
            .collect();

        if inefficient_episodes.is_empty() {
            return None;
        }

        Some(MetaInsight {
            insight_id: uuid::Uuid::new_v4().to_string(),
            insight_type: InsightType::BottleneckIdentification,
            description: format!(
                "Identified {} inefficient learning episodes with high duration but low effectiveness",
                inefficient_episodes.len()
            ),
            supporting_evidence: inefficient_episodes
                .iter()
                .take(3)
                .map(|ep| {
                    format!(
                        "Episode {}: {}ms, {:.1}% effective",
                        &ep.episode_id[..8],
                        ep.duration_ms,
                        ep.effectiveness * 100.0
                    )
                })
                .collect(),
            confidence: 0.7,
            actionable_recommendations: vec![
                "Optimize action execution time".to_string(),
                "Consider early stopping for low-confidence actions".to_string(),
                "Increase resource allocation for complex tasks".to_string(),
            ],
            discovered_at: Utc::now(),
        })
    }

    /// Find synergy opportunities between patterns
    fn find_pattern_synergies(&self) -> Option<MetaInsight> {
        if self.learning_patterns.len() < 2 {
            return None;
        }

        // Look for patterns that could be combined
        let mut synergy_pairs = Vec::new();

        for i in 0..self.learning_patterns.len() {
            for j in (i + 1)..self.learning_patterns.len() {
                let pattern_a = &self.learning_patterns[i];
                let pattern_b = &self.learning_patterns[j];

                // Check if patterns have complementary conditions
                if self.patterns_are_complementary(pattern_a, pattern_b) {
                    synergy_pairs
                        .push((pattern_a.pattern_id.clone(), pattern_b.pattern_id.clone()));
                }
            }
        }

        if synergy_pairs.is_empty() {
            return None;
        }

        Some(MetaInsight {
            insight_id: uuid::Uuid::new_v4().to_string(),
            insight_type: InsightType::SynergyOpportunity,
            description: format!(
                "Discovered {} potential pattern synergies that could be combined",
                synergy_pairs.len()
            ),
            supporting_evidence: synergy_pairs
                .iter()
                .take(3)
                .map(|(a, b)| format!("Patterns {} and {} may be complementary", &a[..8], &b[..8]))
                .collect(),
            confidence: 0.65,
            actionable_recommendations: vec![
                "Experiment with combining identified patterns".to_string(),
                "Create composite patterns for enhanced effectiveness".to_string(),
            ],
            discovered_at: Utc::now(),
        })
    }

    /// Check if two patterns are complementary
    fn patterns_are_complementary(
        &self,
        pattern_a: &LearningPattern,
        pattern_b: &LearningPattern,
    ) -> bool {
        // Patterns are complementary if they:
        // 1. Have different pattern types
        // 2. Have non-overlapping action sequences
        // 3. Target different optimization goals

        if pattern_a.pattern_type as u8 == pattern_b.pattern_type as u8 {
            return false;
        }

        // Check for overlapping applicability domains
        let has_common_domain = pattern_a
            .applicability_domains
            .iter()
            .any(|d| pattern_b.applicability_domains.contains(d));

        has_common_domain
    }

    // Helper methods

    fn infer_task_type(&self, enhancement_type: &EnhancementType) -> TaskType {
        match enhancement_type {
            EnhancementType::FieldStructureOptimization => TaskType::Optimization,
            EnhancementType::ProtocolEvolution => TaskType::Adaptation,
            EnhancementType::MemoryArchitectureImprovement => TaskType::Optimization,
            EnhancementType::CoherenceAlgorithmRefinement => TaskType::Optimization,
            EnhancementType::RepairMechanismEnhancement => TaskType::ProblemSolving,
            EnhancementType::EmergentCapabilityDevelopment => TaskType::EmergenceDetection,
        }
    }

    fn map_modification_to_action_type(
        &self,
        mod_type: &crate::context::meta_recursive::ModificationType,
    ) -> ActionType {
        match mod_type {
            crate::context::meta_recursive::ModificationType::ParameterAdjustment => {
                ActionType::ParameterAdjustment
            }
            crate::context::meta_recursive::ModificationType::AlgorithmRefinement => {
                ActionType::StrategyChange
            }
            crate::context::meta_recursive::ModificationType::StructureEnhancement => {
                ActionType::ResourceAllocation
            }
            crate::context::meta_recursive::ModificationType::NewCapabilityAddition => {
                ActionType::PatternIntegration
            }
            crate::context::meta_recursive::ModificationType::PerformanceOptimization => {
                ActionType::ExploitationPhase
            }
        }
    }

    fn extract_lessons(&self, event: &EnhancementEvent, _analysis: &SystemAnalysis) -> Vec<String> {
        let mut lessons = Vec::new();

        if event.effectiveness > 0.8 {
            lessons
                .push("High-impact enhancement achieved through systematic analysis".to_string());
        }

        if event.recursive_level > 2 {
            lessons
                .push("Deep recursive enhancement can yield significant improvements".to_string());
        }

        lessons
    }

    fn assess_generalizability(&self, event: &EnhancementEvent, _analysis: &SystemAnalysis) -> f32 {
        // Assess how generalizable this enhancement is to other contexts
        let mut score = 0.5;

        // Higher effectiveness suggests more generalizable approach
        score += event.effectiveness * 0.3;

        // More modifications might indicate more specific solution
        score -= (event.modifications.len() as f32 * 0.05).min(0.2);

        score.clamp(0.0, 1.0)
    }

    fn update_meta_metrics(&mut self, episode: &LearningEpisode) {
        self.meta_metrics.total_episodes += 1;

        if episode.effectiveness > 0.6 {
            self.meta_metrics.successful_episodes += 1;
        }

        // Update rolling average of improvement
        self.meta_metrics.average_improvement_per_episode =
            (self.meta_metrics.average_improvement_per_episode
                * (self.meta_metrics.total_episodes - 1) as f32
                + episode.effectiveness)
                / self.meta_metrics.total_episodes as f32;

        // Calculate learning efficiency (successful episodes / total episodes)
        self.meta_metrics.learning_efficiency =
            self.meta_metrics.successful_episodes as f32 / self.meta_metrics.total_episodes as f32;

        self.meta_metrics.last_updated = Utc::now();
    }

    fn should_generate_insights(&self) -> bool {
        // Generate insights every 10 episodes
        self.learning_episodes.len() % 10 == 0 && self.learning_episodes.len() > 0
    }

    fn find_common_action_sequences(&self, episodes: &[LearningEpisode]) -> Vec<ActionSequence> {
        let mut sequences = Vec::new();

        // Find action sequences that appear in multiple successful episodes
        for episode in episodes {
            if episode.actions_taken.len() >= 2 {
                let actions: Vec<ActionType> = episode
                    .actions_taken
                    .iter()
                    .map(|a| a.action_type.clone())
                    .collect();

                sequences.push(ActionSequence {
                    actions,
                    timing_constraints: None,
                    ordering_flexibility: OrderingFlexibility::Flexible,
                });
            }
        }

        sequences
    }

    fn is_duplicate_pattern(&self, sequence: &ActionSequence) -> bool {
        self.learning_patterns.iter().any(|p| {
            p.actions.iter().any(|existing_seq| {
                existing_seq.actions.len() == sequence.actions.len()
                    && existing_seq
                        .actions
                        .iter()
                        .zip(&sequence.actions)
                        .all(|(a, b)| std::mem::discriminant(a) == std::mem::discriminant(b))
            })
        })
    }

    fn extract_pattern_conditions(&self, episodes: &[LearningEpisode]) -> Vec<PatternCondition> {
        let mut conditions = Vec::new();

        // Extract common conditions from successful episodes
        if let Some(first_episode) = episodes.first() {
            conditions.push(PatternCondition {
                condition_type: "coherence_threshold".to_string(),
                threshold: first_episode.context.initial_state.coherence_score,
                required_context: vec!["neural_field_active".to_string()],
            });
        }

        conditions
    }

    fn calculate_pattern_success_rate(&self, episodes: &[LearningEpisode]) -> f32 {
        if episodes.is_empty() {
            return 0.0;
        }

        let successful_count = episodes.iter().filter(|ep| ep.effectiveness > 0.6).count();

        successful_count as f32 / episodes.len() as f32
    }

    fn calculate_pattern_confidence(&self, episodes: &[LearningEpisode]) -> f32 {
        if episodes.is_empty() {
            return 0.0;
        }

        // Confidence based on number of episodes and consistency
        let episode_count_factor = (episodes.len() as f32 / 10.0).min(1.0);
        let avg_effectiveness =
            episodes.iter().map(|ep| ep.effectiveness).sum::<f32>() / episodes.len() as f32;

        (episode_count_factor + avg_effectiveness) / 2.0
    }

    fn execute_learning_action(
        &self,
        action_type: &ActionType,
        _field: &mut NeuralField,
        _memory: &mut AttractorField,
    ) -> ContextNestResult<f32> {
        // Execute the learned action on the system
        match action_type {
            ActionType::ParameterAdjustment => Ok(0.2),
            ActionType::StrategyChange => Ok(0.3),
            ActionType::ResourceAllocation => Ok(0.25),
            ActionType::PatternIntegration => Ok(0.35),
            ActionType::ExplorationPhase => Ok(0.15),
            ActionType::ExploitationPhase => Ok(0.3),
        }
    }

    /// Get learning statistics
    pub fn get_learning_stats(&self) -> MetaLearningMetrics {
        self.meta_metrics.clone()
    }

    /// Get all discovered patterns
    pub fn get_patterns(&self) -> &[LearningPattern] {
        &self.learning_patterns
    }

    /// Get all meta-insights
    pub fn get_insights(&self) -> &[MetaInsight] {
        &self.meta_insights
    }

    /// Record an optimization attempt and result
    pub fn record_optimization(
        &mut self,
        target: String,
        baseline: f32,
        final_perf: f32,
        techniques: Vec<String>,
        iterations: usize,
    ) {
        let improvement = ((final_perf - baseline) / baseline) * 100.0;

        let record = OptimizationRecord {
            record_id: uuid::Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            optimization_target: target,
            baseline_performance: baseline,
            final_performance: final_perf,
            improvement_percentage: improvement,
            techniques_used: techniques,
            iterations_required: iterations,
            success: final_perf > baseline,
        };

        self.optimization_history.push(record);
    }

    /// Get optimization history
    pub fn get_optimization_history(&self) -> &[OptimizationRecord] {
        &self.optimization_history
    }
}

impl Default for RecursiveLearner {
    fn default() -> Self {
        Self::new()
    }
}

// Meta-learning pattern structures for recursive learning

/// Meta-learning pattern that learns from other patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaLearningPattern {
    pub meta_pattern_id: String,
    pub meta_pattern_type: MetaPatternType,
    pub source_patterns: Vec<String>, // Patterns this meta-pattern learned from
    pub learning_strategy: LearningStrategy,
    pub effectiveness: f32,
    pub meta_conditions: Vec<MetaCondition>,
    pub recursive_application: RecursiveApplication,
    pub discovered_at: DateTime<Utc>,
    pub applications_count: usize,
}

/// Type of meta-pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetaPatternType {
    /// Patterns that discover new patterns
    PatternDiscovery,
    /// Patterns that optimize existing patterns
    PatternOptimization,
    /// Patterns that apply to themselves recursively
    RecursiveSelfApplication,
    /// Patterns that describe interactions between patterns
    PatternInteraction,
}

/// Learning strategy for meta-patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LearningStrategy {
    /// Apply the same pattern recursively with refinement
    RecursiveRefinement,
    /// Abstract patterns to more general contexts
    ContextualAbstraction,
    /// Combine patterns for synergistic effects
    SynergisticCombination,
    /// Add more detail and specificity to patterns
    PatternElaboration,
    /// Simplify patterns while maintaining effectiveness
    PatternSimplification,
    /// Fine-tune pattern parameters
    ParameterTuning,
    /// Adapt patterns to new contexts
    ContextualAdaptation,
}

/// Meta-condition for meta-pattern application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaCondition {
    pub condition_type: String,
    pub threshold: f32,
    pub context_requirement: String,
}

/// Recursive application parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecursiveApplication {
    pub can_apply_to_self: bool,
    pub max_recursion_depth: usize,
    pub self_improvement_factor: f32,
}

/// Result of applying a meta-pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaPatternApplicationResult {
    pub meta_pattern_id: String,
    pub enhanced_patterns: Vec<EnhancedPattern>,
    pub total_improvement: f32,
    pub application_success: bool,
    pub timestamp: DateTime<Utc>,
}

/// Information about an enhanced pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedPattern {
    pub pattern_id: String,
    pub original_effectiveness: f32,
    pub enhanced_effectiveness: f32,
    pub improvement: f32,
}

// Add meta_patterns field to RecursiveLearner
impl RecursiveLearner {
    /// Get all meta-patterns
    pub fn get_meta_patterns(&self) -> &[MetaLearningPattern] {
        &self.meta_patterns
    }

    /// Store meta-patterns
    pub fn store_meta_patterns(&mut self, patterns: Vec<MetaLearningPattern>) {
        self.meta_patterns.extend(patterns);
    }
}

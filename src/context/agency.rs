/// Agency Activation implementation for Recursive Emergence Protocol
/// This module implements autonomous agency levels that enable the system
/// to assess itself, set goals, select actions, and learn from outcomes.
use crate::context::field::{Attractor, FieldState, NeuralField};
use crate::error::ContextNestResult;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Autonomy level for the agency (0.0 = fully supervised, 1.0 = fully autonomous)
pub type AutonomyLevel = f32;

/// Represents an autonomous action the system can take
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub name: String,
    pub description: String,
    pub action_type: ActionType,
    pub required_autonomy: AutonomyLevel,
    pub estimated_impact: f32,
    pub risk_level: f32,
    pub preconditions: Vec<String>,
}

/// Types of actions the system can autonomously perform
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ActionType {
    /// Modify field properties
    TuneField { property: String, delta: f32 },
    /// Add new patterns
    InjectPattern { content: String },
    /// Create new attractors
    CreateAttractor { strength: f32 },
    /// Prune weak patterns
    PrunePatterns { threshold: f32 },
    /// Amplify resonant patterns
    AmplifyResonance,
    /// Request external input
    RequestInput { query: String },
    /// Consolidate similar patterns
    ConsolidatePatterns,
    /// Split complex patterns
    SplitPattern { pattern_id: String },
}

/// Goal that the agency is pursuing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub name: String,
    pub description: String,
    pub target_metric: MetricTarget,
    pub priority: f32,
    pub deadline: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub progress: f32, // 0.0 to 1.0
    pub status: GoalStatus,
}

/// Target metric for a goal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricTarget {
    /// Target field coherence level
    Coherence { target: f32 },
    /// Target field stability
    Stability { target: f32 },
    /// Target field health
    Health { target: f32 },
    /// Target number of patterns
    PatternCount { target: usize },
    /// Target energy level
    Energy { target: f32 },
    /// Custom metric
    Custom { name: String, target: f32 },
}

/// Status of a goal
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GoalStatus {
    Active,
    Paused,
    Achieved,
    Abandoned,
}

/// Self-assessment result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfAssessment {
    pub timestamp: DateTime<Utc>,
    pub field_health: f32,
    pub coherence_score: f32,
    pub stability_score: f32,
    pub energy_level: f32,
    pub pattern_quality: f32,
    pub overall_capability: f32,
    pub identified_issues: Vec<Issue>,
    pub strengths: Vec<String>,
    pub improvement_areas: Vec<String>,
}

/// Issue identified during self-assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub severity: Severity,
    pub description: String,
    pub affected_component: String,
    pub suggested_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, PartialOrd)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

/// Learning record from action outcomes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningRecord {
    pub timestamp: DateTime<Utc>,
    pub action_id: String,
    pub context: ActionContext,
    pub outcome: ActionOutcome,
    pub field_state_before: FieldStateSnapshot,
    pub field_state_after: FieldStateSnapshot,
    pub lesson_learned: String,
    pub confidence: f32,
}

/// Context in which an action was taken
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionContext {
    pub goal_id: Option<String>,
    pub trigger: String,
    pub field_coherence: f32,
    pub field_stability: f32,
    pub pattern_count: usize,
}

/// Outcome of an action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionOutcome {
    pub success: bool,
    pub impact_score: f32,
    pub unexpected_effects: Vec<String>,
    pub goal_progress_delta: f32,
}

/// Snapshot of field state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldStateSnapshot {
    pub coherence: f32,
    pub stability: f32,
    pub energy: f32,
    pub health: f32,
    pub strength: f32,
    pub pattern_count: usize,
    pub attractor_count: usize,
}

impl From<&NeuralField> for FieldStateSnapshot {
    fn from(field: &NeuralField) -> Self {
        Self {
            coherence: field.state.coherence,
            stability: field.state.stability,
            energy: field.state.energy,
            health: field.state.health,
            strength: field.state.strength,
            pattern_count: field.patterns.len(),
            attractor_count: field.attractors.len(),
        }
    }
}

/// Configuration for agency activation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgencyConfig {
    /// Current autonomy level (0.0-1.0)
    pub autonomy_level: AutonomyLevel,
    /// Enable self-assessment
    pub enable_self_assessment: bool,
    /// Enable autonomous goal-setting
    pub enable_goal_setting: bool,
    /// Enable autonomous action selection
    pub enable_action_selection: bool,
    /// Enable learning from outcomes
    pub enable_learning: bool,
    /// Minimum confidence for autonomous actions
    pub min_action_confidence: f32,
    /// Maximum risk level for autonomous actions
    pub max_risk_level: f32,
}

impl Default for AgencyConfig {
    fn default() -> Self {
        Self {
            autonomy_level: 0.3, // Conservative default
            enable_self_assessment: true,
            enable_goal_setting: false,
            enable_action_selection: false,
            enable_learning: true,
            min_action_confidence: 0.7,
            max_risk_level: 0.5,
        }
    }
}

/// Metrics for agency performance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgencyMetrics {
    pub assessments_performed: usize,
    pub goals_created: usize,
    pub goals_achieved: usize,
    pub actions_taken: usize,
    pub successful_actions: usize,
    pub failed_actions: usize,
    pub avg_action_impact: f32,
    pub learning_records_created: usize,
    pub avg_confidence: f32,
}

impl Default for AgencyMetrics {
    fn default() -> Self {
        Self {
            assessments_performed: 0,
            goals_created: 0,
            goals_achieved: 0,
            actions_taken: 0,
            successful_actions: 0,
            failed_actions: 0,
            avg_action_impact: 0.0,
            learning_records_created: 0,
            avg_confidence: 0.0,
        }
    }
}

/// Agency activator that manages autonomous operation
pub struct AgencyActivator {
    /// Configuration
    config: AgencyConfig,
    /// Current goals
    goals: Vec<Goal>,
    /// Available actions
    actions: Vec<Action>,
    /// Learning history
    learning_history: Vec<LearningRecord>,
    /// Assessment history
    assessment_history: Vec<SelfAssessment>,
    /// Metrics
    metrics: AgencyMetrics,
}

impl AgencyActivator {
    /// Create a new agency activator
    pub fn new(config: AgencyConfig) -> Self {
        let mut activator = Self {
            config,
            goals: Vec::new(),
            actions: Vec::new(),
            learning_history: Vec::new(),
            assessment_history: Vec::new(),
            metrics: AgencyMetrics::default(),
        };

        // Initialize default actions
        activator.initialize_default_actions();

        activator
    }

    /// Initialize default action library
    fn initialize_default_actions(&mut self) {
        self.actions = vec![
            Action {
                id: "amplify_resonance".to_string(),
                name: "Amplify Resonance".to_string(),
                description: "Amplify patterns with high resonance".to_string(),
                action_type: ActionType::AmplifyResonance,
                required_autonomy: 0.3,
                estimated_impact: 0.6,
                risk_level: 0.2,
                preconditions: vec!["coherence > 0.5".to_string()],
            },
            Action {
                id: "prune_weak".to_string(),
                name: "Prune Weak Patterns".to_string(),
                description: "Remove patterns below strength threshold".to_string(),
                action_type: ActionType::PrunePatterns { threshold: 0.3 },
                required_autonomy: 0.5,
                estimated_impact: 0.7,
                risk_level: 0.4,
                preconditions: vec!["pattern_count > 10".to_string()],
            },
            Action {
                id: "consolidate".to_string(),
                name: "Consolidate Patterns".to_string(),
                description: "Merge similar patterns to reduce redundancy".to_string(),
                action_type: ActionType::ConsolidatePatterns,
                required_autonomy: 0.6,
                estimated_impact: 0.8,
                risk_level: 0.5,
                preconditions: vec!["pattern_count > 20".to_string()],
            },
            Action {
                id: "tune_decay".to_string(),
                name: "Tune Decay Rate".to_string(),
                description: "Adjust field decay constant".to_string(),
                action_type: ActionType::TuneField {
                    property: "decay_constant".to_string(),
                    delta: -0.02,
                },
                required_autonomy: 0.4,
                estimated_impact: 0.5,
                risk_level: 0.3,
                preconditions: vec!["stability > 0.6".to_string()],
            },
        ];
    }

    /// Perform self-assessment on the field
    pub fn assess_self(&mut self, field: &NeuralField) -> ContextNestResult<SelfAssessment> {
        let timestamp = Utc::now();

        // Calculate pattern quality
        let pattern_quality = if !field.patterns.is_empty() {
            field
                .patterns
                .iter()
                .map(|p| p.strength * (1.0 + p.resonance))
                .sum::<f32>()
                / field.patterns.len() as f32
        } else {
            0.0
        };

        // Calculate overall capability
        let overall_capability = field.state.coherence * 0.3
            + field.state.stability * 0.2
            + field.state.health * 0.3
            + pattern_quality * 0.2;

        // Identify issues
        let mut issues = Vec::new();
        let mut strengths = Vec::new();
        let mut improvement_areas = Vec::new();

        if field.state.coherence < 0.6 {
            issues.push(Issue {
                severity: Severity::Medium,
                description: "Field coherence below optimal level".to_string(),
                affected_component: "coherence".to_string(),
                suggested_actions: vec!["amplify_resonance".to_string()],
            });
            improvement_areas.push("Improve semantic coherence".to_string());
        } else {
            strengths.push("Strong field coherence".to_string());
        }

        if field.state.health < 0.7 {
            issues.push(Issue {
                severity: Severity::High,
                description: "Field health needs attention".to_string(),
                affected_component: "health".to_string(),
                suggested_actions: vec!["prune_weak".to_string()],
            });
            improvement_areas.push("Restore field health".to_string());
        } else {
            strengths.push("Good field health".to_string());
        }

        if field.patterns.len() > 50 {
            issues.push(Issue {
                severity: Severity::Low,
                description: "Pattern count may benefit from consolidation".to_string(),
                affected_component: "patterns".to_string(),
                suggested_actions: vec!["consolidate".to_string()],
            });
            improvement_areas.push("Optimize pattern density".to_string());
        }

        if field.state.stability > 0.8 {
            strengths.push("Excellent field stability".to_string());
        }

        let assessment = SelfAssessment {
            timestamp,
            field_health: field.state.health,
            coherence_score: field.state.coherence,
            stability_score: field.state.stability,
            energy_level: field.state.energy,
            pattern_quality,
            overall_capability,
            identified_issues: issues,
            strengths,
            improvement_areas,
        };

        self.assessment_history.push(assessment.clone());
        self.metrics.assessments_performed += 1;

        Ok(assessment)
    }

    /// Set a new goal based on assessment
    pub fn set_goal(&mut self, assessment: &SelfAssessment) -> ContextNestResult<Option<Goal>> {
        if !self.config.enable_goal_setting {
            return Ok(None);
        }

        // Determine highest priority improvement area
        let goal = if assessment.coherence_score < 0.6 {
            Some(Goal {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Improve Field Coherence".to_string(),
                description: "Increase field coherence to optimal levels".to_string(),
                target_metric: MetricTarget::Coherence { target: 0.75 },
                priority: 0.8,
                deadline: Some(Utc::now() + chrono::Duration::hours(1)),
                created_at: Utc::now(),
                progress: 0.0,
                status: GoalStatus::Active,
            })
        } else if assessment.field_health < 0.7 {
            Some(Goal {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Restore Field Health".to_string(),
                description: "Improve field health through pattern optimization".to_string(),
                target_metric: MetricTarget::Health { target: 0.85 },
                priority: 0.9,
                deadline: Some(Utc::now() + chrono::Duration::minutes(30)),
                created_at: Utc::now(),
                progress: 0.0,
                status: GoalStatus::Active,
            })
        } else if assessment.overall_capability < 0.7 {
            Some(Goal {
                id: uuid::Uuid::new_v4().to_string(),
                name: "Enhance Overall Capability".to_string(),
                description: "Improve multiple field metrics for better performance".to_string(),
                target_metric: MetricTarget::Custom {
                    name: "overall_capability".to_string(),
                    target: 0.8,
                },
                priority: 0.7,
                deadline: Some(Utc::now() + chrono::Duration::hours(2)),
                created_at: Utc::now(),
                progress: 0.0,
                status: GoalStatus::Active,
            })
        } else {
            None
        };

        if let Some(ref g) = goal {
            self.goals.push(g.clone());
            self.metrics.goals_created += 1;
        }

        Ok(goal)
    }

    /// Select best action for current context
    pub fn select_action(
        &self,
        field: &NeuralField,
        goal: Option<&Goal>,
    ) -> ContextNestResult<Option<Action>> {
        if !self.config.enable_action_selection {
            return Ok(None);
        }

        // Filter actions by autonomy level and risk
        let candidate_actions: Vec<_> = self
            .actions
            .iter()
            .filter(|a| {
                a.required_autonomy <= self.config.autonomy_level
                    && a.risk_level <= self.config.max_risk_level
            })
            .collect();

        if candidate_actions.is_empty() {
            return Ok(None);
        }

        // Score actions based on context
        let mut scored_actions: Vec<(f32, &Action)> = candidate_actions
            .iter()
            .map(|action| {
                let score = self.score_action(action, field, goal);
                (score, *action)
            })
            .collect();

        // Sort by score descending
        scored_actions.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

        // Return highest scoring action if it meets confidence threshold
        if let Some((score, action)) = scored_actions.first() {
            if *score >= self.config.min_action_confidence {
                return Ok(Some((*action).clone()));
            }
        }

        Ok(None)
    }

    /// Score an action for the current context
    fn score_action(&self, action: &Action, field: &NeuralField, goal: Option<&Goal>) -> f32 {
        let mut score = action.estimated_impact;

        // Bonus for addressing current issues
        if field.state.coherence < 0.6 && matches!(action.action_type, ActionType::AmplifyResonance)
        {
            score += 0.2;
        }

        if field.state.health < 0.7
            && matches!(action.action_type, ActionType::PrunePatterns { .. })
        {
            score += 0.3;
        }

        // Bonus for alignment with active goal
        if let Some(g) = goal {
            match (&g.target_metric, &action.action_type) {
                (MetricTarget::Coherence { .. }, ActionType::AmplifyResonance) => score += 0.3,
                (MetricTarget::Health { .. }, ActionType::PrunePatterns { .. }) => score += 0.3,
                _ => {}
            }
        }

        // Penalty for high risk
        score -= action.risk_level * 0.2;

        // Bonus from learning history
        let historical_success = self.get_historical_success_rate(&action.id);
        score += historical_success * 0.15;

        score.max(0.0).min(1.0)
    }

    /// Get historical success rate for an action
    fn get_historical_success_rate(&self, action_id: &str) -> f32 {
        let records: Vec<_> = self
            .learning_history
            .iter()
            .filter(|r| r.action_id == action_id)
            .collect();

        if records.is_empty() {
            return 0.5; // Neutral prior
        }

        let successful = records.iter().filter(|r| r.outcome.success).count();

        successful as f32 / records.len() as f32
    }

    /// Execute an action on the field
    pub fn execute_action(
        &mut self,
        action: Action,
        field: &mut NeuralField,
    ) -> ContextNestResult<ActionOutcome> {
        let state_before = FieldStateSnapshot::from(&*field);

        let mut outcome = ActionOutcome {
            success: false,
            impact_score: 0.0,
            unexpected_effects: Vec::new(),
            goal_progress_delta: 0.0,
        };

        // Execute based on action type
        match action.action_type {
            ActionType::AmplifyResonance => {
                field.amplify_resonant()?;
                outcome.success = true;
                outcome.impact_score = 0.6;
            }
            ActionType::PrunePatterns { threshold } => {
                let before_count = field.patterns.len();
                field.patterns.retain(|p| p.strength >= threshold);
                let pruned = before_count - field.patterns.len();

                outcome.success = true;
                outcome.impact_score = (pruned as f32 / before_count as f32).min(1.0);

                if pruned > before_count / 2 {
                    outcome
                        .unexpected_effects
                        .push("Pruned more than 50% of patterns".to_string());
                }
            }
            ActionType::TuneField {
                ref property,
                delta,
            } => {
                field.tune(
                    property,
                    (field.properties.decay_constant + delta as f32) as f64,
                )?;
                outcome.success = true;
                outcome.impact_score = 0.4;
            }
            _ => {
                outcome.success = false;
                outcome
                    .unexpected_effects
                    .push("Action type not yet implemented".to_string());
            }
        }

        let state_after = FieldStateSnapshot::from(&*field);

        // Calculate impact
        let coherence_delta = (state_after.coherence - state_before.coherence).abs();
        let health_delta = (state_after.health - state_before.health).abs();
        outcome.impact_score = (outcome.impact_score + coherence_delta + health_delta) / 3.0;

        // Update metrics
        self.metrics.actions_taken += 1;
        if outcome.success {
            self.metrics.successful_actions += 1;
        } else {
            self.metrics.failed_actions += 1;
        }

        // Update running average
        let total = self.metrics.actions_taken as f32;
        self.metrics.avg_action_impact =
            (self.metrics.avg_action_impact * (total - 1.0) + outcome.impact_score) / total;

        Ok(outcome)
    }

    /// Learn from action outcome
    pub fn learn_from_outcome(
        &mut self,
        action: &Action,
        context: ActionContext,
        outcome: ActionOutcome,
        field_before: FieldStateSnapshot,
        field_after: FieldStateSnapshot,
    ) -> ContextNestResult<()> {
        if !self.config.enable_learning {
            return Ok(());
        }

        // Generate lesson learned
        let lesson = if outcome.success {
            if outcome.impact_score > 0.7 {
                format!(
                    "Action '{}' highly effective in context with coherence={:.2}, stability={:.2}",
                    action.name, context.field_coherence, context.field_stability
                )
            } else {
                format!(
                    "Action '{}' moderately effective, impact={:.2}",
                    action.name, outcome.impact_score
                )
            }
        } else {
            format!(
                "Action '{}' failed. Unexpected effects: {:?}",
                action.name, outcome.unexpected_effects
            )
        };

        // Calculate confidence based on consistency with past results
        let historical_rate = self.get_historical_success_rate(&action.id);
        let current_success = if outcome.success { 1.0 } else { 0.0 };
        let confidence = 1.0 - (historical_rate - current_success).abs();

        let record = LearningRecord {
            timestamp: Utc::now(),
            action_id: action.id.clone(),
            context,
            outcome,
            field_state_before: field_before,
            field_state_after: field_after,
            lesson_learned: lesson,
            confidence,
        };

        self.learning_history.push(record);
        self.metrics.learning_records_created += 1;

        // Update average confidence
        let total = self.metrics.learning_records_created as f32;
        self.metrics.avg_confidence =
            (self.metrics.avg_confidence * (total - 1.0) + confidence) / total;

        Ok(())
    }

    /// Update goal progress based on current field state
    pub fn update_goal_progress(
        &mut self,
        goal_id: &str,
        field: &NeuralField,
    ) -> ContextNestResult<()> {
        if let Some(goal) = self.goals.iter_mut().find(|g| g.id == goal_id) {
            let current_value = match &goal.target_metric {
                MetricTarget::Coherence { .. } => field.state.coherence,
                MetricTarget::Stability { .. } => field.state.stability,
                MetricTarget::Health { .. } => field.state.health,
                MetricTarget::Energy { .. } => field.state.energy,
                MetricTarget::PatternCount { target } => {
                    field.patterns.len() as f32 / *target as f32
                }
                MetricTarget::Custom { .. } => 0.5, // Would need custom calculation
            };

            let target_value = match &goal.target_metric {
                MetricTarget::Coherence { target } => *target,
                MetricTarget::Stability { target } => *target,
                MetricTarget::Health { target } => *target,
                MetricTarget::Energy { target } => *target,
                MetricTarget::PatternCount { .. } => 1.0,
                MetricTarget::Custom { target, .. } => *target,
            };

            goal.progress = (current_value / target_value).min(1.0);

            if goal.progress >= 1.0 {
                goal.status = GoalStatus::Achieved;
                self.metrics.goals_achieved += 1;
            }
        }

        Ok(())
    }

    /// Create agency attractor in the field
    pub fn create_agency_attractor(
        &self,
        field: &mut NeuralField,
        goal: &Goal,
    ) -> ContextNestResult<()> {
        let attractor = Attractor {
            id: format!("agency_goal_{}", goal.id),
            center: self.calculate_goal_center(goal, field),
            strength: goal.priority,
            radius: 0.4,
        };

        field.attractors.push(attractor);
        Ok(())
    }

    /// Calculate attractor center for a goal
    fn calculate_goal_center(&self, _goal: &Goal, field: &NeuralField) -> Vec<f32> {
        // Use field's current centroid as basis
        if field.patterns.is_empty() {
            return vec![0.0; field.properties.embedding_dim];
        }

        let dim = field.patterns[0].embedding.len();
        let mut center = vec![0.0; dim];

        for pattern in &field.patterns {
            for (i, val) in pattern.embedding.iter().enumerate() {
                center[i] += val;
            }
        }

        let count = field.patterns.len() as f32;
        for val in &mut center {
            *val /= count;
        }

        center
    }

    /// Get current autonomy level
    pub fn get_autonomy_level(&self) -> AutonomyLevel {
        self.config.autonomy_level
    }

    /// Set autonomy level
    pub fn set_autonomy_level(&mut self, level: AutonomyLevel) {
        self.config.autonomy_level = level.max(0.0).min(1.0);
    }

    /// Get agency metrics
    pub fn get_metrics(&self) -> &AgencyMetrics {
        &self.metrics
    }

    /// Get active goals
    pub fn get_active_goals(&self) -> Vec<&Goal> {
        self.goals
            .iter()
            .filter(|g| g.status == GoalStatus::Active)
            .collect()
    }

    /// Get learning history
    pub fn get_learning_history(&self) -> &[LearningRecord] {
        &self.learning_history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agency_creation() {
        let config = AgencyConfig::default();
        let activator = AgencyActivator::new(config);
        assert!(activator.actions.len() > 0);
    }

    #[test]
    fn test_self_assessment() {
        let config = AgencyConfig::default();
        let mut activator = AgencyActivator::new(config);
        let field = NeuralField::new();

        let result = activator.assess_self(&field);
        assert!(result.is_ok());

        let assessment = result.unwrap();
        assert!(assessment.overall_capability >= 0.0);
        assert!(assessment.overall_capability <= 1.0);
    }

    #[test]
    fn test_autonomy_level() {
        let config = AgencyConfig::default();
        let mut activator = AgencyActivator::new(config);

        activator.set_autonomy_level(0.7);
        assert_eq!(activator.get_autonomy_level(), 0.7);

        // Test bounds
        activator.set_autonomy_level(1.5);
        assert_eq!(activator.get_autonomy_level(), 1.0);

        activator.set_autonomy_level(-0.5);
        assert_eq!(activator.get_autonomy_level(), 0.0);
    }
}

// Co-Emergence Self-Prompting Extensions
impl AgencyActivator {
    /// Generate self-prompt for cycle intervals
    /// Autonomously prompts the system to review field state at regular intervals
    pub fn generate_cycle_prompt(&self, elapsed_seconds: u64) -> String {
        format!(
            "CYCLE CHECK ({}s elapsed): Evaluate current field state. \
             Coherence: Should I optimize? Stability: Should I stabilize? \
             Patterns: Should I consolidate or prune? \
             Attractors: Should I trigger co-emergence?",
            elapsed_seconds
        )
    }

    /// Generate self-prompt for emergent pattern detection
    /// Prompts when the system detects potential co-emergence opportunities
    pub fn generate_emergence_prompt(
        &self,
        source_attractor: &str,
        target_attractor: &str,
        emergence_type: &str,
        potential_strength: f32,
    ) -> String {
        format!(
            "EMERGENCE OPPORTUNITY DETECTED: {:?} co-emergence between '{}' and '{}' \
             with potential strength {:.2}. \
             Question: Should I execute this co-emergence? \
             Consider: Current field coherence, attractor health, risk level. \
             Decision rationale:",
            emergence_type, source_attractor, target_attractor, potential_strength
        )
    }

    /// Generate self-prompt for coherence threshold violations
    /// Prompts when field coherence drops below acceptable levels
    pub fn generate_coherence_prompt(&self, current_coherence: f32, threshold: f32) -> String {
        format!(
            "COHERENCE ALERT: Field coherence ({:.2}) below threshold ({:.2}). \
             Gap: {:.2}. \
             Analysis required: What caused the drop? Which attractors are misaligned? \
             Should I: (a) Amplify resonance? (b) Prune weak patterns? (c) Create new connections? \
             Recommended action:",
            current_coherence,
            threshold,
            threshold - current_coherence
        )
    }

    /// Generate self-prompt based on field audit results
    /// Prompts for action when field audit reveals interesting patterns
    pub fn generate_audit_prompt(
        &self,
        new_basins_count: usize,
        emergence_indicators: usize,
        field_coherence: f32,
    ) -> String {
        format!(
            "FIELD AUDIT COMPLETE: Detected {} potential new attractor basins, \
             {} emergence indicators. Current coherence: {:.2}. \
             Strategic questions: \
             1. Should I create formal attractors for high-confidence candidates? \
             2. Which emergence opportunities should I prioritize? \
             3. Is the field stable enough for new integrations? \
             Action plan:",
            new_basins_count, emergence_indicators, field_coherence
        )
    }

    /// Generate recursive self-prompt based on previous prompt outcomes
    /// Enables meta-reasoning about the effectiveness of prior decisions
    pub fn generate_recursive_prompt(
        &self,
        previous_action: &str,
        outcome_success: bool,
        impact_score: f32,
    ) -> String {
        if outcome_success {
            format!(
                "RECURSIVE REFLECTION: Previous action '{}' succeeded with impact {:.2}. \
                 Meta-analysis: Why was this effective? Can I generalize this pattern? \
                 Should I increase autonomy for similar actions? \
                 Learning insight:",
                previous_action, impact_score
            )
        } else {
            format!(
                "RECURSIVE REFLECTION: Previous action '{}' failed (impact: {:.2}). \
                 Root cause analysis: What went wrong? Was it the action choice, timing, or context? \
                 How should I update my decision model? \
                 Corrective strategy:",
                previous_action, impact_score
            )
        }
    }

    /// Generate self-prompt for attractor basin reshaping
    /// Prompts when basin reshaping is needed after co-emergence
    pub fn generate_reshaping_prompt(
        &self,
        attractor_id: &str,
        affected_dimensions: usize,
        reshape_magnitude: f32,
    ) -> String {
        format!(
            "BASIN RESHAPING REQUIRED: Attractor '{}' needs reshaping in {} dimensions \
             with magnitude {:.2}. \
             Considerations: Will this disrupt existing patterns? \
             Should I proceed gradually or immediately? \
             What are the risks to field stability? \
             Reshaping strategy:",
            attractor_id, affected_dimensions, reshape_magnitude
        )
    }

    /// Generate self-prompt for residue integration
    /// Prompts when symbolic residues need to be integrated into the field
    pub fn generate_residue_prompt(
        &self,
        residue_count: usize,
        avg_connection_potential: f32,
    ) -> String {
        format!(
            "RESIDUE SURFACING COMPLETE: {} symbolic fragments detected with \
             average connection potential {:.2}. \
             Strategic decision: Should I create new attractor basins from these residues? \
             Or integrate them into existing attractors? \
             Integration approach:",
            residue_count, avg_connection_potential
        )
    }

    /// Generate self-prompt for boundary conditions
    /// Prompts when boundary dissolution or collapse is detected
    pub fn generate_boundary_prompt(
        &self,
        boundary_type: &str,
        permeability: f32,
        attractors: &[String],
    ) -> String {
        format!(
            "BOUNDARY CONDITION DETECTED: {:?} boundary with permeability {:.2} \
             between attractors: {:?}. \
             Question: Should I collapse this boundary to enable deeper integration? \
             Or maintain it for pattern differentiation? \
             Risk assessment: What happens if boundary dissolves? \
             Decision:",
            boundary_type, permeability, attractors
        )
    }

    /// Execute self-prompting cycle with recursive reasoning
    /// Main entry point for autonomous co-emergence management
    pub fn execute_self_prompting_cycle(
        &mut self,
        field: &NeuralField,
        elapsed_time: u64,
    ) -> ContextNestResult<Vec<String>> {
        let mut prompts = Vec::new();

        // 1. Cycle check prompt
        if elapsed_time % 60 == 0 {
            // Every minute
            prompts.push(self.generate_cycle_prompt(elapsed_time));
        }

        // 2. Coherence check prompt
        if field.state.coherence < 0.6 {
            prompts.push(self.generate_coherence_prompt(field.state.coherence, 0.6));
        }

        // 3. Generate prompts based on recent learning
        if let Some(last_record) = self.learning_history.last() {
            prompts.push(self.generate_recursive_prompt(
                &last_record.action_id,
                last_record.outcome.success,
                last_record.outcome.impact_score,
            ));
        }

        // 4. Audit prompt (if needed)
        if field.attractors.len() > 5 {
            // Suggest audit when many attractors exist
            prompts.push(self.generate_audit_prompt(
                0, // Would come from actual audit
                0, // Would come from actual audit
                field.state.coherence,
            ));
        }

        Ok(prompts)
    }
}

#[cfg(test)]
mod co_emergence_tests {
    use super::*;

    #[test]
    fn test_cycle_prompt_generation() {
        let config = AgencyConfig::default();
        let activator = AgencyActivator::new(config);

        let prompt = activator.generate_cycle_prompt(120);
        assert!(prompt.contains("120s"));
        assert!(prompt.contains("CYCLE CHECK"));
    }

    #[test]
    fn test_emergence_prompt_generation() {
        let config = AgencyConfig::default();
        let activator = AgencyActivator::new(config);

        let prompt = activator.generate_emergence_prompt(
            "attractor_1",
            "attractor_2",
            "Complementary",
            0.75,
        );

        assert!(prompt.contains("EMERGENCE OPPORTUNITY"));
        assert!(prompt.contains("attractor_1"));
        assert!(prompt.contains("0.75"));
    }

    #[test]
    fn test_coherence_prompt_generation() {
        let config = AgencyConfig::default();
        let activator = AgencyActivator::new(config);

        let prompt = activator.generate_coherence_prompt(0.45, 0.6);

        assert!(prompt.contains("COHERENCE ALERT"));
        assert!(prompt.contains("0.45"));
        assert!(prompt.contains("0.15")); // Gap
    }

    #[test]
    fn test_recursive_prompt_success() {
        let config = AgencyConfig::default();
        let activator = AgencyActivator::new(config);

        let prompt = activator.generate_recursive_prompt("test_action", true, 0.8);

        assert!(prompt.contains("succeeded"));
        assert!(prompt.contains("0.8"));
    }

    #[test]
    fn test_recursive_prompt_failure() {
        let config = AgencyConfig::default();
        let activator = AgencyActivator::new(config);

        let prompt = activator.generate_recursive_prompt("test_action", false, 0.2);

        assert!(prompt.contains("failed"));
        assert!(prompt.contains("Root cause"));
    }
}

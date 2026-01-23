//! Multi-Agent Field Interactions for Co-Emergence Protocol
//! This module implements sophisticated field-level interactions between multiple
//! agents, enabling coordinated behavior, swarm intelligence, and collective
//! emergence patterns in neural fields.

use crate::context::attractor_dynamics::{
    AttractorBasin, AttractorDynamicsEngine, CoEmergenceType,
};
use crate::context::field::{NeuralField, SemanticPattern};
use crate::context::harmonic_integration::{HarmonicIntegrator, IntegrationStrategy};
use crate::context::multi_attractor::MultiAttractorCoordinator;
use crate::error::ContextNestResult;
use crate::{ContextNestError, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Multi-agent field interaction manager
#[derive(Debug, Clone)]
pub struct MultiAgentFieldManager {
    /// Active agents in the field
    pub agents: Vec<FieldAgent>,
    /// Interaction history between agents
    pub interaction_history: Vec<AgentInteraction>,
    /// Swarm intelligence parameters
    pub swarm_params: SwarmParameters,
    /// Field-level interaction patterns
    pub interaction_patterns: Vec<InteractionPattern>,
    /// Collective emergence tracking
    pub collective_emergence: CollectiveEmergenceTracker,
    /// Coordination metrics
    pub metrics: MultiAgentMetrics,
}

/// Individual agent operating in the field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldAgent {
    /// Unique agent identifier
    pub id: String,
    /// Agent type and capabilities
    pub agent_type: AgentType,
    /// Current state in the field
    pub state: AgentState,
    /// Agent's attractor basin (if any)
    pub basin_id: Option<String>,
    /// Agent's influence radius
    pub influence_radius: f32,
    /// Agent's goals and priorities
    pub goals: Vec<AgentGoal>,
    /// Communication capabilities
    pub communication: CommunicationCapabilities,
    /// Learning and adaptation parameters
    pub learning_params: AgentLearningParams,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last activity timestamp
    pub last_active: chrono::DateTime<chrono::Utc>,
}

/// Types of agents in the field
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentType {
    /// Pattern recognition specialist
    PatternRecognizer,
    /// Attractor dynamics expert
    AttractorSpecialist,
    /// Field harmony optimizer
    HarmonyOptimizer,
    /// Emergence detector
    EmergenceDetector,
    /// Integration coordinator
    IntegrationCoordinator,
    /// Meta-learning agent
    MetaLearner,
    /// Boundary negotiator
    BoundaryNegotiator,
    /// Residue processor
    ResidueProcessor,
}

/// Current state of an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    /// Agent's position in embedding space
    pub position: Vec<f32>,
    /// Current energy level
    pub energy: f32,
    /// Agent's confidence in current state
    pub confidence: f32,
    /// Current activity being performed
    pub current_activity: Option<String>,
    /// Agent's internal state
    pub internal_state: HashMap<String, f32>,
    /// Connections to other agents
    pub agent_connections: Vec<String>,
}

/// Goal for an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentGoal {
    /// Goal identifier
    pub id: String,
    /// Goal description
    pub description: String,
    /// Goal priority (0.0-1.0)
    pub priority: f32,
    /// Goal type
    pub goal_type: GoalType,
    /// Target metrics
    pub target_metrics: HashMap<String, f32>,
    /// Deadline (optional)
    pub deadline: Option<chrono::DateTime<chrono::Utc>>,
    /// Progress towards goal (0.0-1.0)
    pub progress: f32,
}

/// Types of goals for agents
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GoalType {
    /// Optimize field coherence
    OptimizeCoherence { target: f32 },
    /// Discover new patterns
    DiscoverPatterns { count: usize },
    /// Create attractor connections
    CreateConnections { target_count: usize },
    /// Reduce field entropy
    ReduceEntropy { target: f32 },
    /// Maximize emergence
    MaximizeEmergence,
    /// Balance field properties
    BalanceField,
    /// Learn from interactions
    LearnFromInteractions,
}

/// Communication capabilities between agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunicationCapabilities {
    /// Communication range
    pub range: f32,
    /// Bandwidth (messages per cycle)
    pub bandwidth: usize,
    /// Message types supported
    pub message_types: Vec<MessageType>,
    /// Signal strength
    pub signal_strength: f32,
}

/// Types of messages between agents
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MessageType {
    /// Pattern sharing
    PatternShare { pattern_id: String },
    /// Co-emergence proposal
    CoEmergenceProposal {
        source_id: String,
        target_id: String,
        emergence_type: CoEmergenceType,
    },
    /// Field state update
    FieldStateUpdate { coherence: f32, stability: f32 },
    /// Request for assistance
    AssistanceRequest { task: String },
    /// Knowledge transfer
    KnowledgeTransfer { knowledge: String },
    /// Coordination signal
    CoordinationSignal {
        action: String,
        parameters: Vec<f32>,
    },
    /// Emergence alert
    EmergenceAlert {
        emergence_type: String,
        location: Vec<f32>,
    },
}

/// Learning parameters for agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLearningParams {
    /// Learning rate
    pub learning_rate: f32,
    /// Exploration rate
    pub exploration_rate: f32,
    /// Memory capacity
    pub memory_capacity: usize,
    /// Adaptation speed
    pub adaptation_speed: f32,
    /// Social learning factor
    pub social_learning_factor: f32,
}

/// Interaction between two agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInteraction {
    /// Interaction ID
    pub id: String,
    /// Participating agents
    pub agent_ids: (String, String),
    /// Interaction type
    pub interaction_type: InteractionType,
    /// Interaction strength
    pub strength: f32,
    /// Outcome of interaction
    pub outcome: InteractionOutcome,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Context of interaction
    pub context: InteractionContext,
}

/// Types of interactions between agents
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InteractionType {
    /// Cooperation towards common goal
    Cooperation { goal_id: String },
    /// Competition for resources
    Competition { resource: String },
    /// Knowledge exchange
    KnowledgeExchange,
    /// Coordinated action
    CoordinatedAction { action: String },
    /// Negotiation
    Negotiation { topic: String },
    /// Emergence triggering
    EmergenceTrigger,
    /// Synchronization
    Synchronization { frequency: f32 },
    /// Conflict resolution
    ConflictResolution,
}

/// Outcome of an interaction
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InteractionOutcome {
    /// Successful interaction
    Success { impact: f32 },
    /// Partial success
    PartialSuccess { impact: f32, issues: Vec<String> },
    /// Failed interaction
    Failed { reason: String },
    /// Ongoing interaction
    Ongoing,
    /// Deferred interaction
    Deferred { reason: String },
}

/// Context of interaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionContext {
    /// Field coherence at interaction time
    pub field_coherence: f32,
    /// Local agent density
    pub local_agent_density: f32,
    /// Available resources
    pub available_resources: HashMap<String, f32>,
    /// Environmental conditions
    pub environmental_conditions: HashMap<String, f32>,
}

/// Swarm intelligence parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmParameters {
    /// Number of agents in the swarm
    pub agent_count: usize,
    /// Initial spread of agents
    pub initial_spread: f32,
    /// Communication range between agents
    pub communication_range: f32,
    /// Strength of agent interactions
    pub interaction_strength: f32,
    /// Frequency of coordination activities
    pub coordination_frequency: f32,
    /// Alignment factor (agents align with neighbors)
    pub alignment_factor: f32,
    /// Cohesion factor (agents move toward group center)
    pub cohesion_factor: f32,
    /// Separation factor (agents avoid crowding)
    pub separation_factor: f32,
    /// Global communication strength
    pub global_communication: f32,
    /// Local interaction radius
    pub local_radius: f32,
    /// Emergence threshold
    pub emergence_threshold: f32,
}

impl Default for SwarmParameters {
    fn default() -> Self {
        Self {
            agent_count: 10,
            initial_spread: 0.5,
            communication_range: 0.3,
            interaction_strength: 0.7,
            coordination_frequency: 0.8,
            alignment_factor: 0.1,
            cohesion_factor: 0.05,
            separation_factor: 0.15,
            global_communication: 0.02,
            local_radius: 0.3,
            emergence_threshold: 0.7,
        }
    }
}

/// Pattern of field-level interactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionPattern {
    /// Pattern ID
    pub id: String,
    /// Pattern name
    pub name: String,
    /// Pattern type
    pub pattern_type: PatternType,
    /// Participating agents
    pub participating_agents: Vec<String>,
    /// Pattern strength
    pub strength: f32,
    /// Pattern frequency
    pub frequency: f32,
    /// Pattern phase
    pub phase: f32,
    /// Detection timestamp
    pub detected_at: chrono::DateTime<chrono::Utc>,
}

/// Types of interaction patterns
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PatternType {
    /// Synchronized oscillation
    SynchronizedOscillation { frequency: f32 },
    /// Wave propagation
    WavePropagation { direction: Vec<f32>, speed: f32 },
    /// Spiral formation
    SpiralFormation { center: Vec<f32>, radius: f32 },
    /// Network clustering
    NetworkClustering { cluster_count: usize },
    /// Cascade effect
    CascadeEffect { trigger_threshold: f32 },
    /// Phase transition
    PhaseTransition { critical_point: f32 },
    /// Chaotic behavior
    ChaoticAttractor { dimension: usize },
}

/// Tracker for collective emergence phenomena
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiveEmergenceTracker {
    /// Active emergence phenomena
    pub active_emergences: Vec<CollectiveEmergence>,
    /// Emergence history
    pub emergence_history: Vec<CollectiveEmergence>,
    /// Emergence prediction model
    pub prediction_model: EmergencePredictionModel,
    /// Emergence metrics
    pub metrics: CollectiveEmergenceMetrics,
}

/// Collective emergence phenomenon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiveEmergence {
    /// Emergence ID
    pub id: String,
    /// Emergence type
    pub emergence_type: CollectiveEmergenceType,
    /// Participating agents
    pub participating_agents: Vec<String>,
    /// Emergence strength
    pub strength: f32,
    /// Spatial location in field
    pub location: Vec<f32>,
    /// Temporal extent
    pub temporal_extent: chrono::Duration,
    /// Emergence properties
    pub properties: HashMap<String, f32>,
    /// Start timestamp
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// End timestamp (if concluded)
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Types of collective emergence
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CollectiveEmergenceType {
    /// Swarm intelligence emergence
    SwarmIntelligence {
        collective_behavior: String,
        coordination_level: f32,
    },
    /// Distributed cognition
    DistributedCognition {
        shared_understanding: f32,
        knowledge_distribution: HashMap<String, f32>,
    },
    /// Self-organized criticality
    SelfOrganizedCriticality {
        critical_exponent: f32,
        correlation_length: f32,
    },
    /// Collective learning
    CollectiveLearning {
        learning_rate: f32,
        knowledge_transfer_efficiency: f32,
    },
    /// Emergent hierarchy
    EmergentHierarchy {
        hierarchy_depth: usize,
        organization_level: f32,
    },
    /// Synchronized behavior
    SynchronizedBehavior {
        synchronization_index: f32,
        phase_coherence: f32,
    },
    /// Adaptive response
    AdaptiveResponse {
        adaptation_speed: f32,
        environmental_coupling: f32,
    },
    /// Metastable states
    MetastableStates {
        stability_duration: chrono::Duration,
        transition_probability: f32,
    },
}

/// Model for predicting emergence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergencePredictionModel {
    /// Model parameters
    pub parameters: HashMap<String, f32>,
    /// Prediction accuracy
    pub accuracy: f32,
    /// Training examples
    pub training_examples: usize,
    /// Model version
    pub version: String,
}

/// Metrics for collective emergence
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CollectiveEmergenceMetrics {
    /// Total emergences detected
    pub total_emergences: usize,
    /// Active emergences
    pub active_emergences: usize,
    /// Average emergence strength
    pub avg_strength: f32,
    /// Average emergence duration
    pub avg_duration: chrono::Duration,
    /// Emergence rate per hour
    pub emergence_rate: f32,
    /// Predictive accuracy
    pub predictive_accuracy: f32,
}

/// Metrics for multi-agent system
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MultiAgentMetrics {
    /// Total agents
    pub total_agents: usize,
    /// Active agents
    pub active_agents: usize,
    /// Total interactions
    pub total_interactions: usize,
    /// Successful interactions
    pub successful_interactions: usize,
    /// Average agent energy
    pub avg_agent_energy: f32,
    /// Swarm coherence
    pub swarm_coherence: f32,
    /// Communication efficiency
    pub communication_efficiency: f32,
    /// Collective intelligence score
    pub collective_intelligence: f32,
}

impl MultiAgentFieldManager {
    /// Create a new multi-agent field manager
    pub fn new(swarm_params: SwarmParameters) -> Self {
        Self {
            agents: Vec::new(),
            interaction_history: Vec::new(),
            swarm_params,
            interaction_patterns: Vec::new(),
            collective_emergence: CollectiveEmergenceTracker {
                active_emergences: Vec::new(),
                emergence_history: Vec::new(),
                prediction_model: EmergencePredictionModel {
                    parameters: HashMap::new(),
                    accuracy: 0.5,
                    training_examples: 0,
                    version: "1.0".to_string(),
                },
                metrics: CollectiveEmergenceMetrics::default(),
            },
            metrics: MultiAgentMetrics::default(),
        }
    }

    /// Add a new agent to the field
    pub fn add_agent(&mut self, agent: FieldAgent) -> ContextNestResult<()> {
        // Validate agent doesn't already exist
        if self.agents.iter().any(|a| a.id == agent.id) {
            return Err(ContextNestError::Validation(format!(
                "Agent with ID '{}' already exists",
                agent.id
            )));
        }

        self.agents.push(agent);
        self.metrics.total_agents += 1;
        self.metrics.active_agents += 1;

        Ok(())
    }

    /// Execute multi-agent field interactions for one cycle
    pub fn execute_interactions(
        &mut self,
        engine: &mut AttractorDynamicsEngine,
        field: &mut NeuralField,
    ) -> ContextNestResult<Vec<AgentInteraction>> {
        let mut interactions = Vec::new();

        // 1. Update agent states
        self.update_agent_states(engine, field)?;

        // 2. Find potential interactions
        let potential_interactions = self.find_potential_interactions()?;

        // 3. Execute interactions
        for interaction_opportunity in potential_interactions {
            if let Ok(interaction) = self.execute_interaction(
                &interaction_opportunity.agent1_id,
                &interaction_opportunity.agent2_id,
                interaction_opportunity.interaction_type,
                engine,
                field,
            ) {
                interactions.push(interaction);
            }
        }

        // 4. Apply swarm intelligence
        self.apply_swarm_intelligence(engine, field)?;

        // 5. Detect collective emergence
        self.detect_collective_emergence(engine, field)?;

        // 6. Update metrics
        self.update_metrics();

        Ok(interactions)
    }

    /// Update internal states of all agents
    fn update_agent_states(
        &mut self,
        engine: &AttractorDynamicsEngine,
        field: &NeuralField,
    ) -> ContextNestResult<()> {
        // Collect local coherence values first to avoid borrow checker issues
        let mut local_coherences = Vec::new();
        for agent in &self.agents {
            local_coherences.push(self.calculate_local_coherence(agent, field)?);
        }

        for (idx, agent) in self.agents.iter_mut().enumerate() {
            // Update energy based on field conditions
            let local_coherence = local_coherences[idx];
            agent.state.energy *= 0.9 + local_coherence * 0.1; // Energy decay modulated by coherence

            // Update confidence based on goal progress
            if !agent.goals.is_empty() {
                let avg_progress =
                    agent.goals.iter().map(|g| g.progress).sum::<f32>() / agent.goals.len() as f32;
                agent.state.confidence =
                    (agent.state.confidence * 0.8 + avg_progress * 0.2).min(1.0);
            }

            // Skip agent_learn_from_interactions due to borrow checker issues
            // This will be handled in a separate method

            // Update last active timestamp
            agent.last_active = chrono::Utc::now();
        }

        Ok(())
    }

    /// Calculate local field coherence around an agent
    fn calculate_local_coherence(
        &self,
        agent: &FieldAgent,
        field: &NeuralField,
    ) -> ContextNestResult<f32> {
        // Find patterns within agent's influence radius
        let nearby_patterns: Vec<_> = field
            .patterns
            .iter()
            .filter(|p| {
                let distance = self.calculate_distance(&agent.state.position, &p.embedding);
                distance <= agent.influence_radius
            })
            .collect();

        if nearby_patterns.is_empty() {
            return Ok(field.state.coherence); // Default to field coherence
        }

        // Calculate average pattern strength in vicinity
        let avg_strength =
            nearby_patterns.iter().map(|p| p.strength).sum::<f32>() / nearby_patterns.len() as f32;

        Ok(avg_strength)
    }

    /// Calculate Euclidean distance between two vectors
    fn calculate_distance(&self, a: &[f32], b: &[f32]) -> f32 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    /// Agent learns from recent interactions
    fn agent_learn_from_interactions(&self, agent: &mut FieldAgent) -> ContextNestResult<()> {
        let recent_interactions: Vec<_> = self
            .interaction_history
            .iter()
            .filter(|i| {
                (i.agent_ids.0 == agent.id || i.agent_ids.1 == agent.id)
                    && (chrono::Utc::now() - i.timestamp).num_minutes() < 10
            })
            .collect();

        if recent_interactions.is_empty() {
            return Ok(());
        }

        // Calculate average interaction success
        let success_rate = recent_interactions
            .iter()
            .map(|i| match i.outcome {
                InteractionOutcome::Success { .. } => 1.0,
                InteractionOutcome::PartialSuccess { .. } => 0.5,
                _ => 0.0,
            })
            .sum::<f32>()
            / recent_interactions.len() as f32;

        // Update learning parameters
        agent.learning_params.learning_rate *= 0.99; // Gradual decay
        agent.learning_params.learning_rate += success_rate * 0.01; // Boost from success
        agent.learning_params.learning_rate =
            agent.learning_params.learning_rate.min(0.1).max(0.001);

        Ok(())
    }

    /// Find potential interactions between agents
    fn find_potential_interactions(&self) -> ContextNestResult<Vec<PotentialInteraction>> {
        let mut potentials = Vec::new();

        for i in 0..self.agents.len() {
            for j in (i + 1)..self.agents.len() {
                let agent1 = &self.agents[i];
                let agent2 = &self.agents[j];

                // Check if agents are within interaction range
                let distance =
                    self.calculate_distance(&agent1.state.position, &agent2.state.position);
                let interaction_range = (agent1.influence_radius + agent2.influence_radius) / 2.0;

                if distance <= interaction_range {
                    // Determine interaction type based on agent types and goals
                    let interaction_type = self.determine_interaction_type(agent1, agent2);

                    if let Some(int_type) = interaction_type {
                        potentials.push(PotentialInteraction {
                            agent1_id: agent1.id.clone(),
                            agent2_id: agent2.id.clone(),
                            interaction_type: int_type,
                            distance,
                            confidence: self.calculate_interaction_confidence(agent1, agent2),
                        });
                    }
                }
            }
        }

        // Sort by confidence descending
        potentials.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        Ok(potentials)
    }

    /// Determine appropriate interaction type between two agents
    fn determine_interaction_type(
        &self,
        agent1: &FieldAgent,
        agent2: &FieldAgent,
    ) -> Option<InteractionType> {
        // Check for compatible goals
        for goal1 in &agent1.goals {
            for goal2 in &agent2.goals {
                if goal1.goal_type == goal2.goal_type {
                    return Some(InteractionType::Cooperation {
                        goal_id: goal1.id.clone(),
                    });
                }
            }
        }

        // Check for complementary agent types
        match (&agent1.agent_type, &agent2.agent_type) {
            (AgentType::PatternRecognizer, AgentType::AttractorSpecialist) => {
                Some(InteractionType::KnowledgeExchange)
            }
            (AgentType::HarmonyOptimizer, AgentType::IntegrationCoordinator) => {
                Some(InteractionType::CoordinatedAction {
                    action: "optimize_field_harmony".to_string(),
                })
            }
            (AgentType::EmergenceDetector, AgentType::MetaLearner) => {
                Some(InteractionType::Cooperation {
                    goal_id: "learn_from_emergence".to_string(),
                })
            }
            _ => None,
        }
    }

    /// Calculate confidence in potential interaction
    fn calculate_interaction_confidence(&self, agent1: &FieldAgent, agent2: &FieldAgent) -> f32 {
        let mut confidence = 0.5; // Base confidence

        // Boost if agents have high energy
        confidence += (agent1.state.energy + agent2.state.energy) * 0.2;

        // Boost if agents have compatible communication
        let comm_compatibility = self.calculate_communication_compatibility(agent1, agent2);
        confidence += comm_compatibility * 0.2;

        // Boost if agents have successful interaction history
        let historical_success = self.get_historical_interaction_success(&agent1.id, &agent2.id);
        confidence += historical_success * 0.1;

        confidence.min(1.0).max(0.0)
    }

    /// Calculate communication compatibility between agents
    fn calculate_communication_compatibility(
        &self,
        agent1: &FieldAgent,
        agent2: &FieldAgent,
    ) -> f32 {
        let mut compatibility = 0.0;

        // Check overlapping message types
        let mut overlap = 0;
        for msg_type1 in &agent1.communication.message_types {
            for msg_type2 in &agent2.communication.message_types {
                if msg_type1 == msg_type2 {
                    overlap += 1;
                    break; // Count each type only once
                }
            }
        }

        if !agent1.communication.message_types.is_empty() {
            compatibility = overlap as f32 / agent1.communication.message_types.len() as f32;
        }

        // Factor in signal strength and range
        let signal_factor =
            (agent1.communication.signal_strength + agent2.communication.signal_strength) / 2.0;
        compatibility *= signal_factor;

        compatibility
    }

    /// Get historical interaction success rate between two agents
    fn get_historical_interaction_success(&self, agent1_id: &str, agent2_id: &str) -> f32 {
        let interactions: Vec<_> = self
            .interaction_history
            .iter()
            .filter(|i| {
                (i.agent_ids.0 == agent1_id && i.agent_ids.1 == agent2_id)
                    || (i.agent_ids.0 == agent2_id && i.agent_ids.1 == agent1_id)
            })
            .collect();

        if interactions.is_empty() {
            return 0.5; // Neutral prior
        }

        let successful = interactions
            .iter()
            .filter(|i| matches!(i.outcome, InteractionOutcome::Success { .. }))
            .count();

        successful as f32 / interactions.len() as f32
    }

    /// Execute an interaction between two agents
    fn execute_interaction(
        &mut self,
        agent1_id: &str,
        agent2_id: &str,
        interaction_type: InteractionType,
        engine: &mut AttractorDynamicsEngine,
        field: &mut NeuralField,
    ) -> ContextNestResult<AgentInteraction> {
        // Find agent indices first to avoid borrow checker issues
        let agent1_idx = self
            .agents
            .iter()
            .position(|a| a.id == agent1_id)
            .ok_or_else(|| ContextNestError::NotFound("Agent not found".to_string()))?;
        let agent2_idx = self
            .agents
            .iter()
            .position(|a| a.id == agent2_id)
            .ok_or_else(|| ContextNestError::NotFound("Agent not found".to_string()))?;

        // Calculate local agent density before borrowing
        let local_density = self.calculate_local_agent_density(&self.agents[agent1_idx]);

        // Record interaction in agent connections using split_at_mut
        {
            let (first_agent, second_agent) = if agent1_idx < agent2_idx {
                let (first, second) = self.agents.split_at_mut(agent2_idx);
                (&mut first[agent1_idx], &mut second[0])
            } else if agent2_idx < agent1_idx {
                let (first, second) = self.agents.split_at_mut(agent1_idx);
                (&mut second[0], &mut first[agent2_idx])
            } else {
                return Err(ContextNestError::Validation(
                    "Cannot interact with same agent".to_string(),
                ));
            };

            if !first_agent
                .state
                .agent_connections
                .contains(&second_agent.id.to_string())
            {
                first_agent
                    .state
                    .agent_connections
                    .push(second_agent.id.clone());
            }
            if !second_agent
                .state
                .agent_connections
                .contains(&first_agent.id.to_string())
            {
                second_agent
                    .state
                    .agent_connections
                    .push(first_agent.id.clone());
            }
        }

        // Execute interaction based on type
        let outcome = match &interaction_type {
            InteractionType::Cooperation { goal_id } => {
                self.execute_cooperation_by_index(agent1_idx, agent2_idx, goal_id, engine, field)?
            }
            InteractionType::KnowledgeExchange => {
                self.execute_knowledge_exchange_by_index(agent1_idx, agent2_idx, field)?
            }
            InteractionType::CoordinatedAction { action } => self
                .execute_coordinated_action_by_index(
                    agent1_idx, agent2_idx, action, engine, field,
                )?,
            InteractionType::Synchronization { frequency } => {
                self.execute_synchronization_by_index(agent1_idx, agent2_idx, *frequency)?
            }
            _ => InteractionOutcome::Success { impact: 0.1 }, // Default minimal impact
        };

        // Create interaction record
        let interaction = AgentInteraction {
            id: uuid::Uuid::new_v4().to_string(),
            agent_ids: (agent1_id.to_string(), agent2_id.to_string()),
            interaction_type,
            strength: match outcome {
                InteractionOutcome::Success { impact } => impact,
                InteractionOutcome::PartialSuccess { impact, .. } => impact,
                _ => 0.0,
            },
            outcome,
            timestamp: chrono::Utc::now(),
            context: InteractionContext {
                field_coherence: field.state.coherence,
                local_agent_density: local_density,
                available_resources: HashMap::new(), // Would be populated based on context
                environmental_conditions: HashMap::new(),
            },
        };

        self.interaction_history.push(interaction.clone());
        self.metrics.total_interactions += 1;

        if matches!(interaction.outcome, InteractionOutcome::Success { .. }) {
            self.metrics.successful_interactions += 1;
        }

        Ok(interaction)
    }

    /// Execute cooperation between two agents
    fn execute_cooperation(
        &self,
        agent1: &mut FieldAgent,
        agent2: &mut FieldAgent,
        goal_id: &str,
        engine: &mut AttractorDynamicsEngine,
        field: &mut NeuralField,
    ) -> ContextNestResult<InteractionOutcome> {
        // Clone the data we need before borrowing
        let agent1_id = agent1.id.clone();
        let agent2_id = agent2.id.clone();
        let agent1_energy = agent1.state.energy;
        let agent2_energy = agent2.state.energy;

        // Find shared goal and clone it
        let goal1_clone = agent1
            .goals
            .iter()
            .find(|g| g.id == goal_id)
            .cloned()
            .ok_or_else(|| ContextNestError::NotFound("Goal not found".to_string()))?;

        // Execute cooperation based on goal type
        let (impact, new_progress) = match &goal1_clone.goal_type {
            GoalType::OptimizeCoherence { target } => {
                // Agents work together to optimize coherence
                let coherence_gain =
                    self.cooperate_optimize_coherence(agent1, agent2, engine, field)?;
                let progress = (field.state.coherence / *target).min(1.0);
                (coherence_gain, progress)
            }
            GoalType::CreateConnections { target_count } => {
                // Agents work together to create attractor connections
                let connections_created =
                    self.cooperate_create_connections(agent1, agent2, engine, field)?;
                let progress = (connections_created as f32 / *target_count as f32).min(1.0);
                (connections_created as f32 * 0.1, progress)
            }
            _ => (0.1, 0.0), // Default minimal impact for other goal types
        };

        // Now update both agents
        if let Some(goal1) = agent1.goals.iter_mut().find(|g| g.id == goal_id) {
            goal1.progress = new_progress;
        }
        if let Some(goal2) = agent2.goals.iter_mut().find(|g| g.id == goal_id) {
            goal2.progress = new_progress;
        }

        // Energy cost for cooperation
        agent1.state.energy = agent1_energy * 0.95;
        agent2.state.energy = agent2_energy * 0.95;

        Ok(InteractionOutcome::Success {
            impact: impact.min(1.0),
        })
    }

    /// Execute cooperation between two agents by index (to avoid borrow checker issues)
    fn execute_cooperation_by_index(
        &mut self,
        agent1_idx: usize,
        agent2_idx: usize,
        goal_id: &str,
        engine: &mut AttractorDynamicsEngine,
        field: &mut NeuralField,
    ) -> ContextNestResult<InteractionOutcome> {
        let agent1 = &self.agents[agent1_idx];
        let agent2 = &self.agents[agent2_idx];

        // Clone data we need
        let agent1_id = agent1.id.clone();
        let agent2_id = agent2.id.clone();
        let goal1_clone = agent1
            .goals
            .iter()
            .find(|g| g.id == goal_id)
            .cloned()
            .ok_or_else(|| ContextNestError::NotFound("Goal not found".to_string()))?;
        let goal2_clone = agent2
            .goals
            .iter()
            .find(|g| g.id == goal_id)
            .cloned()
            .ok_or_else(|| ContextNestError::NotFound("Goal not found".to_string()))?;

        // Execute cooperation based on goal type
        let impact = match &goal1_clone.goal_type {
            GoalType::OptimizeCoherence { target } => {
                // Agents work together to optimize coherence
                let coherence_gain =
                    self.cooperate_optimize_coherence(&agent1, &agent2, engine, field)?;
                coherence_gain
            }
            GoalType::CreateConnections { target_count } => {
                // Agents work together to create attractor connections
                let connections_created =
                    self.cooperate_create_connections(&agent1, &agent2, engine, field)?;
                connections_created as f32 * 0.1
            }
            _ => 0.1, // Default minimal impact for other goal types
        };

        // Now update the agents with the results
        if let Some(agent1) = self.agents.get_mut(agent1_idx) {
            if let Some(goal1) = agent1.goals.iter_mut().find(|g| g.id == goal_id) {
                match &goal1_clone.goal_type {
                    GoalType::OptimizeCoherence { target } => {
                        goal1.progress = (field.state.coherence / *target).min(1.0);
                    }
                    GoalType::CreateConnections { target_count } => {
                        goal1.progress = (impact / *target_count as f32).min(1.0);
                    }
                    _ => {}
                }
                agent1.state.energy *= 0.95;
            }
        }

        // Get goal1 progress before updating agent2
        let goal1_progress = if let Some(agent1) = self.agents.get(agent1_idx) {
            if let Some(goal1) = agent1.goals.iter().find(|g| g.id == goal_id) {
                goal1.progress
            } else {
                0.0
            }
        } else {
            0.0
        };

        if let Some(agent2) = self.agents.get_mut(agent2_idx) {
            if let Some(goal2) = agent2.goals.iter_mut().find(|g| g.id == goal_id) {
                goal2.progress = goal1_progress;
                agent2.state.energy *= 0.95;
            }
        }

        Ok(InteractionOutcome::Success {
            impact: impact.min(1.0),
        })
    }

    /// Cooperate to optimize field coherence
    fn cooperate_optimize_coherence(
        &self,
        agent1: &FieldAgent,
        agent2: &FieldAgent,
        engine: &mut AttractorDynamicsEngine,
        field: &mut NeuralField,
    ) -> ContextNestResult<f32> {
        // Combine agent influences to enhance field coherence
        let combined_influence = (agent1.state.energy + agent2.state.energy) / 2.0;

        // Apply coherence optimization
        let before_coherence = field.state.coherence;

        // Agents contribute to field stability
        field.state.stability = (field.state.stability + combined_influence * 0.1).min(1.0);

        // Enhanced pattern resonance through cooperation
        field.amplify_resonant()?;

        let after_coherence = field.state.coherence;
        let coherence_gain = after_coherence - before_coherence;

        Ok(coherence_gain.max(0.0))
    }

    /// Cooperate to create attractor connections
    fn cooperate_create_connections(
        &self,
        agent1: &FieldAgent,
        agent2: &FieldAgent,
        engine: &mut AttractorDynamicsEngine,
        field: &mut NeuralField,
    ) -> ContextNestResult<usize> {
        let mut connections_created = 0;

        // Find attractors near each agent
        let agent1_attractors: Vec<_> = engine
            .attractor_basins
            .iter()
            .filter(|b| {
                let distance = self.calculate_distance(&agent1.state.position, &b.center);
                distance <= agent1.influence_radius
            })
            .collect();

        let agent2_attractors: Vec<_> = engine
            .attractor_basins
            .iter()
            .filter(|b| {
                let distance = self.calculate_distance(&agent2.state.position, &b.center);
                distance <= agent2.influence_radius
            })
            .collect();

        // Create connections between nearby attractors
        for basin1 in &agent1_attractors {
            for basin2 in &agent2_attractors {
                if basin1.id != basin2.id {
                    // Check if connection already exists (simplified)
                    let should_connect =
                        self.calculate_distance(&basin1.center, &basin2.center) < 0.5;

                    if should_connect {
                        // Create harmonic connection
                        let mut integrator = HarmonicIntegrator::default();
                        if let Ok(_connection) =
                            integrator.connect_attractors(basin1, basin2, field)
                        {
                            connections_created += 1;
                        }
                    }
                }
            }
        }

        Ok(connections_created)
    }

    /// Execute knowledge exchange between agents
    fn execute_knowledge_exchange(
        &self,
        agent1: &mut FieldAgent,
        agent2: &mut FieldAgent,
        field: &NeuralField,
    ) -> ContextNestResult<InteractionOutcome> {
        // Exchange internal states
        for (key, value) in &agent1.state.internal_state.clone() {
            agent2.state.internal_state.insert(
                format!("from_{}_{}", agent1.id, key),
                value * 0.8, // Slight decay in transfer
            );
        }

        for (key, value) in &agent2.state.internal_state.clone() {
            agent1
                .state
                .internal_state
                .insert(format!("from_{}_{}", agent2.id, key), value * 0.8);
        }

        // Update confidence based on knowledge quality
        let knowledge_quality = self.assess_knowledge_quality(field);
        agent1.state.confidence = (agent1.state.confidence + knowledge_quality * 0.1).min(1.0);
        agent2.state.confidence = (agent2.state.confidence + knowledge_quality * 0.1).min(1.0);

        Ok(InteractionOutcome::Success { impact: 0.3 })
    }

    /// Execute knowledge exchange between agents by index
    fn execute_knowledge_exchange_by_index(
        &mut self,
        agent1_idx: usize,
        agent2_idx: usize,
        field: &NeuralField,
    ) -> ContextNestResult<InteractionOutcome> {
        let agent1 = &self.agents[agent1_idx];
        let agent2 = &self.agents[agent2_idx];

        // Clone data we need
        let agent1_id = agent1.id.clone();
        let agent2_id = agent2.id.clone();
        let agent1_state = agent1.state.internal_state.clone();
        let agent2_state = agent2.state.internal_state.clone();
        let knowledge_quality = self.assess_knowledge_quality(field);

        // Now update both agents
        if let Some(agent1) = self.agents.get_mut(agent1_idx) {
            for (key, value) in &agent2_state {
                agent1
                    .state
                    .internal_state
                    .insert(format!("from_{}_{}", agent2_id, key), value * 0.8);
            }
            agent1.state.confidence = (agent1.state.confidence + knowledge_quality * 0.1).min(1.0);
        }

        if let Some(agent2) = self.agents.get_mut(agent2_idx) {
            for (key, value) in &agent1_state {
                agent2
                    .state
                    .internal_state
                    .insert(format!("from_{}_{}", agent1_id, key), value * 0.8);
            }
            agent2.state.confidence = (agent2.state.confidence + knowledge_quality * 0.1).min(1.0);
        }

        Ok(InteractionOutcome::Success { impact: 0.3 })
    }

    /// Execute coordinated action between agents
    fn execute_coordinated_action(
        &self,
        agent1: &mut FieldAgent,
        agent2: &mut FieldAgent,
        action: &str,
        engine: &mut AttractorDynamicsEngine,
        field: &mut NeuralField,
    ) -> ContextNestResult<InteractionOutcome> {
        match action {
            "optimize_field_harmony" => {
                // Coordinated field optimization
                let before_harmony = field.state.coherence;

                // Both agents contribute to optimization
                let agent1_contribution = agent1.state.energy * 0.2;
                let agent2_contribution = agent2.state.energy * 0.2;
                let total_contribution = agent1_contribution + agent2_contribution;

                field.state.coherence = (field.state.coherence + total_contribution).min(1.0);
                field.state.stability = (field.state.stability + total_contribution * 0.5).min(1.0);

                let after_harmony = field.state.coherence;
                let improvement = after_harmony - before_harmony;

                // Energy cost for coordinated action
                agent1.state.energy *= 0.9;
                agent2.state.energy *= 0.9;

                Ok(InteractionOutcome::Success {
                    impact: improvement,
                })
            }
            _ => Ok(InteractionOutcome::PartialSuccess {
                impact: 0.1,
                issues: vec!["Unknown action".to_string()],
            }),
        }
    }

    /// Execute coordinated action between agents by index
    fn execute_coordinated_action_by_index(
        &mut self,
        agent1_idx: usize,
        agent2_idx: usize,
        action: &str,
        engine: &mut AttractorDynamicsEngine,
        field: &mut NeuralField,
    ) -> ContextNestResult<InteractionOutcome> {
        let agent1 = &self.agents[agent1_idx];
        let agent2 = &self.agents[agent2_idx];

        match action {
            "optimize_field_harmony" => {
                // Coordinated field optimization
                let before_harmony = field.state.coherence;

                // Both agents contribute to optimization
                let agent1_contribution = agent1.state.energy * 0.2;
                let agent2_contribution = agent2.state.energy * 0.2;
                let total_contribution = agent1_contribution + agent2_contribution;

                field.state.coherence = (field.state.coherence + total_contribution).min(1.0);
                field.state.stability = (field.state.stability + total_contribution * 0.5).min(1.0);

                let after_harmony = field.state.coherence;
                let improvement = after_harmony - before_harmony;

                // Energy cost for coordinated action
                if let Some(agent1) = self.agents.get_mut(agent1_idx) {
                    agent1.state.energy *= 0.9;
                }
                if let Some(agent2) = self.agents.get_mut(agent2_idx) {
                    agent2.state.energy *= 0.9;
                }

                Ok(InteractionOutcome::Success {
                    impact: improvement,
                })
            }
            _ => Ok(InteractionOutcome::PartialSuccess {
                impact: 0.1,
                issues: vec!["Unknown action".to_string()],
            }),
        }
    }

    /// Execute synchronization between agents
    fn execute_synchronization(
        &self,
        agent1: &mut FieldAgent,
        agent2: &mut FieldAgent,
        frequency: f32,
    ) -> ContextNestResult<InteractionOutcome> {
        // Synchronize agent phases
        let phase_diff = frequency * 0.1; // Simplified phase calculation

        // Adjust agent internal states for synchronization
        agent1
            .state
            .internal_state
            .insert("phase".to_string(), phase_diff);
        agent2
            .state
            .internal_state
            .insert("phase".to_string(), -phase_diff);

        // Energy gain from synchronization
        let sync_energy = 0.05 * (1.0 - phase_diff.abs());
        agent1.state.energy = (agent1.state.energy + sync_energy).min(1.0);
        agent2.state.energy = (agent2.state.energy + sync_energy).min(1.0);

        Ok(InteractionOutcome::Success { impact: 0.2 })
    }

    /// Execute synchronization between agents by index
    fn execute_synchronization_by_index(
        &mut self,
        agent1_idx: usize,
        agent2_idx: usize,
        frequency: f32,
    ) -> ContextNestResult<InteractionOutcome> {
        // Synchronize agent phases
        let phase_diff = frequency * 0.1; // Simplified phase calculation
        let sync_energy = 0.05 * (1.0 - phase_diff.abs());

        // Update both agents
        if let Some(agent1) = self.agents.get_mut(agent1_idx) {
            agent1
                .state
                .internal_state
                .insert("phase".to_string(), phase_diff);
            agent1.state.energy = (agent1.state.energy + sync_energy).min(1.0);
        }

        if let Some(agent2) = self.agents.get_mut(agent2_idx) {
            agent2
                .state
                .internal_state
                .insert("phase".to_string(), -phase_diff);
            agent2.state.energy = (agent2.state.energy + sync_energy).min(1.0);
        }

        Ok(InteractionOutcome::Success { impact: 0.2 })
    }

    /// Calculate local agent density around an agent
    fn calculate_local_agent_density(&self, agent: &FieldAgent) -> f32 {
        let nearby_count = self
            .agents
            .iter()
            .filter(|a| {
                a.id != agent.id
                    && self.calculate_distance(&agent.state.position, &a.state.position)
                        <= agent.influence_radius * 2.0
            })
            .count();

        nearby_count as f32 / (agent.influence_radius.powi(2) * std::f32::consts::PI)
    }

    /// Apply swarm intelligence principles
    fn apply_swarm_intelligence(
        &mut self,
        engine: &mut AttractorDynamicsEngine,
        field: &mut NeuralField,
    ) -> ContextNestResult<()> {
        // Copy all agent data to avoid borrow checker issues
        let agent_snapshots: Vec<_> = self
            .agents
            .iter()
            .map(|a| {
                (
                    a.id.clone(),
                    a.state.position.clone(),
                    a.state.internal_state.clone(),
                    a.influence_radius,
                )
            })
            .collect();

        let swarm_params = self.swarm_params.clone();

        // Calculate all updates first
        let mut updates = Vec::new();
        for (i, agent) in self.agents.iter().enumerate() {
            // Find neighbors
            let neighbors: Vec<_> = agent_snapshots
                .iter()
                .enumerate()
                .filter_map(|(j, (id, pos, internal_state, influence_radius))| {
                    if i != j {
                        let distance = Self::calculate_distance_static(&agent.state.position, pos);
                        if distance <= swarm_params.local_radius {
                            Some((pos.clone(), internal_state.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();

            if !neighbors.is_empty() {
                // Calculate swarm forces
                let alignment_vector = Self::calculate_alignment_vector_static(
                    &agent.state.position,
                    &neighbors,
                    &swarm_params,
                );
                let cohesion_vector = Self::calculate_cohesion_vector_static(
                    &agent.state.position,
                    &neighbors,
                    &swarm_params,
                );
                let separation_vector = Self::calculate_separation_vector_static(
                    &agent.state.position,
                    &neighbors,
                    agent.influence_radius,
                    &swarm_params,
                );

                updates.push((i, alignment_vector, cohesion_vector, separation_vector));
            }
        }

        // Apply updates using split_at_mut for safe mutable access
        let agent_count = self.agents.len();
        for (idx, alignment_vector, cohesion_vector, separation_vector) in updates {
            if idx < agent_count {
                // Apply the swarm forces inline to avoid method borrowing issues
                let agent = &mut self.agents[idx];

                // Apply alignment
                agent.state.internal_state.insert(
                    "alignment_force".to_string(),
                    alignment_vector.iter().map(|x| x.abs()).sum::<f32>()
                        / alignment_vector.len() as f32,
                );

                // Apply cohesion
                agent.state.internal_state.insert(
                    "cohesion_force".to_string(),
                    cohesion_vector.iter().map(|x| x.abs()).sum::<f32>()
                        / cohesion_vector.len() as f32,
                );

                // Apply separation
                agent.state.internal_state.insert(
                    "separation_force".to_string(),
                    separation_vector.iter().map(|x| x.abs()).sum::<f32>()
                        / separation_vector.len() as f32,
                );

                // Update position
                for i in 0..agent.state.position.len() {
                    let total_force =
                        alignment_vector[i] + cohesion_vector[i] + separation_vector[i];
                    agent.state.position[i] += total_force;
                    agent.state.position[i] = agent.state.position[i].max(-1.0).min(1.0);
                }

                // Store velocity for next iteration
                let velocity_magnitude = alignment_vector
                    .iter()
                    .zip(cohesion_vector.iter())
                    .zip(separation_vector.iter())
                    .map(|((a, c), s)| a + c + s)
                    .map(|x| x.abs())
                    .sum::<f32>()
                    / alignment_vector.len() as f32;

                agent
                    .state
                    .internal_state
                    .insert("velocity".to_string(), velocity_magnitude);
            }
        }

        // Apply global communication effects
        self.apply_global_communication(field)?;

        Ok(())
    }

    /// Static helper function to calculate distance
    fn calculate_distance_static(a: &[f32], b: &[f32]) -> f32 {
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    /// Static helper function to calculate alignment vector
    fn calculate_alignment_vector_static(
        agent_position: &[f32],
        neighbor_data: &[(Vec<f32>, HashMap<String, f32>)],
        swarm_params: &SwarmParameters,
    ) -> Vec<f32> {
        if neighbor_data.is_empty() {
            return vec![0.0; agent_position.len()];
        }

        let mut alignment = vec![0.0; agent_position.len()];
        let neighbor_count = neighbor_data.len() as f32;

        for (neighbor_pos, neighbor_state) in neighbor_data {
            // Use velocity magnitude from internal state if available
            if let Some(velocity_magnitude) = neighbor_state.get("velocity") {
                // Apply velocity magnitude to all dimensions
                for i in 0..alignment.len() {
                    alignment[i] += velocity_magnitude;
                }
            } else {
                // Calculate velocity from position difference (simplified)
                for i in 0..alignment.len().min(neighbor_pos.len()) {
                    alignment[i] += neighbor_pos[i] - agent_position[i];
                }
            }
        }

        // Average and scale by alignment factor
        for i in 0..alignment.len() {
            alignment[i] = (alignment[i] / neighbor_count) * swarm_params.alignment_factor;
        }

        alignment
    }

    /// Static helper function to calculate cohesion vector
    fn calculate_cohesion_vector_static(
        agent_position: &[f32],
        neighbor_data: &[(Vec<f32>, HashMap<String, f32>)],
        swarm_params: &SwarmParameters,
    ) -> Vec<f32> {
        if neighbor_data.is_empty() {
            return vec![0.0; agent_position.len()];
        }

        let mut center = vec![0.0; agent_position.len()];
        let neighbor_count = neighbor_data.len() as f32;

        // Calculate center of mass
        for (neighbor_pos, _) in neighbor_data {
            for (i, &pos) in neighbor_pos.iter().enumerate() {
                center[i] += pos;
            }
        }

        for i in 0..center.len() {
            center[i] /= neighbor_count;
        }

        // Vector toward center
        let mut cohesion = Vec::with_capacity(agent_position.len());
        for (i, &agent_pos) in agent_position.iter().enumerate() {
            cohesion.push((center[i] - agent_pos) * swarm_params.cohesion_factor);
        }

        cohesion
    }

    /// Static helper function to calculate separation vector
    fn calculate_separation_vector_static(
        agent_position: &[f32],
        neighbor_data: &[(Vec<f32>, HashMap<String, f32>)],
        influence_radius: f32,
        swarm_params: &SwarmParameters,
    ) -> Vec<f32> {
        let mut separation = vec![0.0; agent_position.len()];

        for (neighbor_pos, _) in neighbor_data {
            let distance = Self::calculate_distance_static(agent_position, neighbor_pos);

            if distance > 0.0 && distance < influence_radius {
                // Repulsion force inversely proportional to distance
                let repulsion_strength = swarm_params.separation_factor / distance;

                for i in 0..separation.len() {
                    let diff = agent_position[i] - neighbor_pos[i];
                    separation[i] += diff * repulsion_strength;
                }
            }
        }

        separation
    }

    /// Calculate alignment vector for swarm behavior
    fn calculate_alignment_vector(
        &self,
        agent: &FieldAgent,
        neighbors: &[&FieldAgent],
    ) -> Vec<f32> {
        if neighbors.is_empty() {
            return vec![0.0; agent.state.position.len()];
        }

        let mut alignment = vec![0.0; agent.state.position.len()];
        let neighbor_count = neighbors.len() as f32;

        for neighbor in neighbors {
            // Use velocity magnitude from internal state if available
            if let Some(velocity_magnitude) = neighbor.state.internal_state.get("velocity") {
                // Apply velocity magnitude to all dimensions
                for i in 0..alignment.len() {
                    alignment[i] += velocity_magnitude;
                }
            } else {
                // Calculate velocity from position difference (simplified)
                let velocity_diff: Vec<f32> = neighbor
                    .state
                    .position
                    .iter()
                    .zip(agent.state.position.iter())
                    .map(|(n, a)| n - a)
                    .collect();

                for i in 0..alignment.len().min(velocity_diff.len()) {
                    alignment[i] += velocity_diff[i];
                }
            }
        }

        // Average and scale by alignment factor
        for i in 0..alignment.len() {
            alignment[i] = (alignment[i] / neighbor_count) * self.swarm_params.alignment_factor;
        }

        alignment
    }

    /// Calculate cohesion vector for swarm behavior
    fn calculate_cohesion_vector(&self, agent: &FieldAgent, neighbors: &[&FieldAgent]) -> Vec<f32> {
        if neighbors.is_empty() {
            return vec![0.0; agent.state.position.len()];
        }

        let mut center = vec![0.0; agent.state.position.len()];
        let neighbor_count = neighbors.len() as f32;

        // Calculate center of mass
        for neighbor in neighbors {
            for (i, &pos) in neighbor.state.position.iter().enumerate() {
                center[i] += pos;
            }
        }

        for i in 0..center.len() {
            center[i] /= neighbor_count;
        }

        // Vector toward center
        let mut cohesion = Vec::with_capacity(agent.state.position.len());
        for (i, &agent_pos) in agent.state.position.iter().enumerate() {
            cohesion.push((center[i] - agent_pos) * self.swarm_params.cohesion_factor);
        }

        cohesion
    }

    /// Calculate separation vector for swarm behavior
    fn calculate_separation_vector(
        &self,
        agent: &FieldAgent,
        neighbors: &[&FieldAgent],
    ) -> Vec<f32> {
        let mut separation = vec![0.0; agent.state.position.len()];

        for neighbor in neighbors {
            let distance = self.calculate_distance(&agent.state.position, &neighbor.state.position);

            if distance > 0.0 && distance < agent.influence_radius {
                // Repulsion force inversely proportional to distance
                let repulsion_strength = self.swarm_params.separation_factor / distance;

                for i in 0..separation.len() {
                    let diff = agent.state.position[i] - neighbor.state.position[i];
                    separation[i] += diff * repulsion_strength;
                }
            }
        }

        separation
    }

    /// Apply alignment to agent
    fn apply_alignment(&self, agent: &mut FieldAgent, alignment: &[f32]) {
        agent.state.internal_state.insert(
            "alignment_force".to_string(),
            alignment.iter().map(|x| x.abs()).sum::<f32>() / alignment.len() as f32,
        );
    }

    /// Apply cohesion to agent
    fn apply_cohesion(&self, agent: &mut FieldAgent, cohesion: &[f32]) {
        agent.state.internal_state.insert(
            "cohesion_force".to_string(),
            cohesion.iter().map(|x| x.abs()).sum::<f32>() / cohesion.len() as f32,
        );
    }

    /// Apply separation to agent
    fn apply_separation(&self, agent: &mut FieldAgent, separation: &[f32]) {
        agent.state.internal_state.insert(
            "separation_force".to_string(),
            separation.iter().map(|x| x.abs()).sum::<f32>() / separation.len() as f32,
        );
    }

    /// Update agent position based on swarm forces
    fn update_agent_position(
        &self,
        agent: &mut FieldAgent,
        alignment: &[f32],
        cohesion: &[f32],
        separation: &[f32],
    ) {
        for i in 0..agent.state.position.len() {
            let total_force = alignment[i] + cohesion[i] + separation[i];
            agent.state.position[i] += total_force;

            // Keep position bounded
            agent.state.position[i] = agent.state.position[i].max(-1.0).min(1.0);
        }

        // Store velocity for next iteration
        let velocity_magnitude = alignment
            .iter()
            .zip(cohesion.iter())
            .zip(separation.iter())
            .map(|((a, c), s)| a + c + s)
            .map(|x| x.abs())
            .sum::<f32>()
            / alignment.len() as f32;

        agent
            .state
            .internal_state
            .insert("velocity".to_string(), velocity_magnitude);
    }

    /// Apply global communication effects
    fn apply_global_communication(&self, field: &mut NeuralField) -> ContextNestResult<()> {
        // Global communication enhances field coherence slightly
        let global_comm_strength =
            self.metrics.communication_efficiency * self.swarm_params.global_communication;

        field.state.coherence = (field.state.coherence + global_comm_strength).min(1.0);

        // Facilitate pattern resonance across the field
        if global_comm_strength > 0.01 {
            field.amplify_resonant()?;
        }

        Ok(())
    }

    /// Detect collective emergence phenomena
    fn detect_collective_emergence(
        &mut self,
        engine: &AttractorDynamicsEngine,
        field: &NeuralField,
    ) -> ContextNestResult<()> {
        // Check for various types of collective emergence

        // 1. Swarm intelligence emergence
        if self.check_swarm_intelligence_emergence()? {
            let emergence = CollectiveEmergence {
                id: uuid::Uuid::new_v4().to_string(),
                emergence_type: CollectiveEmergenceType::SwarmIntelligence {
                    collective_behavior: "coordinated_pattern_optimization".to_string(),
                    coordination_level: self.calculate_swarm_coordination(),
                },
                participating_agents: self.agents.iter().map(|a| a.id.clone()).collect(),
                strength: self.metrics.collective_intelligence,
                location: self.calculate_field_centroid(),
                temporal_extent: chrono::Duration::minutes(5),
                properties: HashMap::new(),
                started_at: chrono::Utc::now(),
                ended_at: None,
            };

            self.collective_emergence
                .active_emergences
                .push(emergence.clone());
            self.collective_emergence.emergence_history.push(emergence);
        }

        // 2. Synchronized behavior emergence
        if self.check_synchronized_emergence()? {
            let emergence = CollectiveEmergence {
                id: uuid::Uuid::new_v4().to_string(),
                emergence_type: CollectiveEmergenceType::SynchronizedBehavior {
                    synchronization_index: self.calculate_synchronization_index(),
                    phase_coherence: self.calculate_phase_coherence(),
                },
                participating_agents: self.find_synchronized_agents(),
                strength: 0.8,
                location: self.calculate_field_centroid(),
                temporal_extent: chrono::Duration::minutes(3),
                properties: HashMap::new(),
                started_at: chrono::Utc::now(),
                ended_at: None,
            };

            self.collective_emergence
                .active_emergences
                .push(emergence.clone());
            self.collective_emergence.emergence_history.push(emergence);
        }

        // 3. Distributed cognition emergence
        if self.check_distributed_cognition_emergence(field)? {
            let emergence = CollectiveEmergence {
                id: uuid::Uuid::new_v4().to_string(),
                emergence_type: CollectiveEmergenceType::DistributedCognition {
                    shared_understanding: field.state.coherence,
                    knowledge_distribution: self.calculate_knowledge_distribution(),
                },
                participating_agents: self.find_cognitive_agents(),
                strength: field.state.coherence,
                location: self.calculate_field_centroid(),
                temporal_extent: chrono::Duration::minutes(10),
                properties: HashMap::new(),
                started_at: chrono::Utc::now(),
                ended_at: None,
            };

            self.collective_emergence
                .active_emergences
                .push(emergence.clone());
            self.collective_emergence.emergence_history.push(emergence);
        }

        Ok(())
    }

    /// Check for swarm intelligence emergence
    fn check_swarm_intelligence_emergence(&self) -> ContextNestResult<bool> {
        // Check if agents are exhibiting coordinated behavior
        let coordination_level = self.calculate_swarm_coordination();
        let agent_density = self.agents.len() as f32 / 10.0; // Normalize to field size

        Ok(coordination_level > 0.7 && agent_density > 0.3)
    }

    /// Calculate swarm coordination level
    fn calculate_swarm_coordination(&self) -> f32 {
        if self.agents.len() < 2 {
            return 0.0;
        }

        let mut total_similarity = 0.0;
        let mut comparisons = 0;

        for i in 0..self.agents.len() {
            for j in (i + 1)..self.agents.len() {
                let agent1 = &self.agents[i];
                let agent2 = &self.agents[j];

                // Compare internal states
                let state_similarity = self.compare_agent_states(agent1, agent2);
                total_similarity += state_similarity;
                comparisons += 1;
            }
        }

        if comparisons == 0 {
            return 0.0;
        }

        total_similarity / comparisons as f32
    }

    /// Compare similarity between two agent states
    fn compare_agent_states(&self, agent1: &FieldAgent, agent2: &FieldAgent) -> f32 {
        // Compare positions
        let position_similarity =
            1.0 - self.calculate_distance(&agent1.state.position, &agent2.state.position) / 2.0;

        // Compare energy levels
        let energy_similarity = 1.0 - (agent1.state.energy - agent2.state.energy).abs();

        // Compare confidence
        let confidence_similarity = 1.0 - (agent1.state.confidence - agent2.state.confidence).abs();

        // Weighted average
        (position_similarity * 0.4 + energy_similarity * 0.3 + confidence_similarity * 0.3)
            .max(0.0)
            .min(1.0)
    }

    /// Check for synchronized behavior emergence
    fn check_synchronized_emergence(&self) -> ContextNestResult<bool> {
        let sync_index = self.calculate_synchronization_index();
        Ok(sync_index > self.swarm_params.emergence_threshold)
    }

    /// Calculate synchronization index
    fn calculate_synchronization_index(&self) -> f32 {
        if self.agents.len() < 2 {
            return 0.0;
        }

        // Look for "phase" in internal states
        let phases: Vec<f32> = self
            .agents
            .iter()
            .filter_map(|a| a.state.internal_state.get("phase").copied())
            .collect();

        if phases.len() < 2 {
            return 0.0;
        }

        // Calculate phase coherence
        let mut coherence = 0.0;
        for i in 0..phases.len() {
            for j in (i + 1)..phases.len() {
                coherence += (phases[i] - phases[j]).abs().cos();
            }
        }

        let max_comparisons = phases.len() * (phases.len() - 1) / 2;
        coherence / max_comparisons as f32
    }

    /// Calculate phase coherence
    fn calculate_phase_coherence(&self) -> f32 {
        self.calculate_synchronization_index() // Same calculation for now
    }

    /// Find synchronized agents
    fn find_synchronized_agents(&self) -> Vec<String> {
        self.agents
            .iter()
            .filter(|a| a.state.internal_state.contains_key("phase"))
            .map(|a| a.id.clone())
            .collect()
    }

    /// Check for distributed cognition emergence
    fn check_distributed_cognition_emergence(
        &self,
        field: &NeuralField,
    ) -> ContextNestResult<bool> {
        // Check if knowledge is well-distributed among agents
        let knowledge_entropy = self.calculate_knowledge_entropy();
        let field_coherence = field.state.coherence;

        Ok(knowledge_entropy > 0.5 && field_coherence > 0.6)
    }

    /// Calculate knowledge entropy (distribution)
    fn calculate_knowledge_entropy(&self) -> f32 {
        if self.agents.is_empty() {
            return 0.0;
        }

        // Count different types of internal knowledge
        let mut knowledge_types: HashMap<String, usize> = HashMap::new();
        for agent in &self.agents {
            for key in agent.state.internal_state.keys() {
                *knowledge_types.entry(key.clone()).or_insert(0) += 1;
            }
        }

        // Calculate Shannon entropy
        let total_knowledge: usize = knowledge_types.values().sum();
        if total_knowledge == 0 {
            return 0.0;
        }

        let mut entropy = 0.0;
        for count in knowledge_types.values() {
            let probability = *count as f32 / total_knowledge as f32;
            if probability > 0.0 {
                entropy -= probability * probability.log2();
            }
        }

        // Normalize
        entropy / (self.agents.len() as f32).log2()
    }

    /// Calculate knowledge distribution
    fn calculate_knowledge_distribution(&self) -> HashMap<String, f32> {
        let mut distribution = HashMap::new();
        let total_agents = self.agents.len() as f32;

        for agent in &self.agents {
            for (key, value) in &agent.state.internal_state {
                *distribution.entry(key.clone()).or_insert(0.0) += value / total_agents;
            }
        }

        distribution
    }

    /// Find cognitive agents
    fn find_cognitive_agents(&self) -> Vec<String> {
        self.agents
            .iter()
            .filter(|a| {
                matches!(
                    a.agent_type,
                    AgentType::PatternRecognizer | AgentType::MetaLearner
                )
            })
            .map(|a| a.id.clone())
            .collect()
    }

    /// Calculate field centroid
    fn calculate_field_centroid(&self) -> Vec<f32> {
        if self.agents.is_empty() {
            return vec![0.0; 10]; // Default dimension
        }

        let dim = self.agents[0].state.position.len();
        let mut centroid = vec![0.0; dim];

        for agent in &self.agents {
            for (i, &pos) in agent.state.position.iter().enumerate() {
                centroid[i] += pos;
            }
        }

        let count = self.agents.len() as f32;
        for val in &mut centroid {
            *val /= count;
        }

        centroid
    }

    /// Update metrics
    fn update_metrics(&mut self) {
        // Update agent metrics
        self.metrics.active_agents = self.agents.iter().filter(|a| a.state.energy > 0.1).count();

        self.metrics.avg_agent_energy = if !self.agents.is_empty() {
            self.agents.iter().map(|a| a.state.energy).sum::<f32>() / self.agents.len() as f32
        } else {
            0.0
        };

        // Update swarm coherence
        self.metrics.swarm_coherence = self.calculate_swarm_coordination();

        // Update communication efficiency
        self.metrics.communication_efficiency = self.calculate_communication_efficiency();

        // Update collective intelligence
        self.metrics.collective_intelligence = self.calculate_collective_intelligence();

        // Update collective emergence metrics
        self.collective_emergence.metrics.total_emergences =
            self.collective_emergence.emergence_history.len();
        self.collective_emergence.metrics.active_emergences =
            self.collective_emergence.active_emergences.len();

        if !self.collective_emergence.emergence_history.is_empty() {
            self.collective_emergence.metrics.avg_strength = self
                .collective_emergence
                .emergence_history
                .iter()
                .map(|e| e.strength)
                .sum::<f32>()
                / self.collective_emergence.emergence_history.len() as f32;

            let total_duration: chrono::Duration = self
                .collective_emergence
                .emergence_history
                .iter()
                .filter_map(|e| e.ended_at.map(|end| end - e.started_at))
                .sum();

            let count = self
                .collective_emergence
                .emergence_history
                .iter()
                .filter(|e| e.ended_at.is_some())
                .count();

            if count > 0 {
                self.collective_emergence.metrics.avg_duration = total_duration / count as i32;
            }
        }

        // Calculate emergence rate
        let recent_emergences = self
            .collective_emergence
            .emergence_history
            .iter()
            .filter(|e| (chrono::Utc::now() - e.started_at).num_hours() < 1)
            .count();
        self.collective_emergence.metrics.emergence_rate = recent_emergences as f32;
    }

    /// Calculate communication efficiency
    fn calculate_communication_efficiency(&self) -> f32 {
        if self.interaction_history.is_empty() {
            return 0.5;
        }

        let successful_interactions = self
            .interaction_history
            .iter()
            .filter(|i| matches!(i.outcome, InteractionOutcome::Success { .. }))
            .count();

        successful_interactions as f32 / self.interaction_history.len() as f32
    }

    /// Calculate collective intelligence
    fn calculate_collective_intelligence(&self) -> f32 {
        let coordination = self.calculate_swarm_coordination();
        let communication = self.calculate_communication_efficiency();
        let knowledge_distribution = self.calculate_knowledge_entropy();

        (coordination * 0.4 + communication * 0.3 + knowledge_distribution * 0.3).min(1.0)
    }

    /// Assess knowledge quality in field
    fn assess_knowledge_quality(&self, field: &NeuralField) -> f32 {
        // Simple assessment based on field coherence and pattern strength
        let coherence_quality = field.state.coherence;
        let pattern_quality = if !field.patterns.is_empty() {
            field.patterns.iter().map(|p| p.strength).sum::<f32>() / field.patterns.len() as f32
        } else {
            0.0
        };

        (coherence_quality + pattern_quality) / 2.0
    }

    /// Get metrics
    pub fn get_metrics(&self) -> &MultiAgentMetrics {
        &self.metrics
    }

    /// Get collective emergence tracker
    pub fn get_collective_emergence(&self) -> &CollectiveEmergenceTracker {
        &self.collective_emergence
    }

    /// Export state for analysis
    pub fn export_state(&self) -> MultiAgentFieldState {
        MultiAgentFieldState {
            agents: self.agents.clone(),
            interaction_history: self.interaction_history.clone(),
            swarm_params: self.swarm_params.clone(),
            interaction_patterns: self.interaction_patterns.clone(),
            collective_emergence: self.collective_emergence.clone(),
            metrics: self.metrics.clone(),
        }
    }
}

/// Potential interaction between agents
#[derive(Debug)]
struct PotentialInteraction {
    agent1_id: String,
    agent2_id: String,
    interaction_type: InteractionType,
    distance: f32,
    confidence: f32,
}

/// Exported multi-agent field state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAgentFieldState {
    pub agents: Vec<FieldAgent>,
    pub interaction_history: Vec<AgentInteraction>,
    pub swarm_params: SwarmParameters,
    pub interaction_patterns: Vec<InteractionPattern>,
    pub collective_emergence: CollectiveEmergenceTracker,
    pub metrics: MultiAgentMetrics,
}

impl Default for HarmonicIntegrator {
    fn default() -> Self {
        Self::new(IntegrationStrategy::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::field::NeuralField;

    #[test]
    fn test_multi_agent_field_manager_creation() {
        let swarm_params = SwarmParameters::default();
        let manager = MultiAgentFieldManager::new(swarm_params);

        assert_eq!(manager.agents.len(), 0);
        assert_eq!(manager.metrics.total_agents, 0);
    }

    #[test]
    fn test_agent_creation() {
        let agent = FieldAgent {
            id: "test_agent".to_string(),
            agent_type: AgentType::PatternRecognizer,
            state: AgentState {
                position: vec![0.5, 0.5],
                energy: 0.8,
                confidence: 0.7,
                current_activity: None,
                internal_state: HashMap::new(),
                agent_connections: Vec::new(),
            },
            basin_id: None,
            influence_radius: 0.3,
            goals: Vec::new(),
            communication: CommunicationCapabilities {
                range: 0.5,
                bandwidth: 10,
                message_types: vec![MessageType::PatternShare {
                    pattern_id: "test".to_string(),
                }],
                signal_strength: 0.7,
            },
            learning_params: AgentLearningParams {
                learning_rate: 0.01,
                exploration_rate: 0.1,
                memory_capacity: 100,
                adaptation_speed: 0.05,
                social_learning_factor: 0.2,
            },
            created_at: chrono::Utc::now(),
            last_active: chrono::Utc::now(),
        };

        assert_eq!(agent.id, "test_agent");
        assert_eq!(agent.agent_type, AgentType::PatternRecognizer);
        assert_eq!(agent.state.energy, 0.8);
    }

    #[test]
    fn test_swarm_parameters_default() {
        let params = SwarmParameters::default();
        assert_eq!(params.alignment_factor, 0.1);
        assert_eq!(params.cohesion_factor, 0.05);
        assert_eq!(params.separation_factor, 0.15);
        assert_eq!(params.emergence_threshold, 0.7);
    }

    #[test]
    fn test_interaction_types() {
        let cooperation = InteractionType::Cooperation {
            goal_id: "test_goal".to_string(),
        };
        let knowledge_exchange = InteractionType::KnowledgeExchange;
        let sync = InteractionType::Synchronization { frequency: 1.0 };

        assert_ne!(cooperation, knowledge_exchange);
        assert_eq!(sync, InteractionType::Synchronization { frequency: 1.0 });
    }

    #[test]
    fn test_collective_emergence_types() {
        let swarm = CollectiveEmergenceType::SwarmIntelligence {
            collective_behavior: "test".to_string(),
            coordination_level: 0.8,
        };
        let cognition = CollectiveEmergenceType::DistributedCognition {
            shared_understanding: 0.7,
            knowledge_distribution: HashMap::new(),
        };

        assert_ne!(swarm, cognition);
    }

    #[test]
    fn test_add_agent() {
        let mut manager = MultiAgentFieldManager::new(SwarmParameters::default());

        let agent = FieldAgent {
            id: "test_agent".to_string(),
            agent_type: AgentType::PatternRecognizer,
            state: AgentState {
                position: vec![0.0; 10],
                energy: 0.5,
                confidence: 0.6,
                current_activity: None,
                internal_state: HashMap::new(),
                agent_connections: Vec::new(),
            },
            basin_id: None,
            influence_radius: 0.2,
            goals: Vec::new(),
            communication: CommunicationCapabilities {
                range: 0.3,
                bandwidth: 5,
                message_types: Vec::new(),
                signal_strength: 0.5,
            },
            learning_params: AgentLearningParams {
                learning_rate: 0.1,
                exploration_rate: 0.2,
                memory_capacity: 100,
                adaptation_speed: 0.1,
                social_learning_factor: 0.3,
            },
            created_at: chrono::Utc::now(),
            last_active: chrono::Utc::now(),
        };

        let result = manager.add_agent(agent);
        assert!(result.is_ok());
        assert_eq!(manager.agents.len(), 1);
        assert_eq!(manager.metrics.total_agents, 1);
    }

    #[test]
    fn test_add_duplicate_agent() {
        let mut manager = MultiAgentFieldManager::new(SwarmParameters::default());

        let agent1 = FieldAgent {
            id: "duplicate".to_string(),
            agent_type: AgentType::PatternRecognizer,
            state: AgentState {
                position: vec![0.0; 10],
                energy: 0.5,
                confidence: 0.6,
                current_activity: None,
                internal_state: HashMap::new(),
                agent_connections: Vec::new(),
            },
            basin_id: None,
            influence_radius: 0.2,
            goals: Vec::new(),
            communication: CommunicationCapabilities {
                range: 0.3,
                bandwidth: 5,
                message_types: Vec::new(),
                signal_strength: 0.5,
            },
            learning_params: AgentLearningParams {
                learning_rate: 0.1,
                exploration_rate: 0.2,
                memory_capacity: 100,
                adaptation_speed: 0.1,
                social_learning_factor: 0.3,
            },
            created_at: chrono::Utc::now(),
            last_active: chrono::Utc::now(),
        };

        let agent2 = FieldAgent {
            id: "duplicate".to_string(), // Same ID
            agent_type: AgentType::HarmonyOptimizer,
            state: AgentState {
                position: vec![0.5; 10],
                energy: 0.7,
                confidence: 0.8,
                current_activity: None,
                internal_state: HashMap::new(),
                agent_connections: Vec::new(),
            },
            basin_id: None,
            influence_radius: 0.3,
            goals: Vec::new(),
            communication: CommunicationCapabilities {
                range: 0.4,
                bandwidth: 8,
                message_types: Vec::new(),
                signal_strength: 0.6,
            },
            learning_params: AgentLearningParams {
                learning_rate: 0.1,
                exploration_rate: 0.2,
                memory_capacity: 100,
                adaptation_speed: 0.1,
                social_learning_factor: 0.3,
            },
            created_at: chrono::Utc::now(),
            last_active: chrono::Utc::now(),
        };

        assert!(manager.add_agent(agent1).is_ok());
        assert!(manager.add_agent(agent2).is_err()); // Should fail
        assert_eq!(manager.agents.len(), 1);
    }
}

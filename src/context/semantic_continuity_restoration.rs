//! Semantic Continuity Restoration for Memory Reconstruction
//! This module implements sophisticated semantic continuity restoration mechanisms
//! that ensure reconstructed memories maintain logical flow and semantic coherence
//! across fragmented reconstructions.

use crate::context::field::{NeuralField, SemanticPattern};
use crate::context::historical_state_recovery::HistoricalStateRecovery;
use crate::context::memory::{MemoryAttractor, MemoryOrchestrator};
use crate::context::memory_reconstruction::{
    ContinuityEdge, ContinuityNode, ContinuitySession, ReconstructionFragment,
    ReconstructionSession,
};
use crate::error::ContextNestResult;
use crate::{ContextNestError, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

/// Semantic continuity restoration manager
#[derive(Debug, Clone)]
pub struct SemanticContinuityRestoration {
    /// Restoration configuration
    config: RestorationConfig,
    /// Semantic graph builder
    graph_builder: SemanticGraphBuilder,
    /// Continuity analyzer
    continuity_analyzer: ContinuityAnalyzer,
    /// Flow optimizer
    flow_optimizer: FlowOptimizer,
    /// Restoration metrics
    metrics: RestorationMetrics,
}

/// Configuration for semantic continuity restoration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorationConfig {
    /// Minimum semantic similarity for connections
    pub min_semantic_similarity: f32,
    /// Minimum logical coherence threshold
    pub min_logical_coherence: f32,
    /// Enable temporal flow optimization
    pub enable_temporal_optimization: bool,
    /// Enable causal relationship detection
    pub enable_causal_detection: bool,
    /// Enable thematic continuity analysis
    pub enable_thematic_analysis: bool,
    /// Maximum continuity breaks allowed
    pub max_continuity_breaks: usize,
    /// Restoration strength (0.0-1.0)
    pub restoration_strength: f32,
    /// Enable cross-session continuity
    pub enable_cross_session_continuity: bool,
}

impl Default for RestorationConfig {
    fn default() -> Self {
        Self {
            min_semantic_similarity: 0.6,
            min_logical_coherence: 0.7,
            enable_temporal_optimization: true,
            enable_causal_detection: true,
            enable_thematic_analysis: true,
            max_continuity_breaks: 3,
            restoration_strength: 0.8,
            enable_cross_session_continuity: true,
        }
    }
}

/// Semantic graph builder for continuity reconstruction
#[derive(Debug, Clone)]
pub struct SemanticGraphBuilder {
    /// Graph nodes (semantic concepts)
    nodes: HashMap<String, SemanticNode>,
    /// Graph edges (semantic relationships)
    edges: HashMap<String, SemanticEdge>,
    /// Graph metrics
    metrics: GraphMetrics,
}

/// Semantic node in the continuity graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticNode {
    /// Node ID
    pub id: String,
    /// Node content
    pub content: String,
    /// Semantic embedding
    pub embedding: Vec<f32>,
    /// Node type
    pub node_type: SemanticNodeType,
    /// Importance score
    pub importance: f32,
    /// Thematic category
    pub thematic_category: Option<String>,
    /// Temporal position
    pub temporal_position: Option<usize>,
    /// Associated fragment IDs
    pub fragment_ids: Vec<String>,
    /// Node properties
    pub properties: HashMap<String, serde_json::Value>,
}

/// Types of semantic nodes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SemanticNodeType {
    /// Core concept
    Concept,
    /// Action or process
    Action,
    /// Entity or object
    Entity,
    /// Relationship or connection
    Relationship,
    /// Attribute or property
    Attribute,
    /// Event or occurrence
    Event,
    /// State or condition
    State,
    /// Transition or change
    Transition,
}

/// Semantic edge in the continuity graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEdge {
    /// Edge ID
    pub id: String,
    /// Source node ID
    pub source_id: String,
    /// Target node ID
    pub target_id: String,
    /// Edge type
    pub edge_type: SemanticEdgeType,
    /// Relationship strength
    pub strength: f32,
    /// Directionality (bidirectional if false)
    pub directional: bool,
    /// Temporal relationship
    pub temporal_relationship: Option<TemporalRelation>,
    /// Confidence in edge
    pub confidence: f32,
}

/// Types of semantic edges
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SemanticEdgeType {
    /// Semantic similarity
    Similarity,
    /// Causal relationship
    Causal,
    /// Temporal sequence
    Sequential,
    /// Hierarchical relationship
    Hierarchical,
    /// Thematic connection
    Thematic,
    /// Logical implication
    Implication,
    /// Contrast or opposition
    Contrast,
    /// Exemplification
    Exemplification,
}

/// Temporal relationship between nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalRelation {
    /// Relation type
    pub relation_type: TemporalRelationType,
    /// Time difference
    pub time_difference: Option<Duration>,
    /// Confidence in temporal relation
    pub confidence: f32,
}

/// Types of temporal relations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TemporalRelationType {
    /// Before
    Before,
    /// After
    After,
    /// Concurrent
    Concurrent,
    /// Overlapping
    Overlapping,
}

/// Graph metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphMetrics {
    /// Total nodes
    pub total_nodes: usize,
    /// Total edges
    pub total_edges: usize,
    /// Graph density
    pub density: f32,
    /// Average node degree
    pub avg_node_degree: f32,
    /// Connected components
    pub connected_components: usize,
    /// Graph diameter
    pub diameter: Option<usize>,
}

/// Continuity analyzer for detecting breaks and inconsistencies
#[derive(Debug, Clone)]
pub struct ContinuityAnalyzer {
    /// Break detector
    break_detector: ContinuityBreakDetector,
    /// Inconsistency detector
    inconsistency_detector: InconsistencyDetector,
    /// Flow analyzer
    flow_analyzer: SemanticFlowAnalyzer,
}

/// Continuity break detector
#[derive(Debug, Clone)]
pub struct ContinuityBreakDetector {
    /// Break types to detect
    break_types: Vec<ContinuityBreakType>,
    /// Detection thresholds
    thresholds: BreakDetectionThresholds,
}

/// Types of continuity breaks
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ContinuityBreakType {
    /// Semantic discontinuity
    Semantic,
    /// Logical inconsistency
    Logical,
    /// Temporal discontinuity
    Temporal,
    /// Thematic shift
    Thematic,
    /// Narrative break
    Narrative,
    /// Causal gap
    Causal,
}

/// Break detection thresholds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakDetectionThresholds {
    /// Semantic similarity threshold
    pub semantic_threshold: f32,
    /// Logical coherence threshold
    pub logical_threshold: f32,
    /// Temporal gap threshold (hours)
    pub temporal_threshold: i64,
    /// Thematic similarity threshold
    pub thematic_threshold: f32,
}

/// Detected continuity break
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuityBreak {
    /// Break ID
    pub id: String,
    /// Break type
    pub break_type: ContinuityBreakType,
    /// Location (between nodes)
    pub location: BreakLocation,
    /// Break severity
    pub severity: f32,
    /// Suggested repairs
    pub suggested_repairs: Vec<RepairStrategy>,
    /// Detection confidence
    pub confidence: f32,
}

/// Location of continuity break
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakLocation {
    /// Before node ID
    pub before_node_id: Option<String>,
    /// After node ID
    pub after_node_id: Option<String>,
    /// Position in sequence
    pub sequence_position: Option<usize>,
}

/// Repair strategy for continuity breaks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairStrategy {
    /// Strategy type
    pub strategy_type: RepairStrategyType,
    /// Expected effectiveness
    pub effectiveness: f32,
    /// Implementation complexity
    pub complexity: RepairComplexity,
    /// Required resources
    pub required_resources: Vec<String>,
}

/// Types of repair strategies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RepairStrategyType {
    /// Insert bridging content
    InsertBridge,
    /// Reorder sequence
    ReorderSequence,
    /// Strengthen weak connections
    StrengthenConnection,
    /// Split complex node
    SplitNode,
    /// Merge similar nodes
    MergeNodes,
    /// Reword content
    RewordContent,
    /// Add contextual information
    AddContext,
}

/// Repair complexity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RepairComplexity {
    Simple,
    Moderate,
    Complex,
    VeryComplex,
}

/// Inconsistency detector
#[derive(Debug, Clone)]
pub struct InconsistencyDetector {
    /// Inconsistency types
    inconsistency_types: Vec<InconsistencyType>,
    /// Detection rules
    rules: Vec<InconsistencyRule>,
}

/// Types of inconsistencies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InconsistencyType {
    /// Contradictory statements
    Contradiction,
    /// Inconsistent terminology
    Terminology,
    /// Logical paradox
    Paradox,
    /// Factual inconsistency
    Factual,
    /// Temporal inconsistency
    Temporal,
    /// Causal inconsistency
    Causal,
}

/// Inconsistency detection rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InconsistencyRule {
    /// Rule ID
    pub id: String,
    /// Rule type
    pub rule_type: InconsistencyType,
    /// Rule pattern
    pub pattern: String,
    /// Rule weight
    pub weight: f32,
}

/// Detected inconsistency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inconsistency {
    /// Inconsistency ID
    pub id: String,
    /// Inconsistency type
    pub inconsistency_type: InconsistencyType,
    /// Affected nodes
    pub affected_nodes: Vec<String>,
    /// Description
    pub description: String,
    /// Severity
    pub severity: f32,
    /// Resolution strategies
    pub resolution_strategies: Vec<ResolutionStrategy>,
}

/// Resolution strategy for inconsistencies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionStrategy {
    /// Strategy description
    pub description: String,
    /// Expected success rate
    pub success_rate: f32,
    /// Implementation steps
    pub implementation_steps: Vec<String>,
}

/// Semantic flow analyzer
#[derive(Debug, Clone)]
pub struct SemanticFlowAnalyzer {
    /// Flow patterns
    flow_patterns: Vec<FlowPattern>,
    /// Flow metrics
    metrics: FlowMetrics,
}

/// Flow pattern in semantic space
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowPattern {
    /// Pattern ID
    pub id: String,
    /// Pattern type
    pub pattern_type: FlowPatternType,
    /// Start and end points
    pub start_point: Vec<f32>,
    pub end_point: Vec<f32>,
    /// Flow strength
    pub strength: f32,
    /// Flow characteristics
    pub characteristics: FlowCharacteristics,
}

/// Types of flow patterns
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FlowPatternType {
    /// Linear progression
    Linear,
    /// Divergent exploration
    Divergent,
    /// Convergent synthesis
    Convergent,
    /// Circular or cyclic
    Circular,
    /// Spiral progression
    Spiral,
    /// Chaotic or random
    Chaotic,
}

/// Flow characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowCharacteristics {
    /// Flow smoothness
    pub smoothness: f32,
    /// Flow consistency
    pub consistency: f32,
    /// Flow predictability
    pub predictability: f32,
    /// Flow complexity
    pub complexity: f32,
}

/// Flow metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowMetrics {
    /// Overall flow coherence
    pub coherence: f32,
    /// Flow smoothness
    pub smoothness: f32,
    /// Flow predictability
    pub predictability: f32,
    /// Number of flow breaks
    pub flow_breaks: usize,
    /// Average segment length
    pub avg_segment_length: f32,
}

/// Flow optimizer for improving semantic continuity
#[derive(Debug, Clone)]
pub struct FlowOptimizer {
    /// Optimization strategies
    strategies: Vec<OptimizationStrategy>,
    /// Optimization parameters
    parameters: OptimizationParameters,
}

/// Optimization strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationStrategy {
    /// Strategy ID
    pub id: String,
    /// Strategy type
    pub strategy_type: OptimizationStrategyType,
    /// Strategy parameters
    pub parameters: HashMap<String, serde_json::Value>,
    /// Expected improvement
    pub expected_improvement: f32,
}

/// Types of optimization strategies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OptimizationStrategyType {
    /// Reordering optimization
    Reordering,
    /// Connection strengthening
    ConnectionStrengthening,
    /// Gap filling
    GapFilling,
    /// Node splitting
    NodeSplitting,
    /// Node merging
    NodeMerging,
    /// Content rewriting
    ContentRewriting,
    /// Bridge insertion
    BridgeInsertion,
}

/// Optimization parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationParameters {
    /// Maximum iterations
    pub max_iterations: usize,
    /// Convergence threshold
    pub convergence_threshold: f32,
    /// Optimization weight
    pub optimization_weight: f32,
    /// Preserve original content
    pub preserve_original: bool,
}

/// Restoration metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorationMetrics {
    /// Total restorations attempted
    pub total_restorations: usize,
    /// Successful restorations
    pub successful_restorations: usize,
    /// Average continuity score improvement
    pub avg_continuity_improvement: f32,
    /// Average semantic coherence improvement
    pub avg_coherence_improvement: f32,
    /// Breaks detected
    pub breaks_detected: usize,
    /// Breaks repaired
    pub breaks_repaired: usize,
    /// Inconsistencies resolved
    pub inconsistencies_resolved: usize,
}

/// Result of semantic continuity restoration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorationResult {
    /// Success status
    pub success: bool,
    /// Original continuity score
    pub original_continuity_score: f32,
    /// Restored continuity score
    pub restored_continuity_score: f32,
    /// Improvement percentage
    pub improvement_percentage: f32,
    /// Breaks detected and repaired
    pub breaks_repaired: Vec<ContinuityBreak>,
    /// Inconsistencies resolved
    pub inconsistencies_resolved: Vec<Inconsistency>,
    /// Modified fragments
    pub modified_fragments: Vec<String>,
    /// Added bridging content
    pub bridging_content: Vec<BridgingContent>,
    /// Restoration quality metrics
    pub quality_metrics: RestorationQualityMetrics,
    /// Processing time
    pub processing_time_ms: i64,
}

/// Bridging content added for continuity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgingContent {
    /// Content ID
    pub id: String,
    /// Content text
    pub content: String,
    /// Position in sequence
    pub position: usize,
    /// Content type
    pub content_type: BridgingContentType,
    /// Confidence in appropriateness
    pub confidence: f32,
}

/// Types of bridging content
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BridgingContentType {
    /// Transition phrase
    Transition,
    /// Explanatory text
    Explanation,
    /// Causal link
    CausalLink,
    /// Temporal marker
    TemporalMarker,
    /// Thematic connector
    ThematicConnector,
}

/// Restoration quality metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorationQualityMetrics {
    /// Overall quality score (0.0-1.0)
    pub overall_quality: f32,
    /// Semantic coherence score
    pub semantic_coherence: f32,
    /// Logical consistency score
    pub logical_consistency: f32,
    /// Temporal flow score
    pub temporal_flow: f32,
    /// Thematic unity score
    pub thematic_unity: f32,
    /// Narrative structure score
    pub narrative_structure: f32,
}

impl SemanticContinuityRestoration {
    /// Create a new semantic continuity restoration manager
    pub fn new(config: RestorationConfig) -> Self {
        Self {
            graph_builder: SemanticGraphBuilder::new(),
            continuity_analyzer: ContinuityAnalyzer::new(),
            flow_optimizer: FlowOptimizer::new(),
            metrics: RestorationMetrics::default(),
            config,
        }
    }

    /// Restore semantic continuity for a reconstruction session
    pub fn restore_continuity(
        &mut self,
        session: &mut ReconstructionSession,
        field: &NeuralField,
        orchestrator: &MemoryOrchestrator,
    ) -> ContextNestResult<RestorationResult> {
        let start_time = Utc::now();

        // Calculate original continuity score
        let original_score = self.calculate_continuity_score(&session.fragments);

        // Build semantic graph from fragments
        let graph = self
            .graph_builder
            .build_from_fragments(&session.fragments)?;

        // Detect continuity breaks
        let breaks = self.continuity_analyzer.detect_breaks(&graph)?;

        // Detect inconsistencies
        let inconsistencies = self.continuity_analyzer.detect_inconsistencies(&graph)?;

        // Optimize semantic flow
        let optimized_graph = self.flow_optimizer.optimize_flow(graph, &self.config)?;

        // Apply repairs and optimizations
        let (modified_fragments, bridging_content) =
            self.apply_repairs(&breaks, &inconsistencies, &optimized_graph, session)?;

        // Update fragments in session
        self.update_session_fragments(session, &modified_fragments, &bridging_content)?;

        // Calculate restored continuity score
        let restored_score = self.calculate_continuity_score(&session.fragments);

        let improvement = if original_score > 0.0 {
            ((restored_score - original_score) / original_score) * 100.0
        } else {
            0.0
        };

        let quality_metrics = self.calculate_restoration_quality(
            original_score,
            restored_score,
            &breaks,
            &inconsistencies,
        );

        // Update metrics before moving values
        let breaks_count = breaks.len();
        let inconsistencies_count = inconsistencies.len();

        let result = RestorationResult {
            success: restored_score > original_score,
            original_continuity_score: original_score,
            restored_continuity_score: restored_score,
            improvement_percentage: improvement,
            breaks_repaired: breaks,
            inconsistencies_resolved: inconsistencies,
            modified_fragments: modified_fragments.iter().map(|f| f.id.clone()).collect(),
            bridging_content,
            quality_metrics,
            processing_time_ms: (Utc::now() - start_time).num_milliseconds(),
        };

        // Update metrics
        self.metrics.total_restorations += 1;
        if result.success {
            self.metrics.successful_restorations += 1;
        }
        self.metrics.breaks_detected += breaks_count;
        self.metrics.breaks_repaired += breaks_count;
        self.metrics.inconsistencies_resolved += inconsistencies_count;

        Ok(result)
    }

    /// Calculate continuity score for fragments
    fn calculate_continuity_score(&self, fragments: &[ReconstructionFragment]) -> f32 {
        if fragments.len() < 2 {
            return 1.0;
        }

        let mut total_similarity = 0.0;
        let mut total_logical_flow = 0.0;
        let mut comparisons = 0;

        // Sort fragments by position
        let mut positioned_fragments: Vec<_> = fragments
            .iter()
            .filter(|f| f.temporal_info.sequence_position.is_some())
            .collect();

        positioned_fragments.sort_by_key(|f| f.temporal_info.sequence_position.unwrap());

        // Compare adjacent fragments
        for window in positioned_fragments.windows(2) {
            let fragment_a = window[0];
            let fragment_b = window[1];

            // Semantic similarity
            let similarity = self.cosine_similarity(&fragment_a.embedding, &fragment_b.embedding);
            total_similarity += similarity;

            // Logical flow (simplified - would use more sophisticated analysis)
            let logical_flow = self.calculate_logical_flow(fragment_a, fragment_b);
            total_logical_flow += logical_flow;

            comparisons += 1;
        }

        if comparisons == 0 {
            return 1.0;
        }

        let avg_similarity = total_similarity / comparisons as f32;
        let avg_logical_flow = total_logical_flow / comparisons as f32;

        // Combined continuity score
        (avg_similarity * 0.6 + avg_logical_flow * 0.4).min(1.0)
    }

    /// Calculate logical flow between fragments
    fn calculate_logical_flow(
        &self,
        fragment_a: &ReconstructionFragment,
        fragment_b: &ReconstructionFragment,
    ) -> f32 {
        let content_a = fragment_a.content.to_lowercase();
        let content_b = fragment_b.content.to_lowercase();

        // Check for logical connectors
        let logical_connectors_a = [
            "therefore",
            "thus",
            "consequently",
            "as a result",
            "hence",
            "because",
            "since",
            "due to",
            "as",
            "for",
            "however",
            "but",
            "although",
            "despite",
            "whereas",
            "additionally",
            "furthermore",
            "moreover",
            "also",
            "in addition",
            "first",
            "second",
            "then",
            "next",
            "finally",
            "lastly",
        ];

        let logical_connectors_b = [
            "this",
            "that",
            "these",
            "those",
            "it",
            "they",
            "such",
            "therefore",
            "thus",
            "consequently",
        ];

        let mut flow_score = 0.0;

        // Check if fragment A ends with logical connector
        for connector in &logical_connectors_a {
            if content_a.ends_with(connector) || content_a.contains(&format!("{} ", connector)) {
                flow_score += 0.3;
                break;
            }
        }

        // Check if fragment B starts with logical connector
        for connector in &logical_connectors_b {
            if content_b.starts_with(connector) || content_b.contains(&format!("{} ", connector)) {
                flow_score += 0.3;
                break;
            }
        }

        // Check for temporal sequence indicators
        let temporal_indicators = [
            "first", "then", "next", "after", "before", "during", "while",
        ];
        for indicator in &temporal_indicators {
            if content_a.contains(indicator) || content_b.contains(indicator) {
                flow_score += 0.2;
                break;
            }
        }

        // Check for referential continuity
        let referential_indicators = ["it", "this", "that", "they", "these", "those"];
        for indicator in &referential_indicators {
            if content_b.starts_with(indicator) {
                flow_score += 0.2;
                break;
            }
        }

        (flow_score as f32).min(1.0)
    }

    /// Apply repairs to fragments
    fn apply_repairs(
        &mut self,
        breaks: &[ContinuityBreak],
        inconsistencies: &[Inconsistency],
        optimized_graph: &SemanticGraph,
        session: &mut ReconstructionSession,
    ) -> ContextNestResult<(Vec<ReconstructionFragment>, Vec<BridgingContent>)> {
        let mut modified_fragments = Vec::new();
        let mut bridging_content = Vec::new();

        // Apply break repairs
        for break_info in breaks {
            if let Some(bridge) = self.create_bridging_content(break_info, session)? {
                bridging_content.push(bridge);
            }
        }

        // Apply inconsistency resolutions
        for inconsistency in inconsistencies {
            if let Some(modified_fragment) = self.resolve_inconsistency(inconsistency, session)? {
                modified_fragments.push(modified_fragment);
            }
        }

        // Apply graph optimizations
        let optimized_fragments = self.apply_graph_optimizations(optimized_graph, session)?;
        modified_fragments.extend(optimized_fragments);

        Ok((modified_fragments, bridging_content))
    }

    /// Create bridging content for continuity breaks
    fn create_bridging_content(
        &mut self,
        break_info: &ContinuityBreak,
        session: &ReconstructionSession,
    ) -> ContextNestResult<Option<BridgingContent>> {
        let location = &break_info.location;

        // Find fragments before and after the break
        let before_fragment = location
            .before_node_id
            .as_ref()
            .and_then(|id| session.fragments.iter().find(|f| f.id == *id));

        let after_fragment = location
            .after_node_id
            .as_ref()
            .and_then(|id| session.fragments.iter().find(|f| f.id == *id));

        if let (Some(before), Some(after)) = (before_fragment, after_fragment) {
            let content = match break_info.break_type {
                ContinuityBreakType::Semantic => {
                    format!(
                        "[Semantic bridge: connecting related concepts between: {} and {}]",
                        &before.content[..before.content.len().min(50)],
                        &after.content[..after.content.len().min(50)]
                    )
                }
                ContinuityBreakType::Logical => {
                    "[Logical connection: establishing relationship between concepts]".to_string()
                }
                ContinuityBreakType::Temporal => {
                    "[Temporal transition: indicating time progression]".to_string()
                }
                ContinuityBreakType::Thematic => {
                    "[Thematic connection: maintaining theme consistency]".to_string()
                }
                ContinuityBreakType::Narrative => {
                    "[Narrative bridge: ensuring story flow]".to_string()
                }
                ContinuityBreakType::Causal => {
                    "[Causal link: explaining cause-effect relationship]".to_string()
                }
            };

            let position = location.sequence_position.unwrap_or(0);

            Ok(Some(BridgingContent {
                id: Uuid::new_v4().to_string(),
                content,
                position,
                content_type: BridgingContentType::Transition,
                confidence: break_info.confidence,
            }))
        } else {
            Ok(None)
        }
    }

    /// Resolve inconsistency
    fn resolve_inconsistency(
        &mut self,
        inconsistency: &Inconsistency,
        session: &mut ReconstructionSession,
    ) -> ContextNestResult<Option<ReconstructionFragment>> {
        // Find affected fragments
        let mut affected_fragments: Vec<_> = session
            .fragments
            .iter_mut()
            .filter(|f| inconsistency.affected_nodes.contains(&f.id))
            .collect();

        if affected_fragments.is_empty() {
            return Ok(None);
        }

        // Apply resolution strategy. We annotate only the first affected
        // fragment as a representative — the original code used a `for`
        // loop with an unconditional `return` inside, which clippy
        // correctly flagged as "loop never actually loops". This explicit
        // `if let` makes the single-fragment semantics intentional.
        if let Some(fragment) = affected_fragments.iter_mut().next() {
            match inconsistency.inconsistency_type {
                InconsistencyType::Contradiction => {
                    fragment.content =
                        format!("[Note: Potential contradiction] {}", fragment.content);
                }
                InconsistencyType::Terminology => {
                    fragment.content = format!("[Terminology note] {}", fragment.content);
                }
                InconsistencyType::Temporal => {
                    fragment.content = format!("[Temporal clarification] {}", fragment.content);
                }
                _ => {
                    fragment.content = format!("[Inconsistency detected] {}", fragment.content);
                }
            }
            return Ok(Some(fragment.clone()));
        }

        Ok(None)
    }

    /// Apply graph optimizations
    fn apply_graph_optimizations(
        &mut self,
        optimized_graph: &SemanticGraph,
        session: &mut ReconstructionSession,
    ) -> ContextNestResult<Vec<ReconstructionFragment>> {
        let mut optimized_fragments = Vec::new();

        // Reorder fragments based on optimized graph
        let reordered_ids = self.get_optimized_fragment_order(optimized_graph)?;

        // Create reordered fragments
        for (position, fragment_id) in reordered_ids.iter().enumerate() {
            if let Some(fragment) = session.fragments.iter_mut().find(|f| &f.id == fragment_id) {
                fragment.temporal_info.sequence_position = Some(position);
                optimized_fragments.push(fragment.clone());
            }
        }

        Ok(optimized_fragments)
    }

    /// Get optimized fragment order from graph
    fn get_optimized_fragment_order(
        &self,
        graph: &SemanticGraph,
    ) -> ContextNestResult<Vec<String>> {
        // Simple topological sort based on graph structure
        let mut visited = HashSet::new();
        let mut order = Vec::new();
        let nodes: Vec<_> = graph.nodes.values().collect();

        // Sort nodes by temporal position if available, then by importance
        let mut sorted_nodes: Vec<_> = nodes.iter().collect();
        sorted_nodes.sort_by(|a, b| match (a.temporal_position, b.temporal_position) {
            (Some(pos_a), Some(pos_b)) => pos_a.cmp(&pos_b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b
                .importance
                .partial_cmp(&a.importance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .reverse(),
        });

        for node in sorted_nodes {
            if !visited.contains(&node.id) {
                self.visit_node(&node.id, graph, &mut visited, &mut order)?;
            }
        }

        Ok(order)
    }

    /// Visit node for topological sort
    fn visit_node(
        &self,
        node_id: &str,
        graph: &SemanticGraph,
        visited: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) -> ContextNestResult<()> {
        if visited.contains(node_id) {
            return Ok(());
        }

        visited.insert(node_id.to_string());

        // Visit dependencies (incoming edges for sequential relationships)
        for edge in graph.edges.values() {
            if edge.target_id == node_id && matches!(edge.edge_type, SemanticEdgeType::Sequential) {
                self.visit_node(&edge.source_id, graph, visited, order)?;
            }
        }

        order.push(node_id.to_string());
        Ok(())
    }

    /// Update session fragments with modifications
    fn update_session_fragments(
        &mut self,
        session: &mut ReconstructionSession,
        modified_fragments: &[ReconstructionFragment],
        bridging_content: &[BridgingContent],
    ) -> ContextNestResult<()> {
        // Update modified fragments
        for modified in modified_fragments {
            if let Some(fragment) = session.fragments.iter_mut().find(|f| f.id == modified.id) {
                *fragment = modified.clone();
            }
        }

        // Add bridging content as new fragments
        for bridge in bridging_content {
            let bridge_fragment = ReconstructionFragment {
                id: bridge.id.clone(),
                source_attractor_id: format!("bridge_{}", bridge.id),
                content: bridge.content.clone(),
                embedding: vec![0.0; 384], // Placeholder embedding
                strength: 0.6,
                confidence: bridge.confidence,
                position: Some(bridge.position),
                connections: Vec::new(),
                temporal_info: TemporalInfo {
                    created_at: Utc::now(),
                    sequence_position: Some(bridge.position),
                    temporal_relationships: Vec::new(),
                },
            };

            session.fragments.push(bridge_fragment);
        }

        // Re-sort fragments by position
        session
            .fragments
            .sort_by_key(|f| f.temporal_info.sequence_position.unwrap_or(usize::MAX));

        Ok(())
    }

    /// Calculate restoration quality metrics
    fn calculate_restoration_quality(
        &self,
        original_score: f32,
        restored_score: f32,
        breaks: &[ContinuityBreak],
        inconsistencies: &[Inconsistency],
    ) -> RestorationQualityMetrics {
        let improvement = restored_score - original_score;
        let overall_quality = if original_score > 0.0 {
            (restored_score / original_score).min(2.0) / 2.0 // Normalize to 0-1
        } else {
            restored_score
        };

        let semantic_coherence = restored_score; // Simplified
        let logical_consistency = 1.0 - (inconsistencies.len() as f32 / 10.0).min(1.0);
        let temporal_flow = 1.0
            - (breaks
                .iter()
                .filter(|b| matches!(b.break_type, ContinuityBreakType::Temporal))
                .count() as f32
                / 10.0)
                .min(1.0);
        let thematic_unity = 1.0
            - (breaks
                .iter()
                .filter(|b| matches!(b.break_type, ContinuityBreakType::Thematic))
                .count() as f32
                / 10.0)
                .min(1.0);
        let narrative_structure = 1.0
            - (breaks
                .iter()
                .filter(|b| matches!(b.break_type, ContinuityBreakType::Narrative))
                .count() as f32
                / 10.0)
                .min(1.0);

        RestorationQualityMetrics {
            overall_quality,
            semantic_coherence,
            logical_consistency,
            temporal_flow,
            thematic_unity,
            narrative_structure,
        }
    }

    /// Calculate cosine similarity between vectors
    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            (dot_product / (norm_a * norm_b)).max(0.0).min(1.0)
        }
    }

    /// Get restoration metrics
    pub fn get_metrics(&self) -> &RestorationMetrics {
        &self.metrics
    }
}

impl SemanticGraphBuilder {
    /// Create a new semantic graph builder
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            metrics: GraphMetrics::default(),
        }
    }

    /// Build semantic graph from fragments
    pub fn build_from_fragments(
        &mut self,
        fragments: &[ReconstructionFragment],
    ) -> ContextNestResult<SemanticGraph> {
        // Create nodes from fragments
        for fragment in fragments {
            let node = SemanticNode {
                id: fragment.id.clone(),
                content: fragment.content.clone(),
                embedding: fragment.embedding.clone(),
                node_type: Self::classify_node_type(&fragment.content),
                importance: fragment.strength * fragment.confidence,
                thematic_category: Self::extract_thematic_category(&fragment.content),
                temporal_position: fragment.temporal_info.sequence_position,
                fragment_ids: vec![fragment.id.clone()],
                properties: HashMap::new(),
            };

            self.nodes.insert(fragment.id.clone(), node);
        }

        // Create edges between nodes
        self.create_edges_between_nodes()?;

        // Calculate graph metrics
        self.calculate_metrics();

        Ok(SemanticGraph {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            metrics: self.metrics.clone(),
        })
    }

    /// Classify node type from content
    fn classify_node_type(content: &str) -> SemanticNodeType {
        let content_lower = content.to_lowercase();

        if content_lower.contains("is")
            || content_lower.contains("are")
            || content_lower.contains("concept")
        {
            SemanticNodeType::Concept
        } else if content_lower.contains("do")
            || content_lower.contains("did")
            || content_lower.contains("process")
        {
            SemanticNodeType::Action
        } else if content_lower.contains("when")
            || content_lower.contains("happened")
            || content_lower.contains("occurred")
        {
            SemanticNodeType::Event
        } else if content_lower.contains("because")
            || content_lower.contains("since")
            || content_lower.contains("due to")
        {
            SemanticNodeType::Relationship
        } else if content_lower.contains("has")
            || content_lower.contains("have")
            || content_lower.contains("contains")
        {
            SemanticNodeType::Attribute
        } else {
            SemanticNodeType::State
        }
    }

    /// Extract thematic category from content
    fn extract_thematic_category(content: &str) -> Option<String> {
        // Simple thematic classification
        let themes = [
            (
                "technical",
                vec!["algorithm", "code", "system", "function", "method"],
            ),
            (
                "business",
                vec!["market", "customer", "product", "service", "revenue"],
            ),
            (
                "scientific",
                vec!["research", "experiment", "theory", "hypothesis", "data"],
            ),
            (
                "educational",
                vec!["learn", "teach", "student", "knowledge", "skill"],
            ),
        ];

        let content_lower = content.to_lowercase();

        for (theme, keywords) in &themes {
            for keyword in keywords {
                if content_lower.contains(keyword) {
                    return Some(theme.to_string());
                }
            }
        }

        None
    }

    /// Create edges between nodes
    fn create_edges_between_nodes(&mut self) -> ContextNestResult<()> {
        let node_ids: Vec<_> = self.nodes.keys().cloned().collect();

        for i in 0..node_ids.len() {
            for j in (i + 1)..node_ids.len() {
                let node_a = &self.nodes[&node_ids[i]];
                let node_b = &self.nodes[&node_ids[j]];

                // Calculate semantic similarity
                let similarity = self.calculate_semantic_similarity(node_a, node_b);

                if similarity > 0.6 {
                    let edge = SemanticEdge {
                        id: Uuid::new_v4().to_string(),
                        source_id: node_a.id.clone(),
                        target_id: node_b.id.clone(),
                        edge_type: SemanticEdgeType::Similarity,
                        strength: similarity,
                        directional: false,
                        temporal_relationship: self.infer_temporal_relationship(node_a, node_b),
                        confidence: similarity,
                    };

                    self.edges.insert(edge.id.clone(), edge);
                }
            }
        }

        Ok(())
    }

    /// Calculate semantic similarity between nodes
    fn calculate_semantic_similarity(&self, node_a: &SemanticNode, node_b: &SemanticNode) -> f32 {
        // Cosine similarity of embeddings
        if node_a.embedding.len() == node_b.embedding.len() {
            let dot_product: f32 = node_a
                .embedding
                .iter()
                .zip(node_b.embedding.iter())
                .map(|(x, y)| x * y)
                .sum();
            let norm_a: f32 = node_a.embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
            let norm_b: f32 = node_b.embedding.iter().map(|x| x * x).sum::<f32>().sqrt();

            if norm_a > 0.0 && norm_b > 0.0 {
                let embedding_similarity = dot_product / (norm_a * norm_b);

                // Factor in thematic similarity
                let thematic_similarity =
                    match (&node_a.thematic_category, &node_b.thematic_category) {
                        (Some(cat_a), Some(cat_b)) if cat_a == cat_b => 0.2,
                        _ => 0.0,
                    };

                // Factor in node type compatibility
                let type_compatibility =
                    self.calculate_type_compatibility(&node_a.node_type, &node_b.node_type);

                (embedding_similarity + thematic_similarity + type_compatibility).min(1.0)
            } else {
                0.0
            }
        } else {
            0.0
        }
    }

    /// Calculate compatibility between node types
    fn calculate_type_compatibility(
        &self,
        type_a: &SemanticNodeType,
        type_b: &SemanticNodeType,
    ) -> f32 {
        match (type_a, type_b) {
            (SemanticNodeType::Concept, SemanticNodeType::Attribute) => 0.3,
            (SemanticNodeType::Action, SemanticNodeType::Entity) => 0.3,
            (SemanticNodeType::Event, SemanticNodeType::State) => 0.3,
            (SemanticNodeType::Relationship, _) => 0.2,
            (_, SemanticNodeType::Relationship) => 0.2,
            (a, b) if a == b => 0.1,
            _ => 0.0,
        }
    }

    /// Infer temporal relationship between nodes
    fn infer_temporal_relationship(
        &self,
        node_a: &SemanticNode,
        node_b: &SemanticNode,
    ) -> Option<TemporalRelation> {
        match (node_a.temporal_position, node_b.temporal_position) {
            (Some(pos_a), Some(pos_b)) => {
                if pos_a < pos_b {
                    Some(TemporalRelation {
                        relation_type: TemporalRelationType::Before,
                        time_difference: None,
                        confidence: 0.8,
                    })
                } else if pos_a > pos_b {
                    Some(TemporalRelation {
                        relation_type: TemporalRelationType::After,
                        time_difference: None,
                        confidence: 0.8,
                    })
                } else {
                    Some(TemporalRelation {
                        relation_type: TemporalRelationType::Concurrent,
                        time_difference: None,
                        confidence: 0.6,
                    })
                }
            }
            _ => None,
        }
    }

    /// Calculate graph metrics
    fn calculate_metrics(&mut self) {
        let total_nodes = self.nodes.len();
        let total_edges = self.edges.len();

        let density = if total_nodes > 1 {
            total_edges as f32 / (total_nodes * (total_nodes - 1)) as f32
        } else {
            0.0
        };

        let avg_node_degree = if total_nodes > 0 {
            (2 * total_edges) as f32 / total_nodes as f32
        } else {
            0.0
        };

        self.metrics = GraphMetrics {
            total_nodes,
            total_edges,
            density,
            avg_node_degree,
            connected_components: 1, // Simplified
            diameter: None,          // Would calculate actual diameter
        };
    }
}

impl ContinuityAnalyzer {
    /// Create a new continuity analyzer
    pub fn new() -> Self {
        Self {
            break_detector: ContinuityBreakDetector::new(),
            inconsistency_detector: InconsistencyDetector::new(),
            flow_analyzer: SemanticFlowAnalyzer::new(),
        }
    }

    /// Detect continuity breaks in graph
    pub fn detect_breaks(&self, graph: &SemanticGraph) -> ContextNestResult<Vec<ContinuityBreak>> {
        self.break_detector.detect_breaks(graph)
    }

    /// Detect inconsistencies in graph
    pub fn detect_inconsistencies(
        &self,
        graph: &SemanticGraph,
    ) -> ContextNestResult<Vec<Inconsistency>> {
        self.inconsistency_detector.detect_inconsistencies(graph)
    }
}

impl ContinuityBreakDetector {
    /// Create a new continuity break detector
    pub fn new() -> Self {
        Self {
            break_types: vec![
                ContinuityBreakType::Semantic,
                ContinuityBreakType::Logical,
                ContinuityBreakType::Temporal,
                ContinuityBreakType::Thematic,
            ],
            thresholds: BreakDetectionThresholds {
                semantic_threshold: 0.5,
                logical_threshold: 0.6,
                temporal_threshold: 24, // 24 hours
                thematic_threshold: 0.4,
            },
        }
    }

    /// Detect continuity breaks
    pub fn detect_breaks(&self, graph: &SemanticGraph) -> ContextNestResult<Vec<ContinuityBreak>> {
        let mut breaks = Vec::new();

        // Check for weak connections
        for edge in graph.edges.values() {
            if edge.strength < self.thresholds.semantic_threshold {
                breaks.push(ContinuityBreak {
                    id: Uuid::new_v4().to_string(),
                    break_type: ContinuityBreakType::Semantic,
                    location: BreakLocation {
                        before_node_id: Some(edge.source_id.clone()),
                        after_node_id: Some(edge.target_id.clone()),
                        sequence_position: None,
                    },
                    severity: 1.0 - edge.strength,
                    suggested_repairs: vec![RepairStrategy {
                        strategy_type: RepairStrategyType::InsertBridge,
                        effectiveness: 0.7,
                        complexity: RepairComplexity::Moderate,
                        required_resources: vec!["semantic_bridge".to_string()],
                    }],
                    confidence: edge.confidence,
                });
            }
        }

        Ok(breaks)
    }
}

impl InconsistencyDetector {
    /// Create a new inconsistency detector
    pub fn new() -> Self {
        Self {
            inconsistency_types: vec![
                InconsistencyType::Contradiction,
                InconsistencyType::Temporal,
                InconsistencyType::Causal,
            ],
            rules: vec![InconsistencyRule {
                id: "contradiction_check".to_string(),
                rule_type: InconsistencyType::Contradiction,
                pattern: "not.*not".to_string(),
                weight: 0.8,
            }],
        }
    }

    /// Detect inconsistencies
    pub fn detect_inconsistencies(
        &self,
        graph: &SemanticGraph,
    ) -> ContextNestResult<Vec<Inconsistency>> {
        let mut inconsistencies = Vec::new();

        // Simple inconsistency detection (would be more sophisticated in real implementation)
        for node in graph.nodes.values() {
            if node.content.to_lowercase().contains("not not") {
                inconsistencies.push(Inconsistency {
                    id: Uuid::new_v4().to_string(),
                    inconsistency_type: InconsistencyType::Contradiction,
                    affected_nodes: vec![node.id.clone()],
                    description: "Double negation detected".to_string(),
                    severity: 0.5,
                    resolution_strategies: vec![ResolutionStrategy {
                        description: "Remove double negation".to_string(),
                        success_rate: 0.9,
                        implementation_steps: vec!["Simplify logical expression".to_string()],
                    }],
                });
            }
        }

        Ok(inconsistencies)
    }
}

impl SemanticFlowAnalyzer {
    /// Create a new semantic flow analyzer
    pub fn new() -> Self {
        Self {
            flow_patterns: Vec::new(),
            metrics: FlowMetrics::default(),
        }
    }
}

impl FlowOptimizer {
    /// Create a new flow optimizer
    pub fn new() -> Self {
        Self {
            strategies: vec![OptimizationStrategy {
                id: "reorder_fragments".to_string(),
                strategy_type: OptimizationStrategyType::Reordering,
                parameters: HashMap::new(),
                expected_improvement: 0.3,
            }],
            parameters: OptimizationParameters {
                max_iterations: 10,
                convergence_threshold: 0.01,
                optimization_weight: 0.7,
                preserve_original: true,
            },
        }
    }

    /// Optimize semantic flow
    pub fn optimize_flow(
        &self,
        graph: SemanticGraph,
        config: &RestorationConfig,
    ) -> ContextNestResult<SemanticGraph> {
        // Simplified optimization - return original graph
        // In a real implementation, this would apply various optimization strategies
        Ok(graph)
    }
}

/// Semantic graph structure
#[derive(Debug, Clone)]
pub struct SemanticGraph {
    /// Graph nodes
    pub nodes: HashMap<String, SemanticNode>,
    /// Graph edges
    pub edges: HashMap<String, SemanticEdge>,
    /// Graph metrics
    pub metrics: GraphMetrics,
}

/// Temporal information for fragments (re-export from memory_reconstruction)
pub use crate::context::memory_reconstruction::TemporalInfo;

impl Default for GraphMetrics {
    fn default() -> Self {
        Self {
            total_nodes: 0,
            total_edges: 0,
            density: 0.0,
            avg_node_degree: 0.0,
            connected_components: 0,
            diameter: None,
        }
    }
}

impl Default for FlowMetrics {
    fn default() -> Self {
        Self {
            coherence: 0.0,
            smoothness: 0.0,
            predictability: 0.0,
            flow_breaks: 0,
            avg_segment_length: 0.0,
        }
    }
}

impl Default for RestorationMetrics {
    fn default() -> Self {
        Self {
            total_restorations: 0,
            successful_restorations: 0,
            avg_continuity_improvement: 0.0,
            avg_coherence_improvement: 0.0,
            breaks_detected: 0,
            breaks_repaired: 0,
            inconsistencies_resolved: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantic_continuity_restoration_creation() {
        let config = RestorationConfig::default();
        let restoration = SemanticContinuityRestoration::new(config);

        assert_eq!(restoration.metrics.total_restorations, 0);
    }

    #[test]
    fn test_semantic_graph_builder() {
        let mut builder = SemanticGraphBuilder::new();

        let fragments = vec![
            ReconstructionFragment {
                id: "frag1".to_string(),
                source_attractor_id: "att1".to_string(),
                content: "First concept about algorithms".to_string(),
                embedding: vec![0.1; 10],
                strength: 0.8,
                confidence: 0.9,
                position: Some(0),
                connections: Vec::new(),
                temporal_info: TemporalInfo {
                    created_at: Utc::now(),
                    sequence_position: Some(0),
                    temporal_relationships: Vec::new(),
                },
            },
            ReconstructionFragment {
                id: "frag2".to_string(),
                source_attractor_id: "att2".to_string(),
                content: "Second concept about systems".to_string(),
                embedding: vec![0.2; 10],
                strength: 0.7,
                confidence: 0.8,
                position: Some(1),
                connections: Vec::new(),
                temporal_info: TemporalInfo {
                    created_at: Utc::now(),
                    sequence_position: Some(1),
                    temporal_relationships: Vec::new(),
                },
            },
        ];

        let graph = builder.build_from_fragments(&fragments).unwrap();
        assert_eq!(graph.nodes.len(), 2);
    }

    #[test]
    fn test_continuity_break_detection() {
        let detector = ContinuityBreakDetector::new();

        let mut graph = SemanticGraph {
            nodes: HashMap::new(),
            edges: HashMap::new(),
            metrics: GraphMetrics::default(),
        };

        // Add a weak edge
        let edge = SemanticEdge {
            id: "edge1".to_string(),
            source_id: "node1".to_string(),
            target_id: "node2".to_string(),
            edge_type: SemanticEdgeType::Similarity,
            strength: 0.3, // Below threshold
            directional: false,
            temporal_relationship: None,
            confidence: 0.5,
        };

        graph.edges.insert(edge.id.clone(), edge);

        let breaks = detector.detect_breaks(&graph).unwrap();
        assert_eq!(breaks.len(), 1);
        assert_eq!(breaks[0].break_type, ContinuityBreakType::Semantic);
    }
}

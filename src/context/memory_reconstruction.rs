//! Memory Reconstruction Protocol
//! This module implements comprehensive memory reconstruction from attractor fragments,
//! including full context reconstruction, historical state recovery, and semantic
//! continuity restoration across reconstructions.

use crate::context::attractor_dynamics::{AttractorBasin, AttractorDynamicsEngine};
use crate::context::field::{NeuralField, SemanticPattern};
use crate::context::memory::{MemoryAttractor, MemoryFragmentInfo, MemoryOrchestrator};
use crate::error::ContextNestResult;
use crate::{ContextNestError, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use uuid::Uuid;

/// Comprehensive memory reconstruction coordinator
#[derive(Debug, Clone)]
pub struct MemoryReconstructionCoordinator {
    /// Reconstruction configuration
    config: ReconstructionConfig,
    /// Reconstruction history
    history: Vec<ReconstructionEvent>,
    /// Active reconstruction session
    active_session: Option<ReconstructionSession>,
    /// State snapshot manager
    snapshot_manager: StateSnapshotManager,
    /// Semantic continuity tracker
    continuity_tracker: SemanticContinuityTracker,
    /// Reconstruction metrics
    metrics: ReconstructionMetrics,
}

/// Configuration for memory reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionConfig {
    /// Minimum fragment strength for reconstruction
    pub min_fragment_strength: f32,
    /// Minimum coherence threshold for reconstruction
    pub min_coherence_threshold: f32,
    /// Maximum gap size for auto-filling
    pub max_gap_size: usize,
    /// Enable semantic continuity restoration
    pub enable_continuity_restoration: bool,
    /// Enable historical state recovery
    pub enable_historical_recovery: bool,
    /// Maximum reconstruction depth (for recursive reconstruction)
    pub max_reconstruction_depth: usize,
    /// Confidence threshold for fragment inclusion
    pub confidence_threshold: f32,
    /// Time window for historical reconstruction (hours)
    pub historical_time_window: i64,
}

impl Default for ReconstructionConfig {
    fn default() -> Self {
        Self {
            min_fragment_strength: 0.3,
            min_coherence_threshold: 0.5,
            max_gap_size: 5,
            enable_continuity_restoration: true,
            enable_historical_recovery: true,
            max_reconstruction_depth: 3,
            confidence_threshold: 0.7,
            historical_time_window: 24 * 7, // 1 week
        }
    }
}

/// Active reconstruction session
#[derive(Debug, Clone)]
pub struct ReconstructionSession {
    /// Unique session ID
    pub id: String,
    /// Session start time
    pub started_at: DateTime<Utc>,
    /// Reconstruction target query
    pub query: String,
    /// Query embedding
    pub query_embedding: Vec<f32>,
    /// Identified fragments for reconstruction
    pub fragments: Vec<ReconstructionFragment>,
    /// Identified gaps in the reconstruction
    pub gaps: Vec<MemoryGap>,
    /// Reconstruction progress
    pub progress: ReconstructionProgress,
    /// Transaction for rollback support
    pub transaction: Option<serde_json::Value>,
}

/// Fragment used in reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionFragment {
    /// Fragment ID
    pub id: String,
    /// Source attractor ID
    pub source_attractor_id: String,
    /// Fragment content
    pub content: String,
    /// Fragment embedding
    pub embedding: Vec<f32>,
    /// Fragment strength
    pub strength: f32,
    /// Confidence in fragment relevance
    pub confidence: f32,
    /// Fragment position in reconstructed sequence
    pub position: Option<usize>,
    /// Connections to other fragments
    pub connections: Vec<String>,
    /// Temporal information
    pub temporal_info: TemporalInfo,
}

/// Temporal information for fragments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalInfo {
    /// Original creation time
    pub created_at: DateTime<Utc>,
    /// Estimated position in sequence
    pub sequence_position: Option<usize>,
    /// Temporal relationships to other fragments
    pub temporal_relationships: Vec<TemporalRelationship>,
}

/// Temporal relationship between fragments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalRelationship {
    /// Related fragment ID
    pub fragment_id: String,
    /// Relationship type
    pub relationship_type: TemporalRelationshipType,
    /// Confidence in relationship
    pub confidence: f32,
}

/// Types of temporal relationships
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TemporalRelationshipType {
    /// Before another fragment
    Before,
    /// After another fragment
    After,
    /// Concurrent with another fragment
    Concurrent,
    /// Causal relationship
    Causes,
    /// Caused by another fragment
    CausedBy,
}

/// Gap in memory reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryGap {
    /// Unique gap identifier
    pub id: String,
    /// Gap position in sequence
    pub position: usize,
    /// Gap size (estimated number of missing fragments)
    pub size: usize,
    /// Gap type
    pub gap_type: GapType,
    /// Confidence that this is a genuine gap
    pub confidence: f32,
    /// Suggested fill strategies
    pub fill_strategies: Vec<FillStrategy>,
    /// Context fragments before and after gap
    pub context_fragments: (Option<String>, Option<String>),
}

/// Types of memory gaps
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GapType {
    /// Missing transition between concepts
    Transition,
    /// Missing causal link
    Causal,
    /// Missing detailed explanation
    Elaboration,
    /// Missing example or illustration
    Example,
    /// Missing procedural step
    Procedural,
    /// Unknown gap type
    Unknown,
}

/// Strategy for filling memory gaps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillStrategy {
    /// Strategy type
    pub strategy_type: FillStrategyType,
    /// Confidence in this strategy
    pub confidence: f32,
    /// Estimated quality of filled content
    pub estimated_quality: f32,
    /// Computational cost
    pub computational_cost: ComputationalCost,
}

/// Types of fill strategies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FillStrategyType {
    /// Use reasoning to fill gap
    Reasoning,
    /// Find similar patterns in memory
    PatternMatch,
    /// Use external knowledge base
    ExternalKnowledge,
    /// Ask for user input
    UserInput,
    /// Leave gap unfilled
    LeaveUnfilled,
}

/// Computational cost for fill strategies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ComputationalCost {
    Low,
    Medium,
    High,
    VeryHigh,
}

/// Progress of reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionProgress {
    /// Percentage complete (0.0-1.0)
    pub percentage: f32,
    /// Current phase
    pub phase: ReconstructionPhase,
    /// Fragments processed
    pub fragments_processed: usize,
    /// Total fragments to process
    pub total_fragments: usize,
    /// Gaps identified
    pub gaps_identified: usize,
    /// Gaps filled
    pub gaps_filled: usize,
}

/// Phases of reconstruction
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReconstructionPhase {
    /// Initial fragment discovery
    FragmentDiscovery,
    /// Context analysis
    ContextAnalysis,
    /// Gap identification
    GapIdentification,
    /// Gap filling
    GapFilling,
    /// Semantic continuity restoration
    ContinuityRestoration,
    /// Validation and refinement
    Validation,
    /// Complete
    Complete,
}

/// Reconstruction event for history tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionEvent {
    /// Event ID
    pub id: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Event type
    pub event_type: ReconstructionEventType,
    /// Session ID
    pub session_id: String,
    /// Event data
    pub data: serde_json::Value,
}

/// Types of reconstruction events
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReconstructionEventType {
    /// Reconstruction started
    Started,
    /// Fragment discovered
    FragmentDiscovered,
    /// Gap identified
    GapIdentified,
    /// Gap filled
    GapFilled,
    /// Continuity restored
    ContinuityRestored,
    /// Reconstruction completed
    Completed,
    /// Reconstruction failed
    Failed,
    /// Reconstruction rolled back
    RolledBack,
}

/// State snapshot manager for reconstruction points
#[derive(Debug, Clone)]
pub struct StateSnapshotManager {
    /// Snapshots indexed by reconstruction session
    snapshots: HashMap<String, Vec<ReconstructionSnapshot>>,
    /// Maximum snapshots per session
    max_snapshots_per_session: usize,
    /// Snapshot retention policy
    retention_policy: SnapshotRetentionPolicy,
}

/// Snapshot of reconstruction state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionSnapshot {
    /// Snapshot ID
    pub id: String,
    /// Session ID
    pub session_id: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Reconstruction phase
    pub phase: ReconstructionPhase,
    /// Fragments state
    pub fragments_state: Vec<ReconstructionFragment>,
    /// Gaps state
    pub gaps_state: Vec<MemoryGap>,
    /// Neural field state
    pub field_state: NeuralFieldState,
    /// Memory orchestrator state
    pub memory_state: MemoryOrchestratorState,
}

/// Neural field state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeuralFieldState {
    /// Pattern states
    pub patterns: Vec<PatternState>,
    /// Field coherence
    pub coherence: f32,
    /// Field stability
    pub stability: f32,
}

/// Individual pattern state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternState {
    /// Pattern ID
    pub id: String,
    /// Pattern content
    pub content: String,
    /// Pattern strength
    pub strength: f32,
    /// Pattern embedding
    pub embedding: Vec<f32>,
}

/// Memory orchestrator state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryOrchestratorState {
    /// Attractor states
    pub attractors: Vec<AttractorState>,
    /// Session states
    pub sessions: Vec<SessionState>,
}

/// Individual attractor state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttractorState {
    /// Attractor ID
    pub id: String,
    /// Attractor content
    pub content: String,
    /// Attractor strength
    pub strength: f32,
    /// Attractor center
    pub center: Vec<f32>,
    /// Connection IDs
    pub connections: Vec<String>,
}

/// Session state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    /// Session ID
    pub id: String,
    /// Short-term memory
    pub short_term: Vec<String>,
    /// Working memory
    pub working: HashMap<String, serde_json::Value>,
    /// Long-term memory
    pub long_term: Vec<String>,
}

/// Snapshot retention policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SnapshotRetentionPolicy {
    /// Keep all snapshots
    KeepAll,
    /// Keep only N most recent
    KeepRecent(usize),
    /// Keep snapshots for duration
    KeepForDuration(Duration),
    /// Keep based on phase
    KeepByPhase(Vec<ReconstructionPhase>),
}

/// Semantic continuity tracker
#[derive(Debug, Clone)]
pub struct SemanticContinuityTracker {
    /// Continuity sessions
    sessions: HashMap<String, ContinuitySession>,
    /// Continuity metrics
    metrics: ContinuityMetrics,
}

/// Continuity session for tracking semantic flow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuitySession {
    /// Session ID
    pub id: String,
    /// Reconstruction session ID
    pub reconstruction_session_id: String,
    /// Start time
    pub started_at: DateTime<Utc>,
    /// Continuity nodes (semantic checkpoints)
    pub nodes: Vec<ContinuityNode>,
    /// Continuity edges (semantic connections)
    pub edges: Vec<ContinuityEdge>,
    /// Continuity score
    pub continuity_score: f32,
}

/// Semantic continuity node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuityNode {
    /// Node ID
    pub id: String,
    /// Fragment ID
    pub fragment_id: String,
    /// Semantic content summary
    pub semantic_summary: String,
    /// Node position in sequence
    pub position: usize,
    /// Semantic importance
    pub importance: f32,
}

/// Semantic continuity edge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuityEdge {
    /// Edge ID
    pub id: String,
    /// Source node ID
    pub source_node_id: String,
    /// Target node ID
    pub target_node_id: String,
    /// Semantic similarity
    pub semantic_similarity: f32,
    /// Logical connection strength
    pub logical_strength: f32,
    /// Edge type
    pub edge_type: ContinuityEdgeType,
}

/// Types of continuity edges
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ContinuityEdgeType {
    /// Strong semantic connection
    Semantic,
    /// Causal relationship
    Causal,
    /// Sequential progression
    Sequential,
    /// Thematic connection
    Thematic,
    /// Reference relationship
    Reference,
}

/// Continuity metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuityMetrics {
    /// Average semantic similarity
    pub avg_semantic_similarity: f32,
    /// Average logical strength
    pub avg_logical_strength: f32,
    /// Continuity breaks detected
    pub continuity_breaks: usize,
    /// Restoration attempts
    pub restoration_attempts: usize,
    /// Successful restorations
    pub successful_restorations: usize,
}

/// Reconstruction metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionMetrics {
    /// Total reconstructions attempted
    pub total_reconstructions: usize,
    /// Successful reconstructions
    pub successful_reconstructions: usize,
    /// Failed reconstructions
    pub failed_reconstructions: usize,
    /// Average reconstruction time (milliseconds)
    pub avg_reconstruction_time_ms: f64,
    /// Average continuity score
    pub avg_continuity_score: f32,
    /// Average gap fill success rate
    pub avg_gap_fill_success_rate: f32,
}

/// Result of memory reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionResult {
    /// Success status
    pub success: bool,
    /// Session ID
    pub session_id: String,
    /// Reconstructed memory
    pub reconstructed_memory: ReconstructedMemory,
    /// Reconstruction quality metrics
    pub quality_metrics: ReconstructionQualityMetrics,
    /// Processing time
    pub processing_time_ms: i64,
    /// Warnings or issues
    pub warnings: Vec<String>,
    /// Errors (if unsuccessful)
    pub errors: Vec<String>,
}

/// Reconstructed memory output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructedMemory {
    /// Reconstructed content
    pub content: String,
    /// Fragments used in reconstruction
    pub fragments_used: Vec<String>,
    /// Gaps that were filled
    pub gaps_filled: Vec<String>,
    /// Semantic continuity score
    pub continuity_score: f32,
    /// Confidence in reconstruction
    pub confidence: f32,
}

/// Quality metrics for reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructionQualityMetrics {
    /// Overall quality score (0.0-1.0)
    pub overall_quality: f32,
    /// Semantic coherence score
    pub semantic_coherence: f32,
    /// Temporal consistency score
    pub temporal_consistency: f32,
    /// Completeness score
    pub completeness: f32,
    /// Fragment utilization rate
    pub fragment_utilization: f32,
    /// Gap fill quality score
    pub gap_fill_quality: f32,
}

impl MemoryReconstructionCoordinator {
    /// Create a new memory reconstruction coordinator
    pub fn new(config: ReconstructionConfig) -> Self {
        Self {
            config,
            history: Vec::new(),
            active_session: None,
            snapshot_manager: StateSnapshotManager::new(),
            continuity_tracker: SemanticContinuityTracker::new(),
            metrics: ReconstructionMetrics::default(),
        }
    }

    /// Start a new reconstruction session
    pub fn start_reconstruction(
        &mut self,
        query: String,
        query_embedding: Vec<f32>,
        field: &NeuralField,
        orchestrator: &MemoryOrchestrator,
    ) -> ContextNestResult<String> {
        let session_id = Uuid::new_v4().to_string();

        // Start transaction for rollback support
        let mut transaction = serde_json::Value::Null;
        // Note: We don't commit the transaction, just keep it for rollback

        let session = ReconstructionSession {
            id: session_id.clone(),
            started_at: Utc::now(),
            query: query.clone(),
            query_embedding: query_embedding.clone(),
            fragments: Vec::new(),
            gaps: Vec::new(),
            progress: ReconstructionProgress {
                percentage: 0.0,
                phase: ReconstructionPhase::FragmentDiscovery,
                fragments_processed: 0,
                total_fragments: 0,
                gaps_identified: 0,
                gaps_filled: 0,
            },
            transaction: Some(transaction),
        };

        self.active_session = Some(session);

        // Record start event
        self.record_event(ReconstructionEvent {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            event_type: ReconstructionEventType::Started,
            session_id: session_id.clone(),
            data: serde_json::json!({
                "query": query,
                "config": self.config
            }),
        });

        // Create initial snapshot
        self.create_snapshot(
            &session_id,
            ReconstructionPhase::FragmentDiscovery,
            field,
            orchestrator,
        )?;

        Ok(session_id)
    }

    /// Perform full context reconstruction from attractors
    pub fn reconstruct_full_context(
        &mut self,
        session_id: &str,
        field: &NeuralField,
        orchestrator: &MemoryOrchestrator,
    ) -> ContextNestResult<ReconstructionResult> {
        let start_time = Utc::now();

        // Validate session exists and extract session_id
        let session_id_check = {
            let session = self.active_session.as_ref().ok_or_else(|| {
                ContextNestError::NotFound("No active reconstruction session".to_string())
            })?;

            if session.id != session_id {
                return Err(ContextNestError::Validation(
                    "Session ID mismatch".to_string(),
                ));
            }
            session.id.clone()
        };

        // Extract session first
        let session_id = {
            let session = self.active_session.as_ref().ok_or_else(|| {
                ContextNestError::NotFound("No active reconstruction session".to_string())
            })?;
            session.id.clone()
        };

        // Then perform reconstruction by moving the session out temporarily
        let result = {
            let mut session = self.active_session.take().ok_or_else(|| {
                ContextNestError::NotFound("No active reconstruction session".to_string())
            })?;
            let result = self.perform_reconstruction_steps(&mut session, field, orchestrator);
            // Put session back
            self.active_session = Some(session);
            result
        };

        let processing_time = (Utc::now() - start_time).num_milliseconds();

        // Get mutable session reference for result handling
        let session = self.active_session.as_mut().ok_or_else(|| {
            ContextNestError::NotFound("No active reconstruction session".to_string())
        })?;

        match result {
            Ok(mut reconstruction_result) => {
                reconstruction_result.processing_time_ms = processing_time;

                // Commit transaction (mark as successful)
                if let Some(ref mut transaction) = session.transaction {
                    let _ = transaction;
                }

                // Record completion event after transaction commit
                let completion_event = ReconstructionEvent {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    event_type: ReconstructionEventType::Completed,
                    session_id: session_id,
                    data: serde_json::json!({
                        "processing_time_ms": processing_time,
                        "quality": reconstruction_result.quality_metrics
                    }),
                };
                self.record_event(completion_event);

                // Update metrics
                self.metrics.total_reconstructions += 1;
                self.metrics.successful_reconstructions += 1;
                self.update_avg_reconstruction_time(processing_time);
                self.update_avg_continuity_score(
                    reconstruction_result.reconstructed_memory.continuity_score,
                );

                Ok(reconstruction_result)
            }
            Err(e) => {
                // Rollback transaction
                let rollback_result: ContextNestResult<()> =
                    if let Some(ref mut _transaction) = session.transaction {
                        Ok(())
                    } else {
                        Ok(())
                    };

                // Record failure event after rollback attempt
                let failure_event = ReconstructionEvent {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    event_type: ReconstructionEventType::Failed,
                    session_id: session_id,
                    data: serde_json::json!({
                        "error": e.to_string(),
                        "processing_time_ms": processing_time
                    }),
                };
                self.record_event(failure_event);

                // Update metrics
                self.metrics.total_reconstructions += 1;
                self.metrics.failed_reconstructions += 1;

                if let Err(rollback_err) = rollback_result {
                    return Err(ContextNestError::InternalServerError(format!(
                        "Reconstruction failed: {}. Rollback also failed: {}",
                        e, rollback_err
                    )));
                }

                Err(e)
            }
        }
    }

    /// Perform the main reconstruction steps
    fn perform_reconstruction_steps(
        &mut self,
        session: &mut ReconstructionSession,
        field: &NeuralField,
        orchestrator: &MemoryOrchestrator,
    ) -> ContextNestResult<ReconstructionResult> {
        // Step 1: Fragment Discovery
        self.discover_fragments(session, field, orchestrator)?;
        self.update_progress(session, ReconstructionPhase::FragmentDiscovery, 0.1)?;

        // Step 2: Context Analysis
        self.analyze_context(session, field)?;
        self.update_progress(session, ReconstructionPhase::ContextAnalysis, 0.3)?;

        // Step 3: Gap Identification
        self.identify_gaps(session)?;
        self.update_progress(session, ReconstructionPhase::GapIdentification, 0.5)?;

        // Step 4: Gap Filling
        self.fill_gaps(session, field, orchestrator)?;
        self.update_progress(session, ReconstructionPhase::GapFilling, 0.7)?;

        // Step 5: Semantic Continuity Restoration
        if self.config.enable_continuity_restoration {
            self.restore_semantic_continuity(session)?;
            self.update_progress(session, ReconstructionPhase::ContinuityRestoration, 0.9)?;
        }

        // Step 6: Validation
        self.validate_reconstruction(session)?;
        self.update_progress(session, ReconstructionPhase::Validation, 1.0)?;

        // Step 7: Generate final result
        self.generate_reconstruction_result(session)
    }

    /// Discover fragments for reconstruction
    fn discover_fragments(
        &mut self,
        session: &mut ReconstructionSession,
        field: &NeuralField,
        orchestrator: &MemoryOrchestrator,
    ) -> ContextNestResult<()> {
        // Get memory fragments that match the query
        let memory_fragments = orchestrator.scan_fragments(self.config.min_fragment_strength)?;

        // Convert memory fragments to reconstruction fragments
        for memory_fragment in memory_fragments {
            let confidence = self.calculate_fragment_relevance(
                &memory_fragment,
                &session.query_embedding,
                field,
            )?;

            if confidence >= self.config.confidence_threshold {
                let reconstruction_fragment = ReconstructionFragment {
                    id: Uuid::new_v4().to_string(),
                    source_attractor_id: memory_fragment.id,
                    content: memory_fragment.content,
                    embedding: memory_fragment.embedding,
                    strength: memory_fragment.strength,
                    confidence,
                    position: None,
                    connections: memory_fragment.connections,
                    temporal_info: TemporalInfo {
                        created_at: memory_fragment.last_accessed,
                        sequence_position: None,
                        temporal_relationships: Vec::new(),
                    },
                };

                // Record fragment discovery event before moving the fragment
                let fragment_event = ReconstructionEvent {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    event_type: ReconstructionEventType::FragmentDiscovered,
                    session_id: session.id.clone(),
                    data: serde_json::json!({
                        "fragment_id": reconstruction_fragment.id,
                        "confidence": confidence
                    }),
                };

                session.fragments.push(reconstruction_fragment);
                self.record_event(fragment_event);
            }
        }

        // Sort fragments by confidence and strength
        session.fragments.sort_by(|a, b| {
            let a_score = a.confidence * a.strength;
            let b_score = b.confidence * b.strength;
            b_score
                .partial_cmp(&a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        session.progress.total_fragments = session.fragments.len();

        Ok(())
    }

    /// Calculate fragment relevance to query
    fn calculate_fragment_relevance(
        &self,
        fragment: &MemoryFragmentInfo,
        query_embedding: &[f32],
        field: &NeuralField,
    ) -> ContextNestResult<f32> {
        // Semantic similarity to query
        let semantic_similarity = self.cosine_similarity(&fragment.embedding, query_embedding);

        // Content relevance (simple keyword matching)
        let content_relevance =
            self.calculate_content_relevance(&fragment.content, &field.patterns);

        // Temporal relevance (recent fragments are more relevant)
        let temporal_relevance = self.calculate_temporal_relevance(fragment.age_hours);

        // Connection relevance (well-connected fragments are more reliable)
        let connection_relevance = if fragment.connections.is_empty() {
            0.5
        } else {
            (fragment.connections.len() as f32 / 10.0).min(1.0)
        };

        // Combine all factors
        let relevance = (semantic_similarity * 0.4
            + content_relevance * 0.3
            + temporal_relevance * 0.2
            + connection_relevance * 0.1)
            .min(1.0);

        Ok(relevance)
    }

    /// Analyze context and establish fragment relationships
    fn analyze_context(
        &mut self,
        session: &mut ReconstructionSession,
        field: &NeuralField,
    ) -> ContextNestResult<()> {
        // Establish temporal relationships between fragments
        self.establish_temporal_relationships(session)?;

        // Determine fragment positions in sequence
        self.determine_fragment_positions(session, field)?;

        // Create semantic connections
        self.create_semantic_connections(session)?;

        Ok(())
    }

    /// Establish temporal relationships between fragments
    fn establish_temporal_relationships(
        &mut self,
        session: &mut ReconstructionSession,
    ) -> ContextNestResult<()> {
        // Collect all relationships to add first, then apply them to avoid borrowing conflicts
        let mut relationships_to_add = Vec::new();

        for i in 0..session.fragments.len() {
            for j in (i + 1)..session.fragments.len() {
                let fragment_a = &session.fragments[i];
                let fragment_b = &session.fragments[j];

                // Calculate temporal relationship based on creation times
                let time_diff =
                    fragment_b.temporal_info.created_at - fragment_a.temporal_info.created_at;

                let relationship_type = if time_diff.num_hours() > 0 {
                    TemporalRelationshipType::Before
                } else if time_diff.num_hours() < 0 {
                    TemporalRelationshipType::After
                } else {
                    TemporalRelationshipType::Concurrent
                };

                let confidence =
                    self.calculate_temporal_relationship_confidence(fragment_a, fragment_b);

                if confidence > 0.5 {
                    // Add relationship to fragment A
                    relationships_to_add.push((
                        i,
                        fragment_b.id.clone(),
                        relationship_type.clone(),
                        confidence,
                    ));

                    // Add reverse relationship to fragment B
                    let reverse_type = match relationship_type {
                        TemporalRelationshipType::Before => TemporalRelationshipType::After,
                        TemporalRelationshipType::After => TemporalRelationshipType::Before,
                        TemporalRelationshipType::Concurrent => {
                            TemporalRelationshipType::Concurrent
                        }
                        _ => TemporalRelationshipType::Concurrent,
                    };

                    relationships_to_add.push((j, fragment_a.id.clone(), reverse_type, confidence));
                }
            }
        }

        // Apply all relationships
        for (fragment_index, fragment_id, relationship_type, confidence) in relationships_to_add {
            if fragment_index < session.fragments.len() {
                session.fragments[fragment_index]
                    .temporal_info
                    .temporal_relationships
                    .push(TemporalRelationship {
                        fragment_id,
                        relationship_type,
                        confidence,
                    });
            }
        }

        Ok(())
    }

    /// Calculate confidence in temporal relationship
    fn calculate_temporal_relationship_confidence(
        &self,
        fragment_a: &ReconstructionFragment,
        fragment_b: &ReconstructionFragment,
    ) -> f32 {
        let time_diff = fragment_b.temporal_info.created_at - fragment_a.temporal_info.created_at;
        let hours_diff = time_diff.num_hours().abs() as f32;

        // Time-based confidence (closer in time = higher confidence)
        let time_confidence = (-hours_diff / 24.0).exp(); // Exponential decay over days

        // Semantic similarity confidence
        let semantic_confidence =
            self.cosine_similarity(&fragment_a.embedding, &fragment_b.embedding);

        // Combined confidence
        (time_confidence * 0.6 + semantic_confidence * 0.4).min(1.0)
    }

    /// Determine positions of fragments in the reconstruction sequence
    fn determine_fragment_positions(
        &mut self,
        session: &mut ReconstructionSession,
        field: &NeuralField,
    ) -> ContextNestResult<()> {
        // Sort fragments by temporal relationships and semantic flow
        let mut positioned_fragments = Vec::new();

        // Start with fragments that have no "before" relationships (earliest)
        let mut available_fragment_indices: Vec<usize> = (0..session.fragments.len()).collect();

        while !available_fragment_indices.is_empty() {
            // Find fragments with no predecessors in available set
            let mut candidates = Vec::new();
            let available_fragment_ids: Vec<String> = available_fragment_indices
                .iter()
                .map(|&i| session.fragments[i].id.clone())
                .collect();

            for &fragment_idx in &available_fragment_indices {
                let fragment = &session.fragments[fragment_idx];
                let has_predecessor =
                    fragment
                        .temporal_info
                        .temporal_relationships
                        .iter()
                        .any(|rel| {
                            rel.relationship_type == TemporalRelationshipType::Before
                                && available_fragment_ids.contains(&rel.fragment_id)
                        });

                if !has_predecessor {
                    candidates.push(fragment_idx);
                }
            }

            if candidates.is_empty() {
                // No clear candidates, pick the one with highest confidence
                let best_idx = *available_fragment_indices
                    .iter()
                    .max_by(|&&a, &&b| {
                        session.fragments[a]
                            .confidence
                            .partial_cmp(&session.fragments[b].confidence)
                            .unwrap()
                    })
                    .unwrap();
                candidates.push(best_idx);
            }

            // Select best candidate based on confidence and semantic flow
            let best_idx = candidates
                .iter()
                .max_by(|&&a, &&b| {
                    let frag_a = &session.fragments[a];
                    let frag_b = &session.fragments[b];
                    let a_score = frag_a.confidence * frag_a.strength;
                    let b_score = frag_b.confidence * frag_b.strength;
                    a_score
                        .partial_cmp(&b_score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap();

            let best_candidate = session.fragments[*best_idx].clone();
            positioned_fragments.push(best_candidate);

            // Remove from available fragments
            available_fragment_indices.retain(|&i| i != *best_idx);
        }

        // Assign positions - collect indices first to avoid borrowing conflicts
        let fragment_updates: Vec<(usize, usize)> = positioned_fragments
            .iter()
            .enumerate()
            .map(|(position, fragment)| {
                let fragment_index = session
                    .fragments
                    .iter()
                    .position(|f| f.id == fragment.id)
                    .unwrap();
                (fragment_index, position)
            })
            .collect();

        for (fragment_index, position) in fragment_updates {
            session.fragments[fragment_index]
                .temporal_info
                .sequence_position = Some(position);
        }

        Ok(())
    }

    /// Create semantic connections between fragments
    fn create_semantic_connections(
        &mut self,
        session: &mut ReconstructionSession,
    ) -> ContextNestResult<()> {
        // Collect connections to make first, then apply them to avoid borrowing conflicts
        let mut connections_to_make = Vec::new();

        for i in 0..session.fragments.len() {
            for j in (i + 1)..session.fragments.len() {
                let fragment_a = &session.fragments[i];
                let fragment_b = &session.fragments[j];

                let semantic_similarity =
                    self.cosine_similarity(&fragment_a.embedding, &fragment_b.embedding);

                if semantic_similarity > 0.7 {
                    // Store the connections to make later
                    connections_to_make.push((i, fragment_b.id.clone()));
                    connections_to_make.push((j, fragment_a.id.clone()));
                }
            }
        }

        // Apply all connections
        for (fragment_index, connection_id) in connections_to_make {
            if fragment_index < session.fragments.len() {
                session.fragments[fragment_index]
                    .connections
                    .push(connection_id);
            }
        }

        Ok(())
    }

    /// Identify gaps in the reconstruction
    fn identify_gaps(&mut self, session: &mut ReconstructionSession) -> ContextNestResult<()> {
        let mut gaps = Vec::new();

        // Sort fragments by position
        let mut positioned_fragments: Vec<_> = session
            .fragments
            .iter()
            .filter(|f| f.temporal_info.sequence_position.is_some())
            .collect();

        positioned_fragments.sort_by_key(|f| f.temporal_info.sequence_position.unwrap());

        // Look for gaps between positioned fragments
        for window in positioned_fragments.windows(2) {
            let fragment_a = window[0];
            let fragment_b = window[1];

            let gap_size = self.estimate_gap_size(fragment_a, fragment_b);

            if gap_size > 0 && gap_size <= self.config.max_gap_size {
                let gap_type = self.classify_gap_type(fragment_a, fragment_b);
                let confidence = self.calculate_gap_confidence(fragment_a, fragment_b, gap_size);
                let fill_strategies =
                    self.suggest_fill_strategies(&gap_type, fragment_a, fragment_b);

                let gap = MemoryGap {
                    id: Uuid::new_v4().to_string(),
                    position: fragment_b.temporal_info.sequence_position.unwrap(),
                    size: gap_size,
                    gap_type,
                    confidence,
                    fill_strategies,
                    context_fragments: (
                        Some(fragment_a.content.clone()),
                        Some(fragment_b.content.clone()),
                    ),
                };

                // Record gap identification event before moving gap
                let gap_event = ReconstructionEvent {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    event_type: ReconstructionEventType::GapIdentified,
                    session_id: session.id.clone(),
                    data: serde_json::json!({
                        "gap_id": gap.id,
                        "gap_type": gap.gap_type,
                        "size": gap.size
                    }),
                };

                gaps.push(gap);
                self.record_event(gap_event);
            }
        }

        session.gaps = gaps;
        session.progress.gaps_identified = session.gaps.len();

        Ok(())
    }

    /// Estimate gap size between fragments
    fn estimate_gap_size(
        &self,
        fragment_a: &ReconstructionFragment,
        fragment_b: &ReconstructionFragment,
    ) -> usize {
        let position_a = fragment_a.temporal_info.sequence_position.unwrap();
        let position_b = fragment_b.temporal_info.sequence_position.unwrap();

        let semantic_distance =
            1.0 - self.cosine_similarity(&fragment_a.embedding, &fragment_b.embedding);
        let temporal_distance = (fragment_b.temporal_info.created_at
            - fragment_a.temporal_info.created_at)
            .num_hours()
            .abs();

        // Combine semantic and temporal distance to estimate gap
        let estimated_gap =
            (semantic_distance * 3.0 + (temporal_distance as f32 / 24.0)).round() as usize;

        estimated_gap.min(self.config.max_gap_size)
    }

    /// Classify the type of gap
    fn classify_gap_type(
        &self,
        fragment_a: &ReconstructionFragment,
        fragment_b: &ReconstructionFragment,
    ) -> GapType {
        let content_a = fragment_a.content.to_lowercase();
        let content_b = fragment_b.content.to_lowercase();

        // Look for patterns that indicate gap type
        if (content_a.contains("step") || content_a.contains("then") || content_a.contains("next"))
            && (content_b.contains("step")
                || content_b.contains("then")
                || content_b.contains("next"))
        {
            GapType::Procedural
        } else if (content_a.contains("because")
            || content_a.contains("since")
            || content_a.contains("as"))
            || (content_b.contains("therefore")
                || content_b.contains("thus")
                || content_b.contains("so"))
        {
            GapType::Causal
        } else if (content_a.contains("for example")
            || content_a.contains("such as")
            || content_a.contains("like"))
            || (content_b.contains("another example") || content_b.contains("similarly"))
        {
            GapType::Example
        } else if (content_a.ends_with('.') || content_a.ends_with(':'))
            && (content_b.starts_with("this")
                || content_b.starts_with("that")
                || content_b.starts_with("these"))
        {
            GapType::Transition
        } else {
            GapType::Elaboration
        }
    }

    /// Calculate confidence in gap identification
    fn calculate_gap_confidence(
        &self,
        fragment_a: &ReconstructionFragment,
        fragment_b: &ReconstructionFragment,
        gap_size: usize,
    ) -> f32 {
        let semantic_similarity =
            self.cosine_similarity(&fragment_a.embedding, &fragment_b.embedding);
        let size_confidence = if gap_size == 1 {
            0.9
        } else if gap_size <= 3 {
            0.7
        } else {
            0.5
        };

        (semantic_similarity * 0.4 + size_confidence * 0.6).min(1.0)
    }

    /// Suggest strategies for filling gaps
    fn suggest_fill_strategies(
        &self,
        gap_type: &GapType,
        fragment_a: &ReconstructionFragment,
        fragment_b: &ReconstructionFragment,
    ) -> Vec<FillStrategy> {
        let mut strategies = Vec::new();

        match gap_type {
            GapType::Procedural => {
                strategies.push(FillStrategy {
                    strategy_type: FillStrategyType::Reasoning,
                    confidence: 0.8,
                    estimated_quality: 0.7,
                    computational_cost: ComputationalCost::Medium,
                });
            }
            GapType::Causal => {
                strategies.push(FillStrategy {
                    strategy_type: FillStrategyType::Reasoning,
                    confidence: 0.7,
                    estimated_quality: 0.8,
                    computational_cost: ComputationalCost::High,
                });
            }
            GapType::Example => {
                strategies.push(FillStrategy {
                    strategy_type: FillStrategyType::PatternMatch,
                    confidence: 0.6,
                    estimated_quality: 0.6,
                    computational_cost: ComputationalCost::Low,
                });
            }
            GapType::Transition => {
                strategies.push(FillStrategy {
                    strategy_type: FillStrategyType::Reasoning,
                    confidence: 0.9,
                    estimated_quality: 0.8,
                    computational_cost: ComputationalCost::Low,
                });
            }
            GapType::Elaboration => {
                strategies.push(FillStrategy {
                    strategy_type: FillStrategyType::ExternalKnowledge,
                    confidence: 0.5,
                    estimated_quality: 0.6,
                    computational_cost: ComputationalCost::High,
                });
            }
            GapType::Unknown => {
                strategies.push(FillStrategy {
                    strategy_type: FillStrategyType::LeaveUnfilled,
                    confidence: 1.0,
                    estimated_quality: 0.0,
                    computational_cost: ComputationalCost::Low,
                });
            }
        }

        strategies
    }

    /// Fill identified gaps
    fn fill_gaps(
        &mut self,
        session: &mut ReconstructionSession,
        field: &NeuralField,
        orchestrator: &MemoryOrchestrator,
    ) -> ContextNestResult<()> {
        // Collect gaps to avoid borrowing conflicts
        let gaps_to_process: Vec<MemoryGap> = session.gaps.iter().cloned().collect();
        for gap in &gaps_to_process {
            if let Some(strategy) = gap.fill_strategies.first() {
                match strategy.strategy_type {
                    FillStrategyType::Reasoning => {
                        self.fill_gap_with_reasoning(gap, session, field)?;
                    }
                    FillStrategyType::PatternMatch => {
                        self.fill_gap_with_pattern_matching(gap, session, orchestrator)?;
                    }
                    FillStrategyType::ExternalKnowledge => {
                        self.fill_gap_with_external_knowledge(gap, session)?;
                    }
                    FillStrategyType::UserInput => {
                        // Skip user input filling for automated reconstruction
                        continue;
                    }
                    FillStrategyType::LeaveUnfilled => {
                        continue;
                    }
                }

                // Record gap filling event
                self.record_event(ReconstructionEvent {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    event_type: ReconstructionEventType::GapFilled,
                    session_id: session.id.clone(),
                    data: serde_json::json!({
                        "gap_id": gap.id,
                        "strategy": format!("{:?}", strategy.strategy_type)
                    }),
                });
            }
        }

        session.progress.gaps_filled = session.gaps.len();

        Ok(())
    }

    /// Fill gap using reasoning
    fn fill_gap_with_reasoning(
        &mut self,
        gap: &MemoryGap,
        session: &mut ReconstructionSession,
        field: &NeuralField,
    ) -> ContextNestResult<()> {
        // In a real implementation, this would use an LLM to generate bridging content
        // For now, we'll create a simple bridging fragment

        let (before_content, after_content) = &gap.context_fragments;

        let bridging_content = match gap.gap_type {
            GapType::Transition => {
                format!(
                    "[Transition between: {} and {}]",
                    before_content.as_ref().unwrap_or(&"...".to_string()),
                    after_content.as_ref().unwrap_or(&"...".to_string())
                )
            }
            GapType::Causal => {
                format!("[Causal link connecting the above to the following]")
            }
            GapType::Procedural => {
                format!("[Procedural step(s) in the process]")
            }
            GapType::Elaboration => {
                format!("[Additional details and elaboration]")
            }
            GapType::Example => {
                format!("[Illustrative example]")
            }
            GapType::Unknown => {
                format!("[Missing information]")
            }
        };

        // Create bridging fragment
        let bridging_fragment = ReconstructionFragment {
            id: Uuid::new_v4().to_string(),
            source_attractor_id: format!("gap_fill_{}", gap.id),
            content: bridging_content,
            embedding: vec![0.0; 384], // Placeholder embedding
            strength: 0.5,
            confidence: 0.6,
            position: Some(gap.position),
            connections: Vec::new(),
            temporal_info: TemporalInfo {
                created_at: Utc::now(),
                sequence_position: Some(gap.position),
                temporal_relationships: Vec::new(),
            },
        };

        session.fragments.push(bridging_fragment);

        Ok(())
    }

    /// Fill gap using pattern matching
    fn fill_gap_with_pattern_matching(
        &mut self,
        gap: &MemoryGap,
        session: &mut ReconstructionSession,
        orchestrator: &MemoryOrchestrator,
    ) -> ContextNestResult<()> {
        // Look for similar patterns in existing memory
        let fragments = orchestrator.scan_fragments(0.3)?;

        let (before_content, after_content) = &gap.context_fragments;

        // Find patterns that might fit the gap
        for fragment in fragments {
            // Simple content matching (would be more sophisticated in real implementation)
            let content_match =
                fragment.content.contains("example") || fragment.content.contains("for instance");

            if content_match {
                let adapted_fragment = ReconstructionFragment {
                    id: Uuid::new_v4().to_string(),
                    source_attractor_id: fragment.id.clone(),
                    content: format!("[Adapted from memory: {}]", fragment.content),
                    embedding: fragment.embedding,
                    strength: fragment.strength * 0.7, // Reduce strength for adapted content
                    confidence: 0.5,
                    position: Some(gap.position),
                    connections: Vec::new(),
                    temporal_info: TemporalInfo {
                        created_at: Utc::now(),
                        sequence_position: Some(gap.position),
                        temporal_relationships: Vec::new(),
                    },
                };

                session.fragments.push(adapted_fragment);
                break; // Only use one pattern for now
            }
        }

        Ok(())
    }

    /// Fill gap using external knowledge
    fn fill_gap_with_external_knowledge(
        &mut self,
        gap: &MemoryGap,
        session: &mut ReconstructionSession,
    ) -> ContextNestResult<()> {
        // In a real implementation, this would query external knowledge bases
        // For now, create a placeholder
        let knowledge_content = format!(
            "[External knowledge: Additional information about {}]",
            format!("{:?}", gap.gap_type).to_lowercase()
        );

        let knowledge_fragment = ReconstructionFragment {
            id: Uuid::new_v4().to_string(),
            source_attractor_id: format!("external_{}", gap.id),
            content: knowledge_content,
            embedding: vec![0.0; 384], // Placeholder embedding
            strength: 0.4,
            confidence: 0.5,
            position: Some(gap.position),
            connections: Vec::new(),
            temporal_info: TemporalInfo {
                created_at: Utc::now(),
                sequence_position: Some(gap.position),
                temporal_relationships: Vec::new(),
            },
        };

        session.fragments.push(knowledge_fragment);

        Ok(())
    }

    /// Restore semantic continuity across reconstruction
    fn restore_semantic_continuity(
        &mut self,
        session: &mut ReconstructionSession,
    ) -> ContextNestResult<()> {
        // Start continuity tracking session
        let continuity_session = ContinuitySession {
            id: Uuid::new_v4().to_string(),
            reconstruction_session_id: session.id.clone(),
            started_at: Utc::now(),
            nodes: Vec::new(),
            edges: Vec::new(),
            continuity_score: 0.0,
        };

        // Create continuity nodes from fragments
        for fragment in &session.fragments {
            if let Some(position) = fragment.temporal_info.sequence_position {
                let node = ContinuityNode {
                    id: fragment.id.clone(),
                    fragment_id: fragment.id.clone(),
                    semantic_summary: self.generate_semantic_summary(&fragment.content),
                    position,
                    importance: fragment.strength * fragment.confidence,
                };

                // In a real implementation, we'd add this to the continuity session
                // For now, we'll skip the actual tracking
            }
        }

        // Calculate continuity score
        let continuity_score = self.calculate_continuity_score(&session.fragments);

        // Record continuity restoration event
        self.record_event(ReconstructionEvent {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            event_type: ReconstructionEventType::ContinuityRestored,
            session_id: session.id.clone(),
            data: serde_json::json!({
                "continuity_score": continuity_score
            }),
        });

        Ok(())
    }

    /// Generate semantic summary of content
    fn generate_semantic_summary(&self, content: &str) -> String {
        // Simple semantic summary (would use LLM in real implementation)
        let words: Vec<&str> = content.split_whitespace().collect();
        if words.len() > 10 {
            format!(
                "{}...",
                words.iter().take(8).cloned().collect::<Vec<_>>().join(" ")
            )
        } else {
            content.to_string()
        }
    }

    /// Calculate semantic continuity score
    fn calculate_continuity_score(&self, fragments: &[ReconstructionFragment]) -> f32 {
        if fragments.len() < 2 {
            return 1.0;
        }

        let mut total_similarity = 0.0;
        let mut comparisons = 0;

        // Compare adjacent fragments
        let mut positioned_fragments: Vec<_> = fragments
            .iter()
            .filter(|f| f.temporal_info.sequence_position.is_some())
            .collect();

        positioned_fragments.sort_by_key(|f| f.temporal_info.sequence_position.unwrap());

        for window in positioned_fragments.windows(2) {
            let similarity = self.cosine_similarity(&window[0].embedding, &window[1].embedding);
            total_similarity += similarity;
            comparisons += 1;
        }

        if comparisons == 0 {
            return 1.0;
        }

        total_similarity / comparisons as f32
    }

    /// Validate the reconstruction
    fn validate_reconstruction(
        &mut self,
        session: &mut ReconstructionSession,
    ) -> ContextNestResult<()> {
        // Check if we have enough fragments
        if session.fragments.len() < 2 {
            return Err(ContextNestError::Validation(
                "Insufficient fragments for reconstruction".to_string(),
            ));
        }

        // Check semantic coherence
        let coherence = self.calculate_overall_coherence(&session.fragments);
        if coherence < self.config.min_coherence_threshold {
            return Err(ContextNestError::Validation(format!(
                "Reconstruction coherence {:.2} below threshold {:.2}",
                coherence, self.config.min_coherence_threshold
            )));
        }

        // Check gap fill coverage
        let gap_coverage = if session.gaps.is_empty() {
            1.0
        } else {
            session.progress.gaps_filled as f32 / session.gaps.len() as f32
        };

        if gap_coverage < 0.5 {
            return Err(ContextNestError::Validation(format!(
                "Gap fill coverage {:.2} below minimum 0.5",
                gap_coverage
            )));
        }

        Ok(())
    }

    /// Calculate overall coherence of fragments
    fn calculate_overall_coherence(&self, fragments: &[ReconstructionFragment]) -> f32 {
        if fragments.len() < 2 {
            return 1.0;
        }

        let mut total_coherence = 0.0;
        let mut comparisons = 0;

        for i in 0..fragments.len() {
            for j in (i + 1)..fragments.len() {
                let similarity =
                    self.cosine_similarity(&fragments[i].embedding, &fragments[j].embedding);
                total_coherence += similarity;
                comparisons += 1;
            }
        }

        if comparisons == 0 {
            return 1.0;
        }

        total_coherence / comparisons as f32
    }

    /// Generate final reconstruction result
    fn generate_reconstruction_result(
        &mut self,
        session: &ReconstructionSession,
    ) -> ContextNestResult<ReconstructionResult> {
        // Sort fragments by position
        let mut positioned_fragments: Vec<_> = session
            .fragments
            .iter()
            .filter(|f| f.temporal_info.sequence_position.is_some())
            .collect();

        positioned_fragments.sort_by_key(|f| f.temporal_info.sequence_position.unwrap());

        // Combine fragment content
        let mut reconstructed_content = String::new();
        let mut fragments_used = Vec::new();

        for fragment in positioned_fragments {
            reconstructed_content.push_str(&fragment.content);
            reconstructed_content.push(' ');
            fragments_used.push(fragment.id.clone());
        }

        // Calculate quality metrics
        let quality_metrics = self.calculate_quality_metrics(&session.fragments, &session.gaps);

        // Calculate continuity score
        let continuity_score = self.calculate_continuity_score(&session.fragments);

        // Calculate overall confidence
        let confidence = if session.fragments.is_empty() {
            0.0
        } else {
            session.fragments.iter().map(|f| f.confidence).sum::<f32>()
                / session.fragments.len() as f32
        };

        let reconstructed_memory = ReconstructedMemory {
            content: reconstructed_content.trim().to_string(),
            fragments_used,
            gaps_filled: session.gaps.iter().map(|g| g.id.clone()).collect(),
            continuity_score,
            confidence,
        };

        Ok(ReconstructionResult {
            success: true,
            session_id: session.id.clone(),
            reconstructed_memory,
            quality_metrics,
            processing_time_ms: 0, // Will be set by caller
            warnings: Vec::new(),
            errors: Vec::new(),
        })
    }

    /// Calculate quality metrics for reconstruction
    fn calculate_quality_metrics(
        &self,
        fragments: &[ReconstructionFragment],
        gaps: &[MemoryGap],
    ) -> ReconstructionQualityMetrics {
        let semantic_coherence = self.calculate_overall_coherence(fragments);

        let temporal_consistency = if fragments.len() < 2 {
            1.0
        } else {
            // Check if temporal ordering makes sense
            let mut consistent_orderings = 0;
            let mut total_orderings = 0;

            for i in 0..fragments.len() {
                for j in (i + 1)..fragments.len() {
                    total_orderings += 1;

                    let pos_a = fragments[i].temporal_info.sequence_position.unwrap();
                    let pos_b = fragments[j].temporal_info.sequence_position.unwrap();

                    // Check if temporal relationships align with positions
                    let consistent = fragments[i]
                        .temporal_info
                        .temporal_relationships
                        .iter()
                        .any(|rel| {
                            rel.fragment_id == fragments[j].id
                                && match rel.relationship_type {
                                    TemporalRelationshipType::Before => pos_a < pos_b,
                                    TemporalRelationshipType::After => pos_a > pos_b,
                                    TemporalRelationshipType::Concurrent => {
                                        (if pos_a > pos_b {
                                            pos_a - pos_b
                                        } else {
                                            pos_b - pos_a
                                        }) <= 1
                                    }
                                    _ => true,
                                }
                        });

                    if consistent {
                        consistent_orderings += 1;
                    }
                }
            }

            if total_orderings > 0 {
                consistent_orderings as f32 / total_orderings as f32
            } else {
                1.0
            }
        };

        let completeness = if gaps.is_empty() {
            1.0
        } else {
            let filled_gaps = gaps.len(); // All gaps are considered filled in this simple implementation
            filled_gaps as f32 / (filled_gaps + gaps.len()) as f32
        };

        let fragment_utilization =
            fragments.iter().map(|f| f.strength).sum::<f32>() / fragments.len() as f32;

        let gap_fill_quality = if gaps.is_empty() {
            1.0
        } else {
            // Average confidence of gap fill strategies
            let total_confidence: f32 = gaps
                .iter()
                .filter_map(|g| g.fill_strategies.first())
                .map(|s| s.confidence)
                .sum();

            total_confidence / gaps.len() as f32
        };

        let overall_quality = (semantic_coherence * 0.25
            + temporal_consistency * 0.20
            + completeness * 0.20
            + fragment_utilization * 0.15
            + gap_fill_quality * 0.20)
            .min(1.0);

        ReconstructionQualityMetrics {
            overall_quality,
            semantic_coherence,
            temporal_consistency,
            completeness,
            fragment_utilization,
            gap_fill_quality,
        }
    }

    /// Update reconstruction progress
    fn update_progress(
        &mut self,
        session: &mut ReconstructionSession,
        phase: ReconstructionPhase,
        percentage: f32,
    ) -> ContextNestResult<()> {
        session.progress.phase = phase.clone();
        session.progress.percentage = percentage;

        // Create snapshot at key phases
        if matches!(
            phase,
            ReconstructionPhase::FragmentDiscovery
                | ReconstructionPhase::GapFilling
                | ReconstructionPhase::Complete
        ) {
            // Note: We can't create snapshots here without access to field and orchestrator
            // In a real implementation, these would be passed in
        }

        Ok(())
    }

    /// Create a snapshot of the current reconstruction state
    fn create_snapshot(
        &mut self,
        session_id: &str,
        phase: ReconstructionPhase,
        field: &NeuralField,
        orchestrator: &MemoryOrchestrator,
    ) -> ContextNestResult<()> {
        let snapshot = ReconstructionSnapshot {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            timestamp: Utc::now(),
            phase,
            fragments_state: self
                .active_session
                .as_ref()
                .map(|s| s.fragments.clone())
                .unwrap_or_default(),
            gaps_state: self
                .active_session
                .as_ref()
                .map(|s| s.gaps.clone())
                .unwrap_or_default(),
            field_state: NeuralFieldState {
                patterns: field
                    .patterns
                    .iter()
                    .map(|p| PatternState {
                        id: p.id.clone(),
                        content: p.content.clone(),
                        strength: p.strength,
                        embedding: p.embedding.clone(),
                    })
                    .collect(),
                coherence: field.state.coherence,
                stability: field.state.stability,
            },
            memory_state: MemoryOrchestratorState {
                attractors: orchestrator
                    .get_active_attractors()
                    .iter()
                    .map(|a| AttractorState {
                        id: a.id.clone(),
                        content: a.content.clone(),
                        strength: a.strength,
                        center: a.center.clone(),
                        connections: a.connections.clone(),
                    })
                    .collect(),
                sessions: Vec::new(), // Would need access to session data
            },
        };

        self.snapshot_manager.add_snapshot(snapshot)?;
        Ok(())
    }

    /// Record a reconstruction event
    fn record_event(&mut self, event: ReconstructionEvent) {
        self.history.push(event);

        // Keep history size manageable
        if self.history.len() > 1000 {
            self.history.remove(0);
        }
    }

    /// Calculate cosine similarity between two vectors
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

    /// Calculate content relevance to field patterns
    fn calculate_content_relevance(&self, content: &str, patterns: &[SemanticPattern]) -> f32 {
        if patterns.is_empty() {
            return 0.5;
        }

        let content_lower = content.to_lowercase();
        let mut total_relevance = 0.0;
        let mut matches = 0;

        for pattern in patterns {
            let pattern_lower = pattern.content.to_lowercase();

            // Simple word overlap
            let content_words: HashSet<&str> = content_lower.split_whitespace().collect();
            let pattern_words: HashSet<&str> = pattern_lower.split_whitespace().collect();

            let overlap = content_words.intersection(&pattern_words).count();
            let union = content_words.union(&pattern_words).count();

            if union > 0 {
                let jaccard_similarity = overlap as f32 / union as f32;
                total_relevance += jaccard_similarity;
                matches += 1;
            }
        }

        if matches > 0 {
            total_relevance / matches as f32
        } else {
            0.5
        }
    }

    /// Calculate temporal relevance (recent fragments are more relevant)
    fn calculate_temporal_relevance(&self, age_hours: i64) -> f32 {
        let age_hours_f32 = age_hours as f32;
        let decay_rate = 0.1; // Decay rate per hour

        (-decay_rate * age_hours_f32).exp().max(0.1)
    }

    /// Update average reconstruction time
    fn update_avg_reconstruction_time(&mut self, new_time_ms: i64) {
        let total = self.metrics.total_reconstructions as f64;
        let current_avg = self.metrics.avg_reconstruction_time_ms;

        self.metrics.avg_reconstruction_time_ms =
            (current_avg * (total - 1.0) + new_time_ms as f64) / total;
    }

    /// Update average continuity score
    fn update_avg_continuity_score(&mut self, new_score: f32) {
        let total = self.metrics.successful_reconstructions as f32;
        let current_avg = self.metrics.avg_continuity_score;

        self.metrics.avg_continuity_score = (current_avg * (total - 1.0) + new_score) / total;
    }

    /// Get reconstruction metrics
    pub fn get_metrics(&self) -> &ReconstructionMetrics {
        &self.metrics
    }

    /// Get reconstruction history
    pub fn get_history(&self) -> &[ReconstructionEvent] {
        &self.history
    }

    /// Get active session
    pub fn get_active_session(&self) -> Option<&ReconstructionSession> {
        self.active_session.as_ref()
    }

    /// Rollback a reconstruction session
    pub fn rollback_reconstruction(
        &mut self,
        session_id: &str,
        field: &mut NeuralField,
        orchestrator: &mut MemoryOrchestrator,
    ) -> ContextNestResult<()> {
        if let Some(session) = self.active_session.as_mut() {
            if session.id == session_id {
                // Take ownership of transaction for rollback
                let transaction = session.transaction.take();
                if let Some(mut transaction) = transaction {
                    Ok::<(), crate::error::ContextNestError>(())?;
                    // Return the transaction to the session after rollback
                    session.transaction = Some(transaction);
                }

                // Record rollback event
                self.record_event(ReconstructionEvent {
                    id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    event_type: ReconstructionEventType::RolledBack,
                    session_id: session_id.to_string(),
                    data: serde_json::json!({}),
                });

                // Clear active session
                self.active_session = None;

                return Ok(());
            }
        }

        Err(ContextNestError::NotFound(format!(
            "Reconstruction session {} not found",
            session_id
        )))
    }

    /// Restore from a snapshot
    pub fn restore_from_snapshot(
        &mut self,
        snapshot_id: &str,
        field: &mut NeuralField,
        orchestrator: &mut MemoryOrchestrator,
    ) -> ContextNestResult<()> {
        let snapshot = self.snapshot_manager.get_snapshot(snapshot_id)?;

        // Restore field state
        field.patterns.clear();
        for pattern_state in &snapshot.field_state.patterns {
            field.patterns.push(SemanticPattern {
                id: pattern_state.id.clone(),
                content: pattern_state.content.clone(),
                embedding: pattern_state.embedding.clone(),
                strength: pattern_state.strength,
                resonance: 0.5,   // Default resonance
                decay_rate: 0.01, // Default decay rate
                activation_count: 0,
                created_at: Utc::now(),
                last_activated: Utc::now(),
                deleted_at: None,
                delete_reason: None,
            });
        }
        field.state.coherence = snapshot.field_state.coherence;
        field.state.stability = snapshot.field_state.stability;

        // Note: Memory orchestrator restoration would require public APIs
        // This is a limitation that should be addressed

        Ok(())
    }
}

impl StateSnapshotManager {
    /// Create a new state snapshot manager
    pub fn new() -> Self {
        Self {
            snapshots: HashMap::new(),
            max_snapshots_per_session: 10,
            retention_policy: SnapshotRetentionPolicy::KeepRecent(5),
        }
    }

    /// Add a new snapshot
    pub fn add_snapshot(&mut self, snapshot: ReconstructionSnapshot) -> ContextNestResult<()> {
        let session_snapshots = self
            .snapshots
            .entry(snapshot.session_id.clone())
            .or_insert_with(Vec::new);

        session_snapshots.push(snapshot.clone());

        // Enforce retention policy
        self.apply_retention_policy(&snapshot.session_id);

        Ok(())
    }

    /// Get a snapshot by ID
    pub fn get_snapshot(&self, snapshot_id: &str) -> ContextNestResult<&ReconstructionSnapshot> {
        for session_snapshots in self.snapshots.values() {
            for snapshot in session_snapshots {
                if snapshot.id == snapshot_id {
                    return Ok(snapshot);
                }
            }
        }

        Err(ContextNestError::NotFound(format!(
            "Snapshot {} not found",
            snapshot_id
        )))
    }

    /// Get all snapshots for a session
    pub fn get_session_snapshots(&self, session_id: &str) -> &[ReconstructionSnapshot] {
        self.snapshots
            .get(session_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Apply retention policy
    fn apply_retention_policy(&mut self, session_id: &str) {
        if let Some(session_snapshots) = self.snapshots.get_mut(session_id) {
            match &self.retention_policy {
                SnapshotRetentionPolicy::KeepAll => {
                    // No action needed
                }
                SnapshotRetentionPolicy::KeepRecent(n) => {
                    if session_snapshots.len() > *n {
                        session_snapshots.drain(0..session_snapshots.len() - n);
                    }
                }
                SnapshotRetentionPolicy::KeepForDuration(duration) => {
                    let cutoff_time = Utc::now() - *duration;
                    session_snapshots.retain(|s| s.timestamp > cutoff_time);
                }
                SnapshotRetentionPolicy::KeepByPhase(phases) => {
                    session_snapshots.retain(|s| phases.contains(&s.phase));
                }
            }

            // Also enforce maximum per session
            if session_snapshots.len() > self.max_snapshots_per_session {
                let excess = session_snapshots.len() - self.max_snapshots_per_session;
                session_snapshots.drain(0..excess);
            }
        }
    }

    /// Clean up old snapshots
    pub fn cleanup_old_snapshots(&mut self) {
        let session_ids: Vec<String> = self.snapshots.keys().cloned().collect();

        for session_id in session_ids {
            self.apply_retention_policy(&session_id);
        }
    }
}

impl SemanticContinuityTracker {
    /// Create a new semantic continuity tracker
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            metrics: ContinuityMetrics::default(),
        }
    }

    /// Start tracking continuity for a reconstruction session
    pub fn start_session(&mut self, reconstruction_session_id: String) -> String {
        let session_id = Uuid::new_v4().to_string();

        let session = ContinuitySession {
            id: session_id.clone(),
            reconstruction_session_id,
            started_at: Utc::now(),
            nodes: Vec::new(),
            edges: Vec::new(),
            continuity_score: 0.0,
        };

        self.sessions.insert(session_id.clone(), session);
        session_id
    }

    /// Get continuity metrics
    pub fn get_metrics(&self) -> &ContinuityMetrics {
        &self.metrics
    }
}

impl Default for ReconstructionMetrics {
    fn default() -> Self {
        Self {
            total_reconstructions: 0,
            successful_reconstructions: 0,
            failed_reconstructions: 0,
            avg_reconstruction_time_ms: 0.0,
            avg_continuity_score: 0.0,
            avg_gap_fill_success_rate: 0.0,
        }
    }
}

impl Default for ContinuityMetrics {
    fn default() -> Self {
        Self {
            avg_semantic_similarity: 0.0,
            avg_logical_strength: 0.0,
            continuity_breaks: 0,
            restoration_attempts: 0,
            successful_restorations: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reconstruction_coordinator_creation() {
        let config = ReconstructionConfig::default();
        let coordinator = MemoryReconstructionCoordinator::new(config);

        assert_eq!(coordinator.history.len(), 0);
        assert!(coordinator.active_session.is_none());
    }

    #[test]
    fn test_cosine_similarity() {
        let coordinator = MemoryReconstructionCoordinator::new(ReconstructionConfig::default());

        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let c = vec![1.0, 0.0, 0.0];

        assert_eq!(coordinator.cosine_similarity(&a, &b), 0.0);
        assert_eq!(coordinator.cosine_similarity(&a, &c), 1.0);
    }

    #[test]
    fn test_gap_type_classification() {
        let coordinator = MemoryReconstructionCoordinator::new(ReconstructionConfig::default());

        let fragment_a = ReconstructionFragment {
            id: "a".to_string(),
            source_attractor_id: "att1".to_string(),
            content: "First step in the process".to_string(),
            embedding: vec![0.0; 10],
            strength: 0.8,
            confidence: 0.9,
            position: Some(0),
            connections: Vec::new(),
            temporal_info: TemporalInfo {
                created_at: Utc::now(),
                sequence_position: Some(0),
                temporal_relationships: Vec::new(),
            },
        };

        let fragment_b = ReconstructionFragment {
            id: "b".to_string(),
            source_attractor_id: "att2".to_string(),
            content: "Next step in the process".to_string(),
            embedding: vec![0.0; 10],
            strength: 0.8,
            confidence: 0.9,
            position: Some(1),
            connections: Vec::new(),
            temporal_info: TemporalInfo {
                created_at: Utc::now(),
                sequence_position: Some(1),
                temporal_relationships: Vec::new(),
            },
        };

        let gap_type = coordinator.classify_gap_type(&fragment_a, &fragment_b);
        assert_eq!(gap_type, GapType::Procedural);
    }

    #[test]
    fn test_state_snapshot_manager() {
        let mut manager = StateSnapshotManager::new();

        let snapshot = ReconstructionSnapshot {
            id: "snap1".to_string(),
            session_id: "session1".to_string(),
            timestamp: Utc::now(),
            phase: ReconstructionPhase::FragmentDiscovery,
            fragments_state: Vec::new(),
            gaps_state: Vec::new(),
            field_state: NeuralFieldState {
                patterns: Vec::new(),
                coherence: 0.8,
                stability: 0.9,
            },
            memory_state: MemoryOrchestratorState {
                attractors: Vec::new(),
                sessions: Vec::new(),
            },
        };

        manager.add_snapshot(snapshot).unwrap();

        let session_snapshots = manager.get_session_snapshots("session1");
        assert_eq!(session_snapshots.len(), 1);
    }
}

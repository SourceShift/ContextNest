//! Adaptive Decay System for Memory Persistence
//! Implements importance-weighted memory decay with adaptive learning
//! and context-aware persistence strategies.

use crate::error::ContextNestResult;
use crate::error::{ContextNestError, Result};
use crate::memory::attractors::{utils, ComponentStatus, MemoryAttractorConfig, MemoryFragment};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock as AsyncRwLock;

/// Adaptive decay system for memory persistence
#[derive(Debug)]
pub struct AdaptiveDecaySystem {
    /// Configuration
    config: MemoryAttractorConfig,
    /// Decay profiles for different memory types
    decay_profiles: Arc<RwLock<HashMap<String, DecayProfile>>>,
    /// Memory importance tracker
    importance_tracker: Arc<AsyncRwLock<ImportanceTracker>>,
    /// Decay scheduler
    scheduler: Arc<DecayScheduler>,
    /// Decay statistics
    statistics: Arc<RwLock<DecayStatistics>>,
    /// Component status
    status: Arc<RwLock<ComponentStatus>>,
}

/// Decay profile for specific memory types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayProfile {
    /// Profile ID
    pub id: String,
    /// Profile name
    pub name: String,
    /// Base decay rate
    pub base_decay_rate: f32,
    /// Importance decay modifier
    pub importance_modifier: f32,
    /// Access frequency modifier
    pub frequency_modifier: f32,
    /// Recency modifier
    pub recency_modifier: f32,
    /// Context relevance modifier
    pub context_modifier: f32,
    /// Minimum persistence time
    pub min_persistence: Duration,
    /// Maximum persistence time
    pub max_persistence: Duration,
    /// Adaptive learning enabled
    pub adaptive_learning: bool,
}

/// Memory importance tracking system
#[derive(Debug)]
pub struct ImportanceTracker {
    /// Importance records for memories
    importance_records: HashMap<String, ImportanceRecord>,
    /// Access patterns
    access_patterns: HashMap<String, AccessPattern>,
    /// Context associations
    context_associations: HashMap<String, Vec<ContextAssociation>>,
    /// Learning data
    learning_data: VecDeque<LearningSample>,
}

/// Importance record for a memory
#[derive(Debug, Clone)]
pub struct ImportanceRecord {
    /// Memory ID
    pub memory_id: String,
    /// Current importance score
    pub current_importance: f32,
    /// Base importance
    pub base_importance: f32,
    /// Importance history
    pub importance_history: VecDeque<ImportanceSnapshot>,
    /// Last update
    pub last_updated: DateTime<Utc>,
    /// Decay profile used
    pub decay_profile: String,
}

/// Importance snapshot at a point in time
#[derive(Debug, Clone)]
pub struct ImportanceSnapshot {
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Importance value
    pub importance: f32,
    /// Triggering event
    pub event: ImportanceEvent,
    /// Context information
    pub context: HashMap<String, f32>,
}

/// Events that affect importance
#[derive(Debug, Clone)]
pub enum ImportanceEvent {
    /// Memory access
    Access,
    /// Memory modification
    Modification,
    /// Association with other memories
    Association,
    /// External importance signal
    ExternalSignal(f32),
    /// System maintenance
    Maintenance,
}

/// Access pattern for memory
#[derive(Debug, Clone)]
pub struct AccessPattern {
    /// Memory ID
    pub memory_id: String,
    /// Total access count
    pub total_accesses: usize,
    /// Recent accesses (last 24 hours)
    pub recent_accesses: VecDeque<DateTime<Utc>>,
    /// Access frequency (per hour)
    pub access_frequency: f32,
    /// Access intervals
    pub access_intervals: VecDeque<Duration>,
    /// Predicted next access
    pub predicted_next_access: Option<DateTime<Utc>>,
}

/// Context association for memory
#[derive(Debug, Clone)]
pub struct ContextAssociation {
    /// Context ID
    pub context_id: String,
    /// Association strength
    pub strength: f32,
    /// Association type
    pub association_type: AssociationType,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Last reinforced
    pub last_reinforced: DateTime<Utc>,
}

/// Types of associations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssociationType {
    /// Semantic similarity
    Semantic,
    /// Temporal proximity
    Temporal,
    /// Causal relationship
    Causal,
    /// Category membership
    Category,
    /// User-defined relationship
    UserDefined,
}

/// Learning sample for adaptive decay
#[derive(Debug, Clone)]
pub struct LearningSample {
    /// Memory ID
    pub memory_id: String,
    /// Initial importance
    pub initial_importance: f32,
    /// Final importance
    pub final_importance: f32,
    /// Time elapsed
    pub time_elapsed: Duration,
    /// Number of accesses
    pub access_count: usize,
    /// Decay rate observed
    pub observed_decay_rate: f32,
    /// Context factors
    pub context_factors: HashMap<String, f32>,
}

/// Decay scheduler for managing decay operations
#[derive(Debug)]
pub struct DecayScheduler {
    /// Scheduled decay operations. Wrapped in `RwLock` because the scheduler
    /// is shared via `&self` (the scheduler instance is owned by the
    /// `AdaptiveDecaySystem`) but operations are pushed/popped from multiple
    /// methods that don't take `&mut self`.
    scheduled_operations: RwLock<VecDeque<ScheduledDecayOperation>>,
    /// Running decay operations
    running_operations: HashMap<String, RunningDecayOperation>,
    /// Decay execution history
    execution_history: VecDeque<DecayExecution>,
}

/// Scheduled decay operation
#[derive(Debug, Clone)]
pub struct ScheduledDecayOperation {
    /// Operation ID
    pub id: String,
    /// Memory IDs to process
    pub memory_ids: Vec<String>,
    /// Scheduled execution time
    pub scheduled_time: DateTime<Utc>,
    /// Priority
    pub priority: DecayPriority,
    /// Operation type
    pub operation_type: DecayOperationType,
}

/// Priority levels for decay operations
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DecayPriority {
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

/// Types of decay operations
#[derive(Debug, Clone)]
pub enum DecayOperationType {
    /// Regular decay update
    RegularUpdate,
    /// Forced decay due to memory pressure
    ForcedDecay,
    /// Importance recalculation
    ImportanceRecalculation,
    /// Profile optimization
    ProfileOptimization,
}

/// Running decay operation
#[derive(Debug, Clone)]
pub struct RunningDecayOperation {
    /// Operation ID
    pub id: String,
    /// Start time
    pub start_time: DateTime<Utc>,
    /// Progress percentage
    pub progress: f32,
    /// Estimated completion time
    pub estimated_completion: Option<DateTime<Utc>>,
    /// Memory IDs processed
    pub processed_ids: Vec<String>,
    /// Memory IDs remaining
    pub remaining_ids: Vec<String>,
}

/// Decay execution record
#[derive(Debug, Clone)]
pub struct DecayExecution {
    /// Execution ID
    pub id: String,
    /// Operation type
    pub operation_type: DecayOperationType,
    /// Start time
    pub start_time: DateTime<Utc>,
    /// End time
    pub end_time: Option<DateTime<Utc>>,
    /// Memories processed
    pub memories_processed: usize,
    /// Success rate
    pub success_rate: f32,
    /// Errors encountered
    pub errors: Vec<String>,
}

/// Decay statistics tracking
#[derive(Debug, Clone, Default)]
pub struct DecayStatistics {
    /// Total decay operations
    pub total_operations: usize,
    /// Successful operations
    pub successful_operations: usize,
    /// Average decay time
    pub avg_decay_time: Duration,
    /// Average importance retention
    pub avg_importance_retention: f32,
    /// Memory evictions
    pub memory_evictions: usize,
    /// Profile optimizations
    pub profile_optimizations: usize,
    /// Learning samples collected
    pub learning_samples: usize,
    /// Performance metrics
    pub performance_metrics: DecayPerformanceMetrics,
}

/// Performance metrics for decay system
#[derive(Debug, Clone, Default)]
pub struct DecayPerformanceMetrics {
    /// Operations per second
    pub operations_per_second: f32,
    /// Memory efficiency
    pub memory_efficiency: f32,
    /// Accuracy of importance predictions
    pub prediction_accuracy: f32,
    /// System responsiveness
    pub responsiveness_ms: f64,
}

impl AdaptiveDecaySystem {
    /// Create a new adaptive decay system
    pub fn new(config: MemoryAttractorConfig) -> Self {
        Self {
            config: config.clone(),
            decay_profiles: Arc::new(RwLock::new(HashMap::new())),
            importance_tracker: Arc::new(AsyncRwLock::new(ImportanceTracker::new())),
            scheduler: Arc::new(DecayScheduler::new()),
            statistics: Arc::new(RwLock::new(DecayStatistics::default())),
            status: Arc::new(RwLock::new(ComponentStatus::Initializing)),
        }
    }

    /// Initialize the adaptive decay system

    pub async fn initialize(&self) -> ContextNestResult<()> {
        // Create default decay profiles
        self.create_default_profiles()?;

        *self.status.write().unwrap() = ComponentStatus::Running;
        Ok(())
    }

    /// Register a memory for decay tracking

    pub async fn register_memory(
        &self,
        memory_id: String,
        base_importance: f32,
        profile_id: String,
    ) -> ContextNestResult<()> {
        let mut tracker = self.importance_tracker.write().await;

        // Create importance record
        let record = ImportanceRecord {
            memory_id: memory_id.clone(),
            current_importance: base_importance,
            base_importance,
            importance_history: VecDeque::new(),
            last_updated: Utc::now(),
            decay_profile: profile_id,
        };

        tracker.importance_records.insert(memory_id.clone(), record);

        // Create access pattern
        let pattern = AccessPattern {
            memory_id: memory_id.clone(),
            total_accesses: 0,
            recent_accesses: VecDeque::new(),
            access_frequency: 0.0,
            access_intervals: VecDeque::new(),
            predicted_next_access: None,
        };

        tracker.access_patterns.insert(memory_id, pattern);

        Ok(())
    }

    /// Update memory importance based on access

    pub async fn update_importance_on_access(
        &self,
        memory_id: &str,
        context_factors: HashMap<String, f32>,
    ) -> ContextNestResult<f32> {
        let mut tracker = self.importance_tracker.write().await;

        // Update access pattern
        if let Some(pattern) = tracker.access_patterns.get_mut(memory_id) {
            let now = Utc::now();
            pattern.total_accesses += 1;
            pattern.recent_accesses.push_back(now);

            // Keep only recent accesses (last 24 hours)
            let cutoff = now - chrono::Duration::hours(24);
            while let Some(&front) = pattern.recent_accesses.front() {
                if front < cutoff {
                    pattern.recent_accesses.pop_front();
                } else {
                    break;
                }
            }

            // Update access frequency
            pattern.access_frequency = pattern.recent_accesses.len() as f32 / 24.0;

            // Update access intervals
            if pattern.recent_accesses.len() > 1 {
                let accesses: Vec<_> = pattern.recent_accesses.iter().cloned().collect();
                for i in 1..accesses.len() {
                    // chrono::Duration -> std::time::Duration via to_std
                    // (negative durations are impossible here because the
                    // VecDeque is push_back ordered, so unwrap is safe).
                    let interval = accesses[i]
                        .signed_duration_since(accesses[i - 1])
                        .to_std()
                        .unwrap_or_default();
                    pattern.access_intervals.push_back(interval);
                }

                // Keep only recent intervals
                while pattern.access_intervals.len() > 100 {
                    pattern.access_intervals.pop_front();
                }

                // Predict next access based on average interval
                if !pattern.access_intervals.is_empty() {
                    let avg_interval = pattern.access_intervals.iter().sum::<Duration>()
                        / pattern.access_intervals.len() as u32;
                    let avg_chrono = chrono::Duration::from_std(avg_interval).unwrap_or_default();
                    pattern.predicted_next_access = Some(now + avg_chrono);
                }
            }
        }

        // Update importance record
        if let Some(record) = tracker.importance_records.get_mut(memory_id) {
            let old_importance = record.current_importance;

            // Calculate new importance
            let new_importance = self.calculate_updated_importance(
                record,
                &ImportanceEvent::Access,
                &context_factors,
            )?;

            record.current_importance = new_importance;
            record.last_updated = Utc::now();

            // Add to history
            // Clone here because the same `context_factors` map is consumed
            // again below for the learning_data sample. Both consumers want
            // owned maps for offline analysis; the clone is once-per-access
            // and only as wide as the active context set.
            record.importance_history.push_back(ImportanceSnapshot {
                timestamp: Utc::now(),
                importance: new_importance,
                event: ImportanceEvent::Access,
                context: context_factors.clone(),
            });

            // Keep history manageable
            while record.importance_history.len() > 1000 {
                record.importance_history.pop_front();
            }

            // Add learning sample
            tracker.learning_data.push_back(LearningSample {
                memory_id: memory_id.to_string(),
                initial_importance: old_importance,
                final_importance: new_importance,
                time_elapsed: Duration::from_secs(1),
                access_count: 1,
                observed_decay_rate: (old_importance - new_importance).max(0.0),
                context_factors,
            });

            // Keep learning data manageable
            while tracker.learning_data.len() > 10000 {
                tracker.learning_data.pop_front();
            }

            Ok(new_importance)
        } else {
            Err(ContextNestError::NotFound(format!(
                "Memory {} not found in importance tracker",
                memory_id
            )))
        }
    }

    /// Apply decay to all registered memories

    pub async fn apply_decay(&self, time_delta: Duration) -> ContextNestResult<DecayResult> {
        let start_time = Utc::now();

        let mut tracker = self.importance_tracker.write().await;
        let mut memories_processed = 0;
        let mut memories_evicted = 0;
        let mut total_importance_loss = 0.0;

        let profiles = self.decay_profiles.read().unwrap();

        // Snapshot access_patterns before iterating importance_records mutably:
        // calculate_frequency_decay_factor needs &access_patterns but the loop
        // already holds &mut importance_records, and Rust can't field-split
        // across method-call boundaries. Clone is per-decay-cycle, not per-access,
        // so the cost is bounded.
        let access_patterns_snapshot = tracker.access_patterns.clone();

        for (memory_id, record) in tracker.importance_records.iter_mut() {
            // Get decay profile
            let profile = profiles.get(&record.decay_profile).ok_or_else(|| {
                ContextNestError::Configuration(format!(
                    "Decay profile {} not found",
                    record.decay_profile
                ))
            })?;

            // Calculate decay factors
            let importance_factor = self.calculate_importance_decay_factor(record, profile);
            let frequency_factor = self.calculate_frequency_decay_factor(
                memory_id,
                &access_patterns_snapshot,
                profile,
            );
            let recency_factor = self.calculate_recency_decay_factor(record, profile);

            // Apply combined decay
            let combined_decay_factor = importance_factor * frequency_factor * recency_factor;
            let decay_amount = record.current_importance
                * profile.base_decay_rate
                * combined_decay_factor
                * time_delta.as_secs_f32()
                / 3600.0; // Normalize to hours

            let old_importance = record.current_importance;
            record.current_importance = (old_importance - decay_amount).max(0.0);

            total_importance_loss += decay_amount;
            memories_processed += 1;

            // Check if memory should be evicted
            if record.current_importance < 0.01 {
                memories_evicted += 1;
            }

            // Update record
            record.last_updated = Utc::now();

            // Add to history
            record.importance_history.push_back(ImportanceSnapshot {
                timestamp: Utc::now(),
                importance: record.current_importance,
                event: ImportanceEvent::Maintenance,
                context: HashMap::new(),
            });
        }

        let execution_time = Utc::now()
            .signed_duration_since(start_time)
            .to_std()
            .unwrap_or_default();

        // Update statistics
        self.update_statistics(|stats| {
            stats.total_operations += 1;
            stats.successful_operations += 1;
            stats.avg_decay_time = (stats.avg_decay_time
                * (stats.successful_operations - 1) as u32
                + Duration::from_millis(execution_time.as_millis() as u64))
                / stats.successful_operations as u32;
            stats.memory_evictions += memories_evicted;
        });

        Ok(DecayResult {
            memories_processed,
            memories_evicted,
            total_importance_loss,
            execution_time,
            success: true,
        })
    }

    /// Schedule decay operation

    pub fn schedule_decay_operation(
        &self,
        operation: ScheduledDecayOperation,
    ) -> ContextNestResult<()> {
        let mut scheduler = self.scheduler.scheduled_operations.write().unwrap();
        scheduler.push_back(operation);
        Ok(())
    }

    /// Process scheduled decay operations

    pub async fn process_scheduled_operations(&self) -> ContextNestResult<Vec<DecayExecution>> {
        let mut executions = Vec::new();
        let now = Utc::now();

        let mut scheduler = self.scheduler.scheduled_operations.write().unwrap();
        let mut remaining_operations = VecDeque::new();

        while let Some(operation) = scheduler.pop_front() {
            if operation.scheduled_time <= now {
                let execution = self.execute_decay_operation(operation).await?;
                executions.push(execution);
            } else {
                remaining_operations.push_back(operation);
            }
        }

        *scheduler = remaining_operations;
        Ok(executions)
    }

    /// Create or update decay profile

    pub fn update_decay_profile(&self, profile: DecayProfile) -> ContextNestResult<()> {
        let mut profiles = self.decay_profiles.write().unwrap();
        profiles.insert(profile.id.clone(), profile);
        Ok(())
    }

    /// Get current importance for a memory

    pub async fn get_importance(&self, memory_id: &str) -> ContextNestResult<f32> {
        let tracker = self.importance_tracker.read().await;
        if let Some(record) = tracker.importance_records.get(memory_id) {
            Ok(record.current_importance)
        } else {
            Err(ContextNestError::NotFound(format!(
                "Memory {} not found",
                memory_id
            )))
        }
    }

    /// Get memories that should be evicted

    pub async fn get_eviction_candidates(
        &self,
        threshold: f32,
        limit: usize,
    ) -> ContextNestResult<Vec<String>> {
        let tracker = self.importance_tracker.read().await;
        let mut candidates = Vec::new();

        for (memory_id, record) in tracker.importance_records.iter() {
            if record.current_importance < threshold {
                candidates.push(memory_id.clone());
                if candidates.len() >= limit {
                    break;
                }
            }
        }

        // Sort by importance (lowest first)
        candidates.sort_by(|a, b| {
            let importance_a = tracker
                .importance_records
                .get(a)
                .unwrap()
                .current_importance;
            let importance_b = tracker
                .importance_records
                .get(b)
                .unwrap()
                .current_importance;
            importance_a.partial_cmp(&importance_b).unwrap()
        });

        Ok(candidates)
    }

    /// Optimize decay profiles based on learning data

    pub async fn optimize_profiles(&self) -> ContextNestResult<ProfileOptimizationResult> {
        let tracker = self.importance_tracker.read().await;
        let mut profiles = self.decay_profiles.write().unwrap();

        let mut optimizations = Vec::new();

        // Analyze learning data to optimize profiles
        let learning_samples: Vec<_> = tracker.learning_data.iter().collect();
        if learning_samples.len() < 100 {
            return Ok(ProfileOptimizationResult {
                optimized_profiles: optimizations,
                improvement_score: 0.0,
                samples_analyzed: learning_samples.len(),
            });
        }

        for (profile_id, profile) in profiles.iter_mut() {
            if profile.adaptive_learning {
                // Calculate optimal decay rate for this profile. `learning_samples`
                // is already `&[LearningSample]`; .iter() yields `&LearningSample`.
                // .filter passes us `&&LearningSample` (reference-to-iterator-item),
                // so .copied() unifies the levels back to `&LearningSample`.
                let profile_samples: Vec<&LearningSample> = learning_samples
                    .iter()
                    .filter(|s| {
                        tracker
                            .importance_records
                            .get(&s.memory_id)
                            .map(|r| r.decay_profile == *profile_id)
                            .unwrap_or(false)
                    })
                    .copied()
                    .collect();

                if profile_samples.len() >= 10 {
                    let optimal_rate = self.calculate_optimal_decay_rate(&profile_samples);
                    let improvement =
                        (optimal_rate - profile.base_decay_rate).abs() / profile.base_decay_rate;

                    if improvement > 0.1 {
                        // 10% improvement threshold
                        let old_rate = profile.base_decay_rate;
                        profile.base_decay_rate = optimal_rate;

                        optimizations.push(ProfileOptimization {
                            profile_id: profile_id.clone(),
                            parameter: "base_decay_rate".to_string(),
                            old_value: old_rate,
                            new_value: optimal_rate,
                            improvement,
                        });
                    }
                }
            }
        }

        let improvement_score = if optimizations.is_empty() {
            0.0
        } else {
            optimizations.iter().map(|o| o.improvement).sum::<f32>() / optimizations.len() as f32
        };

        // Update statistics
        self.update_statistics(|stats| {
            stats.profile_optimizations += optimizations.len();
        });

        Ok(ProfileOptimizationResult {
            optimized_profiles: optimizations,
            improvement_score,
            samples_analyzed: learning_samples.len(),
        })
    }

    /// Get decay statistics

    pub fn get_statistics(&self) -> DecayStatistics {
        self.statistics.read().unwrap().clone()
    }

    // Helper methods

    fn create_default_profiles(&self) -> ContextNestResult<()> {
        let mut profiles = self.decay_profiles.write().unwrap();

        // Default profile for general memories
        profiles.insert(
            "default".to_string(),
            DecayProfile {
                id: "default".to_string(),
                name: "Default Memory Profile".to_string(),
                base_decay_rate: self.config.decay_rate,
                importance_modifier: 0.5,
                frequency_modifier: 0.3,
                recency_modifier: 0.2,
                context_modifier: 0.1,
                min_persistence: Duration::from_secs(300), // 5 minutes
                max_persistence: Duration::from_secs(30 * 24 * 3600), // 30 days
                adaptive_learning: true,
            },
        );

        // High importance profile
        profiles.insert(
            "high_importance".to_string(),
            DecayProfile {
                id: "high_importance".to_string(),
                name: "High Importance Memory".to_string(),
                base_decay_rate: self.config.decay_rate * 0.1, // Slower decay
                importance_modifier: 0.8,
                frequency_modifier: 0.1,
                recency_modifier: 0.05,
                context_modifier: 0.05,
                min_persistence: Duration::from_secs(3600), // 1 hour
                max_persistence: Duration::from_secs(90 * 24 * 3600), // 90 days
                adaptive_learning: true,
            },
        );

        // Temporary profile for transient memories
        profiles.insert(
            "temporary".to_string(),
            DecayProfile {
                id: "temporary".to_string(),
                name: "Temporary Memory".to_string(),
                base_decay_rate: self.config.decay_rate * 3.0, // Faster decay
                importance_modifier: 0.2,
                frequency_modifier: 0.4,
                recency_modifier: 0.3,
                context_modifier: 0.1,
                min_persistence: Duration::from_secs(10), // 10 seconds
                max_persistence: Duration::from_secs(3600), // 1 hour
                adaptive_learning: false,
            },
        );

        Ok(())
    }

    fn calculate_updated_importance(
        &self,
        record: &ImportanceRecord,
        event: &ImportanceEvent,
        context_factors: &HashMap<String, f32>,
    ) -> ContextNestResult<f32> {
        let profile = self.decay_profiles.read().unwrap();
        let decay_profile = profile.get(&record.decay_profile).ok_or_else(|| {
            ContextNestError::Configuration(format!(
                "Decay profile {} not found",
                record.decay_profile
            ))
        })?;

        // Apply event-specific changes. Clippy flagged the previous
        // `let mut = 0.0` pre-assignment as never-read because every
        // match arm overwrites it; bind via the match expression instead.
        let mut importance_change: f32 = match event {
            ImportanceEvent::Access => 0.1 * decay_profile.importance_modifier,
            ImportanceEvent::Modification => 0.2 * decay_profile.importance_modifier,
            ImportanceEvent::Association => 0.05 * decay_profile.importance_modifier,
            ImportanceEvent::ExternalSignal(signal) => *signal * decay_profile.importance_modifier,
            ImportanceEvent::Maintenance => -0.01, // Small decrease during maintenance
        };

        // Apply context factors
        for (key, &value) in context_factors {
            match key.as_str() {
                "urgency" => importance_change += value * 0.1,
                "relevance" => importance_change += value * 0.15,
                "emotional" => importance_change += value * 0.2,
                _ => {} // Unknown context factor
            }
        }

        // Calculate new importance with bounds
        let new_importance = (record.current_importance + importance_change).clamp(0.0, 1.0);

        Ok(new_importance)
    }

    fn calculate_importance_decay_factor(
        &self,
        record: &ImportanceRecord,
        profile: &DecayProfile,
    ) -> f32 {
        let importance_factor = 1.0 - (record.current_importance * profile.importance_modifier);
        importance_factor.max(0.1) // Minimum decay factor
    }

    fn calculate_frequency_decay_factor(
        &self,
        memory_id: &str,
        access_patterns: &HashMap<String, AccessPattern>,
        profile: &DecayProfile,
    ) -> f32 {
        if let Some(pattern) = access_patterns.get(memory_id) {
            let frequency_factor =
                1.0 - (pattern.access_frequency / 24.0).min(1.0) * profile.frequency_modifier;
            frequency_factor.max(0.1)
        } else {
            1.0 // No access pattern, full decay
        }
    }

    fn calculate_recency_decay_factor(
        &self,
        record: &ImportanceRecord,
        profile: &DecayProfile,
    ) -> f32 {
        let time_since_update = Utc::now()
            .signed_duration_since(record.last_updated)
            .to_std()
            .unwrap_or_default()
            .as_secs_f32()
            / 3600.0; // Hours
        let recency_factor = 1.0 - (-time_since_update / 24.0).exp() * profile.recency_modifier;
        recency_factor.max(0.1)
    }

    async fn execute_decay_operation(
        &self,
        operation: ScheduledDecayOperation,
    ) -> ContextNestResult<DecayExecution> {
        let start_time = Utc::now();
        let mut memories_processed = 0;
        let mut success_count = 0;
        let mut errors = Vec::new();

        match operation.operation_type {
            DecayOperationType::RegularUpdate => {
                let result = self.apply_decay(Duration::from_secs(3600)).await?; // 1 hour
                memories_processed = result.memories_processed;
                success_count = if result.success {
                    memories_processed
                } else {
                    0
                };
            }
            DecayOperationType::ImportanceRecalculation => {
                // Recalculate importance for specified memories
                for memory_id in &operation.memory_ids {
                    match self.recalculate_importance(memory_id).await {
                        Ok(_) => success_count += 1,
                        Err(e) => errors.push(e.to_string()),
                    }
                    memories_processed += 1;
                }
            }
            DecayOperationType::ProfileOptimization => {
                let result = self.optimize_profiles().await?;
                memories_processed = result.samples_analyzed;
                success_count = result.optimized_profiles.len();
            }
            DecayOperationType::ForcedDecay => {
                // Apply aggressive decay
                let result = self.apply_decay(Duration::from_secs(3600 * 6)).await?; // 6 hours
                memories_processed = result.memories_processed;
                success_count = if result.success {
                    memories_processed
                } else {
                    0
                };
            }
        }

        let success_rate = if memories_processed > 0 {
            success_count as f32 / memories_processed as f32
        } else {
            0.0
        };

        Ok(DecayExecution {
            id: operation.id,
            operation_type: operation.operation_type,
            start_time,
            end_time: Some(Utc::now()),
            memories_processed,
            success_rate,
            errors,
        })
    }

    async fn recalculate_importance(&self, memory_id: &str) -> ContextNestResult<()> {
        let mut tracker = self.importance_tracker.write().await;

        if let Some(record) = tracker.importance_records.get_mut(memory_id) {
            // Reset to base importance and recalculate based on history
            let mut calculated_importance = record.base_importance;

            // Apply influence from recent history
            for snapshot in record.importance_history.iter().rev().take(10) {
                let age_hours = Utc::now()
                    .signed_duration_since(snapshot.timestamp)
                    .to_std()
                    .unwrap_or_default()
                    .as_secs_f32()
                    / 3600.0;
                let influence = (-age_hours / 24.0).exp(); // Decay over days

                match snapshot.event {
                    ImportanceEvent::Access => calculated_importance += 0.05 * influence,
                    ImportanceEvent::Modification => calculated_importance += 0.1 * influence,
                    ImportanceEvent::ExternalSignal(signal) => {
                        calculated_importance += signal * influence
                    }
                    _ => {}
                }
            }

            record.current_importance = calculated_importance.clamp(0.0, 1.0);
            record.last_updated = Utc::now();
        }

        Ok(())
    }

    fn calculate_optimal_decay_rate(&self, samples: &[&LearningSample]) -> f32 {
        if samples.is_empty() {
            return 0.1; // Default decay rate
        }

        // Calculate average observed decay rate weighted by recency
        let mut weighted_sum = 0.0;
        let mut total_weight = 0.0;

        for sample in samples {
            let recency_weight = (-sample.time_elapsed.as_secs_f32() / (24.0 * 3600.0)).exp(); // Decay over days
            weighted_sum += sample.observed_decay_rate * recency_weight;
            total_weight += recency_weight;
        }

        if total_weight > 0.0 {
            (weighted_sum / total_weight).clamp(0.001, 1.0) // Reasonable bounds
        } else {
            0.1
        }
    }

    fn update_statistics<F>(&self, update_fn: F)
    where
        F: FnOnce(&mut DecayStatistics),
    {
        let mut stats = self.statistics.write().unwrap();
        update_fn(&mut stats);

        // Update performance metrics
        if stats.total_operations > 0 {
            stats.performance_metrics.operations_per_second =
                stats.total_operations as f32 / stats.avg_decay_time.as_secs_f32();
        }
    }
}

impl ImportanceTracker {
    fn new() -> Self {
        Self {
            importance_records: HashMap::new(),
            access_patterns: HashMap::new(),
            context_associations: HashMap::new(),
            learning_data: VecDeque::new(),
        }
    }
}

impl DecayScheduler {
    fn new() -> Self {
        Self {
            scheduled_operations: RwLock::new(VecDeque::new()),
            running_operations: HashMap::new(),
            execution_history: VecDeque::new(),
        }
    }
}

/// Result of decay operation
#[derive(Debug, Clone)]
pub struct DecayResult {
    /// Number of memories processed
    pub memories_processed: usize,
    /// Number of memories evicted
    pub memories_evicted: usize,
    /// Total importance loss
    pub total_importance_loss: f32,
    /// Execution time
    pub execution_time: Duration,
    /// Operation success
    pub success: bool,
}

/// Profile optimization result
#[derive(Debug, Clone)]
pub struct ProfileOptimizationResult {
    /// Optimized profiles
    pub optimized_profiles: Vec<ProfileOptimization>,
    /// Overall improvement score
    pub improvement_score: f32,
    /// Number of samples analyzed
    pub samples_analyzed: usize,
}

/// Individual profile optimization
#[derive(Debug, Clone)]
pub struct ProfileOptimization {
    /// Profile ID
    pub profile_id: String,
    /// Parameter optimized
    pub parameter: String,
    /// Old value
    pub old_value: f32,
    /// New value
    pub new_value: f32,
    /// Improvement achieved
    pub improvement: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::attractors::MemoryAttractorConfig;

    #[tokio::test]
    async fn test_adaptive_decay_system() {
        let config = MemoryAttractorConfig::default();
        let decay_system = AdaptiveDecaySystem::new(config);
        decay_system.initialize().await.unwrap();

        // Register a memory
        decay_system
            .register_memory("test_memory".to_string(), 0.8, "default".to_string())
            .await
            .unwrap();

        // Update importance on access
        let importance = decay_system
            .update_importance_on_access("test_memory", HashMap::new())
            .await
            .unwrap();

        assert!(importance > 0.0);

        // Get importance
        let retrieved_importance = decay_system.get_importance("test_memory").await.unwrap();
        assert_eq!(importance, retrieved_importance);
    }

    #[tokio::test]
    async fn test_decay_application() {
        let config = MemoryAttractorConfig {
            decay_rate: 0.1,
            ..Default::default()
        };
        let decay_system = AdaptiveDecaySystem::new(config);
        decay_system.initialize().await.unwrap();

        // Register memory
        decay_system
            .register_memory("test_memory".to_string(), 0.8, "default".to_string())
            .await
            .unwrap();

        // Apply decay
        let result = decay_system
            .apply_decay(Duration::from_secs(3600))
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.memories_processed, 1);
        assert!(result.total_importance_loss > 0.0);

        // Check that importance decreased
        let new_importance = decay_system.get_importance("test_memory").await.unwrap();
        assert!(new_importance < 0.8);
    }

    #[tokio::test]
    async fn test_eviction_candidates() {
        let config = MemoryAttractorConfig::default();
        let decay_system = AdaptiveDecaySystem::new(config);
        decay_system.initialize().await.unwrap();

        // Register memories with different importance levels
        decay_system
            .register_memory("high_importance".to_string(), 0.9, "default".to_string())
            .await
            .unwrap();

        decay_system
            .register_memory("low_importance".to_string(), 0.05, "default".to_string())
            .await
            .unwrap();

        // Get eviction candidates
        let candidates = decay_system.get_eviction_candidates(0.1, 10).await.unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0], "low_importance");
    }

    #[test]
    fn test_decay_profile_creation() {
        let profile = DecayProfile {
            id: "test".to_string(),
            name: "Test Profile".to_string(),
            base_decay_rate: 0.1,
            importance_modifier: 0.5,
            frequency_modifier: 0.3,
            recency_modifier: 0.2,
            context_modifier: 0.1,
            min_persistence: Duration::from_secs(300),
            max_persistence: Duration::from_secs(86400),
            adaptive_learning: true,
        };

        assert_eq!(profile.base_decay_rate, 0.1);
        assert_eq!(profile.importance_modifier, 0.5);
        assert!(profile.adaptive_learning);
    }

    #[test]
    fn test_access_pattern() {
        let mut pattern = AccessPattern {
            memory_id: "test".to_string(),
            total_accesses: 0,
            recent_accesses: VecDeque::new(),
            access_frequency: 0.0,
            access_intervals: VecDeque::new(),
            predicted_next_access: None,
        };

        let now = Utc::now();

        // Simulate accesses
        for i in 0..5 {
            pattern
                .recent_accesses
                .push_back(now + Duration::from_secs(i * 3600));
        }

        pattern.total_accesses = 5;
        pattern.access_frequency = 5.0 / 24.0;

        assert_eq!(pattern.total_accesses, 5);
        assert!(pattern.access_frequency > 0.0);
        assert_eq!(pattern.recent_accesses.len(), 5);
    }

    #[tokio::test]
    async fn test_profile_optimization() {
        let config = MemoryAttractorConfig::default();
        let decay_system = AdaptiveDecaySystem::new(config);
        decay_system.initialize().await.unwrap();

        let result = decay_system.optimize_profiles().await.unwrap();

        // Should not fail even with no learning data
        let _ = result;
    }
}

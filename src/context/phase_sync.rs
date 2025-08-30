use crate::error::ContextNestResult;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::f32::consts::PI;

/// Phase synchronization state for a field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldPhase {
    /// Current phase angle (0 to 2π)
    pub phase: f32,
    /// Phase velocity (radians per second)
    pub frequency: f32,
    /// Phase amplitude (intensity of oscillations)
    pub amplitude: f32,
    /// Last phase update timestamp
    pub last_update: chrono::DateTime<chrono::Utc>,
}

impl Default for FieldPhase {
    fn default() -> Self {
        Self {
            phase: 0.0,
            frequency: 1.0, // 1 rad/s default frequency
            amplitude: 1.0,
            last_update: chrono::Utc::now(),
        }
    }
}

impl FieldPhase {
    /// Create a new field phase with custom parameters
    pub fn new(frequency: f32, amplitude: f32) -> Self {
        Self {
            phase: 0.0,
            frequency,
            amplitude,
            last_update: chrono::Utc::now(),
        }
    }

    /// Update phase based on elapsed time
    pub fn update(&mut self, now: chrono::DateTime<chrono::Utc>) {
        let elapsed = (now - self.last_update).num_milliseconds() as f32 / 1000.0;
        self.phase = (self.phase + self.frequency * elapsed) % (2.0 * PI);
        self.last_update = now;
    }

    /// Get the current oscillation value (-amplitude to +amplitude)
    pub fn oscillation_value(&self) -> f32 {
        self.amplitude * self.phase.sin()
    }

    /// Get phase difference with another field phase
    pub fn phase_difference(&self, other: &FieldPhase) -> f32 {
        let diff = (self.phase - other.phase).abs();
        if diff > PI {
            2.0 * PI - diff
        } else {
            diff
        }
    }

    /// Check if this phase is synchronized with another (within threshold)
    pub fn is_synchronized_with(&self, other: &FieldPhase, threshold: f32) -> bool {
        self.phase_difference(other) < threshold
    }
}

/// Phase synchronization group configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncGroup {
    /// Group identifier
    pub id: String,
    /// Field IDs in this sync group
    pub field_ids: Vec<String>,
    /// Target synchronization frequency
    pub target_frequency: f32,
    /// Synchronization strength (coupling constant)
    pub coupling_strength: f32,
    /// Tolerance for phase differences
    pub sync_tolerance: f32,
    /// Group phase reference
    pub reference_phase: f32,
}

impl Default for SyncGroup {
    fn default() -> Self {
        Self {
            id: String::new(),
            field_ids: Vec::new(),
            target_frequency: 1.0,
            coupling_strength: 0.1,
            sync_tolerance: 0.1, // ~5.7 degrees
            reference_phase: 0.0,
        }
    }
}

/// Phase synchronization strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncStrategy {
    /// Weak coupling - fields influence each other gradually
    WeakCoupling,
    /// Strong coupling - rapid synchronization
    StrongCoupling,
    /// Master-slave - one field drives others
    MasterSlave { master_field_id: String },
    /// Consensus - all fields vote on phase
    Consensus,
    /// Adaptive - coupling strength changes based on synchronization level
    Adaptive {
        base_strength: f32,
        adaptation_rate: f32,
    },
}

/// Phase synchronization manager
pub struct PhaseSynchronizer {
    /// Field phases
    field_phases: HashMap<String, FieldPhase>,
    /// Synchronization groups
    sync_groups: HashMap<String, SyncGroup>,
    /// Global synchronization strategy
    strategy: SyncStrategy,
    /// Synchronization metrics
    metrics: SyncMetrics,
}

/// Synchronization performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncMetrics {
    /// Total synchronization events
    pub sync_events: usize,
    /// Average synchronization time
    pub avg_sync_time_ms: f32,
    /// Current synchronization quality (0.0 - 1.0)
    pub sync_quality: f32,
    /// Phase coherence across all fields
    pub global_coherence: f32,
    /// Number of desynchronization events
    pub desync_events: usize,
}

impl Default for SyncMetrics {
    fn default() -> Self {
        Self {
            sync_events: 0,
            avg_sync_time_ms: 0.0,
            sync_quality: 1.0,
            global_coherence: 1.0,
            desync_events: 0,
        }
    }
}

/// Result of synchronization operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    /// Fields that were synchronized
    pub synchronized_fields: Vec<String>,
    /// Phase adjustments made
    pub phase_adjustments: HashMap<String, f32>,
    /// Achieved synchronization quality
    pub sync_quality: f32,
    /// Time taken for synchronization
    pub duration_ms: u64,
}

impl PhaseSynchronizer {
    /// Create a new phase synchronizer
    pub fn new(strategy: SyncStrategy) -> Self {
        Self {
            field_phases: HashMap::new(),
            sync_groups: HashMap::new(),
            strategy,
            metrics: SyncMetrics::default(),
        }
    }

    /// Register a field for phase synchronization
    pub fn register_field(&mut self, field_id: String, initial_phase: FieldPhase) {
        self.field_phases.insert(field_id, initial_phase);
    }

    /// Remove a field from synchronization
    pub fn unregister_field(&mut self, field_id: &str) {
        self.field_phases.remove(field_id);

        // Remove from all sync groups
        for group in self.sync_groups.values_mut() {
            group.field_ids.retain(|id| id != field_id);
        }
    }

    /// Create a synchronization group
    pub fn create_sync_group(
        &mut self,
        group_id: String,
        field_ids: Vec<String>,
        target_frequency: f32,
        coupling_strength: f32,
    ) -> ContextNestResult<()> {
        // Validate that all fields exist
        for field_id in &field_ids {
            if !self.field_phases.contains_key(field_id) {
                return Err(crate::error::ContextNestError::Configuration(format!(
                    "Field {} not registered for phase sync",
                    field_id
                )));
            }
        }

        let group = SyncGroup {
            id: group_id.clone(),
            field_ids,
            target_frequency,
            coupling_strength,
            sync_tolerance: 0.1,
            reference_phase: 0.0,
        };

        self.sync_groups.insert(group_id, group);
        Ok(())
    }

    /// Remove a synchronization group
    pub fn remove_sync_group(&mut self, group_id: &str) {
        self.sync_groups.remove(group_id);
    }

    /// Update all field phases based on elapsed time
    pub fn update_phases(&mut self, now: chrono::DateTime<chrono::Utc>) {
        for phase in self.field_phases.values_mut() {
            phase.update(now);
        }
    }

    /// Synchronize fields within each group
    pub fn synchronize(&mut self) -> ContextNestResult<Vec<SyncResult>> {
        let mut results = Vec::new();
        let start_time = std::time::Instant::now();

        // Clone groups to avoid borrow conflicts
        let groups: Vec<SyncGroup> = self.sync_groups.values().cloned().collect();

        for group in groups {
            let result = self.sync_group(&group)?;
            results.push(result);
        }

        // Update metrics
        self.metrics.sync_events += 1;
        let duration = start_time.elapsed().as_millis() as f32;
        self.metrics.avg_sync_time_ms =
            (self.metrics.avg_sync_time_ms * (self.metrics.sync_events - 1) as f32 + duration)
                / self.metrics.sync_events as f32;

        // Calculate global coherence
        self.update_global_coherence();

        Ok(results)
    }

    /// Synchronize fields within a specific group
    fn sync_group(&mut self, group: &SyncGroup) -> ContextNestResult<SyncResult> {
        let start_time = std::time::Instant::now();
        let mut phase_adjustments = HashMap::new();

        // Clone strategy to avoid borrow conflicts
        let strategy = self.strategy.clone();

        match strategy {
            SyncStrategy::WeakCoupling => {
                self.apply_weak_coupling(group, &mut phase_adjustments)?;
            }
            SyncStrategy::StrongCoupling => {
                self.apply_strong_coupling(group, &mut phase_adjustments)?;
            }
            SyncStrategy::MasterSlave { master_field_id } => {
                self.apply_master_slave(group, &master_field_id, &mut phase_adjustments)?;
            }
            SyncStrategy::Consensus => {
                self.apply_consensus(group, &mut phase_adjustments)?;
            }
            SyncStrategy::Adaptive {
                base_strength,
                adaptation_rate,
            } => {
                self.apply_adaptive_coupling(
                    group,
                    base_strength,
                    adaptation_rate,
                    &mut phase_adjustments,
                )?;
            }
        }

        let duration = start_time.elapsed().as_millis() as u64;
        let sync_quality = self.calculate_group_sync_quality(group);

        Ok(SyncResult {
            synchronized_fields: group.field_ids.clone(),
            phase_adjustments,
            sync_quality,
            duration_ms: duration,
        })
    }

    /// Apply weak coupling synchronization
    fn apply_weak_coupling(
        &mut self,
        group: &SyncGroup,
        adjustments: &mut HashMap<String, f32>,
    ) -> ContextNestResult<()> {
        let coupling = group.coupling_strength;

        // Calculate phase center of mass
        let mut phase_sum = 0.0;
        let mut count = 0;

        for field_id in &group.field_ids {
            if let Some(phase) = self.field_phases.get(field_id) {
                phase_sum += phase.phase;
                count += 1;
            }
        }

        if count == 0 {
            return Ok(());
        }

        let center_phase = phase_sum / count as f32;

        // Apply weak coupling adjustment
        for field_id in &group.field_ids {
            if let Some(phase) = self.field_phases.get_mut(field_id) {
                let phase_diff = center_phase - phase.phase;
                let adjustment = coupling * phase_diff;

                phase.phase += adjustment;
                phase.phase = phase.phase % (2.0 * PI);

                adjustments.insert(field_id.clone(), adjustment);
            }
        }

        Ok(())
    }

    /// Apply strong coupling synchronization
    fn apply_strong_coupling(
        &mut self,
        group: &SyncGroup,
        adjustments: &mut HashMap<String, f32>,
    ) -> ContextNestResult<()> {
        // Calculate the average phase using circular statistics
        let mut sin_sum = 0.0;
        let mut cos_sum = 0.0;
        let mut count = 0;

        for field_id in &group.field_ids {
            if let Some(phase) = self.field_phases.get(field_id) {
                sin_sum += phase.phase.sin();
                cos_sum += phase.phase.cos();
                count += 1;
            }
        }

        if count == 0 {
            return Ok(());
        }

        let target_phase = (sin_sum / count as f32).atan2(cos_sum / count as f32);

        // Strong coupling: immediately set all phases to target
        for field_id in &group.field_ids {
            if let Some(phase) = self.field_phases.get_mut(field_id) {
                let adjustment = target_phase - phase.phase;
                phase.phase = target_phase;
                adjustments.insert(field_id.clone(), adjustment);
            }
        }

        Ok(())
    }

    /// Apply master-slave synchronization
    fn apply_master_slave(
        &mut self,
        group: &SyncGroup,
        master_field_id: &str,
        adjustments: &mut HashMap<String, f32>,
    ) -> ContextNestResult<()> {
        // Get master phase
        let master_phase = if let Some(master) = self.field_phases.get(master_field_id) {
            master.phase
        } else {
            return Err(crate::error::ContextNestError::Configuration(format!(
                "Master field {} not found",
                master_field_id
            )));
        };

        // Synchronize all slaves to master
        for field_id in &group.field_ids {
            if field_id != master_field_id {
                if let Some(phase) = self.field_phases.get_mut(field_id) {
                    let adjustment = master_phase - phase.phase;
                    phase.phase = master_phase;
                    adjustments.insert(field_id.clone(), adjustment);
                }
            }
        }

        Ok(())
    }

    /// Apply consensus synchronization
    fn apply_consensus(
        &mut self,
        group: &SyncGroup,
        adjustments: &mut HashMap<String, f32>,
    ) -> ContextNestResult<()> {
        // Use circular mean for consensus
        let mut sin_sum = 0.0;
        let mut cos_sum = 0.0;
        let mut count = 0;

        for field_id in &group.field_ids {
            if let Some(phase) = self.field_phases.get(field_id) {
                sin_sum += phase.phase.sin();
                cos_sum += phase.phase.cos();
                count += 1;
            }
        }

        if count == 0 {
            return Ok(());
        }

        let consensus_phase = (sin_sum / count as f32).atan2(cos_sum / count as f32);
        let coupling = group.coupling_strength;

        // Apply consensus adjustment
        for field_id in &group.field_ids {
            if let Some(phase) = self.field_phases.get_mut(field_id) {
                let phase_diff = consensus_phase - phase.phase;
                let adjustment = coupling * phase_diff;

                phase.phase += adjustment;
                phase.phase = phase.phase % (2.0 * PI);

                adjustments.insert(field_id.clone(), adjustment);
            }
        }

        Ok(())
    }

    /// Apply adaptive coupling synchronization
    fn apply_adaptive_coupling(
        &mut self,
        group: &SyncGroup,
        base_strength: f32,
        adaptation_rate: f32,
        adjustments: &mut HashMap<String, f32>,
    ) -> ContextNestResult<()> {
        // Calculate current sync quality
        let sync_quality = self.calculate_group_sync_quality(group);

        // Adapt coupling strength based on sync quality
        let adaptive_strength = base_strength * (1.0 + adaptation_rate * (1.0 - sync_quality));

        // Use weak coupling with adaptive strength
        let mut modified_group = group.clone();
        modified_group.coupling_strength = adaptive_strength;

        self.apply_weak_coupling(&modified_group, adjustments)
    }

    /// Calculate synchronization quality for a group
    fn calculate_group_sync_quality(&self, group: &SyncGroup) -> f32 {
        if group.field_ids.len() < 2 {
            return 1.0;
        }

        let mut total_coherence = 0.0;
        let mut comparisons = 0;

        for i in 0..group.field_ids.len() {
            for j in (i + 1)..group.field_ids.len() {
                if let (Some(phase1), Some(phase2)) = (
                    self.field_phases.get(&group.field_ids[i]),
                    self.field_phases.get(&group.field_ids[j]),
                ) {
                    let phase_diff = phase1.phase_difference(phase2);
                    let coherence = (PI - phase_diff) / PI; // 1.0 for perfect sync, 0.0 for opposite phases
                    total_coherence += coherence;
                    comparisons += 1;
                }
            }
        }

        if comparisons > 0 {
            total_coherence / comparisons as f32
        } else {
            1.0
        }
    }

    /// Update global coherence metric
    fn update_global_coherence(&mut self) {
        if self.field_phases.len() < 2 {
            self.metrics.global_coherence = 1.0;
            return;
        }

        let phases: Vec<&FieldPhase> = self.field_phases.values().collect();
        let mut total_coherence = 0.0;
        let mut comparisons = 0;

        for i in 0..phases.len() {
            for j in (i + 1)..phases.len() {
                let phase_diff = phases[i].phase_difference(phases[j]);
                let coherence = (PI - phase_diff) / PI;
                total_coherence += coherence;
                comparisons += 1;
            }
        }

        self.metrics.global_coherence = if comparisons > 0 {
            total_coherence / comparisons as f32
        } else {
            1.0
        };
    }

    /// Get phase synchronization metrics
    pub fn get_metrics(&self) -> &SyncMetrics {
        &self.metrics
    }

    /// Get current field phases
    pub fn get_field_phases(&self) -> &HashMap<String, FieldPhase> {
        &self.field_phases
    }

    /// Get synchronization groups
    pub fn get_sync_groups(&self) -> &HashMap<String, SyncGroup> {
        &self.sync_groups
    }

    /// Set synchronization strategy
    pub fn set_strategy(&mut self, strategy: SyncStrategy) {
        self.strategy = strategy;
    }

    /// Check if two fields are synchronized
    pub fn are_fields_synchronized(
        &self,
        field1_id: &str,
        field2_id: &str,
        tolerance: f32,
    ) -> bool {
        if let (Some(phase1), Some(phase2)) = (
            self.field_phases.get(field1_id),
            self.field_phases.get(field2_id),
        ) {
            phase1.is_synchronized_with(phase2, tolerance)
        } else {
            false
        }
    }

    /// Get oscillation values for pattern influence
    pub fn get_field_oscillation(&self, field_id: &str) -> Option<f32> {
        self.field_phases
            .get(field_id)
            .map(|phase| phase.oscillation_value())
    }

    /// Reset synchronization state
    pub fn reset(&mut self) {
        for phase in self.field_phases.values_mut() {
            phase.phase = 0.0;
            phase.last_update = chrono::Utc::now();
        }
        self.metrics = SyncMetrics::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_phase_creation() {
        let phase = FieldPhase::new(2.0, 1.5);
        assert_eq!(phase.frequency, 2.0);
        assert_eq!(phase.amplitude, 1.5);
        assert_eq!(phase.phase, 0.0);
    }

    #[test]
    fn test_phase_update() {
        let mut phase = FieldPhase::new(1.0, 1.0);
        let start = chrono::Utc::now();
        let later = start + chrono::Duration::seconds(1);

        phase.update(later);

        // Should have advanced by approximately 1 radian
        assert!((phase.phase - 1.0).abs() < 0.1);
    }

    #[test]
    fn test_phase_difference() {
        let phase1 = FieldPhase {
            phase: 0.0,
            ..Default::default()
        };
        let phase2 = FieldPhase {
            phase: PI / 2.0,
            ..Default::default()
        };

        let diff = phase1.phase_difference(&phase2);
        assert!((diff - PI / 2.0).abs() < 0.001);
    }

    #[test]
    fn test_phase_synchronization() {
        let phase1 = FieldPhase {
            phase: 0.1,
            ..Default::default()
        };
        let phase2 = FieldPhase {
            phase: 0.15,
            ..Default::default()
        };

        assert!(phase1.is_synchronized_with(&phase2, 0.1));
        assert!(!phase1.is_synchronized_with(&phase2, 0.01));
    }

    #[test]
    fn test_oscillation_value() {
        let phase = FieldPhase {
            phase: PI / 2.0,
            amplitude: 2.0,
            ..Default::default()
        };
        let osc = phase.oscillation_value();

        // sin(π/2) = 1, so 2.0 * 1 = 2.0
        assert!((osc - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_synchronizer_creation() {
        let sync = PhaseSynchronizer::new(SyncStrategy::WeakCoupling);
        assert_eq!(sync.field_phases.len(), 0);
        assert_eq!(sync.sync_groups.len(), 0);
    }

    #[test]
    fn test_field_registration() {
        let mut sync = PhaseSynchronizer::new(SyncStrategy::WeakCoupling);
        let phase = FieldPhase::new(1.0, 1.0);

        sync.register_field("field1".to_string(), phase);
        assert_eq!(sync.field_phases.len(), 1);

        sync.unregister_field("field1");
        assert_eq!(sync.field_phases.len(), 0);
    }

    #[test]
    fn test_sync_group_creation() {
        let mut sync = PhaseSynchronizer::new(SyncStrategy::WeakCoupling);

        // Register fields first
        sync.register_field("field1".to_string(), FieldPhase::default());
        sync.register_field("field2".to_string(), FieldPhase::default());

        let result = sync.create_sync_group(
            "group1".to_string(),
            vec!["field1".to_string(), "field2".to_string()],
            1.0,
            0.1,
        );

        assert!(result.is_ok());
        assert_eq!(sync.sync_groups.len(), 1);
    }

    #[test]
    fn test_sync_group_invalid_field() {
        let mut sync = PhaseSynchronizer::new(SyncStrategy::WeakCoupling);

        let result = sync.create_sync_group(
            "group1".to_string(),
            vec!["nonexistent".to_string()],
            1.0,
            0.1,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_weak_coupling_synchronization() {
        let mut sync = PhaseSynchronizer::new(SyncStrategy::WeakCoupling);

        // Register fields with different phases
        sync.register_field(
            "field1".to_string(),
            FieldPhase {
                phase: 0.0,
                ..Default::default()
            },
        );
        sync.register_field(
            "field2".to_string(),
            FieldPhase {
                phase: 1.0,
                ..Default::default()
            },
        );

        sync.create_sync_group(
            "group1".to_string(),
            vec!["field1".to_string(), "field2".to_string()],
            1.0,
            0.5, // Strong coupling for testing
        )
        .unwrap();

        let results = sync.synchronize().unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].sync_quality > 0.0);

        // Phases should be closer now
        let phase1 = sync.field_phases.get("field1").unwrap().phase;
        let phase2 = sync.field_phases.get("field2").unwrap().phase;
        assert!((phase1 - phase2).abs() < 1.0); // Should be closer than or equal to initial 1.0 difference
    }

    #[test]
    fn test_strong_coupling_synchronization() {
        let mut sync = PhaseSynchronizer::new(SyncStrategy::StrongCoupling);

        sync.register_field(
            "field1".to_string(),
            FieldPhase {
                phase: 0.0,
                ..Default::default()
            },
        );
        sync.register_field(
            "field2".to_string(),
            FieldPhase {
                phase: 1.0,
                ..Default::default()
            },
        );
        sync.register_field(
            "field3".to_string(),
            FieldPhase {
                phase: 2.0,
                ..Default::default()
            },
        );

        sync.create_sync_group(
            "group1".to_string(),
            vec![
                "field1".to_string(),
                "field2".to_string(),
                "field3".to_string(),
            ],
            1.0,
            1.0,
        )
        .unwrap();

        let results = sync.synchronize().unwrap();
        assert_eq!(results.len(), 1);

        // All phases should be exactly the same after strong coupling
        let phases: Vec<f32> = sync.field_phases.values().map(|p| p.phase).collect();
        for i in 1..phases.len() {
            assert!((phases[0] - phases[i]).abs() < 0.001);
        }
    }

    #[test]
    fn test_master_slave_synchronization() {
        let mut sync = PhaseSynchronizer::new(SyncStrategy::MasterSlave {
            master_field_id: "master".to_string(),
        });

        sync.register_field(
            "master".to_string(),
            FieldPhase {
                phase: 1.5,
                ..Default::default()
            },
        );
        sync.register_field(
            "slave1".to_string(),
            FieldPhase {
                phase: 0.0,
                ..Default::default()
            },
        );
        sync.register_field(
            "slave2".to_string(),
            FieldPhase {
                phase: 2.0,
                ..Default::default()
            },
        );

        sync.create_sync_group(
            "group1".to_string(),
            vec![
                "master".to_string(),
                "slave1".to_string(),
                "slave2".to_string(),
            ],
            1.0,
            1.0,
        )
        .unwrap();

        let results = sync.synchronize().unwrap();
        assert_eq!(results.len(), 1);

        // All slaves should match master phase
        let master_phase = sync.field_phases.get("master").unwrap().phase;
        let slave1_phase = sync.field_phases.get("slave1").unwrap().phase;
        let slave2_phase = sync.field_phases.get("slave2").unwrap().phase;

        assert_eq!(master_phase, 1.5); // Master unchanged
        assert_eq!(slave1_phase, 1.5); // Slave synced to master
        assert_eq!(slave2_phase, 1.5); // Slave synced to master
    }

    #[test]
    fn test_adaptive_coupling() {
        let mut sync = PhaseSynchronizer::new(SyncStrategy::Adaptive {
            base_strength: 0.1,
            adaptation_rate: 0.5,
        });

        sync.register_field(
            "field1".to_string(),
            FieldPhase {
                phase: 0.0,
                ..Default::default()
            },
        );
        sync.register_field(
            "field2".to_string(),
            FieldPhase {
                phase: 3.0,
                ..Default::default()
            },
        );

        sync.create_sync_group(
            "group1".to_string(),
            vec!["field1".to_string(), "field2".to_string()],
            1.0,
            0.1,
        )
        .unwrap();

        let results = sync.synchronize().unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].sync_quality >= 0.0);
    }

    #[test]
    fn test_field_synchronization_check() {
        let mut sync = PhaseSynchronizer::new(SyncStrategy::WeakCoupling);

        sync.register_field(
            "field1".to_string(),
            FieldPhase {
                phase: 0.1,
                ..Default::default()
            },
        );
        sync.register_field(
            "field2".to_string(),
            FieldPhase {
                phase: 0.15,
                ..Default::default()
            },
        );
        sync.register_field(
            "field3".to_string(),
            FieldPhase {
                phase: 1.0,
                ..Default::default()
            },
        );

        assert!(sync.are_fields_synchronized("field1", "field2", 0.1));
        assert!(!sync.are_fields_synchronized("field1", "field3", 0.1));
        assert!(!sync.are_fields_synchronized("field1", "nonexistent", 0.1));
    }

    #[test]
    fn test_oscillation_retrieval() {
        let mut sync = PhaseSynchronizer::new(SyncStrategy::WeakCoupling);

        sync.register_field(
            "field1".to_string(),
            FieldPhase {
                phase: PI / 2.0,
                amplitude: 2.0,
                ..Default::default()
            },
        );

        let osc = sync.get_field_oscillation("field1");
        assert!(osc.is_some());
        assert!((osc.unwrap() - 2.0).abs() < 0.001);

        let osc_none = sync.get_field_oscillation("nonexistent");
        assert!(osc_none.is_none());
    }

    #[test]
    fn test_synchronizer_reset() {
        let mut sync = PhaseSynchronizer::new(SyncStrategy::WeakCoupling);

        sync.register_field(
            "field1".to_string(),
            FieldPhase {
                phase: 1.5,
                ..Default::default()
            },
        );
        sync.metrics.sync_events = 10;
        sync.metrics.global_coherence = 0.5;

        sync.reset();

        assert_eq!(sync.field_phases.get("field1").unwrap().phase, 0.0);
        assert_eq!(sync.metrics.sync_events, 0);
        assert_eq!(sync.metrics.global_coherence, 1.0);
    }

    #[test]
    fn test_group_sync_quality_calculation() {
        let mut sync = PhaseSynchronizer::new(SyncStrategy::WeakCoupling);

        // Perfect synchronization
        sync.register_field(
            "field1".to_string(),
            FieldPhase {
                phase: 0.5,
                ..Default::default()
            },
        );
        sync.register_field(
            "field2".to_string(),
            FieldPhase {
                phase: 0.5,
                ..Default::default()
            },
        );

        let group = SyncGroup {
            id: "test".to_string(),
            field_ids: vec!["field1".to_string(), "field2".to_string()],
            ..Default::default()
        };

        let quality = sync.calculate_group_sync_quality(&group);
        assert!((quality - 1.0).abs() < 0.001); // Perfect sync should be 1.0

        // Opposite phases (worst sync)
        sync.field_phases.get_mut("field2").unwrap().phase = 0.5 + PI;
        let quality_bad = sync.calculate_group_sync_quality(&group);
        assert!(quality_bad < 0.1); // Should be very low
    }
}

//! Advanced Attractor Basin System
//! Implements sophisticated attractor basin dynamics for memory pattern stability
//! and convergence in the memory attractor system.

use crate::error::ContextNestResult;
use crate::error::{ContextNestError, Result};
use crate::memory::attractors::{utils, ComponentStatus, MemoryAttractorConfig, MemoryFragment};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock as AsyncRwLock;
use uuid::Uuid;

/// Advanced attractor basin with memory pattern stability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttractorBasin {
    /// Unique basin identifier
    pub id: String,
    /// Basin center vector in embedding space
    pub center: Vec<f32>,
    /// Basin radius (attraction range)
    pub radius: f32,
    /// Basin depth (attraction strength)
    pub depth: f32,
    /// Basin shape characteristics
    pub shape: BasinShape,
    /// Basin dynamics
    pub dynamics: BasinDynamics,
    /// Associated memory fragments
    pub associated_fragments: HashSet<String>,
    /// Basin health metrics
    pub health: BasinHealth,
    /// Basin creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp
    pub last_modified: DateTime<Utc>,
    /// Activation frequency
    pub activation_frequency: f32,
    /// Basin stability score
    pub stability_score: f32,
    /// Basin type
    pub basin_type: BasinType,
}

/// Basin shape characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasinShape {
    /// Shape type (spherical, ellipsoidal, irregular)
    pub shape_type: ShapeType,
    /// Shape parameters (varies by type)
    pub parameters: HashMap<String, f32>,
    /// Eccentricity for non-spherical basins
    pub eccentricity: f32,
    /// Orientation in embedding space
    pub orientation: Option<Vec<f32>>,
}

/// Types of basin shapes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ShapeType {
    /// Perfect sphere
    Spherical,
    /// Ellipsoid with different radii
    Ellipsoidal,
    /// Irregular shape defined by vertices
    Irregular,
    /// Adaptive shape that changes over time
    Adaptive,
}

/// Basin dynamics for evolution over time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasinDynamics {
    /// Movement velocity of basin center
    pub velocity: Vec<f32>,
    /// Expansion/contraction rate
    pub expansion_rate: f32,
    /// Depth change rate
    pub depth_change_rate: f32,
    /// Adaptation strength
    pub adaptation_strength: f32,
    /// Resonance frequency
    pub resonance_frequency: f32,
    /// Damping coefficient
    pub damping_coefficient: f32,
}

/// Basin health metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasinHealth {
    /// Overall health score (0-1)
    pub overall_health: f32,
    /// Pattern coherence
    pub pattern_coherence: f32,
    /// Fragment coverage
    pub fragment_coverage: f32,
    /// Stability measure
    pub stability: f32,
    /// Activity level
    pub activity_level: f32,
    /// Last health check
    pub last_health_check: DateTime<Utc>,
}

/// Types of attractor basins
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum BasinType {
    /// Primary basin for major patterns
    Primary,
    /// Secondary basin for supporting patterns
    Secondary,
    /// Temporary basin for transient patterns
    Temporary,
    /// Meta-basin that contains other basins
    Meta,
    /// Hybrid basin combining multiple patterns
    Hybrid,
}

/// Attractor basin manager for handling multiple basins
#[derive(Debug)]
pub struct AttractorBasinManager {
    /// Configuration
    config: MemoryAttractorConfig,
    /// Active attractor basins
    basins: Arc<AsyncRwLock<HashMap<String, AttractorBasin>>>,
    /// Basin index for spatial queries
    spatial_index: Arc<RwLock<SpatialIndex>>,
    /// Basin interaction network
    interaction_network: Arc<RwLock<BasinInteractionNetwork>>,
    /// Performance metrics
    metrics: Arc<RwLock<BasinMetrics>>,
    /// Component status
    status: Arc<RwLock<ComponentStatus>>,
}

/// Spatial index for efficient basin queries
#[derive(Debug)]
pub struct SpatialIndex {
    /// Grid-based spatial partitioning
    grid: HashMap<GridCell, Vec<String>>,
    /// Grid cell size
    cell_size: f32,
    /// Dimension count
    dimensions: usize,
    /// Total basin count
    total_basins: usize,
}

/// Grid cell for spatial indexing
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct GridCell {
    /// Cell coordinates
    coordinates: Vec<i32>,
}

/// Basin interaction network
#[derive(Debug, Clone)]
pub struct BasinInteractionNetwork {
    /// Interaction strengths between basins
    interactions: HashMap<String, HashMap<String, f32>>,
    /// Network density
    density: f32,
    /// Clustering coefficient
    clustering_coefficient: f32,
    /// Average path length
    avg_path_length: f32,
}

/// Basin performance metrics
#[derive(Debug, Clone, Default)]
pub struct BasinMetrics {
    /// Total basins created
    pub total_created: usize,
    /// Total basins destroyed
    pub total_destroyed: usize,
    /// Average convergence time
    pub avg_convergence_time: Duration,
    /// Average basin lifetime
    pub avg_lifetime: Duration,
    /// Basin creation rate
    pub creation_rate: f32,
    /// Basin destruction rate
    pub destruction_rate: f32,
    /// Network efficiency
    pub network_efficiency: f32,
    /// Average basin health
    pub avg_health: f32,
}

/// Basin convergence result
#[derive(Debug, Clone)]
pub struct BasinConvergenceResult {
    /// Converged basin ID
    pub basin_id: String,
    /// Convergence point
    pub convergence_point: Vec<f32>,
    /// Number of iterations
    pub iterations: usize,
    /// Convergence time
    pub convergence_time: Duration,
    /// Final distance to basin center
    pub final_distance: f32,
    /// Convergence success
    pub success: bool,
}

impl AttractorBasin {
    /// Create a new attractor basin
    pub fn new(
        center: Vec<f32>,
        radius: f32,
        depth: f32,
        basin_type: BasinType,
    ) -> ContextNestResult<Self> {
        if center.is_empty() {
            return Err(ContextNestError::Configuration(
                "Basin center cannot be empty".to_string(),
            ));
        }

        if radius <= 0.0 {
            return Err(ContextNestError::Configuration(
                "Basin radius must be positive".to_string(),
            ));
        }

        if depth <= 0.0 {
            return Err(ContextNestError::Configuration(
                "Basin depth must be positive".to_string(),
            ));
        }

        let now = Utc::now();

        Ok(Self {
            id: utils::generate_unique_id(),
            center,
            radius,
            depth,
            shape: BasinShape::default(),
            dynamics: BasinDynamics::default(),
            associated_fragments: HashSet::new(),
            health: BasinHealth::default(),
            created_at: now,
            last_modified: now,
            activation_frequency: 0.0,
            stability_score: 0.5,
            basin_type,
        })
    }

    /// Calculate attraction force at a given position

    pub fn attraction_force(&self, position: &[f32]) -> Vec<f32> {
        if position.len() != self.center.len() {
            return vec![0.0; position.len()];
        }

        let distance = utils::euclidean_distance(position, &self.center);

        if distance > self.radius * 2.0 {
            // Outside attraction range
            return vec![0.0; position.len()];
        }

        // Calculate attraction force using modified gravitational potential
        let force_magnitude = self.depth * (1.0 - distance / (self.radius * 2.0)).powi(2);

        if distance < f32::EPSILON {
            return vec![0.0; position.len()];
        }

        let mut force = Vec::with_capacity(position.len());
        for (i, &pos) in position.iter().enumerate() {
            let direction = (self.center[i] - pos) / distance;
            force.push(direction * force_magnitude);
        }

        // Apply shape modifications
        self.apply_shape_modifications(&mut force, position);

        force
    }

    /// Check if a position is within the basin

    pub fn contains(&self, position: &[f32]) -> bool {
        let distance = utils::euclidean_distance(position, &self.center);
        distance <= self.radius
    }

    /// Calculate basin energy at a given position

    pub fn energy(&self, position: &[f32]) -> f32 {
        let distance = utils::euclidean_distance(position, &self.center);

        if distance > self.radius * 2.0 {
            return 0.0;
        }

        // Energy well potential
        let normalized_distance = distance / self.radius;
        -self.depth * (1.0 - normalized_distance.powi(2)).exp()
    }

    /// Update basin dynamics

    pub fn update_dynamics(&mut self, dt: Duration) {
        let dt_seconds = dt.as_secs_f32();

        // Update center position based on velocity
        for (i, &velocity) in self.dynamics.velocity.iter().enumerate() {
            if i < self.center.len() {
                self.center[i] += velocity * dt_seconds;
            }
        }

        // Update radius based on expansion rate
        self.radius += self.dynamics.expansion_rate * dt_seconds;
        self.radius = self.radius.max(0.1); // Minimum radius

        // Update depth based on depth change rate
        self.depth += self.dynamics.depth_change_rate * dt_seconds;
        self.depth = self.depth.max(0.1); // Minimum depth

        // Apply damping to velocity
        for velocity in &mut self.dynamics.velocity {
            *velocity *= (1.0 - self.dynamics.damping_coefficient * dt_seconds).max(0.0);
        }

        // Update stability score based on dynamics
        self.update_stability_score();

        self.last_modified = Utc::now();
    }

    /// Add a fragment to this basin

    pub fn add_fragment(&mut self, fragment_id: String) {
        self.associated_fragments.insert(fragment_id);
        self.update_health();
    }

    /// Remove a fragment from this basin

    pub fn remove_fragment(&mut self, fragment_id: &str) {
        self.associated_fragments.remove(fragment_id);
        self.update_health();
    }

    /// Update basin health metrics

    pub fn update_health(&mut self) {
        let now = Utc::now();

        // Calculate pattern coherence based on fragment distribution
        self.health.pattern_coherence = self.calculate_pattern_coherence();

        // Calculate fragment coverage
        self.health.fragment_coverage = (self.associated_fragments.len() as f32 / 10.0).min(1.0);

        // Calculate activity level
        let time_since_last_activity = now
            .signed_duration_since(self.last_modified)
            .num_seconds()
            .max(0) as f32;
        self.health.activity_level = (-time_since_last_activity / 3600.0).exp(); // Decay over hours

        // Update stability based on health components
        self.health.stability = self.stability_score;

        // Calculate overall health
        self.health.overall_health = (self.health.pattern_coherence * 0.3
            + self.health.fragment_coverage * 0.2
            + self.health.activity_level * 0.3
            + self.health.stability * 0.2)
            .min(1.0);

        self.health.last_health_check = now;
    }

    /// Calculate pattern coherence
    fn calculate_pattern_coherence(&self) -> f32 {
        // Simplified coherence calculation
        // In practice, this would analyze the actual fragment patterns
        if self.associated_fragments.is_empty() {
            return 0.5;
        }

        let fragment_count = self.associated_fragments.len() as f32;
        let optimal_count = 10.0;
        let density_factor = 1.0 - (fragment_count - optimal_count).abs() / optimal_count;

        density_factor.max(0.0)
    }

    /// Update stability score
    fn update_stability_score(&mut self) {
        let velocity_magnitude: f32 = self
            .dynamics
            .velocity
            .iter()
            .map(|v| v.powi(2))
            .sum::<f32>()
            .sqrt();

        // Higher stability when velocity is low (stable position)
        let velocity_factor = (-velocity_magnitude).exp();

        // Factor in basin age
        let age_factor = (Utc::now()
            .signed_duration_since(self.created_at)
            .to_std()
            .unwrap_or_default()
            .as_secs_f32()
            / 3600.0)
            .min(1.0);

        // Update stability with exponential moving average
        let new_stability = velocity_factor * 0.7 + age_factor * 0.3;
        self.stability_score = self.stability_score * 0.9 + new_stability * 0.1;
        self.stability_score = self.stability_score.clamp(0.0, 1.0);
    }

    /// Apply shape modifications to force
    fn apply_shape_modifications(&self, force: &mut [f32], position: &[f32]) {
        match self.shape.shape_type {
            ShapeType::Spherical => {
                // No modification needed for spherical basins
            }
            ShapeType::Ellipsoidal => {
                // Apply elliptical distortion based on eccentricity
                let eccentricity_factor = 1.0 + self.shape.eccentricity;
                for (i, f) in force.iter_mut().enumerate() {
                    if i % 2 == 0 {
                        *f *= eccentricity_factor;
                    } else {
                        *f /= eccentricity_factor;
                    }
                }
            }
            ShapeType::Irregular => {
                // Apply irregular shape modifications based on parameters
                if let Some(modifier) = self.shape.parameters.get("force_modifier") {
                    for f in force.iter_mut() {
                        *f *= modifier;
                    }
                }
            }
            ShapeType::Adaptive => {
                // Apply adaptive modifications based on basin health
                let health_factor = self.health.overall_health;
                for f in force.iter_mut() {
                    *f *= 0.5 + health_factor * 0.5;
                }
            }
        }
    }

    /// Check if basin should be merged with another

    pub fn should_merge_with(&self, other: &AttractorBasin, threshold: f32) -> bool {
        if self.id == other.id {
            return false;
        }

        let distance = utils::euclidean_distance(&self.center, &other.center);
        let merged_radius = (self.radius + other.radius) / 2.0;

        distance < merged_radius * threshold
    }

    /// Merge with another basin

    pub fn merge_with(&mut self, other: &AttractorBasin) -> ContextNestResult<()> {
        if self.center.len() != other.center.len() {
            return Err(ContextNestError::Configuration(
                "Cannot merge basins with different dimensions".to_string(),
            ));
        }

        // Weighted average of centers based on depth
        let total_depth = self.depth + other.depth;
        let self_weight = self.depth / total_depth;
        let other_weight = other.depth / total_depth;

        for i in 0..self.center.len() {
            self.center[i] = self.center[i] * self_weight + other.center[i] * other_weight;
        }

        // Combine radii and depths
        self.radius = (self.radius + other.radius) / 2.0;
        self.depth = total_depth;

        // Merge associated fragments (clone because the source is &HashSet — extend wants owned String)
        self.associated_fragments
            .extend(other.associated_fragments.iter().cloned());

        // Update modification time
        self.last_modified = Utc::now();

        Ok(())
    }
}

impl Default for BasinShape {
    fn default() -> Self {
        Self {
            shape_type: ShapeType::Spherical,
            parameters: HashMap::new(),
            eccentricity: 0.0,
            orientation: None,
        }
    }
}

impl Default for BasinDynamics {
    fn default() -> Self {
        Self {
            velocity: Vec::new(),
            expansion_rate: 0.0,
            depth_change_rate: 0.0,
            adaptation_strength: 0.1,
            resonance_frequency: 1.0,
            damping_coefficient: 0.1,
        }
    }
}

impl Default for BasinHealth {
    fn default() -> Self {
        Self {
            overall_health: 0.5,
            pattern_coherence: 0.5,
            fragment_coverage: 0.0,
            stability: 0.5,
            activity_level: 1.0,
            last_health_check: Utc::now(),
        }
    }
}

impl AttractorBasinManager {
    /// Create a new attractor basin manager
    pub fn new(config: MemoryAttractorConfig) -> Self {
        Self {
            config: config.clone(),
            basins: Arc::new(AsyncRwLock::new(HashMap::new())),
            spatial_index: Arc::new(RwLock::new(SpatialIndex::new(config.max_attractor_basins))),
            interaction_network: Arc::new(RwLock::new(BasinInteractionNetwork::new())),
            metrics: Arc::new(RwLock::new(BasinMetrics::default())),
            status: Arc::new(RwLock::new(ComponentStatus::Initializing)),
        }
    }

    /// Initialize the basin manager

    pub async fn initialize(&self) -> ContextNestResult<()> {
        *self.status.write().unwrap() = ComponentStatus::Running;
        Ok(())
    }

    /// Create a new attractor basin

    pub async fn create_basin(
        &self,
        center: Vec<f32>,
        radius: f32,
        depth: f32,
        basin_type: BasinType,
    ) -> ContextNestResult<String> {
        let mut basin = AttractorBasin::new(center, radius, depth, basin_type)?;

        let basin_id = basin.id.clone();

        // Add to basins collection
        self.basins.write().await.insert(basin_id.clone(), basin);

        // Update spatial index
        self.update_spatial_index(&basin_id).await?;

        // Update metrics
        self.update_metrics(|metrics| {
            metrics.total_created += 1;
        });

        Ok(basin_id)
    }

    /// Find nearest basin to a position

    pub async fn find_nearest_basin(&self, position: &[f32]) -> ContextNestResult<Option<String>> {
        let basins = self.basins.read().await;

        let mut nearest_id = None;
        let mut min_distance = f32::INFINITY;

        for (id, basin) in basins.iter() {
            let distance = utils::euclidean_distance(position, &basin.center);
            if distance < min_distance {
                min_distance = distance;
                nearest_id = Some(id.clone());
            }
        }

        Ok(nearest_id)
    }

    /// Converge a position to the nearest attractor basin

    pub async fn converge_to_attractor(
        &self,
        initial_position: &[f32],
        max_iterations: Option<usize>,
    ) -> ContextNestResult<BasinConvergenceResult> {
        let start_time = Utc::now();
        let max_iter = max_iterations.unwrap_or(1000);

        let mut position = initial_position.to_vec();

        for iteration in 0..max_iter {
            // Find nearest basin
            let nearest_basin_id = self.find_nearest_basin(&position).await?;

            if let Some(basin_id) = nearest_basin_id {
                let basins = self.basins.read().await;
                if let Some(basin) = basins.get(&basin_id) {
                    // Calculate attraction force
                    let force = basin.attraction_force(&position);

                    // Apply force to position
                    for (i, &f) in force.iter().enumerate() {
                        if i < position.len() {
                            position[i] += f * 0.1; // Step size
                        }
                    }

                    // Check convergence
                    let distance = utils::euclidean_distance(&position, &basin.center);
                    if distance < self.config.convergence_threshold {
                        return Ok(BasinConvergenceResult {
                            basin_id: basin_id.clone(),
                            convergence_point: position,
                            iterations: iteration + 1,
                            convergence_time: Utc::now()
                                .signed_duration_since(start_time)
                                .to_std()
                                .unwrap_or_default(),
                            final_distance: distance,
                            success: true,
                        });
                    }
                } else {
                    break; // Basin not found
                }
            } else {
                break; // No basins available
            }
        }

        Ok(BasinConvergenceResult {
            basin_id: String::new(),
            convergence_point: position,
            iterations: max_iter,
            convergence_time: Utc::now()
                .signed_duration_since(start_time)
                .to_std()
                .unwrap_or_default(),
            final_distance: f32::INFINITY,
            success: false,
        })
    }

    /// Update all basin dynamics

    pub async fn update_all_dynamics(&self, dt: Duration) -> ContextNestResult<()> {
        {
            // Scope the write lock so it is dropped before we call
            // `rebuild_spatial_index`, which itself needs `self.basins.read()`.
            // Holding the write lock across that `.await` would deadlock tokio's
            // RwLock (writer waiting for itself inside the same task).
            let mut basins = self.basins.write().await;

            for basin in basins.values_mut() {
                basin.update_dynamics(dt);
                basin.update_health();
            }
        } // write lock released here

        // Update spatial index — now safe to re-acquire basins for reading.
        self.rebuild_spatial_index().await?;

        Ok(())
    }

    /// Remove unhealthy basins

    pub async fn remove_unhealthy_basins(&self, health_threshold: f32) -> ContextNestResult<usize> {
        let removed_count = {
            // Scope the write lock so it is dropped before `rebuild_spatial_index`
            // acquires `self.basins.read()`. Without the explicit drop, tokio's
            // RwLock deadlocks: the current task holds the write lock and then
            // waits on a read lock that can only be granted once the write lock
            // is released — which never happens inside the same task.
            let mut basins = self.basins.write().await;
            let mut to_remove = Vec::new();

            for (id, basin) in basins.iter() {
                if basin.health.overall_health < health_threshold {
                    to_remove.push(id.clone());
                }
            }

            let count = to_remove.len();

            for id in to_remove {
                basins.remove(&id);
            }

            count
        }; // write lock released here

        // Update spatial index — now safe to re-acquire basins for reading.
        self.rebuild_spatial_index().await?;

        // Update metrics
        self.update_metrics(|metrics| {
            metrics.total_destroyed += removed_count;
        });

        Ok(removed_count)
    }

    /// Merge nearby basins

    pub async fn merge_nearby_basins(&self, distance_threshold: f32) -> ContextNestResult<usize> {
        let merge_count = {
            // Scope the write lock so it is dropped before `rebuild_spatial_index`
            // acquires `self.basins.read()` — same deadlock class as
            // `remove_unhealthy_basins` and `update_all_dynamics`.
            let mut basins = self.basins.write().await;
            let mut count = 0;
            let mut processed = HashSet::new();

            let basin_ids: Vec<String> = basins.keys().cloned().collect();

            for i in 0..basin_ids.len() {
                if processed.contains(&basin_ids[i]) {
                    continue;
                }

                let mut to_merge = Vec::new();

                for j in (i + 1)..basin_ids.len() {
                    if processed.contains(&basin_ids[j]) {
                        continue;
                    }

                    if let (Some(basin1), Some(basin2)) =
                        (basins.get(&basin_ids[i]), basins.get(&basin_ids[j]))
                    {
                        if basin1.should_merge_with(basin2, distance_threshold) {
                            to_merge.push(basin_ids[j].clone());
                        }
                    }
                }

                if !to_merge.is_empty() {
                    // Two-phase merge to avoid holding `&mut basins` across
                    // `basins.remove` calls: first remove all merge candidates
                    // (yielding owned `AttractorBasin`s), then merge them into
                    // the main basin under a single mutable borrow.
                    let merge_basins: Vec<(String, AttractorBasin)> = to_merge
                        .into_iter()
                        .filter_map(|id| basins.remove(&id).map(|b| (id, b)))
                        .collect();
                    if let Some(main_basin) = basins.get_mut(&basin_ids[i]) {
                        for (merge_id, merge_basin) in merge_basins {
                            let _ = main_basin.merge_with(&merge_basin);
                            processed.insert(merge_id);
                            count += 1;
                        }
                    }
                }

                processed.insert(basin_ids[i].clone());
            }

            count
        }; // write lock released here

        // Update spatial index — now safe to re-acquire basins for reading.
        self.rebuild_spatial_index().await?;

        Ok(merge_count)
    }

    /// Get basin statistics

    pub async fn get_statistics(&self) -> ContextNestResult<BasinStatistics> {
        let basins = self.basins.read().await;
        let metrics = self.metrics.read().unwrap();

        let total_basins = basins.len();
        let mut active_basins = 0;
        let mut total_health = 0.0;
        let mut type_counts = HashMap::new();

        for basin in basins.values() {
            if basin.health.overall_health > 0.5 {
                active_basins += 1;
            }
            total_health += basin.health.overall_health;

            *type_counts.entry(basin.basin_type.clone()).or_insert(0) += 1;
        }

        let avg_health = if total_basins > 0 {
            total_health / total_basins as f32
        } else {
            0.0
        };

        Ok(BasinStatistics {
            total_basins,
            active_basins,
            avg_health,
            type_counts,
            metrics: metrics.clone(),
        })
    }

    // Helper methods

    async fn update_spatial_index(&self, basin_id: &str) -> ContextNestResult<()> {
        // Snapshot the geometry under the async read lock, then release it
        // before acquiring the sync write lock, keeping the two lock scopes
        // non-overlapping and safe to use in an async context.
        let snapshot: Option<(Vec<f32>, f32)> = {
            let basins = self.basins.read().await;
            basins
                .get(basin_id)
                .map(|basin| (basin.center.clone(), basin.radius))
        }; // async read lock dropped here

        if let Some((center, radius)) = snapshot {
            self.spatial_index
                .write()
                .unwrap()
                .add_basin(basin_id, &center, radius);
        }
        Ok(())
    }

    async fn rebuild_spatial_index(&self) -> ContextNestResult<()> {
        // Snapshot the basin geometry under the async read lock so we do not hold
        // a std::sync::RwLock write guard across an `.await` point. Holding a
        // sync lock over an `.await` is unsound in an async runtime: the task may
        // be suspended while the sync write lock remains poisoned/held, starving
        // every other thread that wants `spatial_index`.
        let basin_snapshots: Vec<(String, Vec<f32>, f32)> = {
            let basins = self.basins.read().await;
            basins
                .iter()
                .map(|(id, basin)| (id.clone(), basin.center.clone(), basin.radius))
                .collect()
        }; // async read lock dropped here

        // Now update the spatial index under the sync lock — no `.await` inside.
        let mut index = self.spatial_index.write().unwrap();
        index.clear();
        for (id, center, radius) in &basin_snapshots {
            index.add_basin(id, center, *radius);
        }

        Ok(())
    }

    fn update_metrics<F>(&self, update_fn: F)
    where
        F: FnOnce(&mut BasinMetrics),
    {
        let mut metrics = self.metrics.write().unwrap();
        update_fn(&mut metrics);

        // Update derived metrics
        if metrics.total_created > 0 {
            metrics.creation_rate =
                metrics.total_created as f32 / (metrics.avg_lifetime.as_secs_f32() + 1.0);
        }

        metrics.avg_health = self.calculate_avg_health();
    }

    fn calculate_avg_health(&self) -> f32 {
        // This would need async access, simplified for now
        0.7 // Placeholder
    }
}

impl SpatialIndex {
    fn new(max_basins: usize) -> Self {
        Self {
            grid: HashMap::new(),
            cell_size: 1.0,  // Default cell size
            dimensions: 128, // Default embedding dimension
            total_basins: 0,
        }
    }

    fn add_basin(&mut self, basin_id: &str, center: &[f32], radius: f32) {
        let cells = self.get_cells_for_basin(center, radius);

        for cell in cells {
            self.grid
                .entry(cell)
                .or_insert_with(Vec::new)
                .push(basin_id.to_string());
        }

        self.total_basins += 1;
    }

    fn get_cells_for_basin(&self, center: &[f32], radius: f32) -> Vec<GridCell> {
        let mut cells = Vec::new();

        // Calculate affected grid cells
        let min_coord = ((center[0] - radius) / self.cell_size).floor() as i32;
        let max_coord = ((center[0] + radius) / self.cell_size).ceil() as i32;

        for x in min_coord..=max_coord {
            let cell = GridCell {
                coordinates: vec![x],
            };
            cells.push(cell);
        }

        cells
    }

    fn clear(&mut self) {
        self.grid.clear();
        self.total_basins = 0;
    }
}

impl BasinInteractionNetwork {
    fn new() -> Self {
        Self {
            interactions: HashMap::new(),
            density: 0.0,
            clustering_coefficient: 0.0,
            avg_path_length: 0.0,
        }
    }
}

/// Basin statistics
#[derive(Debug, Clone, Default)]
pub struct BasinStatistics {
    pub total_basins: usize,
    pub active_basins: usize,
    pub avg_health: f32,
    pub type_counts: HashMap<BasinType, usize>,
    pub metrics: BasinMetrics,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::attractors::MemoryAttractorConfig;

    #[tokio::test]
    async fn test_attractor_basin_creation() {
        let center = vec![1.0, 2.0, 3.0];
        let basin = AttractorBasin::new(center.clone(), 1.0, 0.8, BasinType::Primary).unwrap();

        assert_eq!(basin.center, center);
        assert_eq!(basin.radius, 1.0);
        assert_eq!(basin.depth, 0.8);
        assert!(basin.id.len() > 0);
    }

    #[tokio::test]
    async fn test_basin_manager_creation() {
        let config = MemoryAttractorConfig::default();
        let manager = AttractorBasinManager::new(config);

        manager.initialize().await.unwrap();

        let status = manager.status.read().unwrap();
        match *status {
            ComponentStatus::Running => {}
            _ => panic!("Manager should be in running state"),
        }
    }

    #[tokio::test]
    async fn test_basin_creation_and_retrieval() {
        let config = MemoryAttractorConfig::default();
        let manager = AttractorBasinManager::new(config);
        manager.initialize().await.unwrap();

        let center = vec![1.0, 2.0, 3.0];
        let basin_id = manager
            .create_basin(center.clone(), 1.0, 0.8, BasinType::Primary)
            .await
            .unwrap();

        let nearest = manager.find_nearest_basin(&center).await.unwrap();
        assert!(nearest.is_some());
        assert_eq!(nearest.unwrap(), basin_id);
    }

    #[tokio::test]
    async fn test_convergence_to_attractor() {
        let config = MemoryAttractorConfig::default();
        let manager = AttractorBasinManager::new(config);
        manager.initialize().await.unwrap();

        let center = vec![1.0, 2.0, 3.0];
        let _basin_id = manager
            .create_basin(center.clone(), 1.0, 0.8, BasinType::Primary)
            .await
            .unwrap();

        let initial_pos = vec![1.1, 2.1, 3.1];
        let result = manager
            .converge_to_attractor(&initial_pos, None)
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.final_distance < 0.1);
    }

    #[test]
    fn test_attraction_force() {
        let center = vec![0.0, 0.0];
        let basin = AttractorBasin::new(center, 1.0, 0.8, BasinType::Primary).unwrap();

        let position = vec![0.5, 0.5];
        let force = basin.attraction_force(&position);

        assert_eq!(force.len(), 2);
        // Force should point toward center
        assert!(force[0] < 0.0);
        assert!(force[1] < 0.0);
    }

    #[test]
    fn test_basin_contains() {
        let center = vec![0.0, 0.0];
        let basin = AttractorBasin::new(center, 1.0, 0.8, BasinType::Primary).unwrap();

        assert!(basin.contains(&[0.5, 0.5]));
        assert!(!basin.contains(&[2.0, 2.0]));
    }

    #[test]
    fn test_basin_merge() {
        let center1 = vec![0.0, 0.0];
        let mut basin1 = AttractorBasin::new(center1, 1.0, 0.8, BasinType::Primary).unwrap();

        let center2 = vec![0.1, 0.1];
        let basin2 = AttractorBasin::new(center2, 1.0, 0.6, BasinType::Secondary).unwrap();

        assert!(basin1.should_merge_with(&basin2, 0.5));

        basin1.merge_with(&basin2).unwrap();

        // Center should be weighted average
        assert!((basin1.center[0] - 0.043).abs() < 0.01); // Weighted toward deeper basin
    }
}

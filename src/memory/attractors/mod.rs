//! Advanced Memory Attractor System
//! This module implements a sophisticated memory attractor system for ContextNest,
//! providing attractor basin dynamics, memory reconstruction protocols, and AI-powered
//! gap filling for advanced cognitive processing capabilities.

use crate::error::ContextNestResult;
use crate::error::{ContextNestError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock as AsyncRwLock;
use uuid::Uuid;

pub mod adaptive_decay;
/// Core memory attractor system components
pub mod attractor_basin;
pub mod connection_network;
pub mod gap_filling_engine;
pub mod memory_attractor_manager;
pub mod reconstruction_protocol;

// Re-export main components
pub use adaptive_decay::*;
pub use attractor_basin::*;
pub use connection_network::*;
pub use gap_filling_engine::*;
pub use memory_attractor_manager::*;
pub use reconstruction_protocol::*;

/// Configuration for the memory attractor system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAttractorConfig {
    /// Maximum number of attractor basins
    pub max_attractor_basins: usize,
    /// Convergence threshold for attractor dynamics
    pub convergence_threshold: f32,
    /// Memory fragment size
    pub fragment_size: usize,
    /// Gap filling confidence threshold
    pub gap_filling_threshold: f32,
    /// Adaptive decay rate
    pub decay_rate: f32,
    /// Connection network density
    pub connection_density: f32,
    /// Enable AI-powered gap filling
    pub enable_ai_gap_filling: bool,
    /// Enable parallel processing
    pub enable_parallel_processing: bool,
    /// Cache size for reconstructed memories
    pub reconstruction_cache_size: usize,
}

impl Default for MemoryAttractorConfig {
    fn default() -> Self {
        Self {
            max_attractor_basins: 1000,
            convergence_threshold: 0.01,
            fragment_size: 64,
            gap_filling_threshold: 0.8,
            decay_rate: 0.1,
            connection_density: 0.3,
            enable_ai_gap_filling: true,
            enable_parallel_processing: true,
            reconstruction_cache_size: 10000,
        }
    }
}

/// Memory fragment for reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFragment {
    /// Unique fragment identifier
    pub id: String,
    /// Fragment content (semantic embedding)
    pub content: Vec<f32>,
    /// Fragment importance weight
    pub importance: f32,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last access timestamp
    pub last_accessed: DateTime<Utc>,
    /// Associated attractor basin ID
    pub attractor_basin_id: Option<String>,
    /// Fragment connections to other fragments
    pub connections: HashSet<String>,
    /// Confidence score
    pub confidence: f32,
}

/// Reconstructed memory from fragments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructedMemory {
    /// Unique memory identifier
    pub id: String,
    /// Reconstructed content
    pub content: Vec<f32>,
    /// Reconstruction confidence
    pub confidence: f32,
    /// Source fragments
    pub source_fragments: Vec<String>,
    /// Gaps filled during reconstruction
    pub filled_gaps: Vec<GapInfo>,
    /// Reconstruction timestamp
    pub reconstructed_at: DateTime<Utc>,
    /// Memory persistence score
    pub persistence_score: f32,
}

/// Information about filled memory gaps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapInfo {
    /// Gap position in memory
    pub position: usize,
    /// Gap size
    pub size: usize,
    /// Filling method used
    pub filling_method: GapFillingMethod,
    /// Confidence in filled content
    pub confidence: f32,
    /// Source of filled content
    pub source: GapFillSource,
}

/// Methods for filling memory gaps
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum GapFillingMethod {
    /// Interpolation from neighboring fragments
    Interpolation,
    /// AI-powered generation
    AIGeneration,
    /// Pattern-based reconstruction
    PatternReconstruction,
    /// Similarity-based borrowing
    SimilarityBorrowing,
}

/// Sources for gap filling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GapFillSource {
    /// From attractor basin patterns
    AttractorBasin(String),
    /// From similar memories
    SimilarMemory(String),
    /// From AI generation
    AIGeneration,
    /// From pattern matching
    PatternMatching,
}

/// Memory attractor system performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAttractorMetrics {
    /// Total number of attractor basins
    pub total_attractor_basins: usize,
    /// Active attractor basins
    pub active_attractor_basins: usize,
    /// Total memory fragments
    pub total_fragments: usize,
    /// Average reconstruction confidence
    pub avg_reconstruction_confidence: f32,
    /// Gap filling success rate
    pub gap_filling_success_rate: f32,
    /// Memory reconstruction cache hit rate
    pub reconstruction_cache_hit_rate: f32,
    /// Average convergence time (ms)
    pub avg_convergence_time_ms: f64,
    /// System throughput (operations/sec)
    pub throughput: f64,
}

impl Default for MemoryAttractorMetrics {
    fn default() -> Self {
        Self {
            total_attractor_basins: 0,
            active_attractor_basins: 0,
            total_fragments: 0,
            avg_reconstruction_confidence: 0.0,
            gap_filling_success_rate: 0.0,
            reconstruction_cache_hit_rate: 0.0,
            avg_convergence_time_ms: 0.0,
            throughput: 0.0,
        }
    }
}

/// Trait for memory attractor system components
#[async_trait::async_trait]
pub trait MemoryAttractorComponent: Send + Sync {
    /// Initialize the component
    async fn initialize(&mut self) -> ContextNestResult<()>;

    /// Shutdown the component
    async fn shutdown(&mut self) -> ContextNestResult<()>;

    /// Get component status
    fn status(&self) -> ComponentStatus;

    /// Get component metrics
    fn metrics(&self) -> serde_json::Value;
}

/// Component status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComponentStatus {
    /// Component is initializing
    Initializing,
    /// Component is running
    Running,
    /// Component is degraded
    Degraded(String),
    /// Component is stopped
    Stopped,
    /// Component has an error
    Error(String),
}

/// Utility functions for memory attractor operations
pub mod utils {
    use super::*;

    /// Calculate Euclidean distance between two vectors

    pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return f32::INFINITY;
        }

        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            .sqrt()
    }

    /// Calculate cosine similarity between two vectors

    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x.powi(2)).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x.powi(2)).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot_product / (norm_a * norm_b)
    }

    /// Normalize a vector to unit length

    pub fn normalize_vector(vec: &mut [f32]) {
        let norm: f32 = vec.iter().map(|x| x.powi(2)).sum::<f32>().sqrt();
        if norm > 0.0 {
            for val in vec.iter_mut() {
                *val /= norm;
            }
        }
    }

    /// Generate a unique ID for memory components

    pub fn generate_unique_id() -> String {
        Uuid::new_v4().to_string()
    }

    /// Calculate importance weight based on access patterns and age

    pub fn calculate_importance(
        access_count: usize,
        created_at: DateTime<Utc>,
        last_accessed: DateTime<Utc>,
        base_importance: f32,
    ) -> f32 {
        let now = Utc::now();
        let age_secs = now.signed_duration_since(created_at).num_seconds().max(0) as f32;
        let recency_secs = now
            .signed_duration_since(last_accessed)
            .num_seconds()
            .max(0) as f32;
        let age_factor = 1.0 / (1.0 + age_secs / 3600.0); // Decay over hours
        let recency_factor = 1.0 / (1.0 + recency_secs / 300.0); // Decay over minutes
        let frequency_factor = (access_count as f32).ln_1p() / 10.0; // Logarithmic frequency scaling

        base_importance * age_factor * recency_factor * (1.0 + frequency_factor)
    }

    /// Simple non-cryptographic hash for f32 vectors. Used as a quick cache
    /// key when full embedding comparison would be too expensive. Not stable
    /// across struct layout / endianness — purely for in-process bucketing.
    pub fn calculate_simple_hash(vec: &[f32]) -> u64 {
        vec.iter()
            .enumerate()
            .map(|(i, &val)| (val.to_bits() as u64).wrapping_mul((i as u64).wrapping_add(1)))
            .fold(0u64, |acc, val| acc.wrapping_add(val))
    }
}

#[cfg(test)]
mod tests {
    use super::utils::*;
    use super::*;

    #[test]
    fn test_euclidean_distance() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 6.0, 8.0];

        let distance = euclidean_distance(&a, &b);
        assert!((distance - 7.071).abs() < 0.01);
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];

        let similarity = cosine_similarity(&a, &b);
        assert!((similarity - 0.0).abs() < f32::EPSILON);

        let c = vec![1.0, 0.0, 0.0];
        let similarity = cosine_similarity(&a, &c);
        assert!((similarity - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_normalize_vector() {
        let mut vec = vec![3.0, 4.0];
        normalize_vector(&mut vec);

        let norm: f32 = vec.iter().map(|x| x.powi(2)).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_calculate_importance() {
        let now = Utc::now();
        let importance = calculate_importance(10, now, now, 0.8);

        assert!(importance > 0.0);
        assert!(importance <= 1.0);
    }

    #[test]
    fn test_memory_fragment_creation() {
        let fragment = MemoryFragment {
            id: generate_unique_id(),
            content: vec![0.1, 0.2, 0.3],
            importance: 0.8,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            attractor_basin_id: Some("test_basin".to_string()),
            connections: HashSet::new(),
            confidence: 0.9,
        };

        assert_eq!(fragment.content.len(), 3);
        assert!(fragment.importance > 0.0);
        assert!(fragment.confidence > 0.0);
    }

    #[test]
    fn test_reconstructed_memory() {
        let memory = ReconstructedMemory {
            id: generate_unique_id(),
            content: vec![0.1, 0.2, 0.3, 0.4],
            confidence: 0.85,
            source_fragments: vec!["frag1".to_string(), "frag2".to_string()],
            filled_gaps: vec![GapInfo {
                position: 2,
                size: 1,
                filling_method: GapFillingMethod::Interpolation,
                confidence: 0.7,
                source: GapFillSource::AttractorBasin("test".to_string()),
            }],
            reconstructed_at: Utc::now(),
            persistence_score: 0.9,
        };

        assert_eq!(memory.source_fragments.len(), 2);
        assert_eq!(memory.filled_gaps.len(), 1);
        assert!(memory.confidence > 0.0);
    }
}

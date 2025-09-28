//! Memory Reconstruction Protocol
//! Implements fragment-based memory reconstruction with AI-powered gap filling
//! for robust memory recovery from partial information.

use crate::error::ContextNestResult;
use crate::error::{ContextNestError, Result};
use crate::memory::attractors::{
    utils, ComponentStatus, GapFillSource, GapFillingMethod, GapInfo, MemoryAttractorConfig,
    MemoryFragment, ReconstructedMemory,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock as AsyncRwLock;
use uuid::Uuid;

/// Memory reconstruction protocol manager
#[derive(Debug)]
pub struct MemoryReconstructionProtocol {
    /// Configuration
    config: MemoryAttractorConfig,
    /// Fragment store
    fragment_store: Arc<AsyncRwLock<HashMap<String, MemoryFragment>>>,
    /// Reconstruction cache
    reconstruction_cache: Arc<RwLock<HashMap<String, CachedReconstruction>>>,
    /// Pattern matching engine
    pattern_matcher: Arc<PatternMatchingEngine>,
    /// Reconstruction statistics
    statistics: Arc<RwLock<ReconstructionStatistics>>,
    /// Component status
    status: Arc<RwLock<ComponentStatus>>,
}

/// Cached reconstruction result
#[derive(Debug, Clone)]
pub struct CachedReconstruction {
    /// Reconstructed memory
    pub memory: ReconstructedMemory,
    /// Cache creation time
    pub created_at: DateTime<Utc>,
    /// Last access time
    pub last_accessed: DateTime<Utc>,
    /// Access count
    pub access_count: usize,
    /// Estimated recomputation cost
    pub recomputation_cost: f32,
}

/// Pattern matching engine for fragment alignment
#[derive(Debug)]
pub struct PatternMatchingEngine {
    /// Pattern templates
    templates: HashMap<String, PatternTemplate>,
    /// Matching algorithms
    algorithms: Vec<Box<dyn PatternMatchingAlgorithm>>,
    /// Alignment cache
    alignment_cache: HashMap<String, FragmentAlignment>,
}

/// Pattern template for reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternTemplate {
    /// Template ID
    pub id: String,
    /// Template pattern
    pub pattern: Vec<f32>,
    /// Template confidence
    pub confidence: f32,
    /// Template metadata
    pub metadata: HashMap<String, String>,
}

/// Fragment alignment information
#[derive(Debug, Clone)]
pub struct FragmentAlignment {
    /// Aligned fragments
    pub fragments: Vec<String>,
    /// Alignment score
    pub score: f32,
    /// Alignment positions
    pub positions: Vec<usize>,
    /// Gaps in alignment
    pub gaps: Vec<GapPosition>,
}

/// Gap position in memory reconstruction
#[derive(Debug, Clone)]
pub struct GapPosition {
    /// Gap start position
    pub start: usize,
    /// Gap end position
    pub end: usize,
    /// Gap size
    pub size: usize,
    /// Confidence of gap detection
    pub confidence: f32,
}

/// Reconstruction statistics
#[derive(Debug, Clone, Default)]
pub struct ReconstructionStatistics {
    /// Total reconstructions attempted
    pub total_reconstructions: usize,
    /// Successful reconstructions
    pub successful_reconstructions: usize,
    /// Average reconstruction confidence
    pub avg_confidence: f32,
    /// Average fragments per reconstruction
    pub avg_fragments_per_reconstruction: f32,
    /// Average gaps filled per reconstruction
    pub avg_gaps_filled: f32,
    /// Cache hit rate
    pub cache_hit_rate: f32,
    /// Average reconstruction time
    pub avg_reconstruction_time: Duration,
}

/// Reconstruction request
#[derive(Debug, Clone)]
pub struct ReconstructionRequest {
    /// Request ID
    pub id: String,
    /// Available fragments
    pub available_fragments: Vec<String>,
    /// Target memory size
    pub target_size: usize,
    /// Reconstruction quality threshold
    pub quality_threshold: f32,
    /// Enable AI gap filling
    pub enable_ai_filling: bool,
    /// Request priority
    pub priority: ReconstructionPriority,
    /// Request timestamp
    pub created_at: DateTime<Utc>,
}

/// Reconstruction priority levels
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReconstructionPriority {
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

/// Reconstruction result
#[derive(Debug, Clone)]
pub struct ReconstructionResult {
    /// Request ID
    pub request_id: String,
    /// Reconstructed memory
    pub memory: Option<ReconstructedMemory>,
    /// Reconstruction success
    pub success: bool,
    /// Confidence score
    pub confidence: f32,
    /// Fragments used
    pub fragments_used: Vec<String>,
    /// Gaps filled
    pub gaps_filled: usize,
    /// Reconstruction time
    pub reconstruction_time: Duration,
    /// Quality metrics
    pub quality_metrics: ReconstructionQualityMetrics,
}

/// Reconstruction quality metrics
#[derive(Debug, Clone, Default)]
pub struct ReconstructionQualityMetrics {
    /// Fragment coverage percentage
    pub fragment_coverage: f32,
    /// Pattern coherence score
    pub pattern_coherence: f32,
    /// Gap filling quality
    pub gap_filling_quality: f32,
    /// Semantic consistency
    pub semantic_consistency: f32,
    /// Overall quality score
    pub overall_quality: f32,
}

/// Trait for pattern matching algorithms
pub trait PatternMatchingAlgorithm: Send + Sync + std::fmt::Debug {
    /// Match fragments against patterns
    fn match_fragments(
        &self,
        fragments: &[MemoryFragment],
        templates: &[PatternTemplate],
    ) -> Vec<FragmentAlignment>;

    /// Calculate alignment score
    fn calculate_score(&self, alignment: &FragmentAlignment) -> f32;

    /// Get algorithm name
    fn name(&self) -> &str;
}

/// Sequential pattern matching algorithm
#[derive(Debug)]
pub struct SequentialPatternMatcher {
    /// Minimum fragment overlap
    min_overlap: usize,
    /// Alignment threshold
    alignment_threshold: f32,
}

/// Hierarchical pattern matching algorithm
#[derive(Debug)]
pub struct HierarchicalPatternMatcher {
    /// Hierarchy levels
    hierarchy_levels: usize,
    /// Clustering threshold
    clustering_threshold: f32,
}

/// Adaptive pattern matching algorithm
#[derive(Debug)]
pub struct AdaptivePatternMatcher {
    /// Learning rate
    learning_rate: f32,
    /// Adaptation history
    adaptation_history: VecDeque<AdaptationRecord>,
}

/// Adaptation record for learning
#[derive(Debug, Clone)]
pub struct AdaptationRecord {
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Input patterns
    pub input_patterns: Vec<String>,
    /// Output quality
    pub output_quality: f32,
    /// Parameters used
    pub parameters: HashMap<String, f32>,
}

impl MemoryReconstructionProtocol {
    /// Create a new memory reconstruction protocol
    pub fn new(config: MemoryAttractorConfig) -> Self {
        Self {
            config: config.clone(),
            fragment_store: Arc::new(AsyncRwLock::new(HashMap::new())),
            reconstruction_cache: Arc::new(RwLock::new(HashMap::new())),
            pattern_matcher: Arc::new(PatternMatchingEngine::new()),
            statistics: Arc::new(RwLock::new(ReconstructionStatistics::default())),
            status: Arc::new(RwLock::new(ComponentStatus::Initializing)),
        }
    }

    /// Initialize the reconstruction protocol

    pub async fn initialize(&self) -> ContextNestResult<()> {
        *self.status.write().unwrap() = ComponentStatus::Running;
        Ok(())
    }

    /// Add a memory fragment to the store

    pub async fn add_fragment(&self, fragment: MemoryFragment) -> ContextNestResult<()> {
        let fragment_id = fragment.id.clone();
        self.fragment_store
            .write()
            .await
            .insert(fragment_id, fragment);
        Ok(())
    }

    /// Reconstruct memory from fragments

    pub async fn reconstruct_memory(
        &self,
        request: ReconstructionRequest,
    ) -> ContextNestResult<ReconstructionResult> {
        let start_time = Utc::now();

        // Update statistics
        self.update_statistics(|stats| {
            stats.total_reconstructions += 1;
        });

        // Check cache first
        let cache_key = self.generate_cache_key(&request);
        if let Some(cached) = self.get_cached_reconstruction(&cache_key) {
            return Ok(ReconstructionResult {
                request_id: request.id.clone(),
                memory: Some(cached.memory.clone()),
                success: true,
                confidence: cached.memory.confidence,
                fragments_used: cached.memory.source_fragments.clone(),
                gaps_filled: cached.memory.filled_gaps.len(),
                reconstruction_time: Duration::from_millis(1), // Cache hit is fast
                quality_metrics: ReconstructionQualityMetrics::default(),
            });
        }

        // Get available fragments
        let fragments = self
            .get_available_fragments(&request.available_fragments)
            .await?;

        if fragments.is_empty() {
            return Ok(ReconstructionResult {
                request_id: request.id.clone(),
                memory: None,
                success: false,
                confidence: 0.0,
                fragments_used: Vec::new(),
                gaps_filled: 0,
                reconstruction_time: Utc::now()
                    .signed_duration_since(start_time)
                    .to_std()
                    .unwrap_or_default(),
                quality_metrics: ReconstructionQualityMetrics::default(),
            });
        }

        // Find best pattern alignment
        let alignments = self.pattern_matcher.find_alignments(&fragments).await?;

        if alignments.is_empty() {
            return Ok(ReconstructionResult {
                request_id: request.id.clone(),
                memory: None,
                success: false,
                confidence: 0.0,
                fragments_used: Vec::new(),
                gaps_filled: 0,
                reconstruction_time: Utc::now()
                    .signed_duration_since(start_time)
                    .to_std()
                    .unwrap_or_default(),
                quality_metrics: ReconstructionQualityMetrics::default(),
            });
        }

        // Select best alignment
        let best_alignment = self.select_best_alignment(&alignments);

        // Reconstruct memory from alignment
        let reconstructed = self
            .reconstruct_from_alignment(&best_alignment, &fragments, &request)
            .await?;

        // Calculate quality metrics
        let quality_metrics = self.calculate_quality_metrics(&reconstructed, &best_alignment);

        // Update cache if quality is good enough
        if reconstructed.confidence > request.quality_threshold {
            self.cache_reconstruction(cache_key, reconstructed.clone());
        }

        // Update statistics
        let reconstruction_time = Utc::now()
            .signed_duration_since(start_time)
            .to_std()
            .unwrap_or_default();
        self.update_statistics(|stats| {
            if reconstructed.confidence > request.quality_threshold {
                stats.successful_reconstructions += 1;
            }
            stats.avg_confidence = (stats.avg_confidence
                * (stats.total_reconstructions - 1) as f32
                + reconstructed.confidence)
                / stats.total_reconstructions as f32;
            stats.avg_fragments_per_reconstruction = (stats.avg_fragments_per_reconstruction
                * (stats.total_reconstructions - 1) as f32
                + fragments.len() as f32)
                / stats.total_reconstructions as f32;
            stats.avg_gaps_filled = (stats.avg_gaps_filled
                * (stats.total_reconstructions - 1) as f32
                + reconstructed.filled_gaps.len() as f32)
                / stats.total_reconstructions as f32;
            stats.avg_reconstruction_time = stats.avg_reconstruction_time
                * (stats.total_reconstructions - 1) as u32
                + Duration::from_millis(reconstruction_time.as_millis() as u64);
            stats.avg_reconstruction_time /= stats.total_reconstructions as u32;
        });

        Ok(ReconstructionResult {
            request_id: request.id,
            memory: Some(reconstructed.clone()),
            success: reconstructed.confidence > request.quality_threshold,
            confidence: reconstructed.confidence,
            fragments_used: reconstructed.source_fragments.clone(),
            gaps_filled: reconstructed.filled_gaps.len(),
            reconstruction_time,
            quality_metrics,
        })
    }

    /// Batch reconstruct multiple memories

    pub async fn batch_reconstruct(
        &self,
        requests: Vec<ReconstructionRequest>,
    ) -> ContextNestResult<Vec<ReconstructionResult>> {
        let mut results = Vec::with_capacity(requests.len());

        // Sort requests by priority
        let mut sorted_requests = requests;
        sorted_requests.sort_by(|a, b| b.priority.cmp(&a.priority));

        for request in sorted_requests {
            let result = self.reconstruct_memory(request).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// Remove old fragments

    pub async fn cleanup_old_fragments(&self, max_age: Duration) -> ContextNestResult<usize> {
        let mut fragments = self.fragment_store.write().await;
        let now = Utc::now();
        let mut to_remove = Vec::new();

        for (id, fragment) in fragments.iter() {
            let age = now
                .signed_duration_since(fragment.created_at)
                .to_std()
                .unwrap_or_default();
            if age > max_age {
                to_remove.push(id.clone());
            }
        }

        let removed_count = to_remove.len();

        for id in to_remove {
            fragments.remove(&id);
        }

        Ok(removed_count)
    }

    /// Internal accessor for the manager. Returns a clone of the Arc so
    /// the manager can perform CRUD directly on the canonical fragment map
    /// without going through the protocol's own pipeline (which is
    /// reconstruction-oriented, not key-value-oriented). Cloning an Arc is
    /// O(1) — only the reference count is bumped, not the map contents.
    pub fn fragment_store(&self) -> Arc<AsyncRwLock<HashMap<String, MemoryFragment>>> {
        self.fragment_store.clone()
    }

    /// Clear reconstruction cache

    pub fn clear_cache(&self) -> ContextNestResult<usize> {
        let mut cache = self.reconstruction_cache.write().unwrap();
        let size = cache.len();
        cache.clear();
        Ok(size)
    }

    /// Get reconstruction statistics

    pub fn get_statistics(&self) -> ReconstructionStatistics {
        self.statistics.read().unwrap().clone()
    }

    // Helper methods

    async fn get_available_fragments(
        &self,
        fragment_ids: &[String],
    ) -> ContextNestResult<Vec<MemoryFragment>> {
        let fragment_store = self.fragment_store.read().await;
        let mut fragments = Vec::new();

        for id in fragment_ids {
            if let Some(fragment) = fragment_store.get(id) {
                fragments.push(fragment.clone());
            }
        }

        // Sort fragments by importance
        fragments.sort_by(|a, b| b.importance.partial_cmp(&a.importance).unwrap());

        Ok(fragments)
    }

    fn generate_cache_key(&self, request: &ReconstructionRequest) -> String {
        let mut fragment_ids = request.available_fragments.clone();
        fragment_ids.sort();
        format!(
            "{}_{:?}_{}_{}",
            fragment_ids.join("_"),
            request.target_size,
            request.quality_threshold,
            request.enable_ai_filling
        )
    }

    fn get_cached_reconstruction(&self, cache_key: &str) -> Option<CachedReconstruction> {
        let mut cache = self.reconstruction_cache.write().unwrap();
        if let Some(cached) = cache.get_mut(cache_key) {
            cached.last_accessed = Utc::now();
            cached.access_count += 1;
            return Some(cached.clone());
        }
        None
    }

    fn cache_reconstruction(&self, cache_key: String, memory: ReconstructedMemory) {
        let mut cache = self.reconstruction_cache.write().unwrap();

        // Check cache size limit
        if cache.len() >= self.config.reconstruction_cache_size {
            self.evict_oldest_cache_entries(&mut cache);
        }

        cache.insert(
            cache_key,
            CachedReconstruction {
                memory,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                access_count: 1,
                recomputation_cost: 1.0, // Would need actual estimation
            },
        );
    }

    fn evict_oldest_cache_entries(&self, cache: &mut HashMap<String, CachedReconstruction>) {
        // Collect owned (String, DateTime) pairs first so we can drop the
        // immutable borrow on `cache` before mutating it via `.remove(...)`.
        let mut entries: Vec<(String, DateTime<Utc>)> = cache
            .iter()
            .map(|(k, v)| (k.clone(), v.created_at))
            .collect();
        entries.sort_by_key(|(_, created_at)| *created_at);

        let eviction_count = cache.len() / 4; // Remove 25%
        for (key, _) in entries.iter().take(eviction_count) {
            cache.remove(key);
        }
    }

    fn select_best_alignment<'a>(
        &self,
        alignments: &'a [FragmentAlignment],
    ) -> &'a FragmentAlignment {
        // Explicit lifetime: the returned reference points into `alignments`,
        // not into `self`. Without the annotation Rust can't tell which
        // input the return borrow originates from (default elision picks
        // `&self`, which is wrong here).
        alignments
            .iter()
            .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap())
            .unwrap_or(&alignments[0])
    }

    async fn reconstruct_from_alignment(
        &self,
        alignment: &FragmentAlignment,
        fragments: &[MemoryFragment],
        request: &ReconstructionRequest,
    ) -> ContextNestResult<ReconstructedMemory> {
        let mut reconstructed_content = vec![0.0; request.target_size];
        let mut source_fragments = Vec::new();
        let mut filled_gaps = Vec::new();

        // Place fragments according to alignment
        for (i, fragment_id) in alignment.fragments.iter().enumerate() {
            if let Some(position) = alignment.positions.get(i) {
                if let Some(fragment) = fragments.iter().find(|f| f.id == *fragment_id) {
                    let end_pos = (*position + fragment.content.len()).min(request.target_size);
                    if end_pos > *position {
                        reconstructed_content[*position..end_pos]
                            .copy_from_slice(&fragment.content[..end_pos - *position]);
                        source_fragments.push(fragment_id.clone());
                    }
                }
            }
        }

        // Fill gaps
        for gap in &alignment.gaps {
            if gap.start < request.target_size && gap.end <= request.target_size {
                let gap_content = if request.enable_ai_filling {
                    self.fill_gap_with_ai(gap, &reconstructed_content, fragments)
                        .await?
                } else {
                    self.fill_gap_with_interpolation(gap, &reconstructed_content, fragments)?
                };

                let end_pos = (gap.start + gap_content.len()).min(request.target_size);
                reconstructed_content[gap.start..end_pos]
                    .copy_from_slice(&gap_content[..end_pos - gap.start]);

                filled_gaps.push(GapInfo {
                    position: gap.start,
                    size: gap.size,
                    filling_method: if request.enable_ai_filling {
                        GapFillingMethod::AIGeneration
                    } else {
                        GapFillingMethod::Interpolation
                    },
                    confidence: gap.confidence,
                    source: GapFillSource::PatternMatching,
                });
            }
        }

        // Calculate confidence
        let confidence = self.calculate_reconstruction_confidence(
            &reconstructed_content,
            &source_fragments,
            &filled_gaps,
        );

        Ok(ReconstructedMemory {
            id: utils::generate_unique_id(),
            content: reconstructed_content,
            confidence,
            source_fragments,
            filled_gaps,
            reconstructed_at: Utc::now(),
            persistence_score: confidence,
        })
    }

    async fn fill_gap_with_ai(
        &self,
        gap: &GapPosition,
        context: &[f32],
        fragments: &[MemoryFragment],
    ) -> ContextNestResult<Vec<f32>> {
        // Placeholder for AI gap filling
        // In practice, this would call an external AI service
        let mut gap_content = vec![0.0; gap.size];

        // Simple interpolation as fallback
        if gap.start > 0 && gap.end < context.len() {
            let left_context = &context[gap.start.saturating_sub(5)..gap.start];
            let right_context = &context[gap.end..context.len().min(gap.end + 5)];

            if !left_context.is_empty() && !right_context.is_empty() {
                for (i, value) in gap_content.iter_mut().enumerate() {
                    let progress = i as f32 / gap.size as f32;
                    let left_avg = left_context.iter().sum::<f32>() / left_context.len() as f32;
                    let right_avg = right_context.iter().sum::<f32>() / right_context.len() as f32;
                    *value = left_avg * (1.0 - progress) + right_avg * progress;
                }
            }
        }

        Ok(gap_content)
    }

    fn fill_gap_with_interpolation(
        &self,
        gap: &GapPosition,
        context: &[f32],
        fragments: &[MemoryFragment],
    ) -> ContextNestResult<Vec<f32>> {
        let mut gap_content = vec![0.0; gap.size];

        if gap.start > 0 && gap.end < context.len() {
            let left_value = context[gap.start - 1];
            let right_value = context[gap.end];

            for (i, value) in gap_content.iter_mut().enumerate() {
                let progress = i as f32 / gap.size as f32;
                *value = left_value * (1.0 - progress) + right_value * progress;
            }
        }

        Ok(gap_content)
    }

    fn calculate_reconstruction_confidence(
        &self,
        content: &[f32],
        source_fragments: &[String],
        filled_gaps: &[GapInfo],
    ) -> f32 {
        let fragment_coverage =
            source_fragments.len() as f32 / (content.len() as f32 / 64.0).max(1.0);
        let gap_confidence = if filled_gaps.is_empty() {
            1.0
        } else {
            filled_gaps.iter().map(|g| g.confidence).sum::<f32>() / filled_gaps.len() as f32
        };

        (fragment_coverage * 0.7 + gap_confidence * 0.3).min(1.0)
    }

    fn calculate_quality_metrics(
        &self,
        memory: &ReconstructedMemory,
        alignment: &FragmentAlignment,
    ) -> ReconstructionQualityMetrics {
        let fragment_coverage =
            memory.source_fragments.len() as f32 / (memory.content.len() as f32 / 64.0).max(1.0);
        let pattern_coherence = alignment.score;
        let gap_filling_quality = if memory.filled_gaps.is_empty() {
            1.0
        } else {
            memory.filled_gaps.iter().map(|g| g.confidence).sum::<f32>()
                / memory.filled_gaps.len() as f32
        };
        let semantic_consistency = memory.confidence;
        let overall_quality =
            (fragment_coverage + pattern_coherence + gap_filling_quality + semantic_consistency)
                / 4.0;

        ReconstructionQualityMetrics {
            fragment_coverage,
            pattern_coherence,
            gap_filling_quality,
            semantic_consistency,
            overall_quality,
        }
    }

    fn update_statistics<F>(&self, update_fn: F)
    where
        F: FnOnce(&mut ReconstructionStatistics),
    {
        let mut stats = self.statistics.write().unwrap();
        update_fn(&mut stats);

        // Update derived metrics
        if stats.total_reconstructions > 0 {
            stats.cache_hit_rate = (stats.total_reconstructions
                - self.reconstruction_cache.read().unwrap().len())
                as f32
                / stats.total_reconstructions as f32;
        }
    }
}

impl PatternMatchingEngine {
    fn new() -> Self {
        Self {
            templates: HashMap::new(),
            algorithms: vec![
                Box::new(SequentialPatternMatcher::new()),
                Box::new(HierarchicalPatternMatcher::new()),
                Box::new(AdaptivePatternMatcher::new()),
            ],
            alignment_cache: HashMap::new(),
        }
    }

    async fn find_alignments(
        &self,
        fragments: &[MemoryFragment],
    ) -> ContextNestResult<Vec<FragmentAlignment>> {
        let mut alignments = Vec::new();

        for algorithm in &self.algorithms {
            let algorithm_alignments = algorithm.match_fragments(fragments, &[]);
            alignments.extend(algorithm_alignments);
        }

        // Sort by score
        alignments.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        Ok(alignments)
    }
}

impl SequentialPatternMatcher {
    fn new() -> Self {
        Self {
            min_overlap: 10,
            alignment_threshold: 0.5,
        }
    }
}

impl PatternMatchingAlgorithm for SequentialPatternMatcher {
    fn match_fragments(
        &self,
        fragments: &[MemoryFragment],
        _templates: &[PatternTemplate],
    ) -> Vec<FragmentAlignment> {
        let mut alignments = Vec::new();

        // Simple sequential alignment based on content similarity
        for i in 0..fragments.len() {
            for j in (i + 1)..fragments.len() {
                let similarity =
                    utils::cosine_similarity(&fragments[i].content, &fragments[j].content);

                if similarity > self.alignment_threshold {
                    let alignment = FragmentAlignment {
                        fragments: vec![fragments[i].id.clone(), fragments[j].id.clone()],
                        score: similarity,
                        positions: vec![i * 64, j * 64], // Simplified positioning
                        gaps: vec![],
                    };
                    alignments.push(alignment);
                }
            }
        }

        alignments
    }

    fn calculate_score(&self, alignment: &FragmentAlignment) -> f32 {
        alignment.score
    }

    fn name(&self) -> &str {
        "sequential"
    }
}

impl HierarchicalPatternMatcher {
    fn new() -> Self {
        Self {
            hierarchy_levels: 3,
            clustering_threshold: 0.7,
        }
    }
}

impl PatternMatchingAlgorithm for HierarchicalPatternMatcher {
    fn match_fragments(
        &self,
        fragments: &[MemoryFragment],
        _templates: &[PatternTemplate],
    ) -> Vec<FragmentAlignment> {
        // Simplified hierarchical clustering based alignment
        let mut alignments = Vec::new();

        if fragments.len() >= 2 {
            // Create alignment with all fragments
            let avg_similarity =
                fragments.iter().map(|f| f.confidence).sum::<f32>() / fragments.len() as f32;

            if avg_similarity > self.clustering_threshold {
                let positions: Vec<usize> = (0..fragments.len()).map(|i| i * 64).collect();
                let alignment = FragmentAlignment {
                    fragments: fragments.iter().map(|f| f.id.clone()).collect(),
                    score: avg_similarity,
                    positions,
                    gaps: vec![],
                };
                alignments.push(alignment);
            }
        }

        alignments
    }

    fn calculate_score(&self, alignment: &FragmentAlignment) -> f32 {
        alignment.score * (alignment.fragments.len() as f32 / 10.0).min(1.0)
    }

    fn name(&self) -> &str {
        "hierarchical"
    }
}

impl AdaptivePatternMatcher {
    fn new() -> Self {
        Self {
            learning_rate: 0.1,
            adaptation_history: VecDeque::new(),
        }
    }
}

impl PatternMatchingAlgorithm for AdaptivePatternMatcher {
    fn match_fragments(
        &self,
        fragments: &[MemoryFragment],
        _templates: &[PatternTemplate],
    ) -> Vec<FragmentAlignment> {
        // Adaptive matching based on historical performance
        let mut alignments = Vec::new();

        // Simple adaptive strategy: weight fragments by importance
        let total_importance: f32 = fragments.iter().map(|f| f.importance).sum();

        if total_importance > 0.0 && fragments.len() >= 2 {
            let weighted_score = total_importance / fragments.len() as f32;
            let positions: Vec<usize> = (0..fragments.len()).map(|i| i * 64).collect();

            let alignment = FragmentAlignment {
                fragments: fragments.iter().map(|f| f.id.clone()).collect(),
                score: weighted_score,
                positions,
                gaps: vec![],
            };
            alignments.push(alignment);
        }

        alignments
    }

    fn calculate_score(&self, alignment: &FragmentAlignment) -> f32 {
        alignment.score * 1.2 // Adaptive bonus
    }

    fn name(&self) -> &str {
        "adaptive"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::attractors::MemoryAttractorConfig;

    #[tokio::test]
    async fn test_memory_reconstruction_protocol() {
        let config = MemoryAttractorConfig::default();
        let protocol = MemoryReconstructionProtocol::new(config);
        protocol.initialize().await.unwrap();

        // Add some fragments
        let fragment1 = MemoryFragment {
            id: "frag1".to_string(),
            content: vec![0.1; 64],
            importance: 0.8,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            attractor_basin_id: None,
            connections: HashSet::new(),
            confidence: 0.9,
        };

        protocol.add_fragment(fragment1).await.unwrap();

        // Create reconstruction request
        let request = ReconstructionRequest {
            id: "req1".to_string(),
            available_fragments: vec!["frag1".to_string()],
            target_size: 128,
            quality_threshold: 0.7,
            enable_ai_filling: false,
            priority: ReconstructionPriority::Medium,
            created_at: Utc::now(),
        };

        let result = protocol.reconstruct_memory(request).await.unwrap();
        let _ = result;
    }

    #[tokio::test]
    async fn test_batch_reconstruction() {
        let config = MemoryAttractorConfig::default();
        let protocol = MemoryReconstructionProtocol::new(config);
        protocol.initialize().await.unwrap();

        let requests = vec![
            ReconstructionRequest {
                id: "req1".to_string(),
                available_fragments: vec![],
                target_size: 64,
                quality_threshold: 0.5,
                enable_ai_filling: false,
                priority: ReconstructionPriority::Low,
                created_at: Utc::now(),
            },
            ReconstructionRequest {
                id: "req2".to_string(),
                available_fragments: vec![],
                target_size: 64,
                quality_threshold: 0.5,
                enable_ai_filling: false,
                priority: ReconstructionPriority::High,
                created_at: Utc::now(),
            },
        ];

        let results = protocol.batch_reconstruct(requests).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_sequential_pattern_matcher() {
        let matcher = SequentialPatternMatcher::new();

        let fragments = vec![
            MemoryFragment {
                id: "frag1".to_string(),
                content: vec![0.1, 0.2, 0.3],
                importance: 0.8,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                attractor_basin_id: None,
                connections: HashSet::new(),
                confidence: 0.9,
            },
            MemoryFragment {
                id: "frag2".to_string(),
                content: vec![0.1, 0.2, 0.4],
                importance: 0.7,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                attractor_basin_id: None,
                connections: HashSet::new(),
                confidence: 0.8,
            },
        ];

        let alignments = matcher.match_fragments(&fragments, &[]);
        assert!(!alignments.is_empty());

        for alignment in &alignments {
            assert_eq!(alignment.fragments.len(), 2);
            assert!(alignment.score > 0.0);
        }
    }

    #[test]
    fn test_hierarchical_pattern_matcher() {
        let matcher = HierarchicalPatternMatcher::new();

        let fragments = vec![
            MemoryFragment {
                id: "frag1".to_string(),
                content: vec![0.1; 32],
                importance: 0.9,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                attractor_basin_id: None,
                connections: HashSet::new(),
                confidence: 0.95,
            },
            MemoryFragment {
                id: "frag2".to_string(),
                content: vec![0.2; 32],
                importance: 0.85,
                created_at: Utc::now(),
                last_accessed: Utc::now(),
                attractor_basin_id: None,
                connections: HashSet::new(),
                confidence: 0.9,
            },
        ];

        let alignments = matcher.match_fragments(&fragments, &[]);

        if !alignments.is_empty() {
            assert_eq!(alignments[0].fragments.len(), 2);
            assert!(alignments[0].score > 0.7);
        }
    }
}

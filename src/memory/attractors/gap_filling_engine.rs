//! AI-Powered Gap Filling Engine
//! Implements sophisticated gap filling algorithms for memory reconstruction,
//! including AI generation, pattern-based reconstruction, and similarity-based borrowing.

use crate::error::ContextNestResult;
use crate::error::{ContextNestError, Result};
use crate::memory::attractors::{
    utils, ComponentStatus, GapFillSource, GapFillingMethod, GapInfo, MemoryAttractorConfig,
    MemoryFragment,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::RwLock as AsyncRwLock;

/// Advanced gap filling engine with AI capabilities
#[derive(Debug)]
pub struct GapFillingEngine {
    /// Configuration
    config: MemoryAttractorConfig,
    /// AI generation service
    ai_generator: Arc<AIGenerationService>,
    /// Pattern-based filler
    pattern_filler: Arc<PatternBasedFiller>,
    /// Similarity engine
    similarity_engine: Arc<SimilarityEngine>,
    /// Gap filling statistics
    statistics: Arc<RwLock<GapFillingStatistics>>,
    /// Component status
    status: Arc<RwLock<ComponentStatus>>,
    /// Filling cache
    filling_cache: Arc<RwLock<HashMap<String, CachedGapFill>>>,
}

/// AI generation service for gap filling.
/// `generation_cache` and `rate_limiter` are wrapped in `Mutex` so the
/// generator can be shared behind `Arc<AIGenerationService>` and still
/// mutate per-call state under `&self` (e.g. `generate_content` lives on
/// `&self` because the service is owned by `Arc` inside `GapFillingEngine`).
/// The locks are not held across `.await` points, so `std::sync::Mutex`
/// is the right primitive here — `tokio::sync::Mutex` would only add cost.
#[derive(Debug)]
pub struct AIGenerationService {
    /// Model configuration
    model_config: AIModelConfig,
    /// Generation cache
    generation_cache: std::sync::Mutex<HashMap<String, AIGenerationResult>>,
    /// API rate limiter
    rate_limiter: std::sync::Mutex<RateLimiter>,
    /// Performance metrics
    metrics: AIGenerationMetrics,
}

/// AI model configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIModelConfig {
    /// Model name
    pub model_name: String,
    /// API endpoint
    pub api_endpoint: String,
    /// Maximum token length
    pub max_tokens: usize,
    /// Temperature parameter
    pub temperature: f32,
    /// Top-p sampling parameter
    pub top_p: f32,
    /// Generation timeout
    pub timeout_seconds: u64,
    /// Retry configuration
    retry_config: RetryConfig,
}

/// Retry configuration for AI generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum retry attempts
    pub max_attempts: usize,
    /// Base delay between retries
    pub base_delay_ms: u64,
    /// Exponential backoff factor
    pub backoff_factor: f32,
    /// Jitter added to delay
    pub jitter_ms: u64,
}

/// AI generation result
#[derive(Debug, Clone)]
pub struct AIGenerationResult {
    /// Generated content
    pub content: Vec<f32>,
    /// Generation confidence
    pub confidence: f32,
    /// Generation time
    pub generation_time: Duration,
    /// Tokens used
    pub tokens_used: usize,
    /// Model used
    pub model_used: String,
}

/// Rate limiter for API calls
#[derive(Debug)]
pub struct RateLimiter {
    /// Maximum requests per second
    pub max_rps: f32,
    /// Request history (timestamps of recent requests). Uses `DateTime<Utc>`
    /// rather than `Instant` so the rate-limiter state can be serialized
    /// for diagnostics and survives process boundaries; precision is
    /// adequate for sub-second rate limiting.
    pub request_history: VecDeque<DateTime<Utc>>,
    /// Current request count
    pub current_count: usize,
}

/// AI generation performance metrics
#[derive(Debug, Clone, Default)]
pub struct AIGenerationMetrics {
    /// Total generations requested
    pub total_generations: usize,
    /// Successful generations
    pub successful_generations: usize,
    /// Average generation time
    pub avg_generation_time: Duration,
    /// Average confidence
    pub avg_confidence: f32,
    /// API error rate
    pub api_error_rate: f32,
    /// Cache hit rate
    pub cache_hit_rate: f32,
}

/// Pattern-based filler using known patterns
#[derive(Debug)]
pub struct PatternBasedFiller {
    /// Pattern database
    pattern_database: Arc<RwLock<PatternDatabase>>,
    /// Pattern matching algorithms
    matchers: Vec<Box<dyn PatternMatcher>>,
    /// Filling statistics
    metrics: PatternFillingMetrics,
}

/// Pattern database for reconstruction
#[derive(Debug, Clone)]
pub struct PatternDatabase {
    /// Common patterns
    pub common_patterns: Vec<MemoryPattern>,
    /// Pattern frequency map
    pub pattern_frequency: HashMap<String, usize>,
    /// Pattern context associations
    pub context_associations: HashMap<String, Vec<String>>,
}

/// Memory pattern for gap filling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPattern {
    /// Pattern ID
    pub id: String,
    /// Pattern content
    pub content: Vec<f32>,
    /// Pattern type
    pub pattern_type: PatternType,
    /// Context tags
    pub context_tags: Vec<String>,
    /// Usage count
    pub usage_count: usize,
    /// Last used timestamp
    pub last_used: DateTime<Utc>,
}

/// Types of memory patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PatternType {
    /// Sequential pattern
    Sequential,
    /// Recurrent pattern
    Recurrent,
    /// Hierarchical pattern
    Hierarchical,
    /// Semantic pattern
    Semantic,
    /// Transitional pattern
    Transitional,
}

/// Pattern filler metrics
#[derive(Debug, Clone, Default)]
pub struct PatternFillingMetrics {
    /// Total pattern fills
    pub total_fills: usize,
    /// Successful fills
    pub successful_fills: usize,
    /// Average match score
    pub avg_match_score: f32,
    /// Pattern database size
    pub database_size: usize,
    /// Most used patterns
    pub most_used_patterns: Vec<String>,
}

/// Trait for pattern matching algorithms
pub trait PatternMatcher: Send + Sync + std::fmt::Debug {
    /// Find matching patterns for context
    fn find_matches(&self, context: &[f32], patterns: &[MemoryPattern]) -> Vec<PatternMatch>;

    /// Calculate match confidence
    fn calculate_confidence(&self, context: &[f32], pattern: &MemoryPattern) -> f32;

    /// Get matcher name
    fn name(&self) -> &str;
}

/// Pattern match result
#[derive(Debug, Clone)]
pub struct PatternMatch {
    /// Pattern ID
    pub pattern_id: String,
    /// Match confidence
    pub confidence: f32,
    /// Match position
    pub position: usize,
    /// Match score
    pub score: f32,
}

/// Similarity engine for finding similar content
#[derive(Debug)]
pub struct SimilarityEngine {
    /// Similarity database
    similarity_database: Arc<RwLock<SimilarityDatabase>>,
    /// Similarity algorithms
    algorithms: Vec<Box<dyn SimilarityAlgorithm>>,
    /// Cache for similarity computations
    similarity_cache: HashMap<String, f32>,
    /// Metrics
    metrics: SimilarityEngineMetrics,
}

/// Similarity database
#[derive(Debug, Clone)]
pub struct SimilarityDatabase {
    /// Content vectors indexed by ID
    pub content_vectors: HashMap<String, Vec<f32>>,
    /// Precomputed similarity matrix
    pub similarity_matrix: HashMap<String, HashMap<String, f32>>,
    /// Last update timestamp
    pub last_updated: DateTime<Utc>,
}

/// Similarity engine metrics
#[derive(Debug, Clone, Default)]
pub struct SimilarityEngineMetrics {
    /// Total similarity computations
    pub total_computations: usize,
    /// Cache hit rate
    pub cache_hit_rate: f32,
    /// Average similarity score
    pub avg_similarity_score: f32,
    /// Database size
    pub database_size: usize,
}

/// Trait for similarity algorithms
pub trait SimilarityAlgorithm: Send + Sync + std::fmt::Debug {
    /// Calculate similarity between two vectors
    fn calculate_similarity(&self, vec1: &[f32], vec2: &[f32]) -> f32;

    /// Get algorithm name
    fn name(&self) -> &str;
}

/// Gap filling statistics
#[derive(Debug, Clone, Default)]
pub struct GapFillingStatistics {
    /// Total gaps filled
    pub total_gaps_filled: usize,
    /// Successful fills
    pub successful_fills: usize,
    /// Fills by method
    pub fills_by_method: HashMap<GapFillingMethod, usize>,
    /// Average fill confidence
    pub avg_confidence: f32,
    /// Average fill time
    pub avg_fill_time: Duration,
    /// Success rate by gap size
    pub success_rate_by_size: HashMap<usize, f32>,
}

/// Cached gap fill result
#[derive(Debug, Clone)]
pub struct CachedGapFill {
    /// Filled content
    pub content: Vec<f32>,
    /// Filling method
    pub method: GapFillingMethod,
    /// Confidence score
    pub confidence: f32,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Access count
    pub access_count: usize,
}

/// Gap filling request
#[derive(Debug, Clone)]
pub struct GapFillingRequest {
    /// Request ID
    pub id: String,
    /// Gap position
    pub position: usize,
    /// Gap size
    pub size: usize,
    /// Left context
    pub left_context: Vec<f32>,
    /// Right context
    pub right_context: Vec<f32>,
    /// Available fragments
    pub available_fragments: Vec<MemoryFragment>,
    /// Filling methods to try (in order)
    pub methods: Vec<GapFillingMethod>,
    /// Confidence threshold
    pub confidence_threshold: f32,
    /// Request priority
    pub priority: GapFillingPriority,
}

/// Gap filling priority levels
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum GapFillingPriority {
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

/// Gap filling result
#[derive(Debug, Clone)]
pub struct GapFillingResult {
    /// Request ID
    pub request_id: String,
    /// Filled content
    pub content: Option<Vec<f32>>,
    /// Filling method used
    pub method_used: Option<GapFillingMethod>,
    /// Content source
    pub source: Option<GapFillSource>,
    /// Confidence score
    pub confidence: f32,
    /// Fill success
    pub success: bool,
    /// Filling time
    pub fill_time: Duration,
    /// Method attempts
    pub method_attempts: Vec<MethodAttempt>,
}

/// Attempt information for each method tried
#[derive(Debug, Clone)]
pub struct MethodAttempt {
    /// Method attempted
    pub method: GapFillingMethod,
    /// Success status
    pub success: bool,
    /// Confidence achieved
    pub confidence: f32,
    /// Time taken
    pub duration: Duration,
    /// Error message (if any)
    pub error: Option<String>,
}

impl GapFillingEngine {
    /// Create a new gap filling engine
    pub fn new(config: MemoryAttractorConfig) -> Self {
        Self {
            config: config.clone(),
            ai_generator: Arc::new(AIGenerationService::new()),
            pattern_filler: Arc::new(PatternBasedFiller::new()),
            similarity_engine: Arc::new(SimilarityEngine::new()),
            statistics: Arc::new(RwLock::new(GapFillingStatistics::default())),
            status: Arc::new(RwLock::new(ComponentStatus::Initializing)),
            filling_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Initialize the gap filling engine

    pub async fn initialize(&self) -> ContextNestResult<()> {
        *self.status.write().unwrap() = ComponentStatus::Running;
        Ok(())
    }

    /// Fill a memory gap using available methods

    pub async fn fill_gap(
        &self,
        request: GapFillingRequest,
    ) -> ContextNestResult<GapFillingResult> {
        let start_time = Utc::now();

        // Update statistics
        self.update_statistics(|stats| {
            stats.total_gaps_filled += 1;
        });

        // Check cache first
        let cache_key = self.generate_cache_key(&request);
        if let Some(cached) = self.get_cached_fill(&cache_key) {
            return Ok(GapFillingResult {
                request_id: request.id.clone(),
                content: Some(cached.content.clone()),
                method_used: Some(cached.method.clone()),
                source: Some(GapFillSource::PatternMatching),
                confidence: cached.confidence,
                success: true,
                fill_time: Duration::from_millis(1),
                method_attempts: vec![MethodAttempt {
                    method: cached.method.clone(),
                    success: true,
                    confidence: cached.confidence,
                    duration: Duration::from_millis(1),
                    error: None,
                }],
            });
        }

        let mut method_attempts = Vec::new();
        let mut best_result: Option<(Vec<f32>, GapFillingMethod, f32)> = None;

        // Try each method in order
        for method in &request.methods {
            let method_start = Utc::now();

            let attempt = match method {
                GapFillingMethod::AIGeneration => self.try_ai_generation(&request).await,
                GapFillingMethod::PatternReconstruction => {
                    self.try_pattern_reconstruction(&request).await
                }
                GapFillingMethod::SimilarityBorrowing => {
                    self.try_similarity_borrowing(&request).await
                }
                GapFillingMethod::Interpolation => self.try_interpolation(&request).await,
            };

            let method_duration = Utc::now()
                .signed_duration_since(method_start)
                .to_std()
                .unwrap_or_default();

            match attempt {
                Ok((content, confidence)) if confidence >= request.confidence_threshold => {
                    let method_attempt = MethodAttempt {
                        method: method.clone(),
                        success: true,
                        confidence,
                        duration: method_duration,
                        error: None,
                    };
                    method_attempts.push(method_attempt);

                    // Update best result
                    if best_result.is_none() || confidence > best_result.as_ref().unwrap().2 {
                        best_result = Some((content, method.clone(), confidence));
                    }

                    // If we have a high confidence result, we can stop
                    if confidence >= request.confidence_threshold * 1.1 {
                        break;
                    }
                }
                Ok((content, confidence)) => {
                    let method_attempt = MethodAttempt {
                        method: method.clone(),
                        success: true,
                        confidence,
                        duration: method_duration,
                        error: None,
                    };
                    method_attempts.push(method_attempt);

                    if best_result.is_none() || confidence > best_result.as_ref().unwrap().2 {
                        best_result = Some((content, method.clone(), confidence));
                    }
                }
                Err(e) => {
                    let method_attempt = MethodAttempt {
                        method: method.clone(),
                        success: false,
                        confidence: 0.0,
                        duration: method_duration,
                        error: Some(e.to_string()),
                    };
                    method_attempts.push(method_attempt);
                }
            }
        }

        let fill_time = Utc::now()
            .signed_duration_since(start_time)
            .to_std()
            .unwrap_or_default();

        // Process result
        if let Some((content, method, confidence)) = best_result {
            // Update statistics for success
            self.update_statistics(|stats| {
                stats.successful_fills += 1;
                *stats.fills_by_method.entry(method.clone()).or_insert(0) += 1;
                stats.avg_confidence = (stats.avg_confidence * (stats.successful_fills - 1) as f32
                    + confidence)
                    / stats.successful_fills as f32;
                stats.avg_fill_time = (stats.avg_fill_time * (stats.successful_fills - 1) as u32
                    + Duration::from_millis(fill_time.as_millis() as u64))
                    / stats.successful_fills as u32;

                let size_category = (request.size / 16) * 16; // Group by 16-element chunks
                let success_rate = stats
                    .success_rate_by_size
                    .entry(size_category)
                    .or_insert(0.0);
                *success_rate = (*success_rate * (stats.successful_fills - 1) as f32 + 1.0)
                    / stats.successful_fills as f32;
            });

            // Cache successful result
            self.cache_fill(cache_key, &content, method.clone(), confidence);

            Ok(GapFillingResult {
                request_id: request.id,
                content: Some(content),
                method_used: Some(method),
                source: Some(GapFillSource::PatternMatching),
                confidence,
                success: true,
                fill_time,
                method_attempts,
            })
        } else {
            // Update statistics for failure
            self.update_statistics(|stats| {
                let size_category = (request.size / 16) * 16;
                let success_rate = stats
                    .success_rate_by_size
                    .entry(size_category)
                    .or_insert(0.0);
                if stats.total_gaps_filled > 1 {
                    *success_rate = (*success_rate * (stats.total_gaps_filled - 1) as f32)
                        / (stats.total_gaps_filled) as f32;
                }
            });

            Ok(GapFillingResult {
                request_id: request.id,
                content: None,
                method_used: None,
                source: None,
                confidence: 0.0,
                success: false,
                fill_time,
                method_attempts,
            })
        }
    }

    /// Batch fill multiple gaps

    pub async fn batch_fill_gaps(
        &self,
        requests: Vec<GapFillingRequest>,
    ) -> ContextNestResult<Vec<GapFillingResult>> {
        let mut results = Vec::with_capacity(requests.len());

        // Sort requests by priority
        let mut sorted_requests = requests;
        sorted_requests.sort_by(|a, b| b.priority.cmp(&a.priority));

        // TODO(parallelism): the original implementation spawned each
        // fill_gap as its own tokio task, but that requires `engine` to be
        // `'static` — `Self::clone()` would have to produce an owned value,
        // which today's struct can't (RwLock and Mutex don't implement Clone).
        // Refactor to `Arc<Self>` (wrapping the lock fields too) before
        // re-introducing parallelism. Sequential iteration is correct and
        // adequate for v0.1.0.
        for request in sorted_requests {
            results.push(self.fill_gap(request).await?);
        }

        Ok(results)
    }

    /// Get gap filling statistics

    pub fn get_statistics(&self) -> GapFillingStatistics {
        self.statistics.read().unwrap().clone()
    }

    /// Clear filling cache

    pub fn clear_cache(&self) -> usize {
        let mut cache = self.filling_cache.write().unwrap();
        let size = cache.len();
        cache.clear();
        size
    }

    // Helper methods

    async fn try_ai_generation(
        &self,
        request: &GapFillingRequest,
    ) -> ContextNestResult<(Vec<f32>, f32)> {
        if !self.config.enable_ai_gap_filling {
            return Err(ContextNestError::Configuration(
                "AI gap filling is disabled".to_string(),
            ));
        }

        self.ai_generator
            .generate_content(&request.left_context, &request.right_context, request.size)
            .await
    }

    async fn try_pattern_reconstruction(
        &self,
        request: &GapFillingRequest,
    ) -> ContextNestResult<(Vec<f32>, f32)> {
        let combined_context: Vec<f32> = request
            .left_context
            .iter()
            .chain(request.right_context.iter())
            .cloned()
            .collect();

        self.pattern_filler
            .fill_from_patterns(&combined_context, request.position, request.size)
            .await
    }

    async fn try_similarity_borrowing(
        &self,
        request: &GapFillingRequest,
    ) -> ContextNestResult<(Vec<f32>, f32)> {
        let combined_context: Vec<f32> = request
            .left_context
            .iter()
            .chain(request.right_context.iter())
            .cloned()
            .collect();

        self.similarity_engine
            .borrow_from_similar(
                &combined_context,
                request.size,
                &request.available_fragments,
            )
            .await
    }

    async fn try_interpolation(
        &self,
        request: &GapFillingRequest,
    ) -> ContextNestResult<(Vec<f32>, f32)> {
        let mut interpolated = vec![0.0; request.size];

        if !request.left_context.is_empty() && !request.right_context.is_empty() {
            let left_value = request.left_context[request.left_context.len() - 1];
            let right_value = request.right_context[0];

            for (i, value) in interpolated.iter_mut().enumerate() {
                let progress = i as f32 / request.size as f32;
                *value = left_value * (1.0 - progress) + right_value * progress;
            }
        } else if !request.left_context.is_empty() {
            let left_value = request.left_context[request.left_context.len() - 1];
            interpolated.fill(left_value);
        } else if !request.right_context.is_empty() {
            let right_value = request.right_context[0];
            interpolated.fill(right_value);
        }

        // Simple confidence calculation for interpolation
        let confidence = if !request.left_context.is_empty() && !request.right_context.is_empty() {
            0.6
        } else {
            0.3
        };

        Ok((interpolated, confidence))
    }

    fn generate_cache_key(&self, request: &GapFillingRequest) -> String {
        let left_hash = utils::calculate_simple_hash(&request.left_context);
        let right_hash = utils::calculate_simple_hash(&request.right_context);
        format!(
            "gap_{}_{}_{}_{}_{}",
            request.position, request.size, left_hash, right_hash, request.confidence_threshold
        )
    }

    fn get_cached_fill(&self, cache_key: &str) -> Option<CachedGapFill> {
        let mut cache = self.filling_cache.write().unwrap();
        if let Some(cached) = cache.get_mut(cache_key) {
            cached.access_count += 1;
            return Some(cached.clone());
        }
        None
    }

    fn cache_fill(
        &self,
        cache_key: String,
        content: &[f32],
        method: GapFillingMethod,
        confidence: f32,
    ) {
        let mut cache = self.filling_cache.write().unwrap();

        // Simple cache size management
        if cache.len() > 1000 {
            let to_remove = cache.len() - 1000;
            let keys: Vec<String> = cache.keys().take(to_remove).cloned().collect();
            for key in keys {
                cache.remove(&key);
            }
        }

        cache.insert(
            cache_key,
            CachedGapFill {
                content: content.to_vec(),
                method,
                confidence,
                created_at: Utc::now(),
                access_count: 1,
            },
        );
    }

    fn update_statistics<F>(&self, update_fn: F)
    where
        F: FnOnce(&mut GapFillingStatistics),
    {
        let mut stats = self.statistics.write().unwrap();
        update_fn(&mut stats);
    }
}

impl AIGenerationService {
    fn new() -> Self {
        Self {
            model_config: AIModelConfig::default(),
            generation_cache: std::sync::Mutex::new(HashMap::new()),
            rate_limiter: std::sync::Mutex::new(RateLimiter::new(10.0)),
            metrics: AIGenerationMetrics::default(),
        }
    }

    async fn generate_content(
        &self,
        left_context: &[f32],
        right_context: &[f32],
        size: usize,
    ) -> ContextNestResult<(Vec<f32>, f32)> {
        // Check rate limit
        {
            let mut rl = self.rate_limiter.lock().unwrap();
            if !rl.can_make_request() {
                return Err(ContextNestError::Api(
                    "Rate limit exceeded for AI generation".to_string(),
                ));
            }
        }

        // Check cache first
        let cache_key = self.generate_cache_key(left_context, right_context, size);
        {
            let cache = self.generation_cache.lock().unwrap();
            if let Some(cached) = cache.get(&cache_key) {
                return Ok((cached.content.clone(), cached.confidence));
            }
        }

        // Simulate AI generation (placeholder)
        let start_time = Utc::now();
        let generated_content = self.simulate_ai_generation(left_context, right_context, size)?;
        let generation_time = Utc::now()
            .signed_duration_since(start_time)
            .to_std()
            .unwrap_or_default();

        let confidence = 0.8; // Placeholder confidence

        // Cache result
        let result = AIGenerationResult {
            content: generated_content.clone(),
            confidence,
            generation_time,
            tokens_used: size,
            model_used: self.model_config.model_name.clone(),
        };

        // Add to cache
        self.generation_cache
            .lock()
            .unwrap()
            .insert(cache_key, result);

        Ok((generated_content, confidence))
    }

    fn simulate_ai_generation(
        &self,
        left_context: &[f32],
        right_context: &[f32],
        size: usize,
    ) -> ContextNestResult<Vec<f32>> {
        let mut generated = vec![0.0; size];

        // Simple interpolation-based generation as placeholder
        if !left_context.is_empty() && !right_context.is_empty() {
            let left_avg = left_context.iter().sum::<f32>() / left_context.len() as f32;
            let right_avg = right_context.iter().sum::<f32>() / right_context.len() as f32;

            for (i, value) in generated.iter_mut().enumerate() {
                let progress = i as f32 / size as f32;
                *value = left_avg * (1.0 - progress) + right_avg * progress;
                // Add some randomness for "AI" feel
                *value += (rand::random::<f32>() - 0.5) * 0.1;
            }
        } else if !left_context.is_empty() {
            let avg = left_context.iter().sum::<f32>() / left_context.len() as f32;
            generated.fill(avg);
        } else if !right_context.is_empty() {
            let avg = right_context.iter().sum::<f32>() / right_context.len() as f32;
            generated.fill(avg);
        }

        Ok(generated)
    }

    fn generate_cache_key(
        &self,
        left_context: &[f32],
        right_context: &[f32],
        size: usize,
    ) -> String {
        let left_hash = utils::calculate_simple_hash(left_context);
        let right_hash = utils::calculate_simple_hash(right_context);
        format!("ai_gen_{}_{}_{}", left_hash, right_hash, size)
    }
}

impl RateLimiter {
    fn new(max_rps: f32) -> Self {
        Self {
            max_rps,
            request_history: VecDeque::new(),
            current_count: 0,
        }
    }

    fn can_make_request(&mut self) -> bool {
        let now = Utc::now();
        let one_second_ago = now - chrono::Duration::seconds(1);

        // Remove old requests
        while let Some(&front) = self.request_history.front() {
            if front < one_second_ago {
                self.request_history.pop_front();
                self.current_count = self.current_count.saturating_sub(1);
            } else {
                break;
            }
        }

        // Check if we can make a new request. Parens are required: without
        // them, the parser greedily interprets `f32 <` as the start of generic
        // type arguments (`f32::<...>`), not a comparison.
        if (self.current_count as f32) < self.max_rps {
            self.request_history.push_back(now);
            self.current_count += 1;
            true
        } else {
            false
        }
    }
}

impl Default for AIModelConfig {
    fn default() -> Self {
        Self {
            model_name: "gpt-3.5-turbo".to_string(),
            api_endpoint: "https://api.openai.com/v1/completions".to_string(),
            max_tokens: 1000,
            temperature: 0.7,
            top_p: 0.9,
            timeout_seconds: 30,
            retry_config: RetryConfig {
                max_attempts: 3,
                base_delay_ms: 1000,
                backoff_factor: 2.0,
                jitter_ms: 100,
            },
        }
    }
}

impl PatternBasedFiller {
    fn new() -> Self {
        Self {
            pattern_database: Arc::new(RwLock::new(PatternDatabase::new())),
            matchers: vec![
                Box::new(SequentialPatternMatcher::new()),
                Box::new(SemanticPatternMatcher::new()),
            ],
            metrics: PatternFillingMetrics::default(),
        }
    }

    async fn fill_from_patterns(
        &self,
        context: &[f32],
        position: usize,
        size: usize,
    ) -> ContextNestResult<(Vec<f32>, f32)> {
        let database = self.pattern_database.read().unwrap();
        let patterns = &database.common_patterns;

        let mut best_match: Option<(MemoryPattern, f32)> = None;

        // Try each matcher
        for matcher in &self.matchers {
            let matches = matcher.find_matches(context, patterns);

            for pattern_match in matches {
                if let Some(pattern) = patterns.iter().find(|p| p.id == pattern_match.pattern_id) {
                    let confidence = matcher.calculate_confidence(context, pattern);

                    if best_match.is_none() || confidence > best_match.as_ref().unwrap().1 {
                        best_match = Some((pattern.clone(), confidence));
                    }
                }
            }
        }

        if let Some((pattern, confidence)) = best_match {
            // Extract appropriate portion from pattern
            let start_pos = (position % pattern.content.len()).min(pattern.content.len() - size);
            let end_pos = (start_pos + size).min(pattern.content.len());
            let content = pattern.content[start_pos..end_pos].to_vec();

            // Update pattern usage
            drop(database);
            self.update_pattern_usage(&pattern.id);

            Ok((content, confidence))
        } else {
            Err(ContextNestError::NotFound(
                "No suitable pattern found".to_string(),
            ))
        }
    }

    fn update_pattern_usage(&self, pattern_id: &str) {
        let mut database = self.pattern_database.write().unwrap();
        if let Some(pattern) = database
            .common_patterns
            .iter_mut()
            .find(|p| p.id == pattern_id)
        {
            pattern.usage_count += 1;
            pattern.last_used = Utc::now();
        }
    }
}

impl PatternDatabase {
    fn new() -> Self {
        Self {
            common_patterns: Vec::new(),
            pattern_frequency: HashMap::new(),
            context_associations: HashMap::new(),
        }
    }
}

// Pattern matcher implementations

#[derive(Debug)]
struct SequentialPatternMatcher {
    min_overlap: usize,
}

impl SequentialPatternMatcher {
    fn new() -> Self {
        Self { min_overlap: 5 }
    }
}

impl PatternMatcher for SequentialPatternMatcher {
    fn find_matches(&self, context: &[f32], patterns: &[MemoryPattern]) -> Vec<PatternMatch> {
        let mut matches = Vec::new();

        for pattern in patterns {
            let confidence = self.calculate_confidence(context, pattern);
            if confidence > 0.5 {
                matches.push(PatternMatch {
                    pattern_id: pattern.id.clone(),
                    confidence,
                    position: 0, // Simplified
                    score: confidence,
                });
            }
        }

        matches.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        matches
    }

    fn calculate_confidence(&self, context: &[f32], pattern: &MemoryPattern) -> f32 {
        if context.len() < self.min_overlap || pattern.content.len() < self.min_overlap {
            return 0.0;
        }

        let overlap_size = self
            .min_overlap
            .min(context.len())
            .min(pattern.content.len());
        let context_overlap = &context[..overlap_size];
        let pattern_overlap = &pattern.content[..overlap_size];

        utils::cosine_similarity(context_overlap, pattern_overlap)
    }

    fn name(&self) -> &str {
        "sequential"
    }
}

#[derive(Debug)]
struct SemanticPatternMatcher {
    semantic_threshold: f32,
}

impl SemanticPatternMatcher {
    fn new() -> Self {
        Self {
            semantic_threshold: 0.6,
        }
    }
}

impl PatternMatcher for SemanticPatternMatcher {
    fn find_matches(&self, context: &[f32], patterns: &[MemoryPattern]) -> Vec<PatternMatch> {
        let mut matches = Vec::new();

        for pattern in patterns {
            let confidence = self.calculate_confidence(context, pattern);
            if confidence > self.semantic_threshold {
                matches.push(PatternMatch {
                    pattern_id: pattern.id.clone(),
                    confidence,
                    position: 0,
                    score: confidence,
                });
            }
        }

        matches.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
        matches
    }

    fn calculate_confidence(&self, context: &[f32], pattern: &MemoryPattern) -> f32 {
        if context.is_empty() || pattern.content.is_empty() {
            return 0.0;
        }

        // Use semantic similarity
        utils::cosine_similarity(context, &pattern.content)
    }

    fn name(&self) -> &str {
        "semantic"
    }
}

impl SimilarityEngine {
    fn new() -> Self {
        Self {
            similarity_database: Arc::new(RwLock::new(SimilarityDatabase::new())),
            algorithms: vec![
                Box::new(CosineSimilarity::new()),
                Box::new(EuclideanSimilarity::new()),
            ],
            similarity_cache: HashMap::new(),
            metrics: SimilarityEngineMetrics::default(),
        }
    }

    async fn borrow_from_similar(
        &self,
        context: &[f32],
        size: usize,
        available_fragments: &[MemoryFragment],
    ) -> ContextNestResult<(Vec<f32>, f32)> {
        let mut best_content: Option<Vec<f32>> = None;
        let mut best_similarity = 0.0;

        for fragment in available_fragments {
            if fragment.content.len() >= size {
                for algorithm in &self.algorithms {
                    let similarity = algorithm.calculate_similarity(context, &fragment.content);

                    if similarity > best_similarity {
                        best_similarity = similarity;
                        best_content = Some(fragment.content[..size].to_vec());
                    }
                }
            }
        }

        if let Some(content) = best_content {
            Ok((content, best_similarity))
        } else {
            Err(ContextNestError::NotFound(
                "No similar content found".to_string(),
            ))
        }
    }
}

impl SimilarityDatabase {
    fn new() -> Self {
        Self {
            content_vectors: HashMap::new(),
            similarity_matrix: HashMap::new(),
            last_updated: Utc::now(),
        }
    }
}

// Similarity algorithm implementations

#[derive(Debug)]
struct CosineSimilarity;

impl CosineSimilarity {
    fn new() -> Self {
        Self
    }
}

impl SimilarityAlgorithm for CosineSimilarity {
    fn calculate_similarity(&self, vec1: &[f32], vec2: &[f32]) -> f32 {
        utils::cosine_similarity(vec1, vec2)
    }

    fn name(&self) -> &str {
        "cosine"
    }
}

#[derive(Debug)]
struct EuclideanSimilarity;

impl EuclideanSimilarity {
    fn new() -> Self {
        Self
    }
}

impl SimilarityAlgorithm for EuclideanSimilarity {
    fn calculate_similarity(&self, vec1: &[f32], vec2: &[f32]) -> f32 {
        let distance = utils::euclidean_distance(vec1, vec2);
        (-distance / 10.0).exp() // Convert distance to similarity
    }

    fn name(&self) -> &str {
        "euclidean"
    }
}

// calculate_simple_hash moved to attractors/mod.rs::utils for reuse.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::attractors::MemoryAttractorConfig;

    #[tokio::test]
    async fn test_gap_filling_engine() {
        let config = MemoryAttractorConfig::default();
        let engine = GapFillingEngine::new(config);
        engine.initialize().await.unwrap();

        let request = GapFillingRequest {
            id: "test_req".to_string(),
            position: 64,
            size: 32,
            left_context: vec![0.1; 32],
            right_context: vec![0.3; 32],
            available_fragments: vec![],
            methods: vec![
                GapFillingMethod::Interpolation,
                GapFillingMethod::PatternReconstruction,
            ],
            confidence_threshold: 0.5,
            priority: GapFillingPriority::Medium,
        };

        let result = engine.fill_gap(request).await.unwrap();
        let _ = result;
    }

    #[tokio::test]
    async fn test_ai_generation_service() {
        let service = AIGenerationService::new();

        let left_context = vec![0.1; 16];
        let right_context = vec![0.3; 16];
        let size = 32;

        let result = service
            .generate_content(&left_context, &right_context, size)
            .await
            .unwrap();
        assert_eq!(result.0.len(), size);
        assert!(result.1 > 0.0);
    }

    #[test]
    fn test_rate_limiter() {
        let mut limiter = RateLimiter::new(2.0); // 2 requests per second

        assert!(limiter.can_make_request());
        assert!(limiter.can_make_request());
        assert!(!limiter.can_make_request()); // Should be rate limited
    }

    #[test]
    fn test_cosine_similarity() {
        let algorithm = CosineSimilarity::new();

        let vec1 = vec![1.0, 0.0, 0.0];
        let vec2 = vec![0.0, 1.0, 0.0];
        let similarity = algorithm.calculate_similarity(&vec1, &vec2);
        assert!((similarity - 0.0).abs() < f32::EPSILON);

        let vec3 = vec![1.0, 0.0, 0.0];
        let similarity = algorithm.calculate_similarity(&vec1, &vec3);
        assert!((similarity - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_sequential_pattern_matcher() {
        let matcher = SequentialPatternMatcher::new();

        // `SequentialPatternMatcher::new()` sets min_overlap = 5, so any
        // shorter input returns 0.0 confidence by design (insufficient
        // signal). The pre-Phase-A version of this test used 3-element
        // vectors that silently returned 0.0 — masked by other test
        // failures earlier in the module. Use vectors at least min_overlap
        // long to exercise the actual similarity path.
        let context = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
        let pattern = MemoryPattern {
            id: "test".to_string(),
            content: vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7],
            pattern_type: PatternType::Sequential,
            context_tags: vec![],
            usage_count: 0,
            last_used: Utc::now(),
        };

        let confidence = matcher.calculate_confidence(&context, &pattern);
        assert!(
            confidence > 0.5,
            "expected confidence > 0.5, got {}",
            confidence
        );
    }

    #[test]
    fn test_util_hash() {
        let vec1 = vec![0.1, 0.2, 0.3];
        let vec2 = vec![0.1, 0.2, 0.3];
        let hash1 = utils::calculate_simple_hash(&vec1);
        let hash2 = utils::calculate_simple_hash(&vec2);
        assert_eq!(hash1, hash2);

        let vec3 = vec![0.1, 0.2, 0.4];
        let hash3 = utils::calculate_simple_hash(&vec3);
        assert_ne!(hash1, hash3);
    }
}

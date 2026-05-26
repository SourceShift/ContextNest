use crate::context::field::{NeuralField, SemanticPattern};
use crate::context::memory::{AttractorField, MemoryAttractor};
use crate::error::ContextNestResult;
use crate::{
    config::{Config, EmbeddingModelConfig, EmbeddingServicesConfig},
    error::{ContextNestError, Result},
};
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Embedding service for generating vector embeddings
#[derive(Clone)]
pub struct EmbeddingService {
    config: EmbeddingServicesConfig,
    client: Client,
    cache: std::sync::Arc<tokio::sync::RwLock<HashMap<String, Vec<f32>>>>,
}

#[derive(Debug, Serialize)]
struct OpenAIEmbeddingRequest {
    input: String,
    model: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingResponse {
    data: Vec<OpenAIEmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingData {
    embedding: Vec<f32>,
}

impl EmbeddingService {
    pub fn new(config: EmbeddingServicesConfig) -> ContextNestResult<Self> {
        let client = Client::new();
        let cache = std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new()));

        Ok(Self {
            config,
            client,
            cache,
        })
    }

    /// Get the default model configuration
    fn get_default_model(&self) -> ContextNestResult<&EmbeddingModelConfig> {
        self.config
            .models
            .get(&self.config.default_model)
            .ok_or_else(|| ContextNestError::Api("Default embedding model not found".to_string()))
    }

    /// Resolve the embedding API key from the layered sources documented on
    /// [`Self::generate_openai_embedding`]. Returns `None` only if every
    /// source is empty / unset.
    fn resolve_api_key(&self) -> Option<String> {
        // Layer 1: literal in config.toml (typically used in tests / CI).
        if let Some(key) = self.config.api_key.as_ref() {
            if !key.is_empty() {
                return Some(key.clone());
            }
        }
        // Layer 2: env var name from config (lets the operator point at any
        // var name without code change — useful for $VENDOR_KEY conventions).
        if let Some(env_name) = self.config.api_key_env.as_deref() {
            if let Ok(val) = std::env::var(env_name) {
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
        // Layer 3: well-known fallbacks. Order is intentional —
        // DeepInfra-first reflects our current production default (Qwen3
        // embeddings via DeepInfra). OpenAI second so existing callers
        // with only $OPENAI_API_KEY set keep working.
        for env_name in ["DEEPINFRA_API_KEY", "OPENAI_API_KEY"] {
            if let Ok(val) = std::env::var(env_name) {
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
        None
    }

    /// Generate embedding for text.
    ///
    /// Inputs longer than the configured `max_input_length` (interpreted as
    /// a conservative **character** budget) are truncated before being
    /// sent to the provider. This guards the OpenAI-compatible providers
    /// (Qwen3 on DeepInfra, OpenAI text-embedding-3, etc.) from rejecting
    /// the call with a "context_length exceeded" error — a single token
    /// over the model's limit is enough to fail the request and lose the
    /// fragment's embedding. Char-based clamping is intentionally
    /// conservative: for typical English / code text, 1 token ≈ 3-5
    /// characters, so capping by char count keeps the input safely under
    /// the token limit without requiring a tokenizer dependency. A
    /// follow-up could swap in a proper BPE tokenizer (tiktoken) if a
    /// tighter fit becomes worth the dep.
    pub async fn generate_embedding(&self, text: &str) -> ContextNestResult<Vec<f32>> {
        // Resolve the default model up-front so we can read its
        // max_input_length before the cache lookup — different models
        // produce different vectors for the same text, but the cache key
        // is content-only, so we must clamp the text first to keep the
        // key stable across pre-vs-post-clamp calls of the same payload.
        let default_model = self
            .config
            .models
            .get(&self.config.default_model)
            .ok_or_else(|| {
                ContextNestError::Api("Default embedding model not found".to_string())
            })?;

        let max_chars = default_model.settings.max_input_length;
        let truncated: Option<String> = first_n_chars(text, max_chars).map(|s| {
            tracing::warn!(
                original_chars = text.chars().count(),
                truncated_chars = s.chars().count(),
                max_chars,
                "embedding input exceeded max_input_length — truncating before \
                 provider call (raise [services.embedding.models.<name>.settings] \
                 max_input_length if the model's context window allows more)"
            );
            s.to_string()
        });
        let text = truncated.as_deref().unwrap_or(text);

        // Check cache first (post-clamp so the key matches what we'll
        // actually send to the provider).
        let cache_key = self.create_cache_key(text);

        {
            let cache_read = self.cache.read().await;
            if let Some(cached) = cache_read.get(&cache_key) {
                return Ok(cached.clone());
            }
        }

        let embedding = match default_model.model_type {
            crate::config::EmbeddingModelType::OpenAI => {
                self.generate_openai_embedding(text).await?
            }
            crate::config::EmbeddingModelType::Local => self.generate_local_embedding(text).await?,
            _ => {
                return Err(ContextNestError::Api(format!(
                    "Unsupported embedding model type: {:?}",
                    default_model.model_type
                )))
            }
        };

        // Cache the result
        {
            let mut cache_write = self.cache.write().await;
            cache_write.insert(cache_key, embedding.clone());
        }

        Ok(embedding)
    }

    /// Generate embeddings for multiple texts
    pub async fn generate_embeddings(&self, texts: Vec<&str>) -> ContextNestResult<Vec<Vec<f32>>> {
        let mut embeddings = Vec::new();

        for text in texts {
            let embedding = self.generate_embedding(text).await?;
            embeddings.push(embedding);
        }

        Ok(embeddings)
    }

    /// Generate embedding using an OpenAI-shaped embeddings endpoint.
    ///
    /// Despite the historical name, this handler also serves drop-in
    /// OpenAI-compatible providers (DeepInfra, Together, Anyscale, …) —
    /// the request/response shape is identical and only the base URL +
    /// model string change. The provider is chosen by setting
    /// `EmbeddingServicesConfig::base_url`; when unset, falls back to
    /// the real OpenAI endpoint for backward compatibility.
    ///
    /// API key resolution order (first match wins):
    ///   1. `EmbeddingServicesConfig::api_key` (literal in config)
    ///   2. Env var named by `EmbeddingServicesConfig::api_key_env`
    ///   3. `DEEPINFRA_API_KEY` (convention for DeepInfra deployments)
    ///   4. `OPENAI_API_KEY` (convention for OpenAI deployments)
    /// This chain lets you keep secrets out of `config.toml` (preferred)
    /// while still supporting the literal-config path for tests / CI.
    async fn generate_openai_embedding(&self, text: &str) -> ContextNestResult<Vec<f32>> {
        let api_key = self.resolve_api_key().ok_or_else(|| {
            ContextNestError::Api(
                "no embedding API key found in config.api_key, $DEEPINFRA_API_KEY, or \
                 $OPENAI_API_KEY"
                    .to_string(),
            )
        })?;

        let request = OpenAIEmbeddingRequest {
            input: text.to_string(),
            model: self.config.model.clone(),
        };

        let url = self
            .config
            .base_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("https://api.openai.com/v1/embeddings");

        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(ContextNestError::Api(format!(
                "OpenAI API error: {}",
                error_text
            )));
        }

        let embedding_response: OpenAIEmbeddingResponse = response.json().await?;

        embedding_response
            .data
            .into_iter()
            .next()
            .map(|data| data.embedding)
            .ok_or_else(|| ContextNestError::Api("No embedding data in response".to_string()))
    }

    /// Generate embedding using local semantic model
    async fn generate_local_embedding(&self, text: &str) -> ContextNestResult<Vec<f32>> {
        // Advanced local embedding using semantic analysis and TF-IDF style features
        tracing::debug!("Generating local semantic embedding for text: {}", text);

        let preprocessed_text = self.preprocess_text(text);
        let mut embedding = vec![0.0; self.config.dimensions];

        // Multi-component embedding approach
        let word_features = self.extract_word_features(&preprocessed_text);
        let semantic_features = self.extract_semantic_features(&preprocessed_text);
        let structural_features = self.extract_structural_features(text);
        let positional_features = self.extract_positional_features(&preprocessed_text);

        // Distribute features across embedding dimensions
        let word_dim = self.config.dimensions / 4;
        let semantic_dim = self.config.dimensions / 4;
        let structural_dim = self.config.dimensions / 4;
        let positional_dim = self.config.dimensions - (word_dim + semantic_dim + structural_dim);

        // Fill word features (0 to word_dim)
        for (i, &feature) in word_features.iter().enumerate() {
            if i < word_dim {
                embedding[i] = feature;
            }
        }

        // Fill semantic features (word_dim to word_dim + semantic_dim)
        for (i, &feature) in semantic_features.iter().enumerate() {
            if i < semantic_dim {
                embedding[word_dim + i] = feature;
            }
        }

        // Fill structural features
        for (i, &feature) in structural_features.iter().enumerate() {
            if i < structural_dim {
                embedding[word_dim + semantic_dim + i] = feature;
            }
        }

        // Fill positional features
        for (i, &feature) in positional_features.iter().enumerate() {
            if i < positional_dim {
                embedding[word_dim + semantic_dim + structural_dim + i] = feature;
            }
        }

        // Normalize the embedding vector
        self.normalize_embedding(&mut embedding);

        // Add some noise to prevent identical embeddings for similar texts
        self.add_controlled_noise(&mut embedding, text);

        Ok(embedding)
    }

    /// Preprocess text for embedding generation
    fn preprocess_text(&self, text: &str) -> Vec<String> {
        text.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .split_whitespace()
            .filter(|word| word.len() > 2) // Filter short words
            .map(|word| word.to_string())
            .collect()
    }

    /// Extract word-level features using TF-IDF style approach
    fn extract_word_features(&self, words: &[String]) -> Vec<f32> {
        if words.is_empty() {
            return vec![0.0; self.config.dimensions / 4];
        }

        let mut features = Vec::new();
        let word_count = words.len() as f32;

        // Word frequency features
        let mut word_freq = std::collections::HashMap::new();
        for word in words {
            *word_freq.entry(word.clone()).or_insert(0) += 1;
        }

        // Convert to TF features
        for (word, freq) in &word_freq {
            let tf = *freq as f32 / word_count;
            // Simple IDF approximation based on word length and common patterns
            let idf = self.estimate_idf(word);
            features.push(tf * idf);
        }

        // Pad or truncate to desired dimension
        features.resize(self.config.dimensions / 4, 0.0);
        features
    }

    /// Extract semantic features based on word patterns and meanings
    fn extract_semantic_features(&self, words: &[String]) -> Vec<f32> {
        let mut features = Vec::new();
        let target_dim = self.config.dimensions / 4;

        // Semantic categories and their weights
        let tech_words = [
            "function",
            "class",
            "method",
            "variable",
            "data",
            "system",
            "process",
            "algorithm",
        ];
        let action_words = [
            "create", "update", "delete", "process", "analyze", "generate", "execute",
        ];
        let concept_words = [
            "pattern",
            "structure",
            "context",
            "field",
            "memory",
            "neural",
            "semantic",
        ];

        // Calculate semantic scores
        let tech_score = self.calculate_category_score(words, &tech_words);
        let action_score = self.calculate_category_score(words, &action_words);
        let concept_score = self.calculate_category_score(words, &concept_words);

        features.push(tech_score);
        features.push(action_score);
        features.push(concept_score);

        // Add word relationship features
        let avg_word_len =
            words.iter().map(|w| w.len()).sum::<usize>() as f32 / words.len().max(1) as f32;
        let unique_ratio = self.word_freq_uniqueness(words);

        features.push(avg_word_len / 10.0); // Normalize
        features.push(unique_ratio);

        // Add linguistic pattern features
        features.extend(self.extract_linguistic_patterns(words));

        // Pad or truncate to desired dimension
        features.resize(target_dim, 0.0);
        features
    }

    /// Extract structural features from original text
    fn extract_structural_features(&self, text: &str) -> Vec<f32> {
        let mut features = Vec::new();

        // Text length features
        features.push((text.len() as f32).ln() / 10.0); // Log-normalized length

        // Character type ratios
        let total_chars = text.len().max(1) as f32;
        let alpha_ratio = text.chars().filter(|c| c.is_alphabetic()).count() as f32 / total_chars;
        let numeric_ratio = text.chars().filter(|c| c.is_numeric()).count() as f32 / total_chars;
        let space_ratio = text.chars().filter(|c| c.is_whitespace()).count() as f32 / total_chars;
        let punct_ratio =
            text.chars().filter(|c| c.is_ascii_punctuation()).count() as f32 / total_chars;

        features.extend([alpha_ratio, numeric_ratio, space_ratio, punct_ratio]);

        // Sentence and word count features
        let sentence_count = text
            .split(&['.', '!', '?'])
            .filter(|s| !s.trim().is_empty())
            .count() as f32;
        let word_count = text.split_whitespace().count() as f32;
        let avg_sentence_len = if sentence_count > 0.0 {
            word_count / sentence_count
        } else {
            0.0
        };

        features.push(sentence_count.ln() / 5.0); // Log-normalized
        features.push(avg_sentence_len / 20.0); // Normalized

        // Structural complexity
        let parentheses = text.matches(&['(', ')', '[', ']', '{', '}']).count() as f32;
        let quotes = text.matches(&['"', '\'', '`']).count() as f32;
        features.push(parentheses / total_chars);
        features.push(quotes / total_chars);

        // Pad or truncate to desired dimension
        features.resize(self.config.dimensions / 4, 0.0);
        features
    }

    /// Extract positional and sequence features
    fn extract_positional_features(&self, words: &[String]) -> Vec<f32> {
        let mut features = Vec::new();

        if words.is_empty() {
            features.resize(self.config.dimensions / 4, 0.0);
            return features;
        }

        // Position-based features
        for (i, word) in words.iter().enumerate().take(20) {
            // Limit to first 20 words
            let position_weight = 1.0 - (i as f32 / words.len() as f32);
            let word_importance = self.estimate_word_importance(word);
            features.push(position_weight * word_importance);
        }

        // N-gram features (bigrams)
        for window in words.windows(2) {
            if let [w1, w2] = window {
                let bigram_score = self.calculate_bigram_score(w1, w2);
                features.push(bigram_score);
                if features.len() >= self.config.dimensions / 8 {
                    break;
                }
            }
        }

        // Pad or truncate to desired dimension
        features.resize(self.config.dimensions / 4, 0.0);
        features
    }

    /// Normalize embedding vector to unit length
    fn normalize_embedding(&self, embedding: &mut [f32]) {
        let norm = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-8 {
            // Avoid division by zero
            for val in embedding.iter_mut() {
                *val /= norm;
            }
        }
    }

    /// Add controlled noise to prevent identical embeddings
    fn add_controlled_noise(&self, embedding: &mut [f32], text: &str) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let seed = hasher.finish();

        // Add small amount of deterministic noise based on text content
        for (i, val) in embedding.iter_mut().enumerate() {
            let noise_factor = ((seed.wrapping_add(i as u64)) as f32 * 0.001).sin() * 0.01;
            *val += noise_factor;
        }

        // Re-normalize after adding noise
        self.normalize_embedding(embedding);
    }

    /// Estimate IDF (Inverse Document Frequency) for a word
    fn estimate_idf(&self, word: &str) -> f32 {
        // Simple IDF estimation based on word characteristics
        let base_idf = match word.len() {
            1..=3 => 0.5,  // Very common short words
            4..=6 => 1.0,  // Common words
            7..=10 => 1.5, // Less common words
            _ => 2.0,      // Rare long words
        };

        // Adjust based on word patterns
        let pattern_bonus = if word.contains("ing") || word.contains("ed") || word.contains("ly") {
            0.2 // Common suffixes
        } else if word.chars().all(|c| c.is_uppercase()) {
            0.5 // Acronyms
        } else if word.contains("_") || word.contains("-") {
            0.3 // Compound words
        } else {
            0.0
        };

        base_idf + pattern_bonus
    }

    /// Calculate semantic category score
    fn calculate_category_score(&self, words: &[String], category_words: &[&str]) -> f32 {
        if words.is_empty() {
            return 0.0;
        }

        let matches = words
            .iter()
            .filter(|word| category_words.iter().any(|&cat| word.contains(cat)))
            .count() as f32;

        matches / words.len() as f32
    }

    /// Extract linguistic patterns
    fn extract_linguistic_patterns(&self, words: &[String]) -> Vec<f32> {
        let mut patterns = Vec::new();

        // Vowel/consonant ratios
        let vowel_ratio = words
            .iter()
            .flat_map(|w| w.chars())
            .filter(|&c| "aeiouAEIOU".contains(c))
            .count() as f32
            / words.iter().map(|w| w.len()).sum::<usize>().max(1) as f32;

        patterns.push(vowel_ratio);

        // Average syllable estimate (simple heuristic)
        let avg_syllables = words
            .iter()
            .map(|w| self.estimate_syllables(w))
            .sum::<f32>()
            / words.len().max(1) as f32;

        patterns.push(avg_syllables / 5.0); // Normalize

        // Word complexity (ratio of long words)
        let long_word_ratio =
            words.iter().filter(|w| w.len() > 6).count() as f32 / words.len().max(1) as f32;

        patterns.push(long_word_ratio);

        patterns
    }

    /// Estimate word importance based on characteristics
    fn estimate_word_importance(&self, word: &str) -> f32 {
        let mut importance: f32 = 0.5; // Base importance

        // Length-based importance
        importance += match word.len() {
            1..=3 => -0.2, // Short words less important
            4..=8 => 0.1,  // Medium words normal
            _ => 0.3,      // Long words more important
        };

        // Pattern-based importance
        if word.chars().any(|c| c.is_uppercase()) {
            importance += 0.2; // Capitalized words
        }

        if word.contains("_") || word.contains("-") {
            importance += 0.1; // Compound words
        }

        importance.max(0.0).min(1.0) // Clamp to [0, 1]
    }

    /// Calculate bigram co-occurrence score
    fn calculate_bigram_score(&self, w1: &str, w2: &str) -> f32 {
        // Simple bigram scoring based on word characteristics
        let len_similarity = 1.0 - ((w1.len() as f32 - w2.len() as f32).abs() / 10.0);
        let first_char_match = if w1.chars().next() == w2.chars().next() {
            0.3
        } else {
            0.0
        };
        let semantic_relation = self.estimate_semantic_relation(w1, w2);

        (len_similarity + first_char_match + semantic_relation) / 3.0
    }

    /// Estimate semantic relationship between two words
    fn estimate_semantic_relation(&self, w1: &str, w2: &str) -> f32 {
        // Simple semantic relationship estimation
        let common_prefixes = ["pre", "post", "sub", "super", "anti", "pro"];
        let common_suffixes = ["ing", "ed", "ly", "tion", "ness", "ment"];

        let mut relation_score: f32 = 0.0;

        // Check for common prefixes
        for prefix in &common_prefixes {
            if w1.starts_with(prefix) && w2.starts_with(prefix) {
                relation_score += 0.3;
                break;
            }
        }

        // Check for common suffixes
        for suffix in &common_suffixes {
            if w1.ends_with(suffix) && w2.ends_with(suffix) {
                relation_score += 0.2;
                break;
            }
        }

        // Check for root similarity (first 3 chars match). Use char-aware
        // slicing — `str::len()` is byte-length, so `&w[..3]` on multibyte
        // input (math symbols, CJK, emoji) panics at the char boundary.
        // Example panic seen in prod: "η²" is 4 bytes but only 2 chars;
        // `len() > 3` was true but `&w[..3]` landed inside the `²` byte
        // sequence.
        if let (Some(root1), Some(root2)) = (first_n_chars(w1, 3), first_n_chars(w2, 3)) {
            if root1 == root2 {
                relation_score += 0.4;
            }
        }

        relation_score.min(1.0)
    }

    /// Cryptographic hash function for embeddings using SHA-256
    fn simple_hash(&self, text: &str) -> Vec<u8> {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let hash = hasher.finalize();

        // For larger dimensions, we can hash multiple times or use different seeds
        let mut result = Vec::new();
        let mut i: u32 = 0;

        while result.len() < self.config.dimensions {
            let mut hasher = Sha256::new();
            hasher.update(text.as_bytes());
            hasher.update(i.to_le_bytes());
            let hash_iteration = hasher.finalize();
            result.extend_from_slice(&hash_iteration);
            i += 1;
        }

        result.truncate(self.config.dimensions);
        result
    }

    /// Calculate word frequency uniqueness ratio
    fn word_freq_uniqueness(&self, words: &[String]) -> f32 {
        if words.is_empty() {
            return 0.0;
        }

        let unique_words = words.iter().collect::<std::collections::HashSet<_>>().len();
        unique_words as f32 / words.len() as f32
    }

    /// Estimate syllable count for a word (simple heuristic)
    fn estimate_syllables(&self, word: &str) -> f32 {
        if word.is_empty() {
            return 0.0;
        }

        let vowels = "aeiouAEIOU";
        let mut syllable_count = 0;
        let mut prev_was_vowel = false;

        for ch in word.chars() {
            let is_vowel = vowels.contains(ch);
            if is_vowel && !prev_was_vowel {
                syllable_count += 1;
            }
            prev_was_vowel = is_vowel;
        }

        // Handle silent 'e' at the end
        if word.ends_with('e') && syllable_count > 1 {
            syllable_count -= 1;
        }

        // Every word has at least one syllable
        if syllable_count == 0 {
            syllable_count = 1;
        }

        syllable_count as f32
    }

    /// Create cache key for text
    fn create_cache_key(&self, text: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        format!("{}_{}", self.config.model, hasher.finish())
    }

    /// Calculate similarity between two embeddings
    pub fn calculate_similarity(&self, embedding1: &[f32], embedding2: &[f32]) -> f32 {
        if embedding1.len() != embedding2.len() {
            return 0.0;
        }

        let dot_product: f32 = embedding1
            .iter()
            .zip(embedding2.iter())
            .map(|(a, b)| a * b)
            .sum();

        let norm1: f32 = embedding1.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm2: f32 = embedding2.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm1 == 0.0 || norm2 == 0.0 {
            0.0
        } else {
            dot_product / (norm1 * norm2)
        }
    }

    /// Find most similar embeddings from a collection
    pub fn find_most_similar(
        &self,
        target: &[f32],
        candidates: &[(String, Vec<f32>)],
        limit: usize,
    ) -> Vec<(String, f32)> {
        let mut similarities: Vec<(String, f32)> = candidates
            .iter()
            .map(|(id, embedding)| {
                let similarity = self.calculate_similarity(target, embedding);
                (id.clone(), similarity)
            })
            .collect();

        similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        similarities.truncate(limit);

        similarities
    }

    /// Clear embedding cache
    pub async fn clear_cache(&self) {
        let mut cache_write = self.cache.write().await;
        cache_write.clear();
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> CacheStats {
        let cache_read = self.cache.read().await;
        CacheStats {
            size: cache_read.len(),
            memory_usage: cache_read.len() * self.config.dimensions * std::mem::size_of::<f32>(),
        }
    }

    /// Health check for embedding service.
    ///
    /// Strategy: ask "can we actually serve a request right now?" rather
    /// than string-match the `provider` config tag. The legacy version
    /// rejected any provider literal other than `"openai"` / `"local"`,
    /// which mis-reported `provider = "deepinfra"` (or anything served via
    /// the OpenAI-compat HTTP path) as unhealthy even when fully working.
    pub async fn health_check(&self) -> ContextNestResult<bool> {
        // The local TF-IDF path needs no key — always serviceable.
        if let Ok(model) = self.get_default_model() {
            if matches!(model.model_type, crate::config::EmbeddingModelType::Local) {
                return Ok(true);
            }
        }

        // Any remote provider (OpenAI, DeepInfra, Together, Anyscale, ...)
        // routes through the OpenAI-shaped HTTP handler, which resolves
        // the key from the same layered chain `generate_openai_embedding`
        // uses. If that chain yields a key, the service is ready.
        Ok(self.resolve_api_key().is_some())
    }

    /// Batch generate embeddings with rate limiting
    pub async fn generate_embeddings_batch(
        &self,
        texts: Vec<String>,
        batch_size: usize,
    ) -> ContextNestResult<Vec<Vec<f32>>> {
        let mut all_embeddings = Vec::new();

        for chunk in texts.chunks(batch_size) {
            let chunk_embeddings = self
                .generate_embeddings(chunk.iter().map(|s| s.as_str()).collect())
                .await?;

            all_embeddings.extend(chunk_embeddings);

            // Rate limiting: small delay between batches
            if chunk.len() == batch_size {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        Ok(all_embeddings)
    }

    /// Generate semantic field embeddings with Context Engineering enhancements
    pub async fn generate_semantic_field_embedding(
        &self,
        content: &str,
        context_field: Option<&NeuralField>,
    ) -> ContextNestResult<SemanticFieldEmbedding> {
        // Generate base embedding
        let base_embedding = self.generate_embedding(content).await?;

        // Apply field-aware enhancements if context field provided
        let enhanced_embedding = if let Some(field) = context_field {
            self.apply_field_enhancement(&base_embedding, field, content)
                .await?
        } else {
            base_embedding.clone()
        };

        // Calculate semantic richness and field resonance
        let semantic_richness = self.calculate_semantic_richness(content);
        let field_resonance = if let Some(field) = context_field {
            self.calculate_field_resonance(&enhanced_embedding, field)
        } else {
            0.5 // Neutral resonance without field context
        };

        Ok(SemanticFieldEmbedding {
            content: content.to_string(),
            base_embedding,
            enhanced_embedding,
            semantic_richness,
            field_resonance,
            context_markers: self.extract_context_markers(content),
            generation_timestamp: Utc::now(),
            field_aligned: context_field.is_some(),
        })
    }

    /// Apply field-aware enhancement to embeddings using neural field context
    async fn apply_field_enhancement(
        &self,
        base_embedding: &[f32],
        field: &NeuralField,
        content: &str,
    ) -> ContextNestResult<Vec<f32>> {
        let mut enhanced = base_embedding.to_vec();

        // Find resonant patterns in the field
        let mut resonance_weights = Vec::new();
        for pattern in &field.patterns {
            let similarity = self.calculate_similarity(base_embedding, &pattern.embedding);
            let weight = similarity * pattern.strength * pattern.resonance;
            resonance_weights.push((weight, &pattern.embedding));
        }

        // Sort by resonance strength
        resonance_weights
            .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Apply top resonant patterns as enhancements
        let enhancement_factor = 0.1; // Conservative enhancement
        for (weight, pattern_embedding) in resonance_weights.iter().take(3) {
            if *weight > 0.3 {
                // Only apply significant resonances
                for (i, &pattern_val) in pattern_embedding.iter().enumerate() {
                    if i < enhanced.len() {
                        enhanced[i] += pattern_val * weight * enhancement_factor;
                    }
                }
            }
        }

        // Renormalize the enhanced embedding
        let norm = enhanced.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for val in &mut enhanced {
                *val /= norm;
            }
        }

        Ok(enhanced)
    }

    /// Calculate semantic richness score for content
    fn calculate_semantic_richness(&self, content: &str) -> f32 {
        let words: Vec<&str> = content.split_whitespace().collect();
        let unique_words: std::collections::HashSet<&str> = words.iter().cloned().collect();

        if words.is_empty() {
            return 0.0;
        }

        // Base richness on vocabulary diversity
        let vocabulary_diversity = unique_words.len() as f32 / words.len() as f32;

        // Bonus for longer content (up to a point)
        let length_factor = (words.len() as f32 / 100.0).min(1.0);

        // Bonus for complex terms (simplified heuristic)
        let complex_terms =
            words.iter().filter(|word| word.len() > 6).count() as f32 / words.len() as f32;

        (vocabulary_diversity + length_factor + complex_terms) / 3.0
    }

    /// Calculate how well an embedding resonates with the neural field
    fn calculate_field_resonance(&self, embedding: &[f32], field: &NeuralField) -> f32 {
        if field.patterns.is_empty() {
            return 0.5;
        }

        let mut total_resonance = 0.0;
        let mut weight_sum = 0.0;

        for pattern in &field.patterns {
            let similarity = self.calculate_similarity(embedding, &pattern.embedding);
            let weight = pattern.strength * pattern.resonance;
            total_resonance += similarity * weight;
            weight_sum += weight;
        }

        if weight_sum > 0.0 {
            total_resonance / weight_sum
        } else {
            0.5
        }
    }

    /// Extract context markers from content
    fn extract_context_markers(&self, content: &str) -> Vec<ContextMarker> {
        let mut markers = Vec::new();
        let words: Vec<&str> = content.split_whitespace().collect();

        // Extract entity markers (simplified)
        for (i, word) in words.iter().enumerate() {
            if word.chars().next().map_or(false, |c| c.is_uppercase()) {
                markers.push(ContextMarker {
                    marker_type: ContextMarkerType::Entity,
                    text: word.to_string(),
                    position: i,
                    confidence: 0.7,
                });
            }
        }

        // Extract sentiment markers (very simplified)
        let positive_words = ["good", "great", "excellent", "amazing", "wonderful"];
        let negative_words = ["bad", "terrible", "awful", "horrible", "disappointing"];

        for (i, word) in words.iter().enumerate() {
            let lower_word = word.to_lowercase();
            if positive_words.contains(&lower_word.as_str()) {
                markers.push(ContextMarker {
                    marker_type: ContextMarkerType::Sentiment,
                    text: "positive".to_string(),
                    position: i,
                    confidence: 0.8,
                });
            } else if negative_words.contains(&lower_word.as_str()) {
                markers.push(ContextMarker {
                    marker_type: ContextMarkerType::Sentiment,
                    text: "negative".to_string(),
                    position: i,
                    confidence: 0.8,
                });
            }
        }

        markers
    }

    /// Create memory attractor from semantic field embedding
    pub async fn create_memory_attractor_from_embedding(
        &self,
        embedding_result: &SemanticFieldEmbedding,
        importance_threshold: f32,
    ) -> ContextNestResult<Option<MemoryAttractor>> {
        // Only create attractor if content is semantically rich and important
        if embedding_result.semantic_richness < importance_threshold {
            return Ok(None);
        }

        let attractor = MemoryAttractor {
            id: uuid::Uuid::new_v4().to_string(),
            center: embedding_result.enhanced_embedding.clone(),
            strength: embedding_result.semantic_richness,
            radius: 0.3 + (embedding_result.field_resonance * 0.2), // Adaptive radius
            importance: (embedding_result.semantic_richness + embedding_result.field_resonance)
                / 2.0,
            created_at: Utc::now(),
            last_accessed: Utc::now(),
            access_count: 1,
            content: embedding_result.content.clone(),
            connections: Vec::new(),
            delete_reason: None,
            deleted_at: None,
        };

        Ok(Some(attractor))
    }

    /// Add semantic pattern to neural field from embedding
    pub async fn add_pattern_to_field(
        &self,
        field: &mut NeuralField,
        embedding_result: &SemanticFieldEmbedding,
        strength_threshold: f32,
    ) -> ContextNestResult<bool> {
        // Only add pattern if it has sufficient strength and uniqueness
        if embedding_result.semantic_richness < strength_threshold {
            return Ok(false);
        }

        // Check for duplicates
        for existing in &field.patterns {
            let similarity = self
                .calculate_similarity(&embedding_result.enhanced_embedding, &existing.embedding);
            if similarity > 0.9 {
                // Very similar pattern already exists
                return Ok(false);
            }
        }

        let pattern = SemanticPattern {
            id: uuid::Uuid::new_v4().to_string(),
            embedding: embedding_result.enhanced_embedding.clone(),
            strength: embedding_result.semantic_richness,
            resonance: embedding_result.field_resonance,
            created_at: Utc::now(),
            last_activated: Utc::now(),
            activation_count: 1,
            decay_rate: 0.01,
            content: embedding_result.content.clone(),
            delete_reason: None,
            deleted_at: None,
        };

        field.patterns.push(pattern);
        Ok(true)
    }

    /// Batch process content into semantic field components
    pub async fn batch_process_for_semantic_field(
        &self,
        contents: Vec<String>,
        target_field: &mut NeuralField,
        target_memory: &mut AttractorField,
        config: SemanticProcessingConfig,
    ) -> ContextNestResult<SemanticProcessingResult> {
        let mut results = Vec::new();
        let mut patterns_added = 0;
        let mut attractors_created = 0;

        for content in contents {
            // Generate semantic field embedding
            let embedding_result = self
                .generate_semantic_field_embedding(&content, Some(target_field))
                .await?;

            // Add to field if worthy
            if self
                .add_pattern_to_field(
                    target_field,
                    &embedding_result,
                    config.pattern_strength_threshold,
                )
                .await?
            {
                patterns_added += 1;
            }

            // Create memory attractor if worthy
            if let Some(attractor) = self
                .create_memory_attractor_from_embedding(
                    &embedding_result,
                    config.attractor_importance_threshold,
                )
                .await?
            {
                target_memory
                    .attractors
                    .insert(attractor.id.clone(), attractor);
                attractors_created += 1;
            }

            results.push(embedding_result);
        }

        Ok(SemanticProcessingResult {
            embeddings: results,
            patterns_added,
            attractors_created,
            processing_timestamp: Utc::now(),
        })
    }
}

/// Return the first `n` Unicode scalars (chars) of `s` as a slice, or `None`
/// if `s` has fewer than `n + 1` chars (i.e. there's nothing past the cut
/// point — preserves the original `> N` length-guard semantics).
///
/// Char-aware replacement for `&s[..n]`, which panics if byte index `n`
/// lands inside a multibyte UTF-8 sequence (`η²` → 4 bytes, 2 chars).
fn first_n_chars(s: &str, n: usize) -> Option<&str> {
    let end = s.char_indices().nth(n).map(|(i, _)| i)?;
    Some(&s[..end])
}

#[cfg(test)]
mod first_n_chars_tests {
    use super::first_n_chars;

    #[test]
    fn ascii_word_returns_first_n_chars() {
        assert_eq!(first_n_chars("hello", 3), Some("hel"));
    }

    #[test]
    fn multibyte_word_does_not_panic() {
        // Regression: the pre-fix code panicked here because byte index 3
        // lands inside the second char of `η²` (η=2B, ²=2B).
        assert_eq!(first_n_chars("η²", 3), None);
    }

    #[test]
    fn cjk_input_returns_first_n_chars_safely() {
        assert_eq!(first_n_chars("中文测试", 2), Some("中文"));
    }

    #[test]
    fn word_with_exactly_n_chars_returns_none() {
        // Mirrors original guard (`len() > 3` ⇒ require at least 4 chars).
        assert_eq!(first_n_chars("cat", 3), None);
    }

    #[test]
    fn empty_input_returns_none() {
        assert_eq!(first_n_chars("", 3), None);
    }
}

#[cfg(test)]
mod max_input_length_clamping_tests {
    //! Regression for the embedder context-window overflow bug.
    //!
    //! The default `local` model has `max_input_length = 512`. Inputs
    //! longer than that must be truncated to that char count before any
    //! provider dispatch, otherwise OpenAI-shaped providers reject the
    //! request with a "context_length exceeded" 400 and the fragment
    //! never gets an embedding.
    //!
    //! We exercise the wire-up via the local-embedding path (no HTTP
    //! provider, no API key needed) and use the cache as a witness:
    //! both the over-long input and the manually-truncated input must
    //! produce the same embedding, because they hash to the same cache
    //! key after truncation.

    use super::*;
    use crate::config::EmbeddingServicesConfig;

    #[tokio::test]
    async fn long_input_is_truncated_to_max_input_length() {
        let config = EmbeddingServicesConfig::default();
        let max_chars = config
            .models
            .get(&config.default_model)
            .expect("default model present in default config")
            .settings
            .max_input_length;
        assert_eq!(
            max_chars, 512,
            "this regression assumes the v0.1 default — adjust if the default changes"
        );

        let service = EmbeddingService::new(config).expect("service builds");

        let long_text = "x".repeat(max_chars * 4); // 4× over budget
        let truncated_text = "x".repeat(max_chars);

        let embedding_long = service
            .generate_embedding(&long_text)
            .await
            .expect("embed long");
        let embedding_truncated = service
            .generate_embedding(&truncated_text)
            .await
            .expect("embed truncated");

        assert_eq!(
            embedding_long, embedding_truncated,
            "over-budget input must be truncated to max_input_length before \
             dispatch, so both yield the same cache-keyed embedding"
        );
    }

    #[tokio::test]
    async fn input_at_or_under_limit_is_not_truncated() {
        let config = EmbeddingServicesConfig::default();
        let max_chars = config
            .models
            .get(&config.default_model)
            .expect("default model present")
            .settings
            .max_input_length;
        let service = EmbeddingService::new(config).expect("service builds");

        // Two distinct inputs, both under the limit, must produce
        // distinct embeddings — proving we do NOT collapse inputs that
        // fit within the budget.
        let short_a = "alpha alpha alpha alpha";
        let short_b = "beta beta beta beta";
        assert!(short_a.chars().count() < max_chars);
        assert!(short_b.chars().count() < max_chars);

        let emb_a = service.generate_embedding(short_a).await.expect("embed a");
        let emb_b = service.generate_embedding(short_b).await.expect("embed b");

        assert_ne!(
            emb_a, emb_b,
            "distinct under-budget inputs must produce distinct embeddings — \
             clamping must not fire when text fits"
        );
    }

    #[tokio::test]
    async fn multibyte_input_truncates_at_char_boundary_not_byte_boundary() {
        // Guard against the classic "&s[..n] panics inside a UTF-8
        // codepoint" trap. Use CJK characters which are 3 bytes each so
        // a naive byte slice at max_input_length would land mid-codepoint.
        let config = EmbeddingServicesConfig::default();
        let service = EmbeddingService::new(config).expect("service builds");

        // 1000 CJK chars = 3000 bytes, char count > max_input_length=512
        let multibyte_text: String = "中".repeat(1000);

        // Must not panic — if it does, the test fails noisily.
        let _ = service
            .generate_embedding(&multibyte_text)
            .await
            .expect("multibyte embed should not panic on byte-vs-char boundary");
    }
}

#[cfg(test)]
mod multibyte_regression_tests {
    use super::*;
    use crate::config::EmbeddingServicesConfig;

    /// End-to-end regression for the `byte index 3 is not a char boundary`
    /// panic seen in prod logs at 2026-05-20T21:29:31Z. The path:
    ///
    /// `generate_embedding` → `generate_local_embedding` →
    /// `extract_word_features` (bigram loop) → `calculate_bigram_score` →
    /// `estimate_semantic_relation` → unsafe `&w[..3]` byte slice
    ///
    /// Triggers when input contains multibyte UTF-8 words like `η²` that
    /// have `len() > 3` (4 bytes) but a char boundary inside that range.
    #[tokio::test]
    async fn generate_embedding_handles_multibyte_words_without_panic() {
        let service = EmbeddingService::new(EmbeddingServicesConfig::default())
            .expect("default local config builds");

        // Mix of multibyte math symbols and ASCII — matches the kind of
        // content the substrate sees from research/learning sessions.
        let text = "η² ψ² normalized eta squared partial χ²";
        let embedding = service
            .generate_embedding(text)
            .await
            .expect("multibyte input must not panic or error");
        assert!(!embedding.is_empty());
        assert_eq!(embedding.len(), service.config.dimensions);
    }

    #[tokio::test]
    async fn generate_embedding_handles_cjk_input() {
        let service = EmbeddingService::new(EmbeddingServicesConfig::default())
            .expect("default local config builds");
        let _ = service
            .generate_embedding("中文 输入 测试 文字 处理 路径")
            .await
            .expect("CJK input must not panic");
    }

    #[tokio::test]
    async fn generate_embedding_handles_emoji_input() {
        let service = EmbeddingService::new(EmbeddingServicesConfig::default())
            .expect("default local config builds");
        let _ = service
            .generate_embedding("hello 🚀 world 🌍 testing 🔥 stuff")
            .await
            .expect("emoji input must not panic");
    }
}

/// Enhanced embedding with semantic field awareness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticFieldEmbedding {
    pub content: String,
    pub base_embedding: Vec<f32>,
    pub enhanced_embedding: Vec<f32>,
    pub semantic_richness: f32,
    pub field_resonance: f32,
    pub context_markers: Vec<ContextMarker>,
    pub generation_timestamp: chrono::DateTime<Utc>,
    pub field_aligned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMarker {
    pub marker_type: ContextMarkerType,
    pub text: String,
    pub position: usize,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextMarkerType {
    Entity,
    Sentiment,
    Topic,
    Intent,
    Temporal,
    Spatial,
}

#[derive(Debug, Clone)]
pub struct SemanticProcessingConfig {
    pub pattern_strength_threshold: f32,
    pub attractor_importance_threshold: f32,
    pub field_enhancement_enabled: bool,
    pub context_marker_extraction: bool,
}

impl Default for SemanticProcessingConfig {
    fn default() -> Self {
        Self {
            pattern_strength_threshold: 0.5,
            attractor_importance_threshold: 0.4,
            field_enhancement_enabled: true,
            context_marker_extraction: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticProcessingResult {
    pub embeddings: Vec<SemanticFieldEmbedding>,
    pub patterns_added: usize,
    pub attractors_created: usize,
    pub processing_timestamp: chrono::DateTime<Utc>,
}

#[derive(Debug, serde::Serialize)]
pub struct CacheStats {
    pub size: usize,
    pub memory_usage: usize,
}

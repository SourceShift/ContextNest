use super::{field::NeuralField, MemoryCell, MemoryStrategy};
use crate::error::ContextNestResult;
use crate::{ContextNestError, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Memory orchestrator for managing different memory strategies
#[derive(Debug, Clone)]
pub struct MemoryOrchestrator {
    cells: HashMap<String, MemoryCell>,
    global_strategy: MemoryStrategy,
    attractor_field: AttractorField,
    persistence_params: PersistenceParameters,
}

impl MemoryOrchestrator {
    pub fn new(global_strategy: MemoryStrategy) -> Self {
        Self {
            cells: HashMap::new(),
            global_strategy,
            attractor_field: AttractorField::new(),
            persistence_params: PersistenceParameters::default(),
        }
    }

    /// Set persistence parameters for attractor-based memory
    pub fn set_persistence_params(&mut self, params: PersistenceParameters) {
        self.persistence_params = params;
    }

    /// Add interaction with attractor-based persistence
    pub fn add_interaction_with_attractors(
        &mut self,
        session_id: &str,
        interaction: Interaction,
        embedding: Vec<f32>,
        importance_signals: ImportanceSignals,
    ) -> ContextNestResult<Option<String>> {
        // Add to traditional memory cell
        let cell = self.get_or_create_cell(session_id);
        cell.add_interaction(interaction.clone())?;

        // Assess importance for attractor formation
        let importance = self.attractor_field.assess_importance(
            &interaction.content,
            &format!("session:{}", session_id),
            &importance_signals,
        );

        // Form attractor if important enough
        let attractor_id = if importance > self.persistence_params.importance_threshold {
            Some(self.attractor_field.form_attractor(
                interaction.content,
                embedding,
                importance,
                &self.persistence_params,
            )?)
        } else {
            None
        };

        // Apply memory attraction and decay
        self.maintain_attractor_field()?;

        Ok(attractor_id)
    }

    /// Get relevant memory including attractor-based memories
    pub fn get_comprehensive_memory(
        &self,
        session_id: &str,
        query: &str,
        query_embedding: &[f32],
    ) -> Option<ComprehensiveMemory> {
        // Get traditional memory
        let traditional_memory = self
            .cells
            .get(session_id)
            .map(|cell| cell.get_relevant(query));

        // Get attractor-based memory
        let activated_attractors = self.attractor_field.get_activated_attractors(
            query_embedding,
            0.4, // Activation threshold
        );

        Some(ComprehensiveMemory {
            traditional: traditional_memory,
            attractors: activated_attractors.into_iter().cloned().collect(),
        })
    }

    /// Integrate attractor field with neural field
    pub fn integrate_with_neural_field(
        &self,
        neural_field: &mut NeuralField,
    ) -> ContextNestResult<()> {
        self.attractor_field.integrate_with_field(neural_field, 0.7)
    }

    /// Maintain attractor field (apply decay, create connections)
    pub fn maintain_attractor_field(&mut self) -> ContextNestResult<()> {
        // Apply adaptive decay
        self.attractor_field
            .apply_adaptive_decay(&self.persistence_params)?;

        // Create new connections
        self.attractor_field
            .create_connections(self.persistence_params.connection_strength_threshold)?;

        Ok(())
    }

    /// Get attractor field metrics
    pub fn get_attractor_metrics(&self) -> &PersistenceMetrics {
        &self.attractor_field.persistence_metrics
    }

    /// Cleanup old attractors based on age and strength (legacy - hard delete only)
    pub fn cleanup_attractors(&mut self, max_age_days: i64) -> ContextNestResult<usize> {
        let now = Utc::now();
        let mut removed_count = 0;
        let mut to_remove = Vec::new();

        for (id, attractor) in &self.attractor_field.attractors {
            let age_days = (now - attractor.created_at).num_days();
            if age_days > max_age_days && attractor.strength < 0.3 {
                to_remove.push(id.clone());
            }
        }

        for id in to_remove {
            self.attractor_field.attractors.remove(&id);
            removed_count += 1;
        }

        self.attractor_field.update_metrics();
        Ok(removed_count)
    }

    /// Delete attractor with soft/hard delete option and cascade support
    pub fn delete_attractor(
        &mut self,
        attractor_id: &str,
        soft_delete: bool,
        reason: Option<String>,
    ) -> ContextNestResult<()> {
        self.delete_attractor_internal(attractor_id, soft_delete, reason)
    }

    /// Delete attractor with cascade options (internal - direct delete)
    fn delete_attractor_internal(
        &mut self,
        attractor_id: &str,
        soft_delete: bool,
        reason: Option<String>,
    ) -> ContextNestResult<()> {
        if soft_delete {
            // Soft delete - mark as deleted
            if let Some(attractor) = self.attractor_field.attractors.get_mut(attractor_id) {
                attractor.deleted_at = Some(Utc::now());
                attractor.delete_reason = reason;
                attractor.strength = 0.0; // Zero out strength
                self.attractor_field.update_metrics();
            }
            // Silently succeed if attractor doesn't exist (idempotent operation)
            Ok(())
        } else {
            // Hard delete - remove permanently with cascade cleanup
            // remove_attractor returns Ok even if attractor doesn't exist
            self.attractor_field.remove_attractor(attractor_id)?;
            self.attractor_field.update_metrics();
            Ok(())
        }
    }

    /// Restore soft-deleted attractor
    pub fn restore_attractor(&mut self, attractor_id: &str) -> ContextNestResult<()> {
        let attractor = self
            .attractor_field
            .attractors
            .get_mut(attractor_id)
            .ok_or_else(|| {
                ContextNestError::NotFound(format!("Attractor not found: {}", attractor_id))
            })?;

        if attractor.deleted_at.is_none() {
            return Err(ContextNestError::Validation(
                "Attractor is not deleted".to_string(),
            ));
        }

        attractor.deleted_at = None;
        attractor.delete_reason = None;
        attractor.strength = attractor.importance * 0.8; // Restore with reduced strength

        self.attractor_field.update_metrics();
        Ok(())
    }

    /// Get active attractors (excluding soft-deleted)
    pub fn get_active_attractors(&self) -> Vec<&MemoryAttractor> {
        self.attractor_field
            .attractors
            .values()
            .filter(|a| a.deleted_at.is_none())
            .collect()
    }

    /// Get deleted attractors (soft-deleted only)
    pub fn get_deleted_attractors(&self) -> Vec<&MemoryAttractor> {
        self.attractor_field
            .attractors
            .values()
            .filter(|a| a.deleted_at.is_some())
            .collect()
    }

    /// Permanently remove all soft-deleted attractors
    pub fn purge_deleted_attractors(&mut self) -> ContextNestResult<usize> {
        let to_remove: Vec<String> = self
            .attractor_field
            .attractors
            .iter()
            .filter(|(_, a)| a.deleted_at.is_some())
            .map(|(id, _)| id.clone())
            .collect();

        let removed_count = to_remove.len();

        for id in to_remove {
            self.attractor_field.remove_attractor(&id)?;
        }

        self.attractor_field.update_metrics();
        Ok(removed_count)
    }

    /// Scan memory fragments for reconstruction with activation threshold filtering
    pub fn scan_fragments(
        &self,
        activation_threshold: f32,
    ) -> ContextNestResult<Vec<MemoryFragmentInfo>> {
        let mut fragments = Vec::new();

        for (id, attractor) in &self.attractor_field.attractors {
            // Skip soft-deleted attractors
            if attractor.deleted_at.is_some() {
                continue;
            }

            // Filter by activation threshold
            if attractor.strength >= activation_threshold {
                let now = Utc::now();
                let age_hours = (now - attractor.created_at).num_hours();

                // Calculate coherence based on connections and strength
                let coherence = if attractor.connections.is_empty() {
                    attractor.strength * 0.5 // Lower coherence for isolated fragments
                } else {
                    attractor.strength
                        * (1.0 + (attractor.connections.len() as f32 / 10.0).min(0.5))
                };

                let fragment = MemoryFragmentInfo {
                    id: id.clone(),
                    fragment_type: FragmentType::from_content(&attractor.content),
                    embedding: attractor.center.clone(),
                    content: attractor.content.clone(),
                    strength: attractor.strength,
                    importance: attractor.importance,
                    coherence: coherence.min(1.0),
                    age_hours,
                    access_count: attractor.access_count,
                    connections: attractor.connections.clone(),
                    last_accessed: attractor.last_accessed,
                };

                fragments.push(fragment);
            }
        }

        // Sort fragments by relevance (strength * coherence)
        fragments.sort_by(|a, b| {
            let a_relevance = a.strength * a.coherence;
            let b_relevance = b.strength * b.coherence;
            b_relevance.partial_cmp(&a_relevance).unwrap()
        });

        Ok(fragments)
    }

    /// Adapt fragments based on reconstruction success
    pub fn adapt_fragments(
        &mut self,
        successful_fragments: &[String],
        problematic_fragments: &[String],
    ) -> ContextNestResult<AdaptationResult> {
        let mut strengthened = 0;
        let mut weakened = 0;
        let mut new_connections = 0;

        // Strengthen successful fragments
        for fragment_id in successful_fragments {
            if let Some(attractor) = self.attractor_field.attractors.get_mut(fragment_id) {
                // Increase strength by 10%
                attractor.strength = (attractor.strength * 1.1).min(1.0);
                strengthened += 1;
            }
        }

        // Weaken problematic fragments
        for fragment_id in problematic_fragments {
            if let Some(attractor) = self.attractor_field.attractors.get_mut(fragment_id) {
                // Decrease strength by 5%
                attractor.strength = (attractor.strength * 0.95).max(0.0);
                weakened += 1;
            }
        }

        // Create new connections between co-activated successful fragments
        for i in 0..successful_fragments.len() {
            for j in (i + 1)..successful_fragments.len() {
                let frag_a_id = &successful_fragments[i];
                let frag_b_id = &successful_fragments[j];

                // Check if connection already exists
                let needs_connection =
                    if let Some(frag_a) = self.attractor_field.attractors.get(frag_a_id) {
                        !frag_a.connections.contains(frag_b_id)
                    } else {
                        false
                    };

                if needs_connection {
                    // Add bidirectional connection
                    if let Some(frag_a) = self.attractor_field.attractors.get_mut(frag_a_id) {
                        frag_a.connections.push(frag_b_id.clone());
                        new_connections += 1;
                    }

                    if let Some(frag_b) = self.attractor_field.attractors.get_mut(frag_b_id) {
                        frag_b.connections.push(frag_a_id.clone());
                        new_connections += 1;
                    }
                }
            }
        }

        self.attractor_field.update_metrics();

        Ok(AdaptationResult {
            strengthened_count: strengthened,
            weakened_count: weakened,
            new_connections_count: new_connections,
        })
    }

    /// Consolidate memory with adaptive decay and pattern strengthening
    pub fn consolidate_memory(&mut self) -> ContextNestResult<ConsolidationResult> {
        let now = Utc::now();
        let mut strengthened_patterns = 0;
        let mut pruned_fragments = 0;
        let mut to_remove = Vec::new();

        // Calculate co-activation patterns
        let mut coactivation_counts: HashMap<(String, String), u32> = HashMap::new();

        // Track which fragments are frequently co-activated
        for attractor in self.attractor_field.attractors.values() {
            for connection_id in &attractor.connections {
                let pair = if attractor.id < *connection_id {
                    (attractor.id.clone(), connection_id.clone())
                } else {
                    (connection_id.clone(), attractor.id.clone())
                };
                *coactivation_counts.entry(pair).or_insert(0) += 1;
            }
        }

        // Process each attractor
        for (id, attractor) in &mut self.attractor_field.attractors {
            // Skip soft-deleted
            if attractor.deleted_at.is_some() {
                continue;
            }

            // Calculate adaptive decay factors
            let age_hours = (now - attractor.created_at).num_hours() as f32;
            let hours_since_access = (now - attractor.last_accessed).num_hours() as f32;

            // Age factor: older memories decay slower (consolidation effect)
            let age_factor = if age_hours < 24.0 {
                1.0 // Recent memories decay normally
            } else {
                0.5 // Older memories are more stable
            };

            // Use factor: frequently accessed memories decay slower
            let use_factor = if attractor.access_count > 5 {
                0.3 // Frequently used memories are more stable
            } else if attractor.access_count > 2 {
                0.6
            } else {
                1.0 // Rarely used memories decay normally
            };

            // Importance factor: important memories decay slower
            let importance_factor = 1.0 - (attractor.importance * 0.7);

            // Combined decay rate
            let decay_rate =
                self.persistence_params.decay_rate * age_factor * use_factor * importance_factor;

            // Apply decay
            attractor.strength *= 1.0 - decay_rate;

            // Check for frequent co-activation with other fragments
            let mut total_coactivations = 0;
            for connection_id in &attractor.connections {
                let pair = if attractor.id < *connection_id {
                    (attractor.id.clone(), connection_id.clone())
                } else {
                    (connection_id.clone(), attractor.id.clone())
                };
                if let Some(count) = coactivation_counts.get(&pair) {
                    total_coactivations += *count;
                }
            }

            // Strengthen frequently co-activated patterns
            if total_coactivations > 3 {
                attractor.strength = (attractor.strength * 1.05).min(1.0);
                strengthened_patterns += 1;
            }

            // Mark for pruning if below threshold
            if attractor.strength < 0.05 {
                to_remove.push(id.clone());
            }
        }

        // Prune weak fragments
        for id in &to_remove {
            self.attractor_field.remove_attractor(id)?;
            pruned_fragments += 1;
        }

        self.attractor_field.update_metrics();

        Ok(ConsolidationResult {
            strengthened_patterns,
            pruned_fragments,
            total_fragments: self.attractor_field.attractors.len(),
            average_strength: self.attractor_field.persistence_metrics.average_strength,
        })
    }

    /// Get or create memory cell for session
    pub fn get_or_create_cell(&mut self, session_id: &str) -> &mut MemoryCell {
        self.cells
            .entry(session_id.to_string())
            .or_insert_with(|| MemoryCell {
                strategy: self.global_strategy.clone(),
                short_term: Vec::new(),
                working: HashMap::new(),
                long_term: Vec::new(),
            })
    }

    /// Add interaction to memory
    pub fn add_interaction(
        &mut self,
        session_id: &str,
        interaction: Interaction,
    ) -> ContextNestResult<()> {
        let cell = self.get_or_create_cell(session_id);
        cell.add_interaction(interaction)?;
        Ok(())
    }

    /// Get relevant memory for context building
    pub fn get_relevant_memory(&self, session_id: &str, query: &str) -> Option<RelevantMemory> {
        self.cells
            .get(session_id)
            .map(|cell| cell.get_relevant(query))
    }

    /// Update memory strategy for session
    pub fn update_strategy(
        &mut self,
        session_id: &str,
        strategy: MemoryStrategy,
    ) -> ContextNestResult<()> {
        if let Some(cell) = self.cells.get_mut(session_id) {
            cell.update_strategy(strategy)?;
        }
        Ok(())
    }

    /// Clear memory for session
    pub fn clear_session(&mut self, session_id: &str) {
        self.cells.remove(session_id);
    }

    /// Get comprehensive memory statistics
    pub fn get_stats(&self) -> ComprehensiveMemoryStats {
        let total_sessions = self.cells.len();
        let total_interactions: usize = self
            .cells
            .values()
            .map(|cell| cell.short_term.len() + cell.long_term.len())
            .sum();
        let total_facts: usize = self.cells.values().map(|cell| cell.working.len()).sum();
        let traditional_memory_usage = std::mem::size_of::<MemoryCell>() * self.cells.len();
        let attractor_memory_usage =
            std::mem::size_of::<MemoryAttractor>() * self.attractor_field.attractors.len();

        ComprehensiveMemoryStats {
            total_sessions,
            total_interactions,
            total_facts,
            traditional_memory_usage,
            total_attractors: self.attractor_field.persistence_metrics.total_attractors,
            active_attractors: self.attractor_field.persistence_metrics.active_attractors,
            attractor_memory_usage,
            average_attractor_strength: self.attractor_field.persistence_metrics.average_strength,
            connection_density: self.attractor_field.persistence_metrics.connection_density,
        }
    }
}

impl MemoryCell {
    /// Add new interaction to memory
    pub fn add_interaction(&mut self, interaction: Interaction) -> ContextNestResult<()> {
        match &self.strategy {
            MemoryStrategy::Windowing { size } => {
                self.short_term.push(interaction.content);
                if self.short_term.len() > *size {
                    // Move oldest to long-term before removing
                    if let Some(oldest) = self.short_term.first() {
                        self.long_term.push(oldest.clone());
                    }
                    self.short_term.remove(0);
                }
            }
            MemoryStrategy::Summarization { threshold } => {
                self.short_term.push(interaction.content);
                if self.short_term.len() > *threshold {
                    let summary = self.summarize_short_term()?;
                    self.long_term.push(summary);
                    self.short_term.clear();
                }
            }
            MemoryStrategy::KeyValue => {
                // Extract key-value pairs from interaction
                let facts = self.extract_facts(&interaction.content)?;
                for (key, value) in facts {
                    self.working.insert(key, value);
                }
                // Still keep in short-term for context
                self.short_term.push(interaction.content);
                if self.short_term.len() > 5 {
                    self.short_term.remove(0);
                }
            }
            MemoryStrategy::PriorityPruning { max_tokens } => {
                self.short_term.push(interaction.content.clone());
                self.prune_by_priority(*max_tokens)?;
            }
        }
        Ok(())
    }

    /// Update memory strategy and migrate existing data
    pub fn update_strategy(&mut self, new_strategy: MemoryStrategy) -> ContextNestResult<()> {
        // Migrate existing data based on new strategy
        match &new_strategy {
            MemoryStrategy::KeyValue => {
                // Extract facts from existing short-term memory
                for item in &self.short_term {
                    let facts = self.extract_facts(item)?;
                    for (key, value) in facts {
                        self.working.insert(key, value);
                    }
                }
            }
            MemoryStrategy::Windowing { size } => {
                // Truncate to window size
                if self.short_term.len() > *size {
                    let excess = self.short_term.split_off(*size);
                    self.long_term.extend(excess);
                }
            }
            _ => {
                // Other strategies don't require immediate migration
            }
        }

        self.strategy = new_strategy;
        Ok(())
    }

    /// Get relevant memory for query
    pub fn get_relevant(&self, query: &str) -> RelevantMemory {
        let mut relevant = RelevantMemory {
            short_term: Vec::new(),
            working: HashMap::new(),
            long_term: Vec::new(),
        };

        // Find relevant short-term memories
        for item in &self.short_term {
            if self.is_relevant(item, query) {
                relevant.short_term.push(item.clone());
            }
        }

        // Find relevant working memory facts
        for (key, value) in &self.working {
            if self.is_relevant(key, query) || self.is_relevant(&value.to_string(), query) {
                relevant.working.insert(key.clone(), value.clone());
            }
        }

        // Find relevant long-term memories
        for item in &self.long_term {
            if self.is_relevant(item, query) {
                relevant.long_term.push(item.clone());
            }
        }

        relevant
    }

    /// Check if memory item is relevant to query
    fn is_relevant(&self, item: &str, query: &str) -> bool {
        let item_lower = item.to_lowercase();
        let query_lower = query.to_lowercase();

        // Simple keyword matching - could be enhanced with embeddings
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        query_words
            .iter()
            .any(|word| word.len() > 2 && item_lower.contains(word))
    }

    /// Summarize short-term memory
    fn summarize_short_term(&self) -> ContextNestResult<String> {
        if self.short_term.is_empty() {
            return Ok(String::new());
        }

        // Simple summarization - in practice, would use an LLM
        let total_length: usize = self.short_term.iter().map(|s| s.len()).sum();
        let avg_length = total_length / self.short_term.len();

        let summary = format!(
            "Summary of {} interactions (avg {} chars): Recent topics include {}",
            self.short_term.len(),
            avg_length,
            self.extract_key_topics().join(", ")
        );

        Ok(summary)
    }

    /// Extract key topics from short-term memory
    fn extract_key_topics(&self) -> Vec<String> {
        let mut word_counts = HashMap::new();

        for item in &self.short_term {
            for word in item.split_whitespace() {
                let word = word.to_lowercase();
                if word.len() > 4 && !is_common_word(&word) {
                    *word_counts.entry(word).or_insert(0) += 1;
                }
            }
        }

        let mut topics: Vec<(String, usize)> = word_counts.into_iter().collect();
        topics.sort_by(|a, b| b.1.cmp(&a.1));

        topics.into_iter().take(5).map(|(word, _)| word).collect()
    }

    /// Extract key-value facts from text
    fn extract_facts(&self, text: &str) -> ContextNestResult<Vec<(String, serde_json::Value)>> {
        let mut facts = Vec::new();

        // Simple fact extraction patterns
        let patterns = [
            (r"(\w+)\s+is\s+(.+)", "property"),
            (r"(\w+)\s*=\s*(.+)", "assignment"),
            (r"(\w+):\s*(.+)", "definition"),
        ];

        for (pattern, fact_type) in &patterns {
            if let Ok(regex) = regex::Regex::new(pattern) {
                for cap in regex.captures_iter(text) {
                    if cap.len() >= 3 {
                        let key = cap[1].to_string();
                        let value = serde_json::Value::String(cap[2].trim().to_string());
                        facts.push((key, value));
                    }
                }
            }
        }

        Ok(facts)
    }

    /// Prune memory based on priority
    fn prune_by_priority(&mut self, max_tokens: usize) -> ContextNestResult<()> {
        let current_tokens = self.estimate_tokens();

        if current_tokens <= max_tokens {
            return Ok(());
        }

        // Prioritize recent items and important facts
        let mut items_with_priority: Vec<(usize, String)> = self
            .short_term
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let priority = self.calculate_priority(item, i);
                (priority, item.clone())
            })
            .collect();

        // Sort by priority (higher is better)
        items_with_priority.sort_by(|a, b| b.0.cmp(&a.0));

        // Keep items until we hit the token limit
        let mut kept_items = Vec::new();
        let mut tokens_used = 0;

        for (_, item) in items_with_priority {
            let item_tokens = item.len() / 4; // Rough estimate
            if tokens_used + item_tokens <= max_tokens {
                kept_items.push(item);
                tokens_used += item_tokens;
            } else {
                // Move to long-term storage
                self.long_term.push(item);
            }
        }

        self.short_term = kept_items;
        Ok(())
    }

    /// Calculate priority score for memory item
    fn calculate_priority(&self, item: &str, recency_index: usize) -> usize {
        let mut score = 0;

        // Recency bonus (more recent = higher score)
        score += (100 - recency_index) * 10;

        // Length penalty (very long items get lower priority)
        if item.len() > 500 {
            score = score.saturating_sub(50);
        }

        // Keyword bonus
        let important_keywords = ["error", "widget", "screen", "style", "theme"];
        for keyword in &important_keywords {
            if item.to_lowercase().contains(keyword) {
                score += 20;
            }
        }

        score
    }

    /// Estimate token count for all memory
    fn estimate_tokens(&self) -> usize {
        let short_term_tokens: usize = self.short_term.iter().map(|s| s.len() / 4).sum();
        let working_tokens: usize = self
            .working
            .iter()
            .map(|(k, v)| (k.len() + v.to_string().len()) / 4)
            .sum();
        let long_term_tokens: usize = self.long_term.iter().map(|s| s.len() / 4).sum();

        short_term_tokens + working_tokens + long_term_tokens
    }
}

/// Represents an interaction to be stored in memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interaction {
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub interaction_type: InteractionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InteractionType {
    Query,
    Response,
    Action,
    Result,
}

/// Relevant memory extracted for context building
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelevantMemory {
    pub short_term: Vec<String>,
    pub working: HashMap<String, serde_json::Value>,
    pub long_term: Vec<String>,
}

/// Comprehensive memory that includes both traditional and attractor-based memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComprehensiveMemory {
    pub traditional: Option<RelevantMemory>,
    pub attractors: Vec<MemoryAttractor>,
}

/// Comprehensive memory statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComprehensiveMemoryStats {
    pub total_sessions: usize,
    pub total_interactions: usize,
    pub total_facts: usize,
    pub traditional_memory_usage: usize,
    pub total_attractors: usize,
    pub active_attractors: usize,
    pub attractor_memory_usage: usize,
    pub average_attractor_strength: f32,
    pub connection_density: f32,
}

/// Memory statistics (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub total_sessions: usize,
    pub total_interactions: usize,
    pub total_facts: usize,
    pub memory_usage: usize,
}

/// Attractor-based memory persistence structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAttractor {
    pub id: String,
    pub center: Vec<f32>,
    pub strength: f32,
    pub radius: f32,
    pub importance: f32,
    pub created_at: DateTime<Utc>,
    pub last_accessed: DateTime<Utc>,
    pub access_count: u32,
    pub content: String,
    pub connections: Vec<String>, // IDs of connected attractors
    // Soft delete support
    pub deleted_at: Option<DateTime<Utc>>,
    pub delete_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttractorField {
    pub attractors: HashMap<String, MemoryAttractor>,
    pub field_properties: AttractorFieldProperties,
    pub persistence_metrics: PersistenceMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttractorFieldProperties {
    pub formation_threshold: f32,
    pub decay_rate: f32,
    pub connection_threshold: f32,
    pub max_attractors: usize,
    pub stability_factor: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceMetrics {
    pub total_attractors: usize,
    pub active_attractors: usize,
    pub average_strength: f32,
    pub connection_density: f32,
    pub last_cleanup: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportanceSignals {
    pub explicit_markers: Vec<String>,
    pub emotional_weight: f32,
    pub repetition_count: u32,
    pub novelty_score: f32,
    pub context_relevance: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceParameters {
    pub importance_threshold: f32,
    pub decay_rate: f32,
    pub minimum_strength: f32,
    pub consolidation_factor: f32,
    pub connection_strength_threshold: f32,
}

impl Default for AttractorFieldProperties {
    fn default() -> Self {
        Self {
            formation_threshold: 0.6,
            decay_rate: 0.05,
            connection_threshold: 0.7,
            max_attractors: 1000,
            stability_factor: 0.8,
        }
    }
}

impl Default for PersistenceParameters {
    fn default() -> Self {
        Self {
            importance_threshold: 0.6,
            decay_rate: 0.02,
            minimum_strength: 0.2,
            consolidation_factor: 1.2,
            connection_strength_threshold: 0.5,
        }
    }
}

impl AttractorField {
    pub fn new() -> Self {
        Self {
            attractors: HashMap::new(),
            field_properties: AttractorFieldProperties::default(),
            persistence_metrics: PersistenceMetrics {
                total_attractors: 0,
                active_attractors: 0,
                average_strength: 0.0,
                connection_density: 0.0,
                last_cleanup: Utc::now(),
            },
        }
    }

    /// Form new memory attractor from important information
    pub fn form_attractor(
        &mut self,
        content: String,
        embedding: Vec<f32>,
        importance: f32,
        params: &PersistenceParameters,
    ) -> ContextNestResult<String> {
        if importance < params.importance_threshold {
            return Ok(String::new()); // Not important enough to form attractor
        }

        let attractor_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        let attractor = MemoryAttractor {
            id: attractor_id.clone(),
            center: embedding,
            strength: importance * params.consolidation_factor,
            radius: 0.5, // Dynamic radius based on importance
            importance,
            created_at: now,
            last_accessed: now,
            access_count: 1,
            content,
            connections: Vec::new(),
            deleted_at: None,            delete_reason: None,
        };

        // Check for connections with existing attractors
        let connections = self.find_connections(&attractor, params.connection_strength_threshold);

        // Update connections
        let mut updated_attractor = attractor;
        updated_attractor.connections = connections.clone();

        // Add connections in both directions
        for connection_id in &connections {
            if let Some(connected_attractor) = self.attractors.get_mut(connection_id) {
                if !connected_attractor.connections.contains(&attractor_id) {
                    connected_attractor.connections.push(attractor_id.clone());
                }
            }
        }

        self.attractors
            .insert(attractor_id.clone(), updated_attractor);
        self.update_metrics();

        Ok(attractor_id)
    }

    /// Apply memory attraction based on resonance with current field
    pub fn apply_memory_attraction(
        &mut self,
        current_field: &NeuralField,
        params: &PersistenceParameters,
    ) -> ContextNestResult<Vec<String>> {
        let mut activated_attractors = Vec::new();
        let now = Utc::now();

        // Find attractors that resonate with current field
        let attractor_ids: Vec<String> = self.attractors.keys().cloned().collect();
        for id in attractor_ids {
            let resonance = if let Some(attractor) = self.attractors.get(&id) {
                self.calculate_field_resonance(attractor, current_field)?
            } else {
                continue;
            };

            if resonance > 0.4 {
                // Activation threshold
                // Strengthen attractor based on resonance
                let strength_boost = resonance * params.consolidation_factor * 0.5;

                if let Some(attractor) = self.attractors.get_mut(&id) {
                    attractor.strength = (attractor.strength + strength_boost).min(1.0);

                    // Update access information
                    attractor.last_accessed = now;
                    attractor.access_count += 1;

                    activated_attractors.push(id.clone());
                }
            }
        }

        self.update_metrics();
        Ok(activated_attractors)
    }

    /// Apply memory attraction based on semantic query
    pub fn apply_semantic_attraction(
        &mut self,
        query_embedding: &[f32],
        params: &PersistenceParameters,
    ) -> ContextNestResult<Vec<String>> {
        let mut activated_attractors = Vec::new();
        let now = Utc::now();

        // Find attractors that are semantically similar to query
        let attractor_ids: Vec<String> = self.attractors.keys().cloned().collect();
        for id in attractor_ids {
            let (semantic_similarity, activation_strength) =
                if let Some(attractor) = self.attractors.get(&id) {
                    let similarity = cosine_similarity(&attractor.center, query_embedding);
                    (similarity, similarity * attractor.importance)
                } else {
                    continue;
                };

            if semantic_similarity > 0.5 && activation_strength > 0.3 {
                // Strengthen attractor based on activation
                let strength_boost = activation_strength * params.consolidation_factor * 0.3;

                if let Some(attractor) = self.attractors.get_mut(&id) {
                    attractor.strength = (attractor.strength + strength_boost).min(1.0);

                    // Update access information
                    attractor.last_accessed = now;
                    attractor.access_count += 1;
                }

                // Propagate activation to connected attractors
                self.propagate_activation(&id, activation_strength * 0.5, params)?;

                activated_attractors.push(id);
            }
        }

        self.update_metrics();
        Ok(activated_attractors)
    }

    /// Propagate activation through attractor network
    fn propagate_activation(
        &mut self,
        source_id: &str,
        activation_strength: f32,
        params: &PersistenceParameters,
    ) -> ContextNestResult<()> {
        let connections = if let Some(source_attractor) = self.attractors.get(source_id) {
            source_attractor.connections.clone()
        } else {
            return Ok(());
        };

        let now = Utc::now();

        for connection_id in connections {
            if let Some(connected_attractor) = self.attractors.get_mut(&connection_id) {
                // Apply scaled activation
                let propagated_strength = activation_strength * 0.7; // Dampening factor
                let strength_boost = propagated_strength * params.consolidation_factor * 0.2;

                connected_attractor.strength =
                    (connected_attractor.strength + strength_boost).min(1.0);
                connected_attractor.last_accessed = now;
            }
        }

        Ok(())
    }

    /// Apply adaptive decay to memory attractors
    pub fn apply_adaptive_decay(
        &mut self,
        params: &PersistenceParameters,
    ) -> ContextNestResult<usize> {
        let now = Utc::now();
        let mut decayed_count = 0;
        let mut to_remove = Vec::new();

        for (id, attractor) in &mut self.attractors {
            // Calculate age-based decay factor
            let age_hours = (now - attractor.last_accessed).num_hours() as f32;
            let age_factor = 1.0 - (age_hours / (24.0 * 30.0)).min(0.9); // Slower decay over time

            // Calculate importance-based decay resistance
            let importance_factor = 1.0 - (0.8 * attractor.importance);

            // Calculate connection-based decay resistance
            let connection_factor =
                1.0 - (0.5 * (attractor.connections.len() as f32 / 10.0).min(0.9));

            // Combined decay factor
            let decay_factor =
                params.decay_rate * age_factor * importance_factor * connection_factor;

            // Apply decay
            attractor.strength *= 1.0 - decay_factor;
            decayed_count += 1;

            // Mark for removal if below minimum strength
            if attractor.strength < params.minimum_strength {
                to_remove.push(id.clone());
            }
        }

        // Remove weak attractors
        for id in &to_remove {
            self.remove_attractor(id)?;
        }

        self.update_metrics();
        Ok(decayed_count)
    }

    /// Assess importance of new information using multiple factors
    pub fn assess_importance(
        &self,
        content: &str,
        context: &str,
        signals: &ImportanceSignals,
    ) -> f32 {
        let mut importance_score = 0.0;
        let mut factor_count = 0;

        // 1. Explicit importance markers
        for marker in &signals.explicit_markers {
            if content.to_lowercase().contains(&marker.to_lowercase()) {
                importance_score += 0.8;
                factor_count += 1;
                break;
            }
        }

        // 2. Emotional weight
        importance_score += signals.emotional_weight * 0.6;
        factor_count += 1;

        // 3. Novelty score
        importance_score += signals.novelty_score * 0.7;
        factor_count += 1;

        // 4. Context relevance
        importance_score += signals.context_relevance * 0.5;
        factor_count += 1;

        // 5. Repetition emphasis
        if signals.repetition_count > 1 {
            let repetition_score = (signals.repetition_count as f32 / 10.0).min(0.9);
            importance_score += repetition_score;
            factor_count += 1;
        }

        // 6. Content length and structure (longer, structured content often more important)
        let length_score = (content.len() as f32 / 1000.0).min(0.5);
        importance_score += length_score;
        factor_count += 1;

        // Calculate average and normalize
        if factor_count > 0 {
            importance_score /= factor_count as f32;
        }

        importance_score.min(1.0)
    }

    /// Create connections between related attractors
    pub fn create_connections(&mut self, threshold: f32) -> ContextNestResult<usize> {
        let mut connection_count = 0;
        let attractor_ids: Vec<String> = self.attractors.keys().cloned().collect();

        for i in 0..attractor_ids.len() {
            for j in (i + 1)..attractor_ids.len() {
                let id1 = &attractor_ids[i];
                let id2 = &attractor_ids[j];

                if let (Some(att1), Some(att2)) =
                    (self.attractors.get(id1), self.attractors.get(id2))
                {
                    let similarity = cosine_similarity(&att1.center, &att2.center);

                    if similarity > threshold {
                        // Add bidirectional connection
                        if let Some(attractor1) = self.attractors.get_mut(id1) {
                            if !attractor1.connections.contains(id2) {
                                attractor1.connections.push(id2.clone());
                                connection_count += 1;
                            }
                        }

                        if let Some(attractor2) = self.attractors.get_mut(id2) {
                            if !attractor2.connections.contains(id1) {
                                attractor2.connections.push(id1.clone());
                                connection_count += 1;
                            }
                        }
                    }
                }
            }
        }

        self.update_metrics();
        Ok(connection_count)
    }

    /// Integrate attractor field with neural field for persistent memory influence
    pub fn integrate_with_field(
        &self,
        neural_field: &mut NeuralField,
        harmony_threshold: f32,
    ) -> ContextNestResult<()> {
        // Add strong attractors as persistent patterns in the neural field
        for attractor in self.attractors.values() {
            if attractor.strength > harmony_threshold {
                neural_field.inject(attractor.content.clone(), attractor.center.clone())?;

                // Tune the field to strengthen persistent patterns
                neural_field.tune("amplification_factor", 1.1)?;
            }
        }

        // Apply field tuning based on attractor network properties
        let avg_strength = self.persistence_metrics.average_strength;
        let connection_density = self.persistence_metrics.connection_density;

        // Adjust field properties based on memory characteristics
        if avg_strength > 0.7 {
            neural_field.tune("resonance_threshold", 0.6)?; // Lower threshold for strong memories
        }

        if connection_density > 0.3 {
            neural_field.tune("coherence_weight", 0.9)?; // Higher coherence for connected memories
        }

        Ok(())
    }

    /// Create bidirectional integration between attractor field and neural field
    pub fn bidirectional_integration(
        &mut self,
        neural_field: &mut NeuralField,
        integration_strength: f32,
    ) -> ContextNestResult<()> {
        // Step 1: Inject strong attractors into neural field
        self.integrate_with_field(neural_field, 0.7)?;

        // Step 2: Extract patterns from neural field that could form new attractors
        // This would require pattern extraction from neural field
        // For now, we'll simulate this process

        // Step 3: Strengthen attractors that resonate with current field state
        let params = PersistenceParameters::default();
        self.apply_memory_attraction(neural_field, &params)?;

        // Step 4: Create harmonic bridges between attractors and field patterns
        self.create_harmonic_bridges(neural_field, integration_strength)?;

        Ok(())
    }

    /// Create harmonic bridges between attractors and neural field patterns
    fn create_harmonic_bridges(
        &mut self,
        neural_field: &NeuralField,
        strength: f32,
    ) -> ContextNestResult<()> {
        // This would create connections between memory attractors and active field patterns
        // For now, we'll implement a simplified version

        let now = Utc::now();

        // Strengthen recently accessed attractors based on field coherence
        for attractor in self.attractors.values_mut() {
            let hours_since_access = (now - attractor.last_accessed).num_hours() as f32;

            if hours_since_access < 24.0 {
                // Recent attractors
                let bridge_strength = strength * (1.0 - hours_since_access / 24.0);
                attractor.strength = (attractor.strength + bridge_strength * 0.2).min(1.0);
            }
        }

        Ok(())
    }

    /// Get activated attractors based on query (excludes soft-deleted)
    pub fn get_activated_attractors(
        &self,
        query_embedding: &[f32],
        threshold: f32,
    ) -> Vec<&MemoryAttractor> {
        let mut activated = Vec::new();

        for attractor in self.attractors.values() {
            // Skip soft-deleted attractors
            if attractor.deleted_at.is_some() {
                continue;
            }

            let similarity = cosine_similarity(&attractor.center, query_embedding);
            if similarity > threshold {
                activated.push(attractor);
            }
        }

        // Sort by activation strength (similarity * strength)
        activated.sort_by(|a, b| {
            let a_activation = cosine_similarity(&a.center, query_embedding) * a.strength;
            let b_activation = cosine_similarity(&b.center, query_embedding) * b.strength;
            b_activation.partial_cmp(&a_activation).unwrap()
        });

        activated
    }

    /// Remove attractor and update connections
    fn remove_attractor(&mut self, attractor_id: &str) -> ContextNestResult<()> {
        if let Some(attractor) = self.attractors.remove(attractor_id) {
            // Remove connections from other attractors
            for connection_id in &attractor.connections {
                if let Some(connected) = self.attractors.get_mut(connection_id) {
                    connected.connections.retain(|id| id != attractor_id);
                }
            }
        }
        Ok(())
    }

    /// Find connections for a new attractor
    fn find_connections(&self, new_attractor: &MemoryAttractor, threshold: f32) -> Vec<String> {
        let mut connections = Vec::new();

        for (id, existing_attractor) in &self.attractors {
            let similarity = cosine_similarity(&new_attractor.center, &existing_attractor.center);
            if similarity > threshold {
                connections.push(id.clone());
            }
        }

        connections
    }

    /// Calculate resonance between attractor and neural field
    fn calculate_field_resonance(
        &self,
        attractor: &MemoryAttractor,
        field: &NeuralField,
    ) -> ContextNestResult<f32> {
        // This would require access to field patterns
        // For now, we'll implement a simplified version that can be enhanced
        // when we have better integration with NeuralField

        // Calculate semantic resonance based on field state
        let field_coherence = 0.8; // Would get from field.state.coherence
        let field_stability = 0.7; // Would get from field.state.stability

        // Base resonance starts with attractor importance
        let mut resonance = attractor.importance * 0.4;

        // Factor in field coherence and stability
        resonance += field_coherence * 0.3;
        resonance += field_stability * 0.2;

        // Factor in attractor age (recent attractors resonate more)
        let now = Utc::now();
        let hours_since_access = (now - attractor.last_accessed).num_hours() as f32;
        let recency_factor = (1.0 - (hours_since_access / (24.0 * 7.0))).max(0.1); // Week decay
        resonance += recency_factor * 0.1;

        Ok(resonance.min(1.0))
    }

    /// Update persistence metrics
    fn update_metrics(&mut self) {
        let total = self.attractors.len();
        let active = self
            .attractors
            .values()
            .filter(|a| a.strength > 0.3)
            .count();
        let avg_strength = if total > 0 {
            self.attractors.values().map(|a| a.strength).sum::<f32>() / total as f32
        } else {
            0.0
        };

        let total_connections: usize = self.attractors.values().map(|a| a.connections.len()).sum();
        let connection_density = if total > 1 {
            total_connections as f32 / (total * (total - 1)) as f32
        } else {
            0.0
        };

        self.persistence_metrics = PersistenceMetrics {
            total_attractors: total,
            active_attractors: active,
            average_strength: avg_strength,
            connection_density,
            last_cleanup: Utc::now(),
        };
    }
}

/// Cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}

impl AttractorField {
    /// Calculate memory utilization metrics for meta-recursive analysis
    pub fn calculate_utilization_metrics(&self) -> f32 {
        if self.attractors.is_empty() {
            return 0.0;
        }

        let total_strength: f32 = self.attractors.iter().map(|(_, a)| a.strength).sum();
        let avg_strength = total_strength / self.attractors.len() as f32;
        let active_attractors = self
            .attractors
            .iter()
            .filter(|(_, a)| a.strength > 0.1)
            .count();

        (avg_strength + (active_attractors as f32 / self.attractors.len() as f32)) / 2.0
    }

    /// Calculate memory efficiency metrics for meta-recursive analysis
    pub fn calculate_efficiency_metrics(&self) -> f32 {
        if self.attractors.is_empty() {
            return 1.0;
        }

        // Efficiency based on connection utilization and access patterns
        let well_connected = self
            .attractors
            .iter()
            .filter(|(_, a)| a.connections.len() > 0)
            .count();

        let recently_accessed = self
            .attractors
            .iter()
            .filter(|(_, a)| {
                let hours_since_access = (chrono::Utc::now() - a.last_accessed).num_hours();
                hours_since_access < 24
            })
            .count();

        let connection_ratio = well_connected as f32 / self.attractors.len() as f32;
        let access_ratio = recently_accessed as f32 / self.attractors.len() as f32;

        (connection_ratio + access_ratio) / 2.0
    }
}

/// Check if word is a common stop word
fn is_common_word(word: &str) -> bool {
    const COMMON_WORDS: &[&str] = &[
        "the", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by", "from", "as",
        "is", "was", "are", "were", "be", "been", "being", "have", "has", "had", "do", "does",
        "did", "will", "would", "could", "should", "may", "might", "must", "can", "this", "that",
        "these", "those",
    ];

    COMMON_WORDS.contains(&word)
}

// Memory Reconstruction Protocol Support Types

/// Memory fragment information for reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFragmentInfo {
    pub id: String,
    pub fragment_type: FragmentType,
    pub embedding: Vec<f32>,
    pub content: String,
    pub strength: f32,
    pub importance: f32,
    pub coherence: f32,
    pub age_hours: i64,
    pub access_count: u32,
    pub connections: Vec<String>,
    pub last_accessed: DateTime<Utc>,
}

/// Types of memory fragments
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FragmentType {
    Event,
    Concept,
    Procedure,
    Emotion,
    Context,
    Unknown,
}

impl FragmentType {
    /// Infer fragment type from content (simple heuristic)
    pub fn from_content(content: &str) -> Self {
        let content_lower = content.to_lowercase();

        if content_lower.contains("step")
            || content_lower.contains("then")
            || content_lower.contains("next")
        {
            FragmentType::Procedure
        } else if content_lower.contains("feel") || content_lower.contains("emotion") {
            FragmentType::Emotion
        } else if content_lower.contains("when") || content_lower.contains("happened") {
            FragmentType::Event
        } else if content_lower.contains("is a") || content_lower.contains("means") {
            FragmentType::Concept
        } else if content_lower.contains("context") || content_lower.contains("background") {
            FragmentType::Context
        } else {
            FragmentType::Unknown
        }
    }
}

/// Result of fragment adaptation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptationResult {
    pub strengthened_count: usize,
    pub weakened_count: usize,
    pub new_connections_count: usize,
}

/// Result of memory consolidation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationResult {
    pub strengthened_patterns: usize,
    pub pruned_fragments: usize,
    pub total_fragments: usize,
    pub average_strength: f32,
}

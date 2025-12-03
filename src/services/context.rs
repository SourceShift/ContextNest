use crate::error::ContextNestResult;
use crate::{
    context::{ContextLevel, ContextManager, ContextMetrics, Example, MemoryStrategy},
    error::{ContextNestError, Result},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use super::EmbeddingService; // GraphService temporarily disabled

/// Request for building context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRequest {
    pub id: String,
    pub query: String,
    pub content: Option<String>,
    pub level: Option<ContextLevel>,
    pub token_budget: Option<usize>,
    pub examples: Option<Vec<Example>>,
    pub memory_strategy: Option<MemoryStrategy>,
}

/// Response containing built context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextResponse {
    pub id: String,
    pub context: String,
    pub level: ContextLevel,
    pub tokens_used: usize,
    pub metrics: ContextMetrics,
    pub suggestions: Vec<ContextSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSuggestion {
    pub suggestion_type: SuggestionType,
    pub message: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SuggestionType {
    Enhancement,
    Optimization,
    Pattern,
    Issue,
}

/// Context manager service orchestrating context operations
#[derive(Clone)]
pub struct ContextManagerService {
    managers: HashMap<String, ContextManager>,
    // graph_service: GraphService, // Temporarily disabled
    embedding_service: EmbeddingService,
    default_token_budget: usize,
}

impl ContextManagerService {
    pub fn new(embedding_service: EmbeddingService) -> Self {
        Self {
            managers: HashMap::new(),
            // graph_service, // Temporarily disabled
            embedding_service,
            default_token_budget: 4000,
        }
    }

    /// Build context for a request
    pub async fn build_context(
        &mut self,
        mut request: ContextRequest,
    ) -> ContextNestResult<ContextResponse> {
        let start_time = std::time::Instant::now();

        // Ensure request has an ID
        if request.id.is_empty() {
            request.id = Uuid::new_v4().to_string();
        }

        // Determine optimal context level if not specified
        let level = match &request.level {
            Some(level) => (*level).clone(),
            None => self.determine_optimal_level(&request).await?,
        };

        // Build context based on level
        let context = match level {
            ContextLevel::Atomic(ref instruction) => {
                self.build_atomic_context(&request, instruction).await?
            }
            ContextLevel::Molecular {
                ref instruction,
                ref examples,
            } => {
                self.build_molecular_context(&request, instruction, examples)
                    .await?
            }
            ContextLevel::Cellular {
                ref instruction,
                ref examples,
                ref memory,
            } => {
                self.build_cellular_context(&request, instruction, examples, memory)
                    .await?
            }
            ContextLevel::Organic { .. } => self.build_organic_context(&request).await?,
            ContextLevel::Field(ref field) => self.build_field_context(&request, field).await?,
            ContextLevel::Programmatic { ref field, .. } => {
                // Delegate to field context for programmatic level
                field.build_context()?
            }
            ContextLevel::ProtocolBased { ref field, .. } => {
                // Delegate to field context for protocol-based level
                field.build_context()?
            }
        };

        // Calculate metrics
        let tokens_used = self.estimate_tokens(&context);
        let _latency = start_time.elapsed();

        let mut metrics = ContextMetrics::default();
        metrics.total_tokens_used = tokens_used;

        // Generate suggestions
        let suggestions = self
            .generate_suggestions(&request, &context, &level)
            .await?;

        // Update manager metrics after building context
        if let Some(manager) = self.managers.get_mut(&request.id) {
            manager.update_metrics(tokens_used);
        }

        Ok(ContextResponse {
            id: request.id,
            context,
            level,
            tokens_used,
            metrics,
            suggestions,
        })
    }

    /// Enhance context level for existing session
    pub async fn enhance_context(&mut self, session_id: &str) -> ContextNestResult<ContextLevel> {
        let manager = self
            .managers
            .get_mut(session_id)
            .ok_or_else(|| ContextNestError::Api(format!("Session not found: {}", session_id)))?;

        let new_level = manager.enhance()?;
        Ok(new_level)
    }

    /// Get context manager for session, creating if needed
    async fn get_or_create_manager(
        &mut self,
        request: &ContextRequest,
    ) -> ContextNestResult<&mut ContextManager> {
        if !self.managers.contains_key(&request.id) {
            let token_budget = request.token_budget.unwrap_or(self.default_token_budget);
            let manager = ContextManager::new(token_budget);
            self.managers.insert(request.id.clone(), manager);
        }

        Ok(self.managers.get_mut(&request.id).unwrap())
    }

    /// Determine optimal context level based on request complexity
    async fn determine_optimal_level(
        &self,
        request: &ContextRequest,
    ) -> ContextNestResult<ContextLevel> {
        let complexity = self.analyze_complexity(request).await?;

        Ok(match complexity {
            ComplexityLevel::Simple => ContextLevel::Atomic(request.query.clone()),
            ComplexityLevel::Pattern => ContextLevel::Molecular {
                instruction: request.query.clone(),
                examples: request.examples.clone().unwrap_or_default(),
            },
            ComplexityLevel::Stateful => ContextLevel::Cellular {
                instruction: request.query.clone(),
                examples: request.examples.clone().unwrap_or_default(),
                memory: crate::context::MemoryCell {
                    strategy: request
                        .memory_strategy
                        .clone()
                        .unwrap_or(MemoryStrategy::Windowing { size: 10 }),
                    short_term: Vec::new(),
                    working: HashMap::new(),
                    long_term: Vec::new(),
                },
            },
            ComplexityLevel::Complex => ContextLevel::Organic {
                components: HashMap::new(),
                orchestrator: Box::new(ContextLevel::Cellular {
                    instruction: request.query.clone(),
                    examples: Vec::new(),
                    memory: crate::context::MemoryCell {
                        strategy: MemoryStrategy::KeyValue,
                        short_term: Vec::new(),
                        working: HashMap::new(),
                        long_term: Vec::new(),
                    },
                }),
            },
            ComplexityLevel::Semantic => {
                let field = crate::context::field::NeuralField::new();
                ContextLevel::Field(field)
            }
        })
    }

    /// Analyze request complexity
    async fn analyze_complexity(
        &self,
        request: &ContextRequest,
    ) -> ContextNestResult<ComplexityLevel> {
        let query_length = request.query.len();
        let has_content = request.content.is_some();
        let has_examples = request.examples.as_ref().map_or(false, |e| !e.is_empty());

        // Simple heuristic - can be enhanced with ML models
        if query_length < 50 && !has_content {
            Ok(ComplexityLevel::Simple)
        } else if query_length < 200 && has_examples {
            Ok(ComplexityLevel::Pattern)
        } else if has_content || query_length > 200 {
            Ok(ComplexityLevel::Stateful)
        } else {
            Ok(ComplexityLevel::Pattern)
        }
    }

    /// Build atomic context (simple instruction)
    async fn build_atomic_context(
        &self,
        request: &ContextRequest,
        instruction: &str,
    ) -> ContextNestResult<String> {
        let mut context = instruction.to_string();

        if let Some(content) = &request.content {
            context.push_str(&format!("\n\nContent: {}", content));
        }

        Ok(context)
    }

    /// Build molecular context (instruction + examples)
    async fn build_molecular_context(
        &self,
        request: &ContextRequest,
        instruction: &str,
        examples: &[Example],
    ) -> ContextNestResult<String> {
        let mut context = instruction.to_string();

        // Add examples if available
        if !examples.is_empty() {
            context.push_str("\n\nExamples:");
            for (i, example) in examples.iter().enumerate() {
                context.push_str(&format!(
                    "\n\n{}. Input: {}\n   Output: {}",
                    i + 1,
                    example.input,
                    example.output
                ));
                if let Some(reasoning) = &example.reasoning {
                    context.push_str(&format!("\n   Reasoning: {}", reasoning));
                }
            }
        }

        // Add relevant patterns from graph
        if let Some(content) = &request.content {
            let similar_patterns = self.find_similar_patterns(content).await?;
            if !similar_patterns.is_empty() {
                context.push_str("\n\nSimilar Patterns:");
                for pattern in similar_patterns.iter().take(3) {
                    context.push_str(&format!("\n- {}", pattern));
                }
            }
        }

        if let Some(content) = &request.content {
            context.push_str(&format!("\n\nCurrent: {}", content));
        }

        Ok(context)
    }

    /// Build cellular context (with memory)
    async fn build_cellular_context(
        &self,
        request: &ContextRequest,
        instruction: &str,
        examples: &[Example],
        memory: &crate::context::MemoryCell,
    ) -> ContextNestResult<String> {
        let mut context = self
            .build_molecular_context(request, instruction, examples)
            .await?;

        // Add memory context
        if !memory.short_term.is_empty() {
            context.push_str("\n\nRecent Context:");
            for item in memory.short_term.iter().take(5) {
                context.push_str(&format!("\n- {}", item));
            }
        }

        if !memory.working.is_empty() {
            context.push_str("\n\nKey Facts:");
            for (key, value) in &memory.working {
                context.push_str(&format!("\n- {}: {}", key, value));
            }
        }

        if !memory.long_term.is_empty() {
            context.push_str("\n\nBackground:");
            for item in memory.long_term.iter().take(3) {
                context.push_str(&format!("\n- {}", item));
            }
        }

        Ok(context)
    }

    /// Build organic context (multi-component)
    async fn build_organic_context(&self, request: &ContextRequest) -> ContextNestResult<String> {
        // Simplified implementation - delegate to orchestrator
        let orchestrator_request = ContextRequest {
            id: format!("{}_orchestrator", request.id),
            query: format!("Orchestrate: {}", request.query),
            content: request.content.clone(),
            level: Some(ContextLevel::Cellular {
                instruction: "Coordinate multiple components".to_string(),
                examples: Vec::new(),
                memory: crate::context::MemoryCell {
                    strategy: MemoryStrategy::KeyValue,
                    short_term: Vec::new(),
                    working: HashMap::new(),
                    long_term: Vec::new(),
                },
            }),
            token_budget: request.token_budget,
            examples: None,
            memory_strategy: None,
        };

        self.build_cellular_context(
            &orchestrator_request,
            &orchestrator_request.query,
            &[],
            &crate::context::MemoryCell {
                strategy: MemoryStrategy::KeyValue,
                short_term: Vec::new(),
                working: HashMap::new(),
                long_term: Vec::new(),
            },
        )
        .await
    }

    /// Build neural field context
    async fn build_field_context(
        &self,
        _request: &ContextRequest,
        field: &crate::context::field::NeuralField,
    ) -> ContextNestResult<String> {
        field.build_context()
    }

    /// Find similar patterns in the graph
    async fn find_similar_patterns(&self, content: &str) -> ContextNestResult<Vec<String>> {
        // Generate embedding for content
        let embedding = self.embedding_service.generate_embedding(content).await?;

        // Search for similar widgets (temporarily disabled - graph service not available)
        // let similar_widgets = self
        //     .graph_service
        //     .find_similar_widgets(&embedding, 5)
        //     .await?;

        // Extract patterns (using placeholder for now)
        let patterns = vec![
            "Container: Generic container widget".to_string(),
            "Text: Text display widget".to_string(),
            "Button: Interactive button widget".to_string(),
        ];

        Ok(patterns)
    }

    /// Generate context suggestions
    async fn generate_suggestions(
        &self,
        request: &ContextRequest,
        context: &str,
        level: &ContextLevel,
    ) -> ContextNestResult<Vec<ContextSuggestion>> {
        let mut suggestions = Vec::new();

        // Suggest enhancement if context is simple
        if matches!(level, ContextLevel::Atomic(_)) && context.len() > 100 {
            suggestions.push(ContextSuggestion {
                suggestion_type: SuggestionType::Enhancement,
                message: "Consider enhancing to molecular level for better pattern recognition"
                    .to_string(),
                confidence: 0.8,
            });
        }

        // Suggest optimization if token usage is high
        let tokens = self.estimate_tokens(context);
        if tokens > 3000 {
            suggestions.push(ContextSuggestion {
                suggestion_type: SuggestionType::Optimization,
                message: "Consider using summarization memory strategy to reduce token usage"
                    .to_string(),
                confidence: 0.9,
            });
        }

        // Check for potential issues
        if let Some(content) = &request.content {
            if content.contains("error") || content.contains("exception") {
                suggestions.push(ContextSuggestion {
                    suggestion_type: SuggestionType::Issue,
                    message: "Error detected in content - consider checking historical issues"
                        .to_string(),
                    confidence: 0.7,
                });
            }
        }

        Ok(suggestions)
    }

    /// Estimate token count (simplified)
    fn estimate_tokens(&self, text: &str) -> usize {
        // Rough approximation: 1 token ≈ 4 characters
        text.len() / 4
    }
}

#[derive(Debug, Clone)]
enum ComplexityLevel {
    Simple,
    Pattern,
    Stateful,
    Complex,
    Semantic,
}

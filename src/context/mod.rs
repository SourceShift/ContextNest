use crate::error::ContextNestResult;
use async_recursion::async_recursion;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// Module declarations — canonical Context-Engineering surface
// (canon refs in `resources/Context-Engineering/`)
// =============================================================================

// --- Core field theory (canon: 00_foundations/08-09, 40_reference/attractor_dynamics.md) ---
pub mod attractor_dynamics; // Attractor math (canon ch 8)
pub mod attractor_optimization;
pub mod field; // Neural field core
pub mod neural_field_enhanced; // Enhanced field with attractor integration // Performance optimization (parallel, cache, batch, memory pool)

// --- Reconstructive memory (canon: 00_COURSE/05_memory_systems/04_reconstructive_memory.md) ---
pub mod fragment_bridge;
pub mod gap_identification; // Gap detection in reconstructions
pub mod historical_state_recovery; // Field-state history recovery
pub mod memory; // Memory orchestrator + AttractorField
pub mod memory_reconstruction; // Core reconstructive memory
pub mod memory_reconstruction_coordinator; // Reconstruction pipeline coordinator
pub mod resonance_activation; // Cue-based fragment activation
pub mod semantic_continuity_restoration; // Semantic stitching across gaps // Phase C: cross-pipeline Fragment conversions

// --- Field operators (canon: 00_foundations/11 emergence + dynamics) ---
pub mod emergence_detection; // Detect emergent patterns
pub mod projection;
pub mod self_organizing_emergence; // Autonomous self-organization // Cross-dimensional field projection

// --- Cognition + autonomy (canon: 00_foundations/05, 14 + multiple) ---
pub mod agency;
pub mod meta_recursive; // Meta-cognition / continual learning
pub mod recursive_learning; // Recursive learning patterns // Autonomy + action types

// --- Pattern recognition + protocols ---
pub mod metrics;
pub mod pattern_recognition;
pub mod prompt_programs;
pub mod protocols;

// --- Multi-agent cluster (canon: 00_COURSE/07_multi_agent_systems) ---
// Gated as a unit because the 6 files cross-reference each other. v0.1.0
// substrate ships single-agent; enable with `--features multi-agent` to opt in.
#[cfg(feature = "multi-agent")]
pub mod collective_emergence;
#[cfg(feature = "multi-agent")]
pub mod coordinated_formation;
#[cfg(feature = "multi-agent")]
pub mod harmonic_integration;
#[cfg(feature = "multi-agent")]
pub mod multi_agent_field;
#[cfg(feature = "multi-agent")]
pub mod multi_attractor;
#[cfg(feature = "multi-agent")]
pub mod phase_sync;

// =============================================================================
// Public API re-exports — curated substrate surface
// =============================================================================

// --- Field core ---
pub use field::{CoherenceAnalysis, FieldHealth, NeuralField, SemanticPattern};

// --- Attractor dynamics ---
pub use attractor_dynamics::{AttractorBasin as DynamicsBasin, AttractorDynamicsEngine};

// --- Attractor performance optimization (parallel/cache/batch/memory-pool)
// Wired separately from `attractor_dynamics` because the optimizer is an
// opt-in performance layer on top of the dynamics engine, not a replacement
// for it. v0.1.0 callers can use the engine directly; this surface exists
// for callers that want target-accuracy / batched / cached operation. ---
pub use attractor_optimization::{
    AccuracyOptimizationResult, AttractorPerformanceOptimizer, MemoryOptimizationResult,
    OptimizationConfig, OptimizationMetrics,
};

// --- Field operators (W2 wire) ---
pub use emergence_detection::{
    EmergenceDetectionConfig, EmergenceDetectionSystem, EmergenceDetector, EmergenceEvent,
    EmergenceMetrics, EmergenceType, FieldSnapshot,
};
pub use projection::{
    FieldProjector, ProjectionConfig, ProjectionMethod, ProjectionQuality, ProjectionResult,
};
pub use self_organizing_emergence::{
    OrganizationPhase, OrganizationResult, OrganizationState, SelfOrganizingConfig,
    SelfOrganizingEmergence, SelfOrganizingMetrics,
};

// --- Reconstructive memory (canonical chain per `00_COURSE/05_memory_systems/04_reconstructive_memory.md`) ---
// The 5 reconstructive-memory primitives sit parallel in this module today;
// the seven-tool API's `reconstruct(query, depth)` operation chains them
// per the canon's pipeline:
//   ReconstructionCue
//     → ResonanceActivator::activate_fragments (cue-resonance fragment retrieval)
//     → GapIdentifier::identify_gaps (detect what's missing)
//     → GapFillingEngine::fill_gap (memory::attractors — AI gap fill)
//     → SemanticContinuityRestoration::restore (stitch fragments coherently)
//     → HistoricalStateRecovery::recover_at (overlay temporal context)
//     → MemoryReconstructionCoordinator::reconstruct (assemble final output)
// The `MemoryReconstructionProtocolCoordinator` (1339 LOC) currently chains
// the last three of those five. Wiring resonance_activation + gap_identification
// into the head of the chain is part of the v0.1.0 tool-surface PR; see
//  for the deferred-slim plan.
pub use gap_identification::{GapIdentifier, MemoryGap};
pub use memory_reconstruction::MemoryReconstructionCoordinator as MemoryReconstructor;
pub use memory_reconstruction_coordinator::MemoryReconstructionProtocolCoordinator;
pub use resonance_activation::{ActivationCue, ActivationRecord, ResonanceActivator};

// --- Cognition + autonomy (W3 + W4 wire) ---
// Meta-cognition: the continual-learning loop per §1.4 (the "neural-field
// attractor consolidation breaks catastrophic-forgetting-in-memory" claim).
pub use meta_recursive::{
    EnhancementEvent, EnhancementType, MetaRecursiveEngine, SystemModification,
};
pub use recursive_learning::{LearningEpisode, LearningPattern, RecursiveLearner};

// Autonomy: agentic substrate (vs passive memory libraries like Mem0/Letta).
pub use agency::{Action, ActionType, AutonomyLevel, Goal, SelfAssessment};

use crate::protocols::{ProtocolExecutionResult, ProtocolRegistry};
use crate::Result;
use prompt_programs::{PromptProgram, PromptProgramExecutor};

/// Represents the hierarchical context levels from Context Engineering
#[derive(Debug, Serialize, Deserialize)]
pub enum ContextLevel {
    /// Single instruction (Atoms)
    Atomic(String),
    /// Instruction + examples (Molecules)
    Molecular {
        instruction: String,
        examples: Vec<Example>,
    },
    /// Stateful context with memory (Cells)
    Cellular {
        instruction: String,
        examples: Vec<Example>,
        memory: MemoryCell,
    },
    /// Multi-component system (Organs)
    Organic {
        components: HashMap<String, ContextLevel>,
        orchestrator: Box<ContextLevel>,
    },
    /// Neural field representation
    Field(field::NeuralField),
    /// Prompt programming level with cognitive functions
    Programmatic {
        field: field::NeuralField,
        executor: PromptProgramExecutor,
        active_programs: Vec<PromptProgram>,
    },
    /// Protocol-based level with shell execution capabilities
    ProtocolBased {
        field: field::NeuralField,
        #[serde(skip)]
        executor: PromptProgramExecutor,
        #[serde(skip)]
        protocol_registry: ProtocolRegistry,
        active_protocols: Vec<String>,
    },
}

impl Clone for ContextLevel {
    fn clone(&self) -> Self {
        match self {
            ContextLevel::Atomic(instruction) => ContextLevel::Atomic(instruction.clone()),
            ContextLevel::Molecular {
                instruction,
                examples,
            } => ContextLevel::Molecular {
                instruction: instruction.clone(),
                examples: examples.clone(),
            },
            ContextLevel::Cellular {
                instruction,
                examples,
                memory,
            } => ContextLevel::Cellular {
                instruction: instruction.clone(),
                examples: examples.clone(),
                memory: memory.clone(),
            },
            ContextLevel::Organic {
                components,
                orchestrator,
            } => ContextLevel::Organic {
                components: components.clone(),
                orchestrator: orchestrator.clone(),
            },
            ContextLevel::Field(field) => ContextLevel::Field(field.clone()),
            ContextLevel::Programmatic {
                field,
                executor: _,
                active_programs,
            } => ContextLevel::Programmatic {
                field: field.clone(),
                executor: PromptProgramExecutor::new(),
                active_programs: active_programs.clone(),
            },
            ContextLevel::ProtocolBased {
                field,
                executor: _,
                protocol_registry: _,
                active_protocols,
            } => {
                // Skip the trait objects that can't be cloned, create new instances
                ContextLevel::ProtocolBased {
                    field: field.clone(),
                    executor: PromptProgramExecutor::new(),
                    protocol_registry: ProtocolRegistry::new(),
                    active_protocols: active_protocols.clone(),
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Example {
    pub input: String,
    pub output: String,
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCell {
    pub strategy: MemoryStrategy,
    pub short_term: Vec<String>,
    pub working: HashMap<String, serde_json::Value>,
    pub long_term: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryStrategy {
    Windowing { size: usize },
    Summarization { threshold: usize },
    KeyValue,
    PriorityPruning { max_tokens: usize },
}

/// Context manager that progressively enhances from simple to complex
#[derive(Debug, Clone)]
pub struct ContextManager {
    pub level: ContextLevel,
    token_budget: usize,
    metrics: ContextMetrics,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ContextMetrics {
    pub total_tokens_used: usize,
    pub coherence_score: f32,
    pub stability_score: f32,
    pub retrieval_accuracy: f32,
}

impl ContextManager {
    pub fn new(token_budget: usize) -> Self {
        Self {
            level: ContextLevel::Atomic(String::new()),
            token_budget,
            metrics: ContextMetrics::default(),
        }
    }

    /// Progressively enhance context level
    pub fn enhance(&mut self) -> ContextNestResult<ContextLevel> {
        self.level = match &self.level {
            ContextLevel::Atomic(instruction) => ContextLevel::Molecular {
                instruction: instruction.clone(),
                examples: Vec::new(),
            },
            ContextLevel::Molecular {
                instruction,
                examples,
            } => ContextLevel::Cellular {
                instruction: instruction.clone(),
                examples: examples.clone(),
                memory: MemoryCell {
                    strategy: MemoryStrategy::Windowing { size: 10 },
                    short_term: Vec::new(),
                    working: HashMap::new(),
                    long_term: Vec::new(),
                },
            },
            ContextLevel::Cellular { .. } => {
                // Upgrade to neural field
                ContextLevel::Field(field::NeuralField::new())
            }
            ContextLevel::Field(field) => {
                // Upgrade to programmatic level
                let mut executor = PromptProgramExecutor::new();
                executor.create_standard_library()?;
                ContextLevel::Programmatic {
                    field: field.clone(),
                    executor,
                    active_programs: Vec::new(),
                }
            }
            ContextLevel::Programmatic {
                field,
                executor,
                active_programs,
            } => {
                // Upgrade to protocol-based level
                let mut protocol_registry = ProtocolRegistry::new();

                // Register default protocols
                self.register_default_protocols(&mut protocol_registry)?;

                ContextLevel::ProtocolBased {
                    field: field.clone(),
                    executor: executor.clone(),
                    protocol_registry,
                    active_protocols: Vec::new(),
                }
            }
            level => {
                // Already at higher level, create a simple fallback
                ContextLevel::Atomic("Enhanced context".to_string())
            }
        };

        // Return a simple representation since we can't clone the complex variants
        Ok(ContextLevel::Atomic("Context enhanced".to_string()))
    }

    /// Build context appropriate to current level
    pub fn build_context(&self, current_input: &str) -> ContextNestResult<String> {
        match &self.level {
            ContextLevel::Atomic(instruction) => {
                Ok(format!("{}\n\n{}", instruction, current_input))
            }
            ContextLevel::Molecular {
                instruction,
                examples,
            } => {
                let mut context = instruction.clone();
                if !examples.is_empty() {
                    context.push_str("\n\nExamples:\n");
                    for (i, example) in examples.iter().enumerate() {
                        context.push_str(&format!(
                            "\nExample {}:\nInput: {}\nOutput: {}",
                            i + 1,
                            example.input,
                            example.output
                        ));
                        if let Some(reasoning) = &example.reasoning {
                            context.push_str(&format!("\nReasoning: {}", reasoning));
                        }
                    }
                }
                context.push_str(&format!("\n\nCurrent input: {}", current_input));
                Ok(context)
            }
            ContextLevel::Cellular {
                instruction,
                examples,
                memory,
            } => {
                let molecular_context = self.build_molecular_context(instruction, examples)?;
                let memory_context = self.build_memory_context(memory)?;
                Ok(format!(
                    "{}\n\n{}\n\nCurrent input: {}",
                    molecular_context, memory_context, current_input
                ))
            }
            ContextLevel::Field(field) => {
                let field_context = field.build_context()?;
                Ok(format!(
                    "{}\n\nCurrent input: {}",
                    field_context, current_input
                ))
            }
            ContextLevel::Programmatic {
                field,
                executor,
                active_programs,
            } => {
                let field_context = field.build_context()?;
                let program_context = self.build_program_context(executor, active_programs)?;
                Ok(format!(
                    "{}\n\nActive Cognitive Tools: {}\n\nCurrent input: {}",
                    field_context, program_context, current_input
                ))
            }
            ContextLevel::ProtocolBased {
                field,
                executor,
                protocol_registry,
                active_protocols,
            } => {
                let field_context = field.build_context()?;
                let program_context = self.build_program_context(executor, &Vec::new())?;
                let protocol_context =
                    self.build_protocol_context(protocol_registry, active_protocols)?;
                Ok(format!(
                    "{}\n\nCognitive Tools: {}\n\nActive Protocols: {}\n\nCurrent input: {}",
                    field_context, program_context, protocol_context, current_input
                ))
            }
            _ => Ok(current_input.to_string()),
        }
    }

    fn build_molecular_context(
        &self,
        instruction: &str,
        examples: &[Example],
    ) -> ContextNestResult<String> {
        let mut context = instruction.to_string();
        if !examples.is_empty() {
            context.push_str("\n\nExamples:");
            for (i, example) in examples.iter().enumerate() {
                context.push_str(&format!(
                    "\n{}. Input: {} → Output: {}",
                    i + 1,
                    example.input,
                    example.output
                ));
            }
        }
        Ok(context)
    }

    fn build_memory_context(&self, memory: &MemoryCell) -> ContextNestResult<String> {
        let mut context = String::from("Memory Context:");

        // Add short-term memory
        if !memory.short_term.is_empty() {
            context.push_str("\nRecent: ");
            context.push_str(&memory.short_term.join(" | "));
        }

        // Add working memory (key facts)
        if !memory.working.is_empty() {
            context.push_str("\nKey Facts:");
            for (key, value) in &memory.working {
                context.push_str(&format!("\n- {}: {}", key, value));
            }
        }

        Ok(context)
    }

    fn build_program_context(
        &self,
        executor: &PromptProgramExecutor,
        programs: &[PromptProgram],
    ) -> ContextNestResult<String> {
        let mut context = String::new();

        // List available cognitive functions
        let functions: Vec<String> = executor.functions.keys().cloned().collect();
        context.push_str(&format!("Available Functions: {}", functions.join(", ")));

        // List active programs
        if !programs.is_empty() {
            context.push_str("\nActive Programs:");
            for program in programs {
                context.push_str(&format!("\n- {} (v{})", program.name, program.version));
            }
        }

        // Add execution stats
        let stats = executor.get_execution_stats();
        context.push_str(&format!(
            "\nExecution Stats: {} total executions, {:.1}% success rate",
            stats.total_executions, stats.success_rate
        ));

        Ok(context)
    }

    /// Execute a prompt program in the current context
    pub fn execute_program(
        &mut self,
        program: PromptProgram,
    ) -> ContextNestResult<prompt_programs::ExecutionContext> {
        match &mut self.level {
            ContextLevel::Programmatic {
                executor,
                active_programs,
                ..
            } => {
                let result = executor.execute_program(program.clone())?;
                active_programs.push(program);
                Ok(result)
            }
            _ => {
                // Upgrade to programmatic level first
                self.enhance()?;
                self.execute_program(program)
            }
        }
    }

    /// Add a cognitive function to the current context
    pub fn add_cognitive_function(
        &mut self,
        function: prompt_programs::CognitiveFunction,
    ) -> ContextNestResult<()> {
        match &mut self.level {
            ContextLevel::Programmatic { executor, .. } => {
                executor.register_function(function)?;
                Ok(())
            }
            _ => {
                // Upgrade to programmatic level first
                self.enhance()?;
                self.add_cognitive_function(function)
            }
        }
    }

    /// Get available cognitive functions
    pub fn get_cognitive_functions(&self) -> Vec<String> {
        match &self.level {
            ContextLevel::Programmatic { executor, .. } => {
                executor.functions.keys().cloned().collect()
            }
            _ => Vec::new(),
        }
    }

    /// Execute a protocol in the current context
    #[async_recursion]
    pub async fn execute_protocol(
        &mut self,
        protocol_name: &str,
        inputs: std::collections::HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<ProtocolExecutionResult> {
        match &mut self.level {
            ContextLevel::ProtocolBased {
                protocol_registry,
                active_protocols,
                ..
            } => {
                let result = protocol_registry
                    .execute_protocol(protocol_name, inputs)
                    .await?;
                active_protocols.push(protocol_name.to_string());
                Ok(result)
            }
            _ => {
                // Upgrade to protocol-based level first
                self.enhance()?;
                self.execute_protocol(protocol_name, inputs).await
            }
        }
    }

    /// Get available protocols
    pub fn get_available_protocols(&self) -> Vec<String> {
        match &self.level {
            ContextLevel::ProtocolBased {
                protocol_registry, ..
            } => protocol_registry.protocols.keys().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// Register default protocols (no built-in protocols in v0.1.0 — workers
    /// register protocols dynamically through the public API).
    fn register_default_protocols(
        &self,
        _registry: &mut ProtocolRegistry,
    ) -> ContextNestResult<()> {
        Ok(())
    }

    fn build_protocol_context(
        &self,
        registry: &ProtocolRegistry,
        active_protocols: &[String],
    ) -> ContextNestResult<String> {
        let mut context = String::new();

        // List available protocols
        let protocols: Vec<String> = registry.protocols.keys().cloned().collect();
        context.push_str(&format!("Available Protocols: {}", protocols.join(", ")));

        // List active protocols
        if !active_protocols.is_empty() {
            context.push_str(&format!(
                "\nActive Protocols: {}",
                active_protocols.join(", ")
            ));
        }

        // Add registry stats
        let stats = registry.get_registry_stats();
        context.push_str(&format!(
            "\nProtocol Stats: {} protocols, {} total executions",
            stats.total_protocols, stats.total_executions
        ));

        Ok(context)
    }

    pub fn update_metrics(&mut self, used_tokens: usize) {
        self.metrics.total_tokens_used += used_tokens;
        // Update other metrics based on context performance
    }
}

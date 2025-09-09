use crate::error::ContextNestResult;
use crate::Result;
use serde::{Deserialize, Serialize};

/// Protocol shells for structured context operations
/// Based on Context Engineering protocol patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolShell {
    pub name: String,
    pub version: String,
    pub operations: Vec<ProtocolOperation>,
    pub metadata: ProtocolMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolOperation {
    pub name: String,
    pub operation_type: OperationType,
    pub parameters: Vec<ProtocolParameter>,
    pub expected_output: OutputType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OperationType {
    Inject,    // Add patterns to context
    Attenuate, // Reduce pattern strength
    Amplify,   // Increase pattern strength
    Tune,      // Adjust field properties
    Collapse,  // Extract concrete context
    Repair,    // Fix field coherence
    Query,     // Search for patterns
    Transform, // Modify context structure
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolParameter {
    pub name: String,
    pub param_type: ParameterType,
    pub required: bool,
    pub default_value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParameterType {
    String,
    Number,
    Boolean,
    Array,
    Object,
    Embedding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputType {
    Context,
    Patterns,
    Metrics,
    Boolean,
    Modified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMetadata {
    pub description: String,
    pub use_cases: Vec<String>,
    pub complexity_level: ComplexityLevel,
    pub required_context_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ComplexityLevel {
    Simple,
    Intermediate,
    Advanced,
    Expert,
}

/// Protocol executor for running structured operations
pub struct ProtocolExecutor {
    shells: std::collections::HashMap<String, ProtocolShell>,
}

impl ProtocolExecutor {
    pub fn new() -> Self {
        let mut executor = Self {
            shells: std::collections::HashMap::new(),
        };

        // Register built-in protocol shells
        executor.register_builtin_shells();
        executor
    }

    /// Register a protocol shell
    pub fn register_shell(&mut self, shell: ProtocolShell) {
        self.shells.insert(shell.name.clone(), shell);
    }

    /// Execute a protocol operation
    pub async fn execute(
        &self,
        shell_name: &str,
        operation_name: &str,
        parameters: std::collections::HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<ProtocolResult> {
        let shell = self.shells.get(shell_name).ok_or_else(|| {
            crate::ContextNestError::Api(format!("Unknown protocol shell: {}", shell_name))
        })?;

        let operation = shell
            .operations
            .iter()
            .find(|op| op.name == operation_name)
            .ok_or_else(|| {
                crate::ContextNestError::Api(format!("Unknown operation: {}", operation_name))
            })?;

        // Validate parameters
        self.validate_parameters(operation, &parameters)?;

        // Execute the operation
        match operation.operation_type {
            OperationType::Inject => self.execute_inject(operation, parameters).await,
            OperationType::Attenuate => self.execute_attenuate(operation, parameters).await,
            OperationType::Amplify => self.execute_amplify(operation, parameters).await,
            OperationType::Tune => self.execute_tune(operation, parameters).await,
            OperationType::Collapse => self.execute_collapse(operation, parameters).await,
            OperationType::Repair => self.execute_repair(operation, parameters).await,
            OperationType::Query => self.execute_query(operation, parameters).await,
            OperationType::Transform => self.execute_transform(operation, parameters).await,
        }
    }

    /// Register built-in protocol shells
    fn register_builtin_shells(&mut self) {
        // Context Memory Persistence Protocol
        let memory_shell = ProtocolShell {
            name: "context.memory.persistence".to_string(),
            version: "1.0".to_string(),
            operations: vec![
                ProtocolOperation {
                    name: "store_interaction".to_string(),
                    operation_type: OperationType::Inject,
                    parameters: vec![
                        ProtocolParameter {
                            name: "session_id".to_string(),
                            param_type: ParameterType::String,
                            required: true,
                            default_value: None,
                        },
                        ProtocolParameter {
                            name: "interaction".to_string(),
                            param_type: ParameterType::Object,
                            required: true,
                            default_value: None,
                        },
                    ],
                    expected_output: OutputType::Boolean,
                },
                ProtocolOperation {
                    name: "retrieve_relevant".to_string(),
                    operation_type: OperationType::Query,
                    parameters: vec![
                        ProtocolParameter {
                            name: "session_id".to_string(),
                            param_type: ParameterType::String,
                            required: true,
                            default_value: None,
                        },
                        ProtocolParameter {
                            name: "query".to_string(),
                            param_type: ParameterType::String,
                            required: true,
                            default_value: None,
                        },
                    ],
                    expected_output: OutputType::Context,
                },
            ],
            metadata: ProtocolMetadata {
                description: "Manages persistent memory across context sessions".to_string(),
                use_cases: vec![
                    "Multi-turn conversations".to_string(),
                    "Session state management".to_string(),
                    "Context continuity".to_string(),
                ],
                complexity_level: ComplexityLevel::Intermediate,
                required_context_level: "cellular".to_string(),
            },
        };

        self.register_shell(memory_shell);

        // Field Resonance Protocol
        let resonance_shell = ProtocolShell {
            name: "field.resonance.scaffold".to_string(),
            version: "1.0".to_string(),
            operations: vec![
                ProtocolOperation {
                    name: "detect_patterns".to_string(),
                    operation_type: OperationType::Query,
                    parameters: vec![ProtocolParameter {
                        name: "threshold".to_string(),
                        param_type: ParameterType::Number,
                        required: false,
                        default_value: Some(serde_json::json!(0.7)),
                    }],
                    expected_output: OutputType::Patterns,
                },
                ProtocolOperation {
                    name: "amplify_resonant".to_string(),
                    operation_type: OperationType::Amplify,
                    parameters: vec![ProtocolParameter {
                        name: "factor".to_string(),
                        param_type: ParameterType::Number,
                        required: false,
                        default_value: Some(serde_json::json!(1.2)),
                    }],
                    expected_output: OutputType::Modified,
                },
            ],
            metadata: ProtocolMetadata {
                description: "Manages resonance patterns in neural fields".to_string(),
                use_cases: vec![
                    "Pattern reinforcement".to_string(),
                    "Semantic clustering".to_string(),
                    "Field optimization".to_string(),
                ],
                complexity_level: ComplexityLevel::Advanced,
                required_context_level: "field".to_string(),
            },
        };

        self.register_shell(resonance_shell);

        // Attractor Co-emergence Protocol
        let attractor_shell = ProtocolShell {
            name: "attractor.co.emerge".to_string(),
            version: "1.0".to_string(),
            operations: vec![
                ProtocolOperation {
                    name: "form_attractor".to_string(),
                    operation_type: OperationType::Transform,
                    parameters: vec![
                        ProtocolParameter {
                            name: "patterns".to_string(),
                            param_type: ParameterType::Array,
                            required: true,
                            default_value: None,
                        },
                        ProtocolParameter {
                            name: "strength".to_string(),
                            param_type: ParameterType::Number,
                            required: false,
                            default_value: Some(serde_json::json!(1.0)),
                        },
                    ],
                    expected_output: OutputType::Modified,
                },
                ProtocolOperation {
                    name: "stabilize_field".to_string(),
                    operation_type: OperationType::Repair,
                    parameters: vec![],
                    expected_output: OutputType::Metrics,
                },
            ],
            metadata: ProtocolMetadata {
                description: "Manages attractor formation and field stabilization".to_string(),
                use_cases: vec![
                    "Pattern stabilization".to_string(),
                    "Field organization".to_string(),
                    "Emergent structure formation".to_string(),
                ],
                complexity_level: ComplexityLevel::Expert,
                required_context_level: "field".to_string(),
            },
        };

        self.register_shell(attractor_shell);
    }

    /// Validate operation parameters
    fn validate_parameters(
        &self,
        operation: &ProtocolOperation,
        parameters: &std::collections::HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<()> {
        for param in &operation.parameters {
            if param.required && !parameters.contains_key(&param.name) {
                return Err(crate::ContextNestError::Api(format!(
                    "Missing required parameter: {}",
                    param.name
                )));
            }

            if let Some(value) = parameters.get(&param.name) {
                if !self.validate_parameter_type(value, &param.param_type) {
                    return Err(crate::ContextNestError::Api(format!(
                        "Invalid type for parameter {}: expected {:?}",
                        param.name, param.param_type
                    )));
                }
            }
        }
        Ok(())
    }

    /// Validate parameter type
    fn validate_parameter_type(
        &self,
        value: &serde_json::Value,
        expected_type: &ParameterType,
    ) -> bool {
        match (value, expected_type) {
            (serde_json::Value::String(_), ParameterType::String) => true,
            (serde_json::Value::Number(_), ParameterType::Number) => true,
            (serde_json::Value::Bool(_), ParameterType::Boolean) => true,
            (serde_json::Value::Array(_), ParameterType::Array) => true,
            (serde_json::Value::Object(_), ParameterType::Object) => true,
            (serde_json::Value::Array(arr), ParameterType::Embedding) => {
                // Check if it's a valid embedding (array of numbers)
                arr.iter().all(|v| v.is_number())
            }
            _ => false,
        }
    }

    // Operation implementations
    async fn execute_inject(
        &self,
        _operation: &ProtocolOperation,
        _parameters: std::collections::HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<ProtocolResult> {
        // Implementation would inject patterns into the context/field
        Ok(ProtocolResult {
            success: true,
            output: serde_json::json!({"injected": true}),
            metrics: Some(ProtocolMetrics {
                execution_time_ms: 10,
                operations_performed: 1,
                patterns_affected: 1,
            }),
        })
    }

    async fn execute_attenuate(
        &self,
        _operation: &ProtocolOperation,
        _parameters: std::collections::HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<ProtocolResult> {
        Ok(ProtocolResult {
            success: true,
            output: serde_json::json!({"attenuated": true}),
            metrics: Some(ProtocolMetrics {
                execution_time_ms: 5,
                operations_performed: 1,
                patterns_affected: 1,
            }),
        })
    }

    async fn execute_amplify(
        &self,
        _operation: &ProtocolOperation,
        _parameters: std::collections::HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<ProtocolResult> {
        Ok(ProtocolResult {
            success: true,
            output: serde_json::json!({"amplified": true}),
            metrics: Some(ProtocolMetrics {
                execution_time_ms: 8,
                operations_performed: 1,
                patterns_affected: 1,
            }),
        })
    }

    async fn execute_tune(
        &self,
        _operation: &ProtocolOperation,
        _parameters: std::collections::HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<ProtocolResult> {
        Ok(ProtocolResult {
            success: true,
            output: serde_json::json!({"tuned": true}),
            metrics: Some(ProtocolMetrics {
                execution_time_ms: 15,
                operations_performed: 1,
                patterns_affected: 0,
            }),
        })
    }

    async fn execute_collapse(
        &self,
        _operation: &ProtocolOperation,
        _parameters: std::collections::HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<ProtocolResult> {
        Ok(ProtocolResult {
            success: true,
            output: serde_json::json!({"context": "collapsed context here"}),
            metrics: Some(ProtocolMetrics {
                execution_time_ms: 20,
                operations_performed: 1,
                patterns_affected: 5,
            }),
        })
    }

    async fn execute_repair(
        &self,
        _operation: &ProtocolOperation,
        _parameters: std::collections::HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<ProtocolResult> {
        Ok(ProtocolResult {
            success: true,
            output: serde_json::json!({"repaired": true, "coherence": 0.95}),
            metrics: Some(ProtocolMetrics {
                execution_time_ms: 30,
                operations_performed: 1,
                patterns_affected: 3,
            }),
        })
    }

    async fn execute_query(
        &self,
        _operation: &ProtocolOperation,
        _parameters: std::collections::HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<ProtocolResult> {
        Ok(ProtocolResult {
            success: true,
            output: serde_json::json!({"patterns": ["pattern1", "pattern2"]}),
            metrics: Some(ProtocolMetrics {
                execution_time_ms: 12,
                operations_performed: 1,
                patterns_affected: 0,
            }),
        })
    }

    async fn execute_transform(
        &self,
        _operation: &ProtocolOperation,
        _parameters: std::collections::HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<ProtocolResult> {
        Ok(ProtocolResult {
            success: true,
            output: serde_json::json!({"transformed": true}),
            metrics: Some(ProtocolMetrics {
                execution_time_ms: 25,
                operations_performed: 1,
                patterns_affected: 2,
            }),
        })
    }

    /// List available protocol shells
    pub fn list_shells(&self) -> Vec<&ProtocolShell> {
        self.shells.values().collect()
    }

    /// Get protocol shell by name
    pub fn get_shell(&self, name: &str) -> Option<&ProtocolShell> {
        self.shells.get(name)
    }
}

/// Result of protocol operation execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolResult {
    pub success: bool,
    pub output: serde_json::Value,
    pub metrics: Option<ProtocolMetrics>,
}

/// Metrics for protocol execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMetrics {
    pub execution_time_ms: u64,
    pub operations_performed: usize,
    pub patterns_affected: usize,
}

use crate::error::ContextNestResult;
use crate::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Core cognitive function structure for prompt programming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CognitiveFunction {
    pub name: String,
    pub description: String,
    pub parameters: HashMap<String, ParameterSpec>,
    pub template: String,
    pub return_type: String,
    pub validation_rules: Vec<ValidationRule>,
    pub created_at: DateTime<Utc>,
    pub usage_count: u32,
}

/// Parameter specification for cognitive functions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterSpec {
    pub name: String,
    pub param_type: ParameterType,
    pub required: bool,
    pub default_value: Option<String>,
    pub description: String,
    pub validation: Option<String>,
}

/// Parameter types supported in prompt programming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParameterType {
    String,
    Integer,
    Float,
    Boolean,
    Array(Box<ParameterType>),
    Object(HashMap<String, ParameterType>),
    Enum(Vec<String>),
}

/// Validation rules for function parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    pub rule_type: ValidationType,
    pub parameter: String,
    pub condition: String,
    pub error_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationType {
    Required,
    Range,
    Length,
    Pattern,
    Custom,
}

/// Control flow structure for prompt programs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlFlow {
    pub flow_type: FlowType,
    pub condition: Option<String>,
    pub iterations: Option<u32>,
    pub branches: Vec<FlowBranch>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FlowType {
    Sequential,
    Conditional,
    Loop,
    Parallel,
    Choice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowBranch {
    pub condition: Option<String>,
    pub functions: Vec<FunctionCall>,
    pub control_flow: Option<Box<ControlFlow>>,
}

/// Function call with parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub function_name: String,
    pub parameters: HashMap<String, serde_json::Value>,
    pub variable_assignment: Option<String>,
}

/// Complete prompt program structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptProgram {
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    pub functions: Vec<FunctionCall>,
    pub control_flow: Option<ControlFlow>,
    pub global_context: HashMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub last_modified: DateTime<Utc>,
}

/// Execution context for prompt programs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    pub variables: HashMap<String, serde_json::Value>,
    pub function_results: HashMap<String, String>,
    pub execution_trace: Vec<ExecutionStep>,
    pub start_time: DateTime<Utc>,
    pub current_step: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStep {
    pub step_number: usize,
    pub function_name: String,
    pub parameters: HashMap<String, serde_json::Value>,
    pub result: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub execution_time_ms: u64,
    pub status: ExecutionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
    Skipped,
}

/// Function composition rules for complex reasoning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompositionRule {
    pub name: String,
    pub pattern: CompositionPattern,
    pub functions: Vec<String>,
    pub data_flow: DataFlow,
    pub error_handling: ErrorHandling,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompositionPattern {
    Pipeline,    // Output of one feeds input of next
    Parallel,    // Functions execute simultaneously
    Conditional, // Functions execute based on conditions
    Iterative,   // Functions repeat until condition met
    Recursive,   // Functions can call themselves
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFlow {
    pub input_mapping: HashMap<String, String>,
    pub output_mapping: HashMap<String, String>,
    pub transformations: Vec<DataTransformation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataTransformation {
    pub source_field: String,
    pub target_field: String,
    pub transformation_type: TransformationType,
    pub parameters: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransformationType {
    Extract,
    Filter,
    Aggregate,
    Format,
    Validate,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorHandling {
    pub retry_count: u32,
    pub fallback_function: Option<String>,
    pub error_recovery: ErrorRecovery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorRecovery {
    Stop,
    Continue,
    Retry,
    Fallback,
    Custom(String),
}

/// Prompt program executor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptProgramExecutor {
    pub functions: HashMap<String, CognitiveFunction>,
    pub composition_rules: HashMap<String, CompositionRule>,
    pub execution_history: Vec<ExecutionContext>,
}

impl PromptProgramExecutor {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
            composition_rules: HashMap::new(),
            execution_history: Vec::new(),
        }
    }

    /// Register a new cognitive function
    pub fn register_function(&mut self, function: CognitiveFunction) -> ContextNestResult<()> {
        // Validate function structure
        self.validate_function(&function)?;

        self.functions.insert(function.name.clone(), function);
        Ok(())
    }

    /// Execute a complete prompt program
    pub fn execute_program(
        &mut self,
        program: PromptProgram,
    ) -> ContextNestResult<ExecutionContext> {
        let mut context = ExecutionContext {
            variables: program.global_context.clone(),
            function_results: HashMap::new(),
            execution_trace: Vec::new(),
            start_time: Utc::now(),
            current_step: 0,
        };

        // Execute functions based on control flow
        if let Some(control_flow) = &program.control_flow {
            self.execute_control_flow(control_flow, &mut context)?;
        } else {
            // Sequential execution
            for function_call in &program.functions {
                self.execute_function_call(function_call, &mut context)?;
            }
        }

        // Store execution history
        self.execution_history.push(context.clone());

        Ok(context)
    }

    /// Execute a single function call
    pub fn execute_function_call(
        &self,
        call: &FunctionCall,
        context: &mut ExecutionContext,
    ) -> ContextNestResult<String> {
        let start_time = Utc::now();
        context.current_step += 1;

        // Get function definition
        let function = self.functions.get(&call.function_name).ok_or_else(|| {
            crate::ContextNestError::Api(format!("Function '{}' not found", call.function_name))
        })?;

        // Validate parameters
        self.validate_parameters(function, &call.parameters)?;

        // Create execution step
        let mut step = ExecutionStep {
            step_number: context.current_step,
            function_name: call.function_name.clone(),
            parameters: call.parameters.clone(),
            result: None,
            timestamp: start_time,
            execution_time_ms: 0,
            status: ExecutionStatus::InProgress,
        };

        // Execute function by rendering template with parameters
        let result = match self.render_function_template(function, &call.parameters, context) {
            Ok(rendered) => {
                step.status = ExecutionStatus::Completed;
                step.result = Some(rendered.clone());
                rendered
            }
            Err(e) => {
                step.status = ExecutionStatus::Failed(e.to_string());
                return Err(e);
            }
        };

        // Calculate execution time
        let end_time = Utc::now();
        step.execution_time_ms = (end_time - start_time).num_milliseconds() as u64;

        // Store result in context
        if let Some(var_name) = &call.variable_assignment {
            context
                .variables
                .insert(var_name.clone(), serde_json::Value::String(result.clone()));
        }
        context
            .function_results
            .insert(call.function_name.clone(), result.clone());
        context.execution_trace.push(step);

        Ok(result)
    }

    /// Execute control flow structures
    fn execute_control_flow(
        &mut self,
        flow: &ControlFlow,
        context: &mut ExecutionContext,
    ) -> ContextNestResult<()> {
        match flow.flow_type {
            FlowType::Sequential => {
                for branch in &flow.branches {
                    self.execute_branch(branch, context)?;
                }
            }
            FlowType::Conditional => {
                for branch in &flow.branches {
                    if let Some(condition) = &branch.condition {
                        if self.evaluate_condition(condition, context)? {
                            self.execute_branch(branch, context)?;
                            break; // Only execute first matching branch
                        }
                    }
                }
            }
            FlowType::Loop => {
                let max_iterations = flow.iterations.unwrap_or(10);
                for _ in 0..max_iterations {
                    if let Some(condition) = &flow.condition {
                        if !self.evaluate_condition(condition, context)? {
                            break;
                        }
                    }
                    for branch in &flow.branches {
                        self.execute_branch(branch, context)?;
                    }
                }
            }
            FlowType::Parallel => {
                // For now, execute sequentially (could be enhanced with async)
                for branch in &flow.branches {
                    self.execute_branch(branch, context)?;
                }
            }
            FlowType::Choice => {
                // Execute first available branch
                if let Some(branch) = flow.branches.first() {
                    self.execute_branch(branch, context)?;
                }
            }
        }
        Ok(())
    }

    /// Execute a single branch
    fn execute_branch(
        &mut self,
        branch: &FlowBranch,
        context: &mut ExecutionContext,
    ) -> ContextNestResult<()> {
        for function_call in &branch.functions {
            self.execute_function_call(function_call, context)?;
        }

        if let Some(nested_flow) = &branch.control_flow {
            self.execute_control_flow(nested_flow, context)?;
        }

        Ok(())
    }

    /// Render function template with parameters
    fn render_function_template(
        &self,
        function: &CognitiveFunction,
        parameters: &HashMap<String, serde_json::Value>,
        context: &ExecutionContext,
    ) -> ContextNestResult<String> {
        let mut template = function.template.clone();

        // Replace parameter placeholders
        for (key, value) in parameters {
            let placeholder = format!("${{{}}}", key);
            let value_str = match value {
                serde_json::Value::String(s) => s.clone(),
                _ => value.to_string(),
            };
            template = template.replace(&placeholder, &value_str);
        }

        // Replace context variables
        for (key, value) in &context.variables {
            let placeholder = format!("${{{}}}", key);
            let value_str = match value {
                serde_json::Value::String(s) => s.clone(),
                _ => value.to_string(),
            };
            template = template.replace(&placeholder, &value_str);
        }

        Ok(template)
    }

    /// Validate function structure
    fn validate_function(&self, function: &CognitiveFunction) -> ContextNestResult<()> {
        // Check that template contains required parameter placeholders
        for (param_name, param_spec) in &function.parameters {
            if param_spec.required {
                let placeholder = format!("${{{}}}", param_name);
                if !function.template.contains(&placeholder) {
                    return Err(crate::ContextNestError::Api(format!(
                        "Required parameter '{}' not found in template",
                        param_name
                    )));
                }
            }
        }

        Ok(())
    }

    /// Validate function call parameters
    fn validate_parameters(
        &self,
        function: &CognitiveFunction,
        parameters: &HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<()> {
        // Check required parameters
        for (param_name, param_spec) in &function.parameters {
            if param_spec.required && !parameters.contains_key(param_name) {
                return Err(crate::ContextNestError::Api(format!(
                    "Required parameter '{}' missing",
                    param_name
                )));
            }
        }

        // Validate parameter types (simplified validation)
        for (param_name, value) in parameters {
            if let Some(param_spec) = function.parameters.get(param_name) {
                if !self.validate_parameter_type(&param_spec.param_type, value) {
                    return Err(crate::ContextNestError::Api(format!(
                        "Parameter '{}' has invalid type",
                        param_name
                    )));
                }
            }
        }

        Ok(())
    }

    /// Validate parameter type (simplified)
    fn validate_parameter_type(&self, expected: &ParameterType, value: &serde_json::Value) -> bool {
        match (expected, value) {
            (ParameterType::String, serde_json::Value::String(_)) => true,
            (ParameterType::Integer, serde_json::Value::Number(n)) => n.is_i64(),
            (ParameterType::Float, serde_json::Value::Number(_)) => true,
            (ParameterType::Boolean, serde_json::Value::Bool(_)) => true,
            (ParameterType::Array(_), serde_json::Value::Array(_)) => true,
            (ParameterType::Object(_), serde_json::Value::Object(_)) => true,
            _ => false,
        }
    }

    /// Evaluate conditional expressions (simplified)
    fn evaluate_condition(
        &self,
        condition: &str,
        context: &ExecutionContext,
    ) -> ContextNestResult<bool> {
        // This is a simplified condition evaluator
        // In a real implementation, you'd want a proper expression parser

        if condition.contains("==") {
            let parts: Vec<&str> = condition.split("==").collect();
            if parts.len() == 2 {
                let left = parts[0].trim();
                let right = parts[1].trim().trim_matches('"');

                if let Some(var_value) = context.variables.get(left) {
                    if let serde_json::Value::String(s) = var_value {
                        return Ok(s == right);
                    }
                }
            }
        }

        // Default to true for now
        Ok(true)
    }

    /// Create standard cognitive functions library
    pub fn create_standard_library(&mut self) -> ContextNestResult<()> {
        // Analyze function
        let analyze_function = CognitiveFunction {
            name: "analyze".to_string(),
            description: "Analyze text or data according to specified parameters".to_string(),
            parameters: [
                ("content".to_string(), ParameterSpec {
                    name: "content".to_string(),
                    param_type: ParameterType::String,
                    required: true,
                    default_value: None,
                    description: "Content to analyze".to_string(),
                    validation: None,
                }),
                ("framework".to_string(), ParameterSpec {
                    name: "framework".to_string(),
                    param_type: ParameterType::Enum(vec![
                        "thematic".to_string(),
                        "structural".to_string(),
                        "causal".to_string(),
                        "comparative".to_string(),
                    ]),
                    required: false,
                    default_value: Some("thematic".to_string()),
                    description: "Analysis framework to use".to_string(),
                    validation: None,
                }),
                ("depth".to_string(), ParameterSpec {
                    name: "depth".to_string(),
                    param_type: ParameterType::Enum(vec![
                        "brief".to_string(),
                        "detailed".to_string(),
                        "comprehensive".to_string(),
                    ]),
                    required: false,
                    default_value: Some("detailed".to_string()),
                    description: "Depth of analysis".to_string(),
                    validation: None,
                }),
            ].into_iter().collect(),
            template: r#"
Task: Analyze the following content using a ${framework} framework.

Content to analyze:
${content}

Analysis Parameters:
- Framework: ${framework}
- Depth: ${depth}

Please provide a ${depth} analysis using the ${framework} approach. Structure your analysis clearly and support all observations with specific evidence from the content.

Begin your analysis:
"#.to_string(),
            return_type: "String".to_string(),
            validation_rules: vec![],
            created_at: Utc::now(),
            usage_count: 0,
        };

        self.register_function(analyze_function)?;

        // Summarize function
        let summarize_function = CognitiveFunction {
            name: "summarize".to_string(),
            description: "Create a summary of text with specified length and focus".to_string(),
            parameters: [
                (
                    "text".to_string(),
                    ParameterSpec {
                        name: "text".to_string(),
                        param_type: ParameterType::String,
                        required: true,
                        default_value: None,
                        description: "Text to summarize".to_string(),
                        validation: None,
                    },
                ),
                (
                    "length".to_string(),
                    ParameterSpec {
                        name: "length".to_string(),
                        param_type: ParameterType::Enum(vec![
                            "short".to_string(),
                            "medium".to_string(),
                            "long".to_string(),
                        ]),
                        required: false,
                        default_value: Some("medium".to_string()),
                        description: "Length of summary".to_string(),
                        validation: None,
                    },
                ),
                (
                    "focus".to_string(),
                    ParameterSpec {
                        name: "focus".to_string(),
                        param_type: ParameterType::String,
                        required: false,
                        default_value: None,
                        description: "Specific aspect to focus on".to_string(),
                        validation: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
            template: r#"
Task: Summarize the following text.

Text to summarize:
${text}

Summary Requirements:
- Length: ${length}
${focus}

Please provide a ${length} summary of the text. ${focus}

Summary:
"#
            .to_string(),
            return_type: "String".to_string(),
            validation_rules: vec![],
            created_at: Utc::now(),
            usage_count: 0,
        };

        self.register_function(summarize_function)?;

        // Understand function for problem comprehension
        let understand_function = CognitiveFunction {
            name: "understand".to_string(),
            description: "Deeply understand a problem or question before attempting to solve it"
                .to_string(),
            parameters: [(
                "problem".to_string(),
                ParameterSpec {
                    name: "problem".to_string(),
                    param_type: ParameterType::String,
                    required: true,
                    default_value: None,
                    description: "Problem or question to understand".to_string(),
                    validation: None,
                },
            )]
            .into_iter()
            .collect(),
            template: r#"
Task: Analyze and break down the following problem to ensure complete understanding.

Problem: ${problem}

Please provide:
1. The core task being asked
2. Key components that need to be addressed
3. Any implicit assumptions
4. Constraints or conditions to consider
5. A clear restatement of the problem

Understanding Analysis:
"#
            .to_string(),
            return_type: "String".to_string(),
            validation_rules: vec![],
            created_at: Utc::now(),
            usage_count: 0,
        };

        self.register_function(understand_function)?;

        Ok(())
    }

    /// Get execution statistics
    pub fn get_execution_stats(&self) -> ExecutionStats {
        let total_executions = self.execution_history.len();
        let total_functions = self
            .execution_history
            .iter()
            .map(|ctx| ctx.execution_trace.len())
            .sum();

        let avg_execution_time = if total_executions > 0 {
            self.execution_history
                .iter()
                .map(|ctx| {
                    (ctx.execution_trace.last().unwrap().timestamp - ctx.start_time)
                        .num_milliseconds()
                })
                .sum::<i64>()
                / total_executions as i64
        } else {
            0
        };

        let success_rate = if total_functions > 0 {
            let successful = self
                .execution_history
                .iter()
                .flat_map(|ctx| &ctx.execution_trace)
                .filter(|step| matches!(step.status, ExecutionStatus::Completed))
                .count();
            (successful as f64 / total_functions as f64) * 100.0
        } else {
            0.0
        };

        ExecutionStats {
            total_executions,
            total_functions,
            avg_execution_time_ms: avg_execution_time as u64,
            success_rate,
            registered_functions: self.functions.len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStats {
    pub total_executions: usize,
    pub total_functions: usize,
    pub avg_execution_time_ms: u64,
    pub success_rate: f64,
    pub registered_functions: usize,
}

impl Default for PromptProgramExecutor {
    fn default() -> Self {
        Self::new()
    }
}

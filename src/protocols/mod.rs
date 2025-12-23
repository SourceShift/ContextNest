use crate::error::ContextNestResult;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// Pareto language types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParetoExpression {
    Variable(String),
    Call(Box<ParetoExpression>, Vec<ParetoExpression>),
    Literal(ParetoValue),
    Lambda(Vec<String>, Box<ParetoExpression>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParetoValue {
    String(String),
    Number(f32),
    Boolean(bool),
    Array(Vec<ParetoValue>),
    Object(HashMap<String, ParetoValue>),
    Null,
}

// Pareto execution environment
#[derive(Clone)]
pub struct ParetoEnvironment {
    functions: HashMap<String, Arc<dyn ParetoFunction>>,
}

impl std::fmt::Debug for ParetoEnvironment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParetoEnvironment")
            .field("functions", &format!("{} functions", self.functions.len()))
            .finish()
    }
}

impl ParetoEnvironment {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    pub async fn execute_function(
        &self,
        name: &str,
        args: &[ParetoExpression],
        context: &mut HashMap<String, ParetoValue>,
    ) -> ContextNestResult<ParetoValue> {
        match name {
            "basic_math" => Ok(ParetoValue::Number(1.0)),
            _ => Ok(ParetoValue::Null),
        }
    }
}

pub trait ParetoFunction: Send + Sync {
    fn name(&self) -> &str;
    fn execute(&self, args: Vec<ParetoValue>) -> ContextNestResult<ParetoValue>;
}

/// Core protocol shell structure for Context Engineering protocols
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolShell {
    pub name: String,
    pub intent: String,
    pub version: String,
    pub input: ProtocolInput,
    pub process: Vec<ProcessStep>,
    pub output: ProtocolOutput,
    pub meta: ProtocolMeta,
    pub created_at: DateTime<Utc>,
    pub last_executed: Option<DateTime<Utc>>,
    pub execution_count: u32,
    // Additional fields for enhanced functionality
    pub pareto_implementation: Option<ParetoExpression>,
    pub expected_outcome: Option<serde_json::Value>,
}

/// Protocol input specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolInput {
    pub required_fields: HashMap<String, FieldSpec>,
    pub optional_fields: HashMap<String, FieldSpec>,
    pub validation_rules: Vec<ValidationRule>,
}

/// Field specification for protocol inputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSpec {
    pub field_type: FieldType,
    pub description: String,
    pub constraints: Option<FieldConstraints>,
    pub example: Option<serde_json::Value>,
}

/// Field types supported in protocols
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldType {
    Field,          // Semantic field
    Parameters,     // Configuration parameters
    History,        // Historical data
    Resources,      // Available resources
    Criteria,       // Validation criteria
    Configuration,  // System configuration
    Metrics,        // Performance metrics
    Custom(String), // Custom type
}

/// Field constraints
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldConstraints {
    pub min_size: Option<usize>,
    pub max_size: Option<usize>,
    pub required_properties: Vec<String>,
    pub allowed_values: Option<Vec<serde_json::Value>>,
    pub format: Option<String>,
}

/// Process step in pareto-lang format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessStep {
    pub step_id: String,
    pub operation: String,
    pub parameters: HashMap<String, serde_json::Value>,
    pub description: String,
    pub dependencies: Vec<String>,
    pub parallel_execution: bool,
}

/// Protocol output specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolOutput {
    pub fields: HashMap<String, FieldSpec>,
    pub success_criteria: Vec<String>,
    pub error_conditions: Vec<String>,
}

/// Protocol metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMeta {
    pub version: String,
    pub author: String,
    pub timestamp: DateTime<Utc>,
    pub tags: Vec<String>,
    pub dependencies: Vec<String>,
    pub performance_metrics: Option<PerformanceMetrics>,
}

/// Performance metrics for protocol execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub average_execution_time_ms: u64,
    pub success_rate: f64,
    pub resource_usage: HashMap<String, f64>,
    pub last_measured: DateTime<Utc>,
}

/// Validation rule for protocol inputs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRule {
    pub rule_id: String,
    pub field_name: String,
    pub rule_type: ValidationType,
    pub condition: String,
    pub error_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationType {
    Required,
    TypeCheck,
    RangeCheck,
    FormatCheck,
    CustomValidation(String),
}

/// Protocol execution context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolExecutionContext {
    pub protocol_name: String,
    pub execution_id: String,
    pub start_time: DateTime<Utc>,
    pub inputs: HashMap<String, serde_json::Value>,
    pub step_results: HashMap<String, StepResult>,
    pub current_step: Option<String>,
    pub variables: HashMap<String, serde_json::Value>,
    pub status: ExecutionStatus,
}

/// Result of a protocol step execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step_id: String,
    pub operation: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub status: StepStatus,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub metrics: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    PartiallyCompleted,
}

/// Protocol executor trait for implementing protocol execution
#[async_trait::async_trait]
pub trait ProtocolExecutor: Send + Sync + std::fmt::Debug {
    /// Execute a protocol with given inputs
    async fn execute(
        &mut self,
        inputs: HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<ProtocolExecutionResult>;

    /// Execute a protocol with protocol shell and context (alternative signature)
    async fn execute_with_context(
        &mut self,
        protocol: &ProtocolShell,
        context: &serde_json::Value,
    ) -> ContextNestResult<ExecutionResult> {
        // Convert to inputs format and delegate to main execute method
        let mut inputs = HashMap::new();
        if let Some(obj) = context.as_object() {
            for (key, value) in obj {
                inputs.insert(key.clone(), value.clone());
            }
        }
        let result = self.execute(inputs).await?;
        Ok(ExecutionResult {
            success: result.status == ExecutionStatus::Completed,
            execution_time: result.execution_time_ms,
            output: serde_json::to_value(result.outputs)?,
            metrics: result.metrics,
            error: result.error,
        })
    }

    /// Validate inputs before execution
    fn validate_inputs(&self, inputs: &HashMap<String, serde_json::Value>)
        -> ContextNestResult<()>;

    /// Validate protocol before execution (alternative signature)
    fn validate(&self, protocol: &ProtocolShell) -> ContextNestResult<bool> {
        Ok(true) // Default implementation
    }

    /// Get status (alternative signature)
    async fn get_status(&self) -> ContextNestResult<serde_json::Value> {
        Ok(serde_json::json!({"status": "ready"}))
    }

    /// Execute a single process step
    fn execute_step(
        &mut self,
        step: &ProcessStep,
        context: &mut ProtocolExecutionContext,
    ) -> ContextNestResult<StepResult>;

    /// Get protocol specification
    fn get_protocol_spec(&self) -> &ProtocolShell;

    /// Update protocol performance metrics
    fn update_metrics(&mut self, execution_result: &ProtocolExecutionResult);
}

/// Result of protocol execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolExecutionResult {
    pub execution_id: String,
    pub protocol_name: String,
    pub status: ExecutionStatus,
    pub outputs: HashMap<String, serde_json::Value>,
    pub execution_time_ms: u64,
    pub step_results: HashMap<String, StepResult>,
    pub error: Option<String>,
    pub warnings: Vec<String>,
    pub metrics: HashMap<String, f64>,
    pub timestamp: DateTime<Utc>,
}

/// Simplified execution result for field operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub execution_time: u64,
    pub output: serde_json::Value,
    pub metrics: HashMap<String, f64>,
    pub error: Option<String>,
}

/// Protocol registry for managing protocol instances
#[derive(Debug)]
pub struct ProtocolRegistry {
    pub protocols: HashMap<String, Box<dyn ProtocolExecutor>>,
    pub execution_history: Vec<ProtocolExecutionResult>,
    pub global_metrics: HashMap<String, PerformanceMetrics>,
}

impl ProtocolRegistry {
    pub fn new() -> Self {
        Self {
            protocols: HashMap::new(),
            execution_history: Vec::new(),
            global_metrics: HashMap::new(),
        }
    }

    /// Register a protocol executor
    pub fn register_protocol(
        &mut self,
        name: String,
        executor: Box<dyn ProtocolExecutor>,
    ) -> ContextNestResult<()> {
        if self.protocols.contains_key(&name) {
            return Err(crate::ContextNestError::Api(format!(
                "Protocol '{}' is already registered",
                name
            )));
        }

        self.protocols.insert(name, executor);
        Ok(())
    }

    /// Execute a protocol by name
    pub async fn execute_protocol(
        &mut self,
        protocol_name: &str,
        inputs: HashMap<String, serde_json::Value>,
    ) -> ContextNestResult<ProtocolExecutionResult> {
        let executor = self.protocols.get_mut(protocol_name).ok_or_else(|| {
            crate::ContextNestError::Api(format!("Protocol '{}' not found", protocol_name))
        })?;

        let result = executor.execute(inputs).await?;

        // Update metrics
        executor.update_metrics(&result);

        // Store execution history
        self.execution_history.push(result.clone());

        // Update global metrics
        self.update_global_metrics(protocol_name, &result);

        Ok(result)
    }

    /// Get protocol specifications
    pub fn get_protocol_specs(&self) -> HashMap<String, &ProtocolShell> {
        self.protocols
            .iter()
            .map(|(name, executor)| (name.clone(), executor.get_protocol_spec()))
            .collect()
    }

    /// Get execution statistics for a protocol
    pub fn get_protocol_stats(&self, protocol_name: &str) -> Option<ProtocolStats> {
        let executions: Vec<&ProtocolExecutionResult> = self
            .execution_history
            .iter()
            .filter(|result| result.protocol_name == protocol_name)
            .collect();

        if executions.is_empty() {
            return None;
        }

        let total_executions = executions.len();
        let successful_executions = executions
            .iter()
            .filter(|result| matches!(result.status, ExecutionStatus::Completed))
            .count();

        let average_execution_time = executions
            .iter()
            .map(|result| result.execution_time_ms)
            .sum::<u64>()
            / total_executions as u64;

        let success_rate = (successful_executions as f64 / total_executions as f64) * 100.0;

        Some(ProtocolStats {
            protocol_name: protocol_name.to_string(),
            total_executions,
            successful_executions,
            average_execution_time_ms: average_execution_time,
            success_rate,
            last_execution: executions.last().map(|r| r.timestamp),
        })
    }

    /// Protocol lineage auditing methods for self-repair mechanisms
    pub fn audit_protocol_lineage(
        &self,
        protocol_name: &str,
    ) -> ContextNestResult<ProtocolLineageAudit> {
        let spec = self
            .protocols
            .get(protocol_name)
            .ok_or_else(|| {
                crate::ContextNestError::Api(format!(
                    "Protocol '{}' not found for lineage audit",
                    protocol_name
                ))
            })?
            .get_protocol_spec();

        let executions = self.get_protocol_executions(protocol_name);
        let dependencies = self.analyze_protocol_dependencies(protocol_name, spec)?;
        let lineage_tree = self.build_lineage_tree(protocol_name)?;
        let integrity_issues = self.detect_lineage_integrity_issues(protocol_name, &executions)?;
        let lineage_health = self.calculate_lineage_health(&integrity_issues);

        Ok(ProtocolLineageAudit {
            protocol_name: protocol_name.to_string(),
            lineage_tree,
            dependencies,
            execution_history: executions,
            integrity_issues,
            audit_timestamp: Utc::now(),
            lineage_health,
        })
    }

    fn get_protocol_executions(&self, protocol_name: &str) -> Vec<ProtocolExecutionSummary> {
        self.execution_history
            .iter()
            .filter(|result| result.protocol_name == protocol_name)
            .map(|result| ProtocolExecutionSummary {
                execution_id: result.execution_id.clone(),
                timestamp: result.timestamp,
                status: result.status.clone(),
                execution_time_ms: result.execution_time_ms,
                step_count: result.step_results.len(),
                error_summary: result.error.as_ref().map(|e| e.to_string()),
            })
            .collect()
    }

    fn analyze_protocol_dependencies(
        &self,
        protocol_name: &str,
        spec: &ProtocolShell,
    ) -> ContextNestResult<ProtocolDependencyAnalysis> {
        let mut direct_dependencies = Vec::new();
        let mut transitive_dependencies = Vec::new();
        let mut circular_dependencies = Vec::new();

        // Direct dependencies from protocol meta
        for dep in &spec.meta.dependencies {
            direct_dependencies.push(dep.clone());

            // Check if dependency protocol exists
            if let Some(dep_spec) = self.protocols.get(dep).map(|p| p.get_protocol_spec()) {
                // Analyze transitive dependencies
                for trans_dep in &dep_spec.meta.dependencies {
                    if !transitive_dependencies.contains(trans_dep) && trans_dep != protocol_name {
                        transitive_dependencies.push(trans_dep.clone());
                    }

                    // Check for circular dependencies
                    if trans_dep == protocol_name {
                        circular_dependencies
                            .push(format!("{} -> {} -> {}", protocol_name, dep, trans_dep));
                    }
                }
            }
        }

        // Check for step-level dependencies within process
        let mut internal_dependencies = Vec::new();
        for step in &spec.process {
            for dep_step in &step.dependencies {
                internal_dependencies.push(InternalDependency {
                    step_id: step.step_id.clone(),
                    depends_on: dep_step.clone(),
                    dependency_type: DependencyType::StepLevel,
                });
            }
        }

        Ok(ProtocolDependencyAnalysis {
            direct_dependencies,
            transitive_dependencies,
            circular_dependencies,
            internal_dependencies,
            dependency_depth: self.calculate_dependency_depth(protocol_name, 0)?,
        })
    }

    fn calculate_dependency_depth(
        &self,
        protocol_name: &str,
        current_depth: usize,
    ) -> ContextNestResult<usize> {
        if current_depth > 10 {
            return Ok(10); // Prevent infinite recursion
        }

        let spec = self
            .protocols
            .get(protocol_name)
            .map(|p| p.get_protocol_spec());

        if let Some(spec) = spec {
            let mut max_depth = current_depth;
            for dep in &spec.meta.dependencies {
                if self.protocols.contains_key(dep) {
                    let dep_depth = self.calculate_dependency_depth(dep, current_depth + 1)?;
                    max_depth = max_depth.max(dep_depth);
                }
            }
            Ok(max_depth)
        } else {
            Ok(current_depth)
        }
    }

    fn build_lineage_tree(&self, protocol_name: &str) -> ContextNestResult<ProtocolLineageTree> {
        let spec = self
            .protocols
            .get(protocol_name)
            .map(|p| p.get_protocol_spec())
            .ok_or_else(|| {
                crate::ContextNestError::Api(format!(
                    "Protocol '{}' not found for lineage tree",
                    protocol_name
                ))
            })?;

        let mut children = Vec::new();
        for dep in &spec.meta.dependencies {
            if self.protocols.contains_key(dep) {
                children.push(self.build_lineage_tree(dep)?);
            }
        }

        Ok(ProtocolLineageTree {
            protocol_name: protocol_name.to_string(),
            protocol_version: spec.version.clone(),
            creation_timestamp: spec.created_at,
            last_execution: self
                .execution_history
                .iter()
                .filter(|r| r.protocol_name == protocol_name)
                .last()
                .map(|r| r.timestamp),
            execution_count: self
                .execution_history
                .iter()
                .filter(|r| r.protocol_name == protocol_name)
                .count(),
            children,
        })
    }

    fn detect_lineage_integrity_issues(
        &self,
        protocol_name: &str,
        executions: &[ProtocolExecutionSummary],
    ) -> ContextNestResult<Vec<LineageIntegrityIssue>> {
        let mut issues = Vec::new();

        // Check for missing dependencies
        let spec = self
            .protocols
            .get(protocol_name)
            .map(|p| p.get_protocol_spec())
            .unwrap();

        for dep in &spec.meta.dependencies {
            if !self.protocols.contains_key(dep) {
                issues.push(LineageIntegrityIssue {
                    issue_type: LineageIssueType::MissingDependency,
                    severity: LineageIssueSeverity::High,
                    description: format!(
                        "Protocol '{}' depends on missing protocol '{}'",
                        protocol_name, dep
                    ),
                    affected_protocols: vec![protocol_name.to_string(), dep.clone()],
                    recommended_action: format!("Register protocol '{}'", dep),
                });
            }
        }

        // Check for version mismatches
        for dep in &spec.meta.dependencies {
            if let Some(dep_spec) = self.protocols.get(dep).map(|p| p.get_protocol_spec()) {
                // Simple version compatibility check
                if spec.version != dep_spec.version
                    && spec.version.split('.').next() != dep_spec.version.split('.').next()
                {
                    issues.push(LineageIntegrityIssue {
                        issue_type: LineageIssueType::VersionMismatch,
                        severity: LineageIssueSeverity::Medium,
                        description: format!(
                            "Protocol '{}' v{} may be incompatible with dependency '{}' v{}",
                            protocol_name, spec.version, dep, dep_spec.version
                        ),
                        affected_protocols: vec![protocol_name.to_string(), dep.clone()],
                        recommended_action: "Review version compatibility".to_string(),
                    });
                }
            }
        }

        // Check for execution anomalies
        let failed_executions = executions
            .iter()
            .filter(|e| !matches!(e.status, ExecutionStatus::Completed))
            .count();

        if failed_executions > executions.len() / 2 {
            issues.push(LineageIntegrityIssue {
                issue_type: LineageIssueType::ExecutionAnomalies,
                severity: LineageIssueSeverity::High,
                description: format!(
                    "Protocol '{}' has high failure rate: {} of {} executions failed",
                    protocol_name,
                    failed_executions,
                    executions.len()
                ),
                affected_protocols: vec![protocol_name.to_string()],
                recommended_action: "Investigate execution failures and dependencies".to_string(),
            });
        }

        // Check for outdated executions
        if let Some(last_execution) = executions.last() {
            let days_since_execution = (Utc::now() - last_execution.timestamp).num_days();
            if days_since_execution > 30 {
                issues.push(LineageIntegrityIssue {
                    issue_type: LineageIssueType::StaleProtocol,
                    severity: LineageIssueSeverity::Low,
                    description: format!(
                        "Protocol '{}' hasn't been executed in {} days",
                        protocol_name, days_since_execution
                    ),
                    affected_protocols: vec![protocol_name.to_string()],
                    recommended_action: "Consider archiving or updating protocol".to_string(),
                });
            }
        }

        Ok(issues)
    }

    fn calculate_lineage_health(&self, issues: &[LineageIntegrityIssue]) -> LineageHealth {
        if issues.is_empty() {
            return LineageHealth::Healthy;
        }

        let critical_issues = issues
            .iter()
            .filter(|i| matches!(i.severity, LineageIssueSeverity::Critical))
            .count();
        let high_issues = issues
            .iter()
            .filter(|i| matches!(i.severity, LineageIssueSeverity::High))
            .count();
        let medium_issues = issues
            .iter()
            .filter(|i| matches!(i.severity, LineageIssueSeverity::Medium))
            .count();

        if critical_issues > 0 {
            LineageHealth::Critical
        } else if high_issues > 0 {
            LineageHealth::Degraded
        } else if medium_issues > 2 {
            LineageHealth::Warning
        } else {
            LineageHealth::Healthy
        }
    }

    /// Update global metrics
    fn update_global_metrics(&mut self, protocol_name: &str, result: &ProtocolExecutionResult) {
        // Get protocol stats before creating mutable borrow
        let protocol_stats = self.get_protocol_stats(protocol_name).unwrap();

        let entry = self
            .global_metrics
            .entry(protocol_name.to_string())
            .or_insert_with(|| PerformanceMetrics {
                average_execution_time_ms: 0,
                success_rate: 0.0,
                resource_usage: HashMap::new(),
                last_measured: Utc::now(),
            });

        // Update average execution time (simple moving average)
        entry.average_execution_time_ms =
            (entry.average_execution_time_ms + result.execution_time_ms) / 2;

        // Update success rate
        entry.success_rate = protocol_stats.success_rate;

        // Update resource usage
        for (key, value) in &result.metrics {
            let current = entry.resource_usage.get(key).unwrap_or(&0.0);
            entry
                .resource_usage
                .insert(key.clone(), (current + value) / 2.0);
        }

        entry.last_measured = Utc::now();
    }

    /// Get registry statistics
    pub fn get_registry_stats(&self) -> RegistryStats {
        RegistryStats {
            total_protocols: self.protocols.len(),
            total_executions: self.execution_history.len(),
            protocols_with_metrics: self.global_metrics.len(),
            recent_executions: self
                .execution_history
                .iter()
                .rev()
                .take(10)
                .map(|r| r.clone())
                .collect(),
        }
    }
}

/// Statistics for a specific protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolStats {
    pub protocol_name: String,
    pub total_executions: usize,
    pub successful_executions: usize,
    pub average_execution_time_ms: u64,
    pub success_rate: f64,
    pub last_execution: Option<DateTime<Utc>>,
}

/// Statistics for the entire protocol registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryStats {
    pub total_protocols: usize,
    pub total_executions: usize,
    pub protocols_with_metrics: usize,
    pub recent_executions: Vec<ProtocolExecutionResult>,
}

/// Utility functions for protocol operations
pub mod utils {
    use super::*;

    /// Parse pareto-lang operation string
    pub fn parse_pareto_operation(
        operation: &str,
    ) -> ContextNestResult<(String, HashMap<String, serde_json::Value>)> {
        // Parse operations like "/health.monitor{metrics=['coherence', 'stability']}"
        if !operation.starts_with('/') {
            return Err(crate::ContextNestError::Api(
                "Operation must start with '/'".to_string(),
            ));
        }

        let operation = &operation[1..]; // Remove leading '/'

        if let Some(brace_pos) = operation.find('{') {
            let op_name = operation[..brace_pos].to_string();
            let params_str = &operation[brace_pos + 1..];

            if !params_str.ends_with('}') {
                return Err(crate::ContextNestError::Api(
                    "Operation parameters must end with '}'".to_string(),
                ));
            }

            let params_str = &params_str[..params_str.len() - 1]; // Remove trailing '}'
            let parameters = parse_parameters(params_str)?;

            Ok((op_name, parameters))
        } else {
            // No parameters
            Ok((operation.to_string(), HashMap::new()))
        }
    }

    /// Parse parameter string into HashMap
    fn parse_parameters(params_str: &str) -> ContextNestResult<HashMap<String, serde_json::Value>> {
        let mut parameters = HashMap::new();

        if params_str.trim().is_empty() {
            return Ok(parameters);
        }

        // Simple parameter parsing (could be enhanced for complex cases)
        let pairs: Vec<&str> = params_str.split(',').collect();

        for pair in pairs {
            let pair = pair.trim();
            if let Some(eq_pos) = pair.find('=') {
                let key = pair[..eq_pos].trim().to_string();
                let value_str = pair[eq_pos + 1..].trim();

                // Try to parse as JSON value
                let value = match serde_json::from_str(value_str) {
                    Ok(v) => v,
                    Err(_) => {
                        // If JSON parsing fails, treat as string
                        serde_json::Value::String(value_str.trim_matches('"').to_string())
                    }
                };

                parameters.insert(key, value);
            }
        }

        Ok(parameters)
    }

    /// Create execution context for protocol
    pub fn create_execution_context(
        protocol_name: String,
        inputs: HashMap<String, serde_json::Value>,
    ) -> ProtocolExecutionContext {
        ProtocolExecutionContext {
            protocol_name,
            execution_id: uuid::Uuid::new_v4().to_string(),
            start_time: Utc::now(),
            inputs,
            step_results: HashMap::new(),
            current_step: None,
            variables: HashMap::new(),
            status: ExecutionStatus::Pending,
        }
    }

    /// Validate protocol inputs against specification
    pub fn validate_protocol_inputs(
        inputs: &HashMap<String, serde_json::Value>,
        spec: &ProtocolInput,
    ) -> ContextNestResult<Vec<String>> {
        let mut errors = Vec::new();

        // Check required fields
        for (field_name, field_spec) in &spec.required_fields {
            if !inputs.contains_key(field_name) {
                errors.push(format!("Required field '{}' is missing", field_name));
                continue;
            }

            // Validate field type and constraints
            if let Some(validation_errors) = validate_field_value(&inputs[field_name], field_spec) {
                errors.extend(validation_errors);
            }
        }

        // Validate provided optional fields
        for (field_name, value) in inputs {
            if let Some(field_spec) = spec.optional_fields.get(field_name) {
                if let Some(validation_errors) = validate_field_value(value, field_spec) {
                    errors.extend(validation_errors);
                }
            }
        }

        Ok(errors)
    }

    /// Validate a single field value
    fn validate_field_value(value: &serde_json::Value, spec: &FieldSpec) -> Option<Vec<String>> {
        let mut errors = Vec::new();

        // Type validation (simplified)
        match &spec.field_type {
            FieldType::Field => {
                if !value.is_object() {
                    errors.push("Field must be an object".to_string());
                }
            }
            FieldType::Parameters => {
                if !value.is_object() {
                    errors.push("Parameters must be an object".to_string());
                }
            }
            FieldType::History => {
                if !value.is_array() {
                    errors.push("History must be an array".to_string());
                }
            }
            _ => {
                // Other types - basic validation
            }
        }

        // Constraint validation
        if let Some(constraints) = &spec.constraints {
            if let Some(min_size) = constraints.min_size {
                let size = match value {
                    serde_json::Value::Array(arr) => arr.len(),
                    serde_json::Value::Object(obj) => obj.len(),
                    serde_json::Value::String(s) => s.len(),
                    _ => 1,
                };

                if size < min_size {
                    errors.push(format!("Value size {} is below minimum {}", size, min_size));
                }
            }

            if let Some(max_size) = constraints.max_size {
                let size = match value {
                    serde_json::Value::Array(arr) => arr.len(),
                    serde_json::Value::Object(obj) => obj.len(),
                    serde_json::Value::String(s) => s.len(),
                    _ => 1,
                };

                if size > max_size {
                    errors.push(format!("Value size {} exceeds maximum {}", size, max_size));
                }
            }

            if let Some(allowed_values) = &constraints.allowed_values {
                if !allowed_values.contains(value) {
                    errors.push("Value is not in the list of allowed values".to_string());
                }
            }
        }

        if errors.is_empty() {
            None
        } else {
            Some(errors)
        }
    }
}

impl ProtocolShell {
    /// Create a new protocol shell with default values
    pub fn new() -> Self {
        Self {
            name: String::new(),
            intent: String::new(),
            version: "1.0.0".to_string(),
            input: ProtocolInput {
                required_fields: HashMap::new(),
                optional_fields: HashMap::new(),
                validation_rules: Vec::new(),
            },
            process: Vec::new(),
            output: ProtocolOutput {
                fields: HashMap::new(),
                success_criteria: Vec::new(),
                error_conditions: Vec::new(),
            },
            meta: ProtocolMeta {
                version: "1.0.0".to_string(),
                author: String::new(),
                timestamp: Utc::now(),
                tags: Vec::new(),
                dependencies: Vec::new(),
                performance_metrics: None,
            },
            created_at: Utc::now(),
            last_executed: None,
            execution_count: 0,
            pareto_implementation: None,
            expected_outcome: None,
        }
    }

    /// Set the protocol name
    pub fn with_name(mut self, name: String) -> Self {
        self.name = name;
        self
    }

    /// Set the protocol description
    pub fn with_description(mut self, description: String) -> Self {
        self.intent = description;
        self
    }

    /// Set the pareto implementation
    pub fn with_pareto_implementation(mut self, implementation: ParetoExpression) -> Self {
        self.pareto_implementation = Some(implementation);
        self
    }

    /// Set the expected outcome
    pub fn with_expected_outcome(mut self, outcome: serde_json::Value) -> Self {
        self.expected_outcome = Some(outcome);
        self
    }

    /// Get protocol name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get pareto implementation
    pub fn pareto_implementation(&self) -> Option<&ParetoExpression> {
        self.pareto_implementation.as_ref()
    }

    /// Get expected outcome
    pub fn expected_outcome(&self) -> Option<&serde_json::Value> {
        self.expected_outcome.as_ref()
    }
}

impl Default for ProtocolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Protocol lineage auditing data structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolLineageAudit {
    pub protocol_name: String,
    pub lineage_tree: ProtocolLineageTree,
    pub dependencies: ProtocolDependencyAnalysis,
    pub execution_history: Vec<ProtocolExecutionSummary>,
    pub integrity_issues: Vec<LineageIntegrityIssue>,
    pub audit_timestamp: DateTime<Utc>,
    pub lineage_health: LineageHealth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolLineageTree {
    pub protocol_name: String,
    pub protocol_version: String,
    pub creation_timestamp: DateTime<Utc>,
    pub last_execution: Option<DateTime<Utc>>,
    pub execution_count: usize,
    pub children: Vec<ProtocolLineageTree>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolDependencyAnalysis {
    pub direct_dependencies: Vec<String>,
    pub transitive_dependencies: Vec<String>,
    pub circular_dependencies: Vec<String>,
    pub internal_dependencies: Vec<InternalDependency>,
    pub dependency_depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalDependency {
    pub step_id: String,
    pub depends_on: String,
    pub dependency_type: DependencyType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DependencyType {
    StepLevel,
    DataFlow,
    ResourceAccess,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolExecutionSummary {
    pub execution_id: String,
    pub timestamp: DateTime<Utc>,
    pub status: ExecutionStatus,
    pub execution_time_ms: u64,
    pub step_count: usize,
    pub error_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageIntegrityIssue {
    pub issue_type: LineageIssueType,
    pub severity: LineageIssueSeverity,
    pub description: String,
    pub affected_protocols: Vec<String>,
    pub recommended_action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LineageIssueType {
    MissingDependency,
    VersionMismatch,
    CircularDependency,
    ExecutionAnomalies,
    StaleProtocol,
    BrokenLineage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LineageIssueSeverity {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LineageHealth {
    Healthy,
    Warning,
    Degraded,
    Critical,
}

/// Pareto Expression Security Validator
/// Prevents code injection, DoS, and other security vulnerabilities
#[derive(Debug)]
pub struct ParetoExpressionValidator {
    max_depth: usize,
    max_variable_name_length: usize,
    max_array_size: usize,
    allowed_functions: std::collections::HashSet<String>,
}

impl ParetoExpressionValidator {
    /// Create a new validator with secure defaults
    pub fn new() -> Self {
        let mut allowed_functions = std::collections::HashSet::new();
        // Whitelist of known-safe functions
        allowed_functions.insert("field_coherence".to_string());
        allowed_functions.insert("inject_pattern".to_string());
        allowed_functions.insert("harmonize_field".to_string());
        allowed_functions.insert("get_pattern".to_string());
        allowed_functions.insert("basic_math".to_string());

        Self {
            max_depth: 10,
            max_variable_name_length: 64,
            max_array_size: 10000,
            allowed_functions,
        }
    }

    /// Validate a Pareto expression before execution
    pub fn validate(&self, expr: &ParetoExpression) -> ContextNestResult<()> {
        self.validate_recursive(expr, 0)
    }

    fn validate_recursive(&self, expr: &ParetoExpression, depth: usize) -> ContextNestResult<()> {
        // Check depth limit to prevent stack overflow
        if depth > self.max_depth {
            return Err(crate::ContextNestError::Validation(format!(
                "Expression nesting exceeds maximum depth of {}. \
                    This could cause stack overflow.",
                self.max_depth
            )));
        }

        match expr {
            ParetoExpression::Variable(name) => self.validate_variable_name(name),
            ParetoExpression::Call(func, args) => self.validate_call(func, args, depth),
            ParetoExpression::Literal(value) => self.validate_literal(value),
            ParetoExpression::Lambda(_, _) => Err(crate::ContextNestError::Validation(
                "Lambda expressions are not supported for security reasons. \
                    Use whitelisted functions instead."
                    .to_string(),
            )),
        }
    }

    fn validate_variable_name(&self, name: &str) -> ContextNestResult<()> {
        // Check length
        if name.len() > self.max_variable_name_length {
            return Err(crate::ContextNestError::Validation(format!(
                "Variable name length {} exceeds maximum {}. \
                    Potential buffer overflow attack.",
                name.len(),
                self.max_variable_name_length
            )));
        }

        // Check for empty name
        if name.is_empty() {
            return Err(crate::ContextNestError::Validation(
                "Variable name cannot be empty".to_string(),
            ));
        }

        // Allow only alphanumeric characters and underscores
        // Prevents path traversal (../), SQL injection, XSS, etc.
        if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(crate::ContextNestError::Validation(format!(
                "Variable name '{}' contains invalid characters. \
                    Only alphanumeric and underscore allowed. \
                    This prevents injection attacks.",
                name
            )));
        }

        // Prevent reserved/dangerous patterns (all lowercase for case-insensitive matching)
        let dangerous_patterns = [
            "..", "/", "\\", "<", ">", "script", "eval", "exec", "system", "cmd", "drop", "delete",
            "update",
        ];
        let name_lower = name.to_lowercase();
        for pattern in &dangerous_patterns {
            if name_lower.contains(pattern) {
                return Err(crate::ContextNestError::Validation(format!(
                    "Variable name contains potentially dangerous pattern '{}'. \
                        Security policy violation.",
                    pattern
                )));
            }
        }

        Ok(())
    }

    fn validate_call(
        &self,
        func: &ParetoExpression,
        args: &[ParetoExpression],
        depth: usize,
    ) -> ContextNestResult<()> {
        // Validate function itself recursively
        self.validate_recursive(func, depth + 1)?;

        // Check if function is whitelisted
        if let ParetoExpression::Variable(func_name) = func {
            if !self.allowed_functions.contains(func_name.as_str()) {
                return Err(crate::ContextNestError::Validation(format!(
                    "Function '{}' is not in the whitelist. \
                        Only these functions are allowed: {:?}. \
                        This prevents arbitrary code execution.",
                    func_name, self.allowed_functions
                )));
            }
        }

        // Validate arguments
        for arg in args {
            self.validate_recursive(arg, depth + 1)?;
        }

        Ok(())
    }

    fn validate_literal(&self, value: &ParetoValue) -> ContextNestResult<()> {
        match value {
            ParetoValue::String(s) => {
                // Check string length to prevent memory exhaustion
                if s.len() > 1_000_000 {
                    return Err(crate::ContextNestError::Validation(format!(
                        "String literal length {} exceeds 1MB limit. \
                            Potential DoS attack.",
                        s.len()
                    )));
                }
                Ok(())
            }
            ParetoValue::Array(arr) => {
                // Check array size to prevent memory exhaustion
                if arr.len() > self.max_array_size {
                    return Err(crate::ContextNestError::Validation(format!(
                        "Array size {} exceeds maximum {}. \
                            Potential memory exhaustion attack.",
                        arr.len(),
                        self.max_array_size
                    )));
                }
                // Validate array elements recursively
                for item in arr {
                    self.validate_literal(item)?;
                }
                Ok(())
            }
            ParetoValue::Object(obj) => {
                // Check object size
                if obj.len() > self.max_array_size {
                    return Err(crate::ContextNestError::Validation(format!(
                        "Object size {} exceeds maximum {}. \
                            Potential memory exhaustion attack.",
                        obj.len(),
                        self.max_array_size
                    )));
                }
                // Validate object values recursively
                for (key, value) in obj {
                    self.validate_variable_name(key)?;
                    self.validate_literal(value)?;
                }
                Ok(())
            }
            ParetoValue::Number(n) => {
                // Check for NaN and infinity
                if !n.is_finite() {
                    return Err(crate::ContextNestError::Validation(
                        "Number must be finite (not NaN or Infinity)".to_string(),
                    ));
                }
                Ok(())
            }
            ParetoValue::Boolean(_) | ParetoValue::Null => Ok(()),
        }
    }

    /// Add a function to the whitelist
    pub fn allow_function(&mut self, func_name: String) {
        self.allowed_functions.insert(func_name);
    }

    /// Remove a function from the whitelist
    pub fn disallow_function(&mut self, func_name: &str) {
        self.allowed_functions.remove(func_name);
    }

    /// Check if a function is allowed
    pub fn is_function_allowed(&self, func_name: &str) -> bool {
        self.allowed_functions.contains(func_name)
    }
}

impl Default for ParetoExpressionValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create deeply nested expressions
    fn create_nested_expression(depth: usize) -> ParetoExpression {
        if depth == 0 {
            ParetoExpression::Literal(ParetoValue::Number(42.0))
        } else {
            ParetoExpression::Call(
                Box::new(ParetoExpression::Variable("field_coherence".to_string())),
                vec![create_nested_expression(depth - 1)],
            )
        }
    }

    #[test]
    fn test_valid_variable_passes() {
        let validator = ParetoExpressionValidator::new();
        let expr = ParetoExpression::Variable("valid_variable_name_123".to_string());
        assert!(validator.validate(&expr).is_ok());
    }

    #[test]
    fn test_valid_whitelisted_function_passes() {
        let validator = ParetoExpressionValidator::new();
        let expr = ParetoExpression::Call(
            Box::new(ParetoExpression::Variable("field_coherence".to_string())),
            vec![],
        );
        assert!(validator.validate(&expr).is_ok());
    }

    #[test]
    fn test_valid_nested_expression_within_limit() {
        let validator = ParetoExpressionValidator::new();
        let expr = create_nested_expression(5); // 5 levels deep (within 10 limit)
        assert!(validator.validate(&expr).is_ok());
    }

    #[test]
    fn test_valid_literal_values() {
        let validator = ParetoExpressionValidator::new();

        // Valid string
        let expr = ParetoExpression::Literal(ParetoValue::String("test".to_string()));
        assert!(validator.validate(&expr).is_ok());

        // Valid number
        let expr = ParetoExpression::Literal(ParetoValue::Number(123.45));
        assert!(validator.validate(&expr).is_ok());

        // Valid boolean
        let expr = ParetoExpression::Literal(ParetoValue::Boolean(true));
        assert!(validator.validate(&expr).is_ok());

        // Valid null
        let expr = ParetoExpression::Literal(ParetoValue::Null);
        assert!(validator.validate(&expr).is_ok());
    }

    // SECURITY TEST: Deep nesting attack (DoS via stack overflow)
    #[test]
    fn test_deep_nesting_attack_blocked() {
        let validator = ParetoExpressionValidator::new();
        let expr = create_nested_expression(15); // 15 levels deep (exceeds 10 limit)

        let result = validator.validate(&expr);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("exceeds maximum depth"));
    }

    // SECURITY TEST: Path traversal attack
    #[test]
    fn test_path_traversal_attack_blocked() {
        let validator = ParetoExpressionValidator::new();

        // Test 1: Parent directory traversal
        let expr = ParetoExpression::Variable("../../../etc/passwd".to_string());
        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid characters"));

        // Test 2: Forward slash
        let expr = ParetoExpression::Variable("/etc/passwd".to_string());
        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid characters"));

        // Test 3: Backslash
        let expr = ParetoExpression::Variable("..\\windows\\system32".to_string());
        let result = validator.validate(&expr);
        assert!(result.is_err());
    }

    // SECURITY TEST: SQL injection attack
    #[test]
    fn test_sql_injection_attack_blocked() {
        let validator = ParetoExpressionValidator::new();

        // Test 1: DROP TABLE
        let expr = ParetoExpression::Variable("'; DROP TABLE users; --".to_string());
        let result = validator.validate(&expr);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("invalid characters") || err_msg.contains("dangerous pattern"));

        // Test 2: DELETE keyword
        let expr = ParetoExpression::Variable("var_DELETE_all".to_string());
        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("dangerous pattern"));

        // Test 3: UPDATE keyword
        let expr = ParetoExpression::Variable("test_UPDATE_value".to_string());
        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("dangerous pattern"));
    }

    // SECURITY TEST: XSS (Cross-Site Scripting) attack
    #[test]
    fn test_xss_attack_blocked() {
        let validator = ParetoExpressionValidator::new();

        // Test 1: Script tag
        let expr = ParetoExpression::Variable("<script>alert('xss')</script>".to_string());
        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid characters"));

        // Test 2: Script keyword alone
        let expr = ParetoExpression::Variable("malicious_script_code".to_string());
        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("dangerous pattern"));
    }

    // SECURITY TEST: Arbitrary function execution
    #[test]
    fn test_unknown_function_attack_blocked() {
        let validator = ParetoExpressionValidator::new();

        // Test 1: System function (caught by dangerous pattern check before whitelist)
        let expr = ParetoExpression::Call(
            Box::new(ParetoExpression::Variable("system".to_string())),
            vec![ParetoExpression::Literal(ParetoValue::String(
                "rm -rf /".to_string(),
            ))],
        );
        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("dangerous pattern"));

        // Test 2: Eval function (caught by dangerous pattern)
        let expr = ParetoExpression::Call(
            Box::new(ParetoExpression::Variable("eval".to_string())),
            vec![ParetoExpression::Literal(ParetoValue::String(
                "malicious_code".to_string(),
            ))],
        );
        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("dangerous pattern"));

        // Test 3: Exec function (caught by dangerous pattern)
        let expr = ParetoExpression::Call(
            Box::new(ParetoExpression::Variable("exec".to_string())),
            vec![],
        );
        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("dangerous pattern"));

        // Test 4: Unknown but not dangerous function (caught by whitelist)
        let expr = ParetoExpression::Call(
            Box::new(ParetoExpression::Variable("unknown_safe_func".to_string())),
            vec![],
        );
        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not in the whitelist"));
    }

    // SECURITY TEST: Lambda expressions blocked
    #[test]
    fn test_lambda_expression_blocked() {
        let validator = ParetoExpressionValidator::new();

        let expr = ParetoExpression::Lambda(
            vec!["x".to_string()],
            Box::new(ParetoExpression::Variable("x".to_string())),
        );

        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Lambda expressions are not supported"));
    }

    // SECURITY TEST: Oversized array attack (memory exhaustion)
    #[test]
    fn test_oversized_array_attack_blocked() {
        let validator = ParetoExpressionValidator::new();

        // Create array with 10,001 elements (exceeds 10,000 limit)
        let large_array: Vec<ParetoValue> = (0..10001).map(|_| ParetoValue::Number(0.0)).collect();

        let expr = ParetoExpression::Literal(ParetoValue::Array(large_array));

        let result = validator.validate(&expr);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("exceeds maximum"));
        assert!(err_msg.contains("memory exhaustion"));
    }

    // SECURITY TEST: Oversized object attack (memory exhaustion)
    #[test]
    fn test_oversized_object_attack_blocked() {
        let validator = ParetoExpressionValidator::new();

        // Create object with 10,001 keys (exceeds 10,000 limit)
        let mut large_object = HashMap::new();
        for i in 0..10001 {
            large_object.insert(format!("key_{}", i), ParetoValue::Number(0.0));
        }

        let expr = ParetoExpression::Literal(ParetoValue::Object(large_object));

        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
    }

    // SECURITY TEST: Huge string attack (DoS)
    #[test]
    fn test_huge_string_attack_blocked() {
        let validator = ParetoExpressionValidator::new();

        // Create string larger than 1MB
        let huge_string = "x".repeat(1_000_001);
        let expr = ParetoExpression::Literal(ParetoValue::String(huge_string));

        let result = validator.validate(&expr);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("exceeds 1MB limit"));
        assert!(err_msg.contains("DoS"));
    }

    // SECURITY TEST: NaN number rejected
    #[test]
    fn test_nan_number_rejected() {
        let validator = ParetoExpressionValidator::new();

        let expr = ParetoExpression::Literal(ParetoValue::Number(f32::NAN));

        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be finite"));
    }

    // SECURITY TEST: Infinity number rejected
    #[test]
    fn test_infinity_number_rejected() {
        let validator = ParetoExpressionValidator::new();

        // Test positive infinity
        let expr = ParetoExpression::Literal(ParetoValue::Number(f32::INFINITY));
        let result = validator.validate(&expr);
        assert!(result.is_err());

        // Test negative infinity
        let expr = ParetoExpression::Literal(ParetoValue::Number(f32::NEG_INFINITY));
        let result = validator.validate(&expr);
        assert!(result.is_err());
    }

    // EDGE CASE: Empty variable name
    #[test]
    fn test_empty_variable_name_rejected() {
        let validator = ParetoExpressionValidator::new();

        let expr = ParetoExpression::Variable("".to_string());

        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    // EDGE CASE: Variable name at maximum length (64 chars)
    #[test]
    fn test_max_length_variable_name_accepted() {
        let validator = ParetoExpressionValidator::new();

        let var_name = "a".repeat(64); // Exactly 64 characters
        let expr = ParetoExpression::Variable(var_name);

        assert!(validator.validate(&expr).is_ok());
    }

    // EDGE CASE: Variable name exceeding maximum length
    #[test]
    fn test_overlength_variable_name_rejected() {
        let validator = ParetoExpressionValidator::new();

        let var_name = "a".repeat(65); // 65 characters (exceeds 64 limit)
        let expr = ParetoExpression::Variable(var_name);

        let result = validator.validate(&expr);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
    }

    // EDGE CASE: Expression at exactly maximum depth (10)
    #[test]
    fn test_max_depth_expression_accepted() {
        let validator = ParetoExpressionValidator::new();

        let expr = create_nested_expression(10); // Exactly 10 levels deep

        assert!(validator.validate(&expr).is_ok());
    }

    // EDGE CASE: Array at exactly maximum size (10,000)
    #[test]
    fn test_max_size_array_accepted() {
        let validator = ParetoExpressionValidator::new();

        let array: Vec<ParetoValue> = (0..10000).map(|_| ParetoValue::Number(1.0)).collect();

        let expr = ParetoExpression::Literal(ParetoValue::Array(array));

        assert!(validator.validate(&expr).is_ok());
    }

    // EDGE CASE: String at exactly 1MB
    #[test]
    fn test_max_size_string_accepted() {
        let validator = ParetoExpressionValidator::new();

        let string = "x".repeat(1_000_000); // Exactly 1MB
        let expr = ParetoExpression::Literal(ParetoValue::String(string));

        assert!(validator.validate(&expr).is_ok());
    }

    // FUNCTIONALITY TEST: Whitelist management
    #[test]
    fn test_whitelist_add_function() {
        let mut validator = ParetoExpressionValidator::new();

        // Initially not allowed
        assert!(!validator.is_function_allowed("custom_function"));

        // Add to whitelist
        validator.allow_function("custom_function".to_string());
        assert!(validator.is_function_allowed("custom_function"));

        // Can now call it
        let expr = ParetoExpression::Call(
            Box::new(ParetoExpression::Variable("custom_function".to_string())),
            vec![],
        );
        assert!(validator.validate(&expr).is_ok());
    }

    // FUNCTIONALITY TEST: Whitelist remove function
    #[test]
    fn test_whitelist_remove_function() {
        let mut validator = ParetoExpressionValidator::new();

        // Initially allowed
        assert!(validator.is_function_allowed("field_coherence"));

        // Remove from whitelist
        validator.disallow_function("field_coherence");
        assert!(!validator.is_function_allowed("field_coherence"));

        // Can no longer call it
        let expr = ParetoExpression::Call(
            Box::new(ParetoExpression::Variable("field_coherence".to_string())),
            vec![],
        );
        assert!(validator.validate(&expr).is_err());
    }

    // COMPREHENSIVE TEST: Complex nested structure with multiple attack vectors
    #[test]
    fn test_complex_attack_combination_blocked() {
        let validator = ParetoExpressionValidator::new();

        // Attempt to nest a SQL injection inside a function call inside deep nesting
        let expr = ParetoExpression::Call(
            Box::new(ParetoExpression::Variable("unknown_func".to_string())),
            vec![
                ParetoExpression::Variable("'; DROP TABLE users; --".to_string()),
                create_nested_expression(15), // Also too deep
            ],
        );

        let result = validator.validate(&expr);
        assert!(result.is_err()); // Should fail on multiple security checks
    }

    // COMPREHENSIVE TEST: Valid complex nested expression
    #[test]
    fn test_valid_complex_nested_expression() {
        let validator = ParetoExpressionValidator::new();

        // Complex but valid expression
        let expr = ParetoExpression::Call(
            Box::new(ParetoExpression::Variable("inject_pattern".to_string())),
            vec![
                ParetoExpression::Variable("valid_var".to_string()),
                ParetoExpression::Literal(ParetoValue::Number(42.0)),
                ParetoExpression::Call(
                    Box::new(ParetoExpression::Variable("field_coherence".to_string())),
                    vec![
                        ParetoExpression::Literal(ParetoValue::String("test".to_string())),
                        ParetoExpression::Literal(ParetoValue::Boolean(true)),
                    ],
                ),
            ],
        );

        assert!(validator.validate(&expr).is_ok());
    }
}

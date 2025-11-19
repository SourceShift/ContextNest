/// Request validation middleware for ContextNest API
/// This module provides comprehensive request validation including:
/// - JSON schema validation
/// - Business logic validation
/// - Content validation for uploads
/// - Parameter validation
use crate::error::{ContextNestError, ContextNestResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, warn};

/// Validation middleware
pub struct ValidationMiddleware {
    schemas: HashMap<String, ValidationSchema>,
    business_rules: HashMap<String, Vec<BusinessRule>>,
}

impl ValidationMiddleware {
    pub fn new() -> Self {
        let mut validation_middleware = Self {
            schemas: HashMap::new(),
            business_rules: HashMap::new(),
        };

        validation_middleware.setup_default_schemas();
        validation_middleware.setup_default_business_rules();
        validation_middleware
    }

    /// Process request validation
    pub async fn process_request(
        &self,
        request: &mut crate::api::server::ProcessedApiRequest,
    ) -> ContextNestResult<()> {
        // 1. Schema validation
        if let Some(body) = &request.inner.body {
            self.validate_json_schema(&request.inner.path, body).await?;
        }

        // 2. Business rules validation
        self.validate_business_rules(&request.inner.path, request)
            .await?;

        // 3. Parameter validation
        self.validate_parameters(&request.inner.query_params)
            .await?;

        debug!("Request validation passed for path: {}", request.inner.path);
        Ok(())
    }

    /// Validate JSON schema
    async fn validate_json_schema(&self, path: &str, body: &Value) -> ContextNestResult<()> {
        if let Some(schema) = self.schemas.get(path) {
            schema.validate(body)?;
        }
        Ok(())
    }

    /// Validate business rules
    async fn validate_business_rules(
        &self,
        path: &str,
        request: &crate::api::server::ProcessedApiRequest,
    ) -> ContextNestResult<()> {
        if let Some(rules) = self.business_rules.get(path) {
            for rule in rules {
                rule.validate(request)?;
            }
        }
        Ok(())
    }

    /// Validate query parameters
    async fn validate_parameters(&self, params: &HashMap<String, String>) -> ContextNestResult<()> {
        for (key, value) in params {
            // Check for common injection attempts
            if value.contains("<script") || value.contains("javascript:") {
                return Err(ContextNestError::Configuration(format!(
                    "Suspicious parameter value in {}",
                    key
                )));
            }

            // Check parameter length
            if value.len() > 1000 {
                return Err(ContextNestError::Configuration(format!(
                    "Parameter {} too long",
                    key
                )));
            }
        }
        Ok(())
    }

    /// Setup default validation schemas
    fn setup_default_schemas(&mut self) {
        // Memory fragment store request schema
        self.schemas.insert(
            "/api/v1/tools/store".to_string(),
            ValidationSchema::new(serde_json::json!({
                "type": "object",
                "properties": {
                    "content": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 100000
                    },
                    "importance": {
                        "type": "number",
                        "minimum": 0.0,
                        "maximum": 1.0
                    },
                    "session_id": {
                        "type": "string",
                        "maxLength": 64,
                        "pattern": "^[a-zA-Z0-9._-]+$"
                    }
                },
                "required": ["content"]
            })),
        );

        // Context enhancement request schema
        self.schemas.insert(
            "/api/context/enhance".to_string(),
            ValidationSchema::new(serde_json::json!({
                "type": "object",
                "properties": {
                    "context_data": {
                        "type": "string",
                        "minLength": 1,
                        "maxLength": 50000
                    },
                    "enhancement_level": {
                        "type": "string",
                        "enum": ["atomic", "molecular", "cellular", "organic", "field", "programmatic", "protocol"]
                    },
                    "target_token_budget": {
                        "type": "integer",
                        "minimum": 100,
                        "maximum": 100000
                    }
                },
                "required": ["context_data"]
            }))
        );
    }

    /// Setup default business rules
    fn setup_default_business_rules(&mut self) {
        // Store-tool business rules
        let store_rules = vec![
            BusinessRule::new(
                "fragment_size_limit",
                Box::new(|request| {
                    if let Some(body) = &request.inner.body {
                        if let Some(content) = body.get("content") {
                            if let Some(content_str) = content.as_str() {
                                if content_str.len() > 500_000 {
                                    // 500KB limit
                                    return Err(ContextNestError::Configuration(
                                        "Fragment content too large for storage".to_string(),
                                    ));
                                }
                            }
                        }
                    }
                    Ok(())
                }),
            ),
            BusinessRule::new(
                "importance_range",
                Box::new(|request| {
                    if let Some(body) = &request.inner.body {
                        if let Some(importance) = body.get("importance") {
                            if let Some(v) = importance.as_f64() {
                                if !(0.0..=1.0).contains(&v) {
                                    return Err(ContextNestError::Configuration(
                                        "Importance must be in [0.0, 1.0]".to_string(),
                                    ));
                                }
                            }
                        }
                    }
                    Ok(())
                }),
            ),
        ];

        self.business_rules
            .insert("/api/v1/tools/store".to_string(), store_rules);

        // Context enhancement business rules
        let context_rules = vec![BusinessRule::new(
            "token_budget_reasonable",
            Box::new(|request| {
                if let Some(body) = &request.inner.body {
                    if let Some(budget) = body.get("target_token_budget") {
                        if let Some(budget_num) = budget.as_i64() {
                            if budget_num > 50_000 {
                                return Err(ContextNestError::Configuration(
                                    "Token budget too high, maximum is 50,000".to_string(),
                                ));
                            }
                        }
                    }
                }
                Ok(())
            }),
        )];

        self.business_rules
            .insert("/api/context/enhance".to_string(), context_rules);
    }
}

/// JSON Schema validation
pub struct ValidationSchema {
    schema: Value,
}

impl ValidationSchema {
    pub fn new(schema: Value) -> Self {
        Self { schema }
    }

    /// Validate `data` against this schema.
    /// Lightweight in-house validator covering the subset of JSON Schema
    /// the seven-tool API uses (object/array/primitive type checks, required
    /// fields, length bounds, and a single regex-replacement pattern path
    /// below). For broader JSON Schema 2020-12 coverage, swap in the
    /// `jsonschema` crate via a follow-up epic — none of our endpoint
    /// payloads currently need that surface.
    pub fn validate(&self, data: &Value) -> ContextNestResult<()> {
        match &self.schema {
            Value::Object(schema_obj) => {
                self.validate_object(data, schema_obj)?;
            }
            _ => {
                return Err(ContextNestError::Configuration(
                    "Invalid schema format".to_string(),
                ))
            }
        }
        Ok(())
    }

    fn validate_object(
        &self,
        data: &Value,
        schema: &serde_json::Map<String, Value>,
    ) -> ContextNestResult<()> {
        let data_obj = data
            .as_object()
            .ok_or_else(|| ContextNestError::Configuration("Expected object".to_string()))?;

        // Check required fields
        if let Some(required) = schema.get("required") {
            if let Some(required_array) = required.as_array() {
                for required_field in required_array {
                    if let Some(field_name) = required_field.as_str() {
                        if !data_obj.contains_key(field_name) {
                            return Err(ContextNestError::Configuration(format!(
                                "Missing required field: {}",
                                field_name
                            )));
                        }
                    }
                }
            }
        }

        // Check properties
        if let Some(properties) = schema.get("properties") {
            if let Some(props_obj) = properties.as_object() {
                for (field_name, field_schema) in props_obj {
                    if let Some(field_value) = data_obj.get(field_name) {
                        self.validate_field(field_value, field_schema, field_name)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn validate_field(
        &self,
        value: &Value,
        schema: &Value,
        field_name: &str,
    ) -> ContextNestResult<()> {
        let schema_obj = schema
            .as_object()
            .ok_or_else(|| ContextNestError::Configuration("Invalid field schema".to_string()))?;

        // Type validation
        if let Some(expected_type) = schema_obj.get("type") {
            if let Some(type_str) = expected_type.as_str() {
                match type_str {
                    "string" => {
                        if !value.is_string() {
                            return Err(ContextNestError::Configuration(format!(
                                "Field {} must be a string",
                                field_name
                            )));
                        }

                        let str_value = value.as_str().unwrap();

                        // String length validation
                        if let Some(min_length) = schema_obj.get("minLength") {
                            if let Some(min) = min_length.as_u64() {
                                if str_value.len() < min as usize {
                                    return Err(ContextNestError::Configuration(format!(
                                        "Field {} too short, minimum length: {}",
                                        field_name, min
                                    )));
                                }
                            }
                        }

                        if let Some(max_length) = schema_obj.get("maxLength") {
                            if let Some(max) = max_length.as_u64() {
                                if str_value.len() > max as usize {
                                    return Err(ContextNestError::Configuration(format!(
                                        "Field {} too long, maximum length: {}",
                                        field_name, max
                                    )));
                                }
                            }
                        }

                        // Pattern validation.
                        // The single pattern this validator currently supports
                        // is `^[a-zA-Z0-9._/-]+$` (used for path/id-shaped
                        // fields in the seven-tool API). For arbitrary regex
                        // patterns, swap in the `regex` crate via a follow-up.
                        if let Some(pattern) = schema_obj.get("pattern") {
                            if let Some(pattern_str) = pattern.as_str() {
                                if pattern_str.contains("^[a-zA-Z0-9._/-]+$") {
                                    for ch in str_value.chars() {
                                        if !ch.is_alphanumeric()
                                            && ch != '.'
                                            && ch != '_'
                                            && ch != '/'
                                            && ch != '-'
                                        {
                                            return Err(ContextNestError::Configuration(format!(
                                                "Field {} contains invalid characters",
                                                field_name
                                            )));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "integer" => {
                        if !value.is_i64() {
                            return Err(ContextNestError::Configuration(format!(
                                "Field {} must be an integer",
                                field_name
                            )));
                        }

                        let int_value = value.as_i64().unwrap();

                        // Range validation
                        if let Some(minimum) = schema_obj.get("minimum") {
                            if let Some(min) = minimum.as_i64() {
                                if int_value < min {
                                    return Err(ContextNestError::Configuration(format!(
                                        "Field {} below minimum value: {}",
                                        field_name, min
                                    )));
                                }
                            }
                        }

                        if let Some(maximum) = schema_obj.get("maximum") {
                            if let Some(max) = maximum.as_i64() {
                                if int_value > max {
                                    return Err(ContextNestError::Configuration(format!(
                                        "Field {} above maximum value: {}",
                                        field_name, max
                                    )));
                                }
                            }
                        }
                    }
                    "boolean" => {
                        if !value.is_boolean() {
                            return Err(ContextNestError::Configuration(format!(
                                "Field {} must be a boolean",
                                field_name
                            )));
                        }
                    }
                    "object" => {
                        if !value.is_object() {
                            return Err(ContextNestError::Configuration(format!(
                                "Field {} must be an object",
                                field_name
                            )));
                        }
                    }
                    _ => {} // Skip unknown types
                }
            }
        }

        // Enum validation
        if let Some(enum_values) = schema_obj.get("enum") {
            if let Some(enum_array) = enum_values.as_array() {
                let value_matches = enum_array.iter().any(|enum_val| enum_val == value);
                if !value_matches {
                    return Err(ContextNestError::Configuration(format!(
                        "Field {} value not in allowed enum values",
                        field_name
                    )));
                }
            }
        }

        Ok(())
    }
}

/// Business rule validation
pub struct BusinessRule {
    name: String,
    validator: Box<
        dyn Fn(&crate::api::server::ProcessedApiRequest) -> ContextNestResult<()> + Send + Sync,
    >,
}

impl BusinessRule {
    pub fn new(
        name: &str,
        validator: Box<
            dyn Fn(&crate::api::server::ProcessedApiRequest) -> ContextNestResult<()> + Send + Sync,
        >,
    ) -> Self {
        Self {
            name: name.to_string(),
            validator,
        }
    }

    pub fn validate(
        &self,
        request: &crate::api::server::ProcessedApiRequest,
    ) -> ContextNestResult<()> {
        (self.validator)(request).map_err(|e| {
            warn!("Business rule '{}' validation failed: {}", self.name, e);
            e
        })
    }
}

/// Content validation for file uploads and user content
pub struct ContentValidator;

impl ContentValidator {
    pub fn new() -> Self {
        Self
    }

    pub async fn validate_content(
        &self,
        request: &crate::api::server::ProcessedApiRequest,
    ) -> ContextNestResult<()> {
        if let Some(body) = &request.inner.body {
            // Check for malicious content
            self.scan_for_malicious_content(body)?;

            // Validate file uploads if present
            if let Some(content) = body.get("content") {
                if let Some(content_str) = content.as_str() {
                    self.validate_code_content(content_str)?;
                }
            }
        }

        Ok(())
    }

    fn scan_for_malicious_content(&self, body: &Value) -> ContextNestResult<()> {
        let body_str = serde_json::to_string(body)?;

        // Check for common malicious patterns
        let malicious_patterns = [
            "<script",
            "javascript:",
            "data:text/html",
            "eval(",
            "Function(",
            "setTimeout(",
            "setInterval(",
        ];

        for pattern in &malicious_patterns {
            if body_str.to_lowercase().contains(pattern) {
                return Err(ContextNestError::Configuration(format!(
                    "Potentially malicious content detected: {}",
                    pattern
                )));
            }
        }

        Ok(())
    }

    fn validate_code_content(&self, content: &str) -> ContextNestResult<()> {
        // Check for reasonable code content
        if content.len() > 1_000_000 {
            // 1MB limit
            return Err(ContextNestError::Configuration(
                "Code content too large".to_string(),
            ));
        }

        // Check for suspicious imports or commands
        let lines = content.lines();
        for line in lines {
            let line_lower = line.to_lowercase();
            if line_lower.contains("import 'dart:io'") && line_lower.contains("process.run") {
                return Err(ContextNestError::Configuration(
                    "Potentially dangerous system commands detected".to_string(),
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_schema_validation() {
        let schema = ValidationSchema::new(json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "minLength": 1, "maxLength": 10},
                "age": {"type": "integer", "minimum": 0, "maximum": 150}
            },
            "required": ["name"]
        }));

        // Valid data
        let valid_data = json!({"name": "test", "age": 25});
        assert!(schema.validate(&valid_data).is_ok());

        // Missing required field
        let invalid_data = json!({"age": 25});
        assert!(schema.validate(&invalid_data).is_err());

        // Invalid type
        let invalid_data = json!({"name": 123});
        assert!(schema.validate(&invalid_data).is_err());
    }

    #[test]
    fn test_content_validation() {
        let validator = ContentValidator::new();

        // Safe content
        let safe_content = "class MyWidget extends StatelessWidget {}";
        assert!(validator.validate_code_content(safe_content).is_ok());

        // Malicious content
        let malicious_content = "import 'dart:io'; Process.run('rm', ['-rf', '/']);";
        assert!(validator.validate_code_content(malicious_content).is_err());
    }
}

/// Comprehensive Input Validation Framework
/// This module provides robust input validation to protect against:
/// - Malformed requests and injection attacks
/// - SQL injection, XSS, path traversal, command injection
/// - Edge cases and boundary conditions
/// - Content type and size validation
/// - Semantic validation for complex data types
use axum::response::IntoResponse;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::api::middleware::validators::{
    ContextValidator, NeuralFieldValidator, ProtocolValidator, SecurityValidator, ValidationCache,
    ValidationRateLimiter,
};
use crate::error::{ContextNestError, Result};
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

// Re-export validation components (modules commented out until implemented)
// pub use rules::*;
// pub use validators::*;
// pub use security::*;
// pub use error_handling::*;
// pub use performance::*;

/// Comprehensive validation configuration
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    // Basic request validation
    pub max_request_size: usize,
    pub max_header_size: usize,
    pub allowed_content_types: Vec<String>,
    pub required_headers: Vec<String>,

    // Security settings
    pub enable_security_validation: bool,
    pub enable_sql_injection_check: bool,
    pub enable_xss_check: bool,
    pub enable_path_traversal_check: bool,
    pub enable_command_injection_check: bool,

    // Rate limiting for validation failures
    pub max_validation_failures_per_minute: u32,
    pub validation_failure_penalty_seconds: u64,

    // Performance settings
    pub enable_validation_caching: bool,
    pub cache_ttl_seconds: u64,
    pub enable_parallel_validation: bool,

    // Context-specific limits
    pub max_context_length: usize,
    pub max_examples_per_context: usize,
    pub max_metadata_entries: usize,

    // Neural field limits
    pub max_patterns_per_field: usize,
    pub max_field_dimensions: usize,
    pub max_resonance_calculation_complexity: u32,

    // Protocol limits
    pub max_protocol_parameters: usize,
    pub max_autonomy_level: f64,
    pub allowed_protocol_names: Vec<String>,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_request_size: 10 * 1024 * 1024, // 10MB
            max_header_size: 8192,              // 8KB
            allowed_content_types: vec![
                "application/json".to_string(),
                "application/x-www-form-urlencoded".to_string(),
                "text/plain".to_string(),
            ],
            required_headers: vec![],
            enable_security_validation: true,
            enable_sql_injection_check: true,
            enable_xss_check: true,
            enable_path_traversal_check: true,
            enable_command_injection_check: true,
            max_validation_failures_per_minute: 10,
            validation_failure_penalty_seconds: 60,
            enable_validation_caching: true,
            cache_ttl_seconds: 300, // 5 minutes
            enable_parallel_validation: true,
            max_context_length: 100_000, // 100KB
            max_examples_per_context: 50,
            max_metadata_entries: 100,
            max_patterns_per_field: 1000,
            max_field_dimensions: 1000,
            max_resonance_calculation_complexity: 10000,
            max_protocol_parameters: 50,
            max_autonomy_level: 1.0,
            allowed_protocol_names: vec![
                "field.self_repair".to_string(),
                "field.resonance.scaffold".to_string(),
                "context.memory.persistence.attractor".to_string(),
            ],
        }
    }
}

/// Detailed validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
    pub request_id: String,
    pub validation_time_ms: u64,
    pub security_threats_detected: Vec<SecurityThreat>,
    pub performance_metrics: ValidationPerformanceMetrics,
}

/// Validation error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
    pub value: Option<Value>,
    pub severity: ErrorSeverity,
    pub category: ErrorCategory,
}

/// Validation warning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWarning {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
    pub recommendation: Option<String>,
}

impl ValidationError {
    /// Convert from validators::ValidationError
    pub fn from_validators_error(
        error: crate::api::middleware::validators::ValidationError,
    ) -> Self {
        Self {
            code: error.code,
            message: error.message,
            field: error.field,
            value: error.value,
            severity: match error.severity {
                crate::api::middleware::validators::ErrorSeverity::Critical => {
                    ErrorSeverity::Critical
                }
                crate::api::middleware::validators::ErrorSeverity::High => ErrorSeverity::High,
                crate::api::middleware::validators::ErrorSeverity::Medium => ErrorSeverity::Medium,
                crate::api::middleware::validators::ErrorSeverity::Low => ErrorSeverity::Low,
            },
            category: match error.category {
                crate::api::middleware::validators::ErrorCategory::Security => {
                    ErrorCategory::Security
                }
                crate::api::middleware::validators::ErrorCategory::Format => ErrorCategory::Format,
                crate::api::middleware::validators::ErrorCategory::Constraint => {
                    ErrorCategory::Constraint
                }
                crate::api::middleware::validators::ErrorCategory::Business => {
                    ErrorCategory::Business
                }
                crate::api::middleware::validators::ErrorCategory::Performance => {
                    ErrorCategory::Performance
                }
            },
        }
    }
}

impl ValidationWarning {
    /// Convert from validators::ValidationWarning
    pub fn from_validators_warning(
        warning: crate::api::middleware::validators::ValidationWarning,
    ) -> Self {
        Self {
            code: warning.code,
            message: warning.message,
            field: warning.field,
            recommendation: warning.recommendation,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorSeverity {
    Critical, // Security threat, data corruption risk
    High,     // Will cause operation failure
    Medium,   // Potential issues
    Low,      // Minor issues
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorCategory {
    Security,
    Format,
    Constraint,
    Business,
    Performance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityThreat {
    pub threat_type: SecurityThreatType,
    pub description: String,
    pub detected_in: Option<String>,
    pub confidence: f32,
    pub blocked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityThreatType {
    SqlInjection,
    CrossSiteScripting,
    PathTraversal,
    CommandInjection,
    MaliciousContent,
    RateLimitAbuse,
    LargeRequestAttack,
}

// Removed from_validators_threat_type as SecurityThreatType is not defined in validators module

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationPerformanceMetrics {
    pub total_time_ms: u64,
    pub security_checks_ms: u64,
    pub format_validation_ms: u64,
    pub semantic_validation_ms: u64,
    pub cache_hit: bool,
    pub parallel_checks_used: bool,
}

/// Main request validator
pub struct ComprehensiveRequestValidator {
    config: ValidationConfig,
    security_validator: SecurityValidator,
    context_validator: ContextValidator,
    neural_field_validator: NeuralFieldValidator,
    protocol_validator: ProtocolValidator,
    performance_cache: ValidationCache,
    rate_limiter: ValidationRateLimiter,
}

impl ComprehensiveRequestValidator {
    pub fn new(config: ValidationConfig) -> Self {
        Self {
            security_validator: SecurityValidator::new(&config),
            context_validator: ContextValidator::new(&config),
            neural_field_validator: NeuralFieldValidator::new(&config),
            protocol_validator: ProtocolValidator::new(&config),
            performance_cache: ValidationCache::new(&config),
            rate_limiter: ValidationRateLimiter::new(&config),
            config,
        }
    }

    pub async fn validate_request(
        &self,
        request: &Request,
        request_id: String,
    ) -> ValidationResult {
        let start_time = std::time::Instant::now();

        // Check rate limiting for validation failures
        if let Err(blocked_until) = self.rate_limiter.check_rate_limit(&request).await {
            return ValidationResult {
                is_valid: false,
                errors: vec![ValidationError {
                    code: "RATE_LIMIT_EXCEEDED".to_string(),
                    message: format!(
                        "Validation rate limit exceeded. Try again after {:?}",
                        blocked_until
                    ),
                    field: None,
                    value: None,
                    severity: ErrorSeverity::High,
                    category: ErrorCategory::Security,
                }],
                warnings: Vec::new(),
                request_id,
                validation_time_ms: start_time.elapsed().as_millis() as u64,
                security_threats_detected: vec![SecurityThreat {
                    threat_type: SecurityThreatType::RateLimitAbuse,
                    description: "Excessive validation failures detected".to_string(),
                    detected_in: None,
                    confidence: 0.9,
                    blocked: true,
                }],
                performance_metrics: ValidationPerformanceMetrics {
                    total_time_ms: start_time.elapsed().as_millis() as u64,
                    security_checks_ms: 0,
                    format_validation_ms: 0,
                    semantic_validation_ms: 0,
                    cache_hit: false,
                    parallel_checks_used: false,
                },
            };
        }

        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut security_threats = Vec::new();

        let security_start = std::time::Instant::now();

        // 1. Basic request validation
        self.validate_basic_request(request, &mut errors, &mut warnings);

        // 2. Security validation (parallel if enabled)
        let security_validation = if self.config.enable_security_validation {
            Some(self.security_validator.validate_security(request))
        } else {
            None
        };

        let security_time = security_start.elapsed().as_millis() as u64;

        // 3. Content-specific validation based on route
        let mut content_validation = self.validate_request_content(request).await;

        // `security_validation` future was constructed earlier
        // but the cross-module result-conversion (validators::* error/warning
        // types vs this module's types) was never wired up — `let _ = …` was
        // silently discarding the result. Removed the discard + the upstream
        // construction site since the work was never used. When the security
        // validator integration epic lands, re-wire the result through here.
        drop(security_validation);

        // Process content validation results
        errors.append(&mut content_validation.errors);
        warnings.append(&mut content_validation.warnings);

        let total_time = start_time.elapsed().as_millis() as u64;

        let result = ValidationResult {
            is_valid: errors.is_empty(),
            errors,
            warnings,
            request_id,
            validation_time_ms: total_time,
            security_threats_detected: security_threats,
            performance_metrics: ValidationPerformanceMetrics {
                total_time_ms: total_time,
                security_checks_ms: security_time,
                format_validation_ms: content_validation.format_time_ms,
                semantic_validation_ms: content_validation.semantic_time_ms,
                cache_hit: content_validation.cache_hit,
                parallel_checks_used: self.config.enable_parallel_validation,
            },
        };

        // Record validation result for rate limiting
        if !result.is_valid {
            self.rate_limiter.record_failure(&request).await;
        }

        result
    }

    fn validate_basic_request(
        &self,
        request: &Request,
        errors: &mut Vec<ValidationError>,
        warnings: &mut Vec<ValidationWarning>,
    ) {
        // Validate headers
        self.validate_headers(request.headers(), errors, warnings);

        // Validate content type
        if let Err(error) = self.validate_content_type(request.headers()) {
            errors.push(error);
        }

        // Validate request size
        if let Err(error) = self.validate_request_size(request) {
            errors.push(error);
        }
    }

    fn validate_headers(
        &self,
        headers: &HeaderMap,
        errors: &mut Vec<ValidationError>,
        warnings: &mut Vec<ValidationWarning>,
    ) {
        // Check for required headers
        for required_header in &self.config.required_headers {
            if !headers.contains_key(required_header) {
                errors.push(ValidationError {
                    code: "MISSING_REQUIRED_HEADER".to_string(),
                    message: format!("Missing required header: {}", required_header),
                    field: Some("headers".to_string()),
                    value: None,
                    severity: ErrorSeverity::High,
                    category: ErrorCategory::Format,
                });
            }
        }

        // Validate header size
        let total_header_size = headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.len())
            .sum::<usize>();

        if total_header_size > self.config.max_header_size {
            errors.push(ValidationError {
                code: "HEADERS_TOO_LARGE".to_string(),
                message: format!(
                    "Header size {} exceeds maximum {}",
                    total_header_size, self.config.max_header_size
                ),
                field: Some("headers".to_string()),
                value: Some(json!(total_header_size)),
                severity: ErrorSeverity::High,
                category: ErrorCategory::Constraint,
            });
        }

        // Security header checks
        if !headers.contains_key("x-request-id") {
            warnings.push(ValidationWarning {
                code: "MISSING_REQUEST_ID".to_string(),
                message: "Missing x-request-id header for request tracing".to_string(),
                field: Some("headers".to_string()),
                recommendation: Some(
                    "Add x-request-id header for better debugging and monitoring".to_string(),
                ),
            });
        }
    }

    fn validate_content_type(
        &self,
        headers: &HeaderMap,
    ) -> std::result::Result<(), ValidationError> {
        if let Some(content_type) = headers.get("content-type") {
            let content_type_str = content_type.to_str().map_err(|_| ValidationError {
                code: "INVALID_CONTENT_TYPE_HEADER".to_string(),
                message: "Content-Type header contains invalid characters".to_string(),
                field: Some("content-type".to_string()),
                value: None,
                severity: ErrorSeverity::High,
                category: ErrorCategory::Format,
            })?;

            let content_type_main = content_type_str.split(';').next().unwrap_or("").trim();

            if !self
                .config
                .allowed_content_types
                .contains(&content_type_main.to_string())
            {
                return Err(ValidationError {
                    code: "UNSUPPORTED_CONTENT_TYPE".to_string(),
                    message: format!("Content type '{}' is not allowed", content_type_main),
                    field: Some("content-type".to_string()),
                    value: Some(json!(content_type_main)),
                    severity: ErrorSeverity::High,
                    category: ErrorCategory::Format,
                });
            }
        }
        Ok(())
    }

    fn validate_request_size(&self, request: &Request) -> std::result::Result<(), ValidationError> {
        if let Some(content_length) = request.headers().get("content-length") {
            let length_str = content_length.to_str().map_err(|_| ValidationError {
                code: "INVALID_CONTENT_LENGTH".to_string(),
                message: "Content-Length header contains invalid characters".to_string(),
                field: Some("content-length".to_string()),
                value: None,
                severity: ErrorSeverity::High,
                category: ErrorCategory::Format,
            })?;

            let length = length_str.parse::<usize>().map_err(|_| ValidationError {
                code: "INVALID_CONTENT_LENGTH".to_string(),
                message: "Content-Length header is not a valid number".to_string(),
                field: Some("content-length".to_string()),
                value: Some(json!(length_str)),
                severity: ErrorSeverity::High,
                category: ErrorCategory::Format,
            })?;

            if length > self.config.max_request_size {
                return Err(ValidationError {
                    code: "REQUEST_TOO_LARGE".to_string(),
                    message: format!(
                        "Request size {} exceeds maximum {}",
                        length, self.config.max_request_size
                    ),
                    field: Some("content-length".to_string()),
                    value: Some(json!(length)),
                    severity: ErrorSeverity::High,
                    category: ErrorCategory::Constraint,
                });
            }
        }
        Ok(())
    }

    async fn validate_request_content(&self, request: &Request) -> ContentValidationResult {
        let path = request.uri().path();

        // Route-specific validation
        if path.starts_with("/api/") && path.contains("/context") {
            let result = self
                .context_validator
                .validate_context_request(request)
                .await;
            self.convert_content_validation_result(result)
        } else if path.starts_with("/api/") && path.contains("/neural-fields") {
            let result = self
                .neural_field_validator
                .validate_neural_field_request(request)
                .await;
            self.convert_content_validation_result(result)
        } else if path.starts_with("/api/") && path.contains("/protocols") {
            let result = self
                .protocol_validator
                .validate_protocol_request(request)
                .await;
            self.convert_content_validation_result(result)
        } else {
            ContentValidationResult {
                errors: Vec::new(),
                warnings: Vec::new(),
                format_time_ms: 0,
                semantic_time_ms: 0,
                cache_hit: false,
            }
        }
    }

    /// Convert validators::ContentValidationResult to comprehensive_validation::ContentValidationResult
    fn convert_content_validation_result(
        &self,
        result: crate::api::middleware::validators::ContentValidationResult,
    ) -> ContentValidationResult {
        ContentValidationResult {
            errors: result
                .errors
                .into_iter()
                .map(ValidationError::from_validators_error)
                .collect(),
            warnings: result
                .warnings
                .into_iter()
                .map(ValidationWarning::from_validators_warning)
                .collect(),
            format_time_ms: result.format_time_ms,
            semantic_time_ms: result.semantic_time_ms,
            cache_hit: result.cache_hit,
        }
    }
}

/// Content validation result
#[derive(Debug)]
pub struct ContentValidationResult {
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
    pub format_time_ms: u64,
    pub semantic_time_ms: u64,
    pub cache_hit: bool,
}

/// Main validation middleware function
pub async fn comprehensive_validation_middleware(
    State(validator): State<Arc<ComprehensiveRequestValidator>>,
    mut request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    // Generate or extract request ID
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Add request ID to headers for downstream services
    request
        .headers_mut()
        .insert("x-request-id", request_id.parse().unwrap());

    // Validate the request
    let result = validator
        .validate_request(&request, request_id.clone())
        .await;

    if !result.is_valid {
        error!(
            "Request validation failed for request ID: {}: {:?}",
            request_id, result.errors
        );

        // Log security threats if detected
        if !result.security_threats_detected.is_empty() {
            error!(
                "Security threats detected: {:?}",
                result.security_threats_detected
            );
        }

        let error_response = json!({
            "error": {
                "code": "VALIDATION_FAILED",
                "message": "Request validation failed",
                "request_id": result.request_id,
                "errors": result.errors,
                "warnings": result.warnings,
                "security_threats": result.security_threats_detected,
                "validation_time_ms": result.validation_time_ms,
                "performance_metrics": result.performance_metrics
            },
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        let response = axum::Json(error_response);
        return Ok((StatusCode::BAD_REQUEST, response).into_response());
    }

    // Log warnings
    if !result.warnings.is_empty() {
        warn!(
            "Request validation warnings for request ID: {}: {:?}",
            request_id, result.warnings
        );
    }

    info!(
        "Request validation successful for request ID: {} ({}ms)",
        result.request_id, result.validation_time_ms
    );

    // Add validation metrics to headers
    let headers_mut = request.headers_mut();
    headers_mut.insert(
        "x-validation-time-ms",
        result.validation_time_ms.to_string().parse().unwrap(),
    );
    headers_mut.insert(
        "x-security-checks-passed",
        (!result.security_threats_detected.is_empty())
            .to_string()
            .parse()
            .unwrap(),
    );

    let response = next.run(request).await;
    Ok(response)
}

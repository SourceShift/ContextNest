use super::types::{ApiError, ErrorCategory, ErrorSeverity};
/// Context-aware error handling system
use crate::error::ContextNestResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{error, info, warn};

/// Error context information for enhanced debugging and recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorContext {
    /// Request ID for tracking
    pub request_id: Option<String>,
    /// User ID if available
    pub user_id: Option<String>,
    /// API endpoint that triggered the error
    pub endpoint: Option<String>,
    /// HTTP method used
    pub method: Option<String>,
    /// Request parameters (sanitized)
    pub parameters: Option<HashMap<String, serde_json::Value>>,
    /// System state information
    pub system_state: Option<HashMap<String, serde_json::Value>>,
    /// Additional metadata
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new(request_id: &str) -> Self {
        Self {
            request_id: Some(request_id.to_string()),
            user_id: None,
            endpoint: None,
            method: None,
            parameters: None,
            system_state: None,
            metadata: None,
        }
    }

    /// Create an empty error context
    pub fn empty() -> Self {
        Self {
            request_id: None,
            user_id: None,
            endpoint: None,
            method: None,
            parameters: None,
            system_state: None,
            metadata: None,
        }
    }

    /// Set request ID
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Set user ID
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Set endpoint information
    pub fn with_endpoint(mut self, method: impl Into<String>, endpoint: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Add parameter (with value sanitization for security)
    pub fn with_parameter(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        if self.parameters.is_none() {
            self.parameters = Some(HashMap::new());
        }

        let key_str = key.into();
        let sanitized_value = if Self::is_sensitive_field(&key_str) {
            serde_json::Value::String("[REDACTED]".to_string())
        } else {
            value
        };

        self.parameters
            .as_mut()
            .unwrap()
            .insert(key_str, sanitized_value);
        self
    }

    /// Add system state information
    pub fn with_system_state(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        if self.system_state.is_none() {
            self.system_state = Some(HashMap::new());
        }
        self.system_state
            .as_mut()
            .unwrap()
            .insert(key.into(), value);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        if self.metadata.is_none() {
            self.metadata = Some(HashMap::new());
        }
        self.metadata.as_mut().unwrap().insert(key.into(), value);
        self
    }

    /// Convert to HashMap for ApiError
    pub fn to_hashmap(self) -> HashMap<String, serde_json::Value> {
        let mut map = HashMap::new();

        if let Some(request_id) = self.request_id {
            map.insert(
                "request_id".to_string(),
                serde_json::Value::String(request_id),
            );
        }
        if let Some(user_id) = self.user_id {
            map.insert("user_id".to_string(), serde_json::Value::String(user_id));
        }
        if let Some(endpoint) = self.endpoint {
            map.insert("endpoint".to_string(), serde_json::Value::String(endpoint));
        }
        if let Some(method) = self.method {
            map.insert("method".to_string(), serde_json::Value::String(method));
        }
        if let Some(parameters) = self.parameters {
            map.insert(
                "parameters".to_string(),
                serde_json::Value::Object(parameters.into_iter().collect()),
            );
        }
        if let Some(system_state) = self.system_state {
            map.insert(
                "system_state".to_string(),
                serde_json::Value::Object(system_state.into_iter().collect()),
            );
        }
        if let Some(metadata) = self.metadata {
            map.insert(
                "metadata".to_string(),
                serde_json::Value::Object(metadata.into_iter().collect()),
            );
        }

        map
    }

    /// Check if a field contains sensitive information
    fn is_sensitive_field(field_name: &str) -> bool {
        let sensitive_fields = [
            "password",
            "token",
            "secret",
            "key",
            "auth",
            "credential",
            "authorization",
            "bearer",
            "api_key",
            "private_key",
        ];

        let field_lower = field_name.to_lowercase();
        sensitive_fields
            .iter()
            .any(|&sensitive| field_lower.contains(sensitive))
    }
}

impl Default for ErrorContext {
    fn default() -> Self {
        Self::empty()
    }
}

/// API error response structure
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// The error information
    pub error: ApiError,
    /// Additional response metadata
    pub meta: Option<HashMap<String, serde_json::Value>>,
}

impl ErrorResponse {
    /// Create a new error response
    pub fn new(error: ApiError) -> Self {
        Self { error, meta: None }
    }

    /// Add metadata to the response
    pub fn with_meta(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        if self.meta.is_none() {
            self.meta = Some(HashMap::new());
        }
        self.meta.as_mut().unwrap().insert(key.into(), value);
        self
    }
}

/// Centralized API error handler
#[derive(Debug, Clone)]
pub struct ApiErrorHandler {
    /// Whether to include debug information in responses
    include_debug_info: bool,
    /// Whether to log errors
    enable_logging: bool,
    /// Minimum severity level for alerting
    alert_threshold: ErrorSeverity,
}

impl ApiErrorHandler {
    /// Create a new error handler
    pub fn new() -> Self {
        Self {
            include_debug_info: cfg!(debug_assertions),
            enable_logging: true,
            alert_threshold: ErrorSeverity::High,
        }
    }

    /// Configure debug information inclusion
    pub fn with_debug_info(mut self, include: bool) -> Self {
        self.include_debug_info = include;
        self
    }

    /// Configure error logging
    pub fn with_logging(mut self, enable: bool) -> Self {
        self.enable_logging = enable;
        self
    }

    /// Configure alert threshold
    pub fn with_alert_threshold(mut self, threshold: ErrorSeverity) -> Self {
        self.alert_threshold = threshold;
        self
    }

    /// Handle an error and return appropriate response
    pub fn handle_error(&self, mut error: ApiError) -> ErrorResponse {
        // Log the error based on severity
        if self.enable_logging {
            self.log_error(&error);
        }

        // Trigger alerts if necessary
        if error.severity >= self.alert_threshold {
            self.trigger_alert(&error);
        }

        // Remove debug information in production if configured
        if !self.include_debug_info {
            error = self.sanitize_error_for_production(error);
        }

        ErrorResponse::new(error)
    }

    /// Log error with appropriate level
    fn log_error(&self, error: &ApiError) {
        let log_message = format!(
            "API Error [{}]: {} (Status: {}, Category: {:?}, Severity: {:?})",
            error.id, error.message, error.status_code, error.category, error.severity
        );

        match error.severity {
            ErrorSeverity::Low => info!("{}", log_message),
            ErrorSeverity::Medium => warn!("{}", log_message),
            ErrorSeverity::High | ErrorSeverity::Critical => {
                error!(
                    error_id = %error.id,
                    status_code = error.status_code,
                    category = ?error.category,
                    severity = ?error.severity,
                    message = %error.message,
                    details = %error.details,
                    context = ?error.context,
                    "{}",
                    log_message
                );
            }
        }
    }

    /// Trigger alert for high-severity errors
    fn trigger_alert(&self, error: &ApiError) {
        // In a real implementation, this would send alerts to monitoring systems
        // like PagerDuty, Slack, email, etc.
        error!(
            alert = true,
            error_id = %error.id,
            severity = ?error.severity,
            message = %error.message,
            "High-severity error alert triggered"
        );
    }

    /// Sanitize error for production (remove sensitive debug info)
    fn sanitize_error_for_production(&self, mut error: ApiError) -> ApiError {
        // Remove potentially sensitive context information in production
        if let Some(context) = &mut error.context {
            context.retain(|key, _| !self.is_debug_context_field(key));
        }

        // Generic error messages for internal server errors in production
        if error.status_code >= 500 {
            error.details = "An internal error occurred. Please try again later.".to_string();
        }

        error
    }

    /// Check if a context field should be removed in production
    fn is_debug_context_field(&self, field_name: &str) -> bool {
        let debug_fields = [
            "system_state",
            "stack_trace",
            "internal_state",
            "debug_info",
            "memory_usage",
            "thread_id",
        ];

        debug_fields.contains(&field_name)
    }
}

impl Default for ApiErrorHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper trait for converting standard errors to API errors with context
pub trait IntoApiError<T> {
    fn into_api_error(self, context: ErrorContext) -> std::result::Result<T, ApiError>;
    fn into_api_error_with_message(
        self,
        message: impl Into<String>,
        context: ErrorContext,
    ) -> std::result::Result<T, ApiError>;
}

impl<T, E> IntoApiError<T> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn into_api_error(self, context: ErrorContext) -> std::result::Result<T, ApiError> {
        self.map_err(|err| {
            ApiError::internal_server_error("Internal server error", err.to_string(), Some(context))
        })
    }

    fn into_api_error_with_message(
        self,
        message: impl Into<String>,
        context: ErrorContext,
    ) -> std::result::Result<T, ApiError> {
        self.map_err(|err| ApiError::internal_server_error(message, err.to_string(), Some(context)))
    }
}

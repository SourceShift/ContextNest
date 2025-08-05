use chrono::{DateTime, Utc};
/// API Error types and classifications
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use uuid::Uuid;

/// Main error type for ContextNest operations
#[derive(Debug, Clone)]
pub enum ContextNestError {
    /// Parser-related errors
    Parser(String),
    /// Database operation errors
    Database(String),
    /// API errors
    Api(String),
    /// Configuration errors (legacy name)
    Config(String),
    /// Configuration errors (new name)
    Configuration(String),
    /// I/O errors
    Io(String),
    /// Serialization errors
    Serialization(String),
    /// HTTP/network errors
    Http(String),
    /// Regex pattern errors
    Regex(String),
    /// Thread pool errors
    ThreadPool(String),
    /// Validation errors
    Validation(String),
    /// Parse errors
    ParseError(String),
    /// System time errors
    SystemTime(String),
    /// Query errors
    Query(String),
    /// Service unavailable errors
    ServiceUnavailable(String),
    /// Service overloaded errors
    ServiceOverloaded(String),
    /// Max retries exceeded
    MaxRetriesExceeded(String),
    /// Timeout errors
    Timeout(String),
    /// Network errors
    NetworkError(String),
    /// Internal server errors
    InternalServerError(String),
    /// Security-related errors
    Security(String),
    /// Cryptographic errors
    Crypto(String),
    /// Communication errors
    Communication(String),
    /// Resource exhausted errors
    ResourceExhausted(String),
    /// Execution errors
    Execution(String),
    /// Not found errors
    NotFound(String),
    /// Cancelled operations (protocol execution cancellation)
    Cancelled(String),
    /// Authentication errors
    Authentication(String),
    /// Parse errors (alias for ParseError)
    Parse(String),
    /// Protocol errors (for protocol system failures)
    ProtocolError(String),
    /// Execution timeout errors
    ExecutionTimeout(String),
    /// Security errors (for protocol security validation)
    SecurityError(String),
    /// Resource errors (for protocol resource management)
    ResourceError(String),
    /// Validation errors (for protocol validation)
    ValidationError(String),
    /// Service errors (for external service integration)
    ServiceError(String),
    /// Event errors (for system event handling)
    EventError(String),
    /// Authentication errors (for protocol authentication)
    AuthenticationError(String),
}

impl fmt::Display for ContextNestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextNestError::Parser(msg) => write!(f, "Parser error: {}", msg),
            ContextNestError::Database(msg) => write!(f, "Database error: {}", msg),
            ContextNestError::Api(msg) => write!(f, "API error: {}", msg),
            ContextNestError::Config(msg) => write!(f, "Configuration error: {}", msg),
            ContextNestError::Configuration(msg) => write!(f, "Configuration error: {}", msg),
            ContextNestError::Io(msg) => write!(f, "I/O error: {}", msg),
            ContextNestError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            ContextNestError::Http(msg) => write!(f, "HTTP error: {}", msg),
            ContextNestError::Regex(msg) => write!(f, "Regex error: {}", msg),
            ContextNestError::ThreadPool(msg) => write!(f, "Thread pool error: {}", msg),
            ContextNestError::Validation(msg) => write!(f, "Validation error: {}", msg),
            ContextNestError::ParseError(msg) => write!(f, "Parse error: {}", msg),
            ContextNestError::SystemTime(msg) => write!(f, "System time error: {}", msg),
            ContextNestError::Query(msg) => write!(f, "Query error: {}", msg),
            ContextNestError::ServiceUnavailable(msg) => write!(f, "Service unavailable: {}", msg),
            ContextNestError::ServiceOverloaded(msg) => write!(f, "Service overloaded: {}", msg),
            ContextNestError::MaxRetriesExceeded(msg) => write!(f, "Max retries exceeded: {}", msg),
            ContextNestError::Timeout(msg) => write!(f, "Timeout error: {}", msg),
            ContextNestError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            ContextNestError::InternalServerError(msg) => {
                write!(f, "Internal server error: {}", msg)
            }
            ContextNestError::Security(msg) => write!(f, "Security error: {}", msg),
            ContextNestError::Crypto(msg) => write!(f, "Crypto error: {}", msg),
            ContextNestError::Communication(msg) => write!(f, "Communication error: {}", msg),
            ContextNestError::ResourceExhausted(msg) => write!(f, "Resource exhausted: {}", msg),
            ContextNestError::Execution(msg) => write!(f, "Execution error: {}", msg),
            ContextNestError::NotFound(msg) => write!(f, "Not found: {}", msg),
            ContextNestError::Cancelled(msg) => write!(f, "Operation cancelled: {}", msg),
            ContextNestError::Authentication(msg) => write!(f, "Authentication error: {}", msg),
            ContextNestError::Parse(msg) => write!(f, "Parse error: {}", msg),
            ContextNestError::ProtocolError(msg) => write!(f, "Protocol error: {}", msg),
            ContextNestError::ExecutionTimeout(msg) => write!(f, "Execution timeout: {}", msg),
            ContextNestError::SecurityError(msg) => write!(f, "Security error: {}", msg),
            ContextNestError::ResourceError(msg) => write!(f, "Resource error: {}", msg),
            ContextNestError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
            ContextNestError::ServiceError(msg) => write!(f, "Service error: {}", msg),
            ContextNestError::EventError(msg) => write!(f, "Event error: {}", msg),
            ContextNestError::AuthenticationError(msg) => {
                write!(f, "Authentication error: {}", msg)
            }
        }
    }
}

impl std::error::Error for ContextNestError {}

// Implement From traits for common error types
impl From<std::io::Error> for ContextNestError {
    fn from(err: std::io::Error) -> Self {
        ContextNestError::Io(err.to_string())
    }
}

impl From<serde_json::Error> for ContextNestError {
    fn from(err: serde_json::Error) -> Self {
        ContextNestError::Serialization(err.to_string())
    }
}

impl From<&str> for ContextNestError {
    fn from(err: &str) -> Self {
        ContextNestError::Configuration(err.to_string())
    }
}

impl From<String> for ContextNestError {
    fn from(err: String) -> Self {
        ContextNestError::Configuration(err)
    }
}

impl From<reqwest::Error> for ContextNestError {
    fn from(err: reqwest::Error) -> Self {
        ContextNestError::Http(err.to_string())
    }
}

impl From<regex::Error> for ContextNestError {
    fn from(err: regex::Error) -> Self {
        ContextNestError::Regex(err.to_string())
    }
}

impl From<std::time::SystemTimeError> for ContextNestError {
    fn from(err: std::time::SystemTimeError) -> Self {
        ContextNestError::SystemTime(err.to_string())
    }
}

impl From<tokio::sync::AcquireError> for ContextNestError {
    fn from(err: tokio::sync::AcquireError) -> Self {
        ContextNestError::ThreadPool(err.to_string())
    }
}

impl From<tokio::task::JoinError> for ContextNestError {
    fn from(err: tokio::task::JoinError) -> Self {
        ContextNestError::ThreadPool(err.to_string())
    }
}

impl From<ApiError> for ContextNestError {
    fn from(err: ApiError) -> Self {
        ContextNestError::Api(err.message)
    }
}

/// Result type alias for ContextNest operations
pub type ContextNestResult<T> = std::result::Result<T, ContextNestError>;

/// Comprehensive API error structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    /// Unique error ID for tracking
    pub id: Uuid,
    /// HTTP status code
    pub status_code: u16,
    /// Error category for classification
    pub category: ErrorCategory,
    /// Error severity level
    pub severity: ErrorSeverity,
    /// Human-readable error message
    pub message: String,
    /// Detailed error description
    pub details: String,
    /// Error timestamp
    pub timestamp: DateTime<Utc>,
    /// Additional context information
    pub context: Option<HashMap<String, serde_json::Value>>,
    /// Suggested actions for recovery
    pub suggestions: Vec<String>,
    /// Whether this error is retryable
    pub retryable: bool,
    /// Retry delay suggestion in milliseconds
    pub retry_after_ms: Option<u64>,
}

/// Error categories for classification
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Client errors (4xx)
    Client,
    /// Server errors (5xx)
    Server,
    /// External service errors
    External,
    /// Validation errors
    Validation,
    /// Authentication/authorization errors
    Auth,
    /// Rate limiting errors
    RateLimit,
    /// Business logic errors
    Business,
    /// Network errors
    Network,
}

/// Error severity levels
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorSeverity {
    /// Low severity - informational
    Low,
    /// Medium severity - warning
    Medium,
    /// High severity - error
    High,
    /// Critical severity - system failure
    Critical,
}

impl ApiError {
    /// Create a new API error
    pub fn new(
        status_code: u16,
        category: ErrorCategory,
        severity: ErrorSeverity,
        message: impl Into<String>,
        details: impl Into<String>,
        context: Option<HashMap<String, serde_json::Value>>,
    ) -> Self {
        let retryable = Self::is_retryable_status(status_code);
        let retry_after_ms = if retryable { Some(1000) } else { None };

        Self {
            id: Uuid::new_v4(),
            status_code,
            category,
            severity,
            message: message.into(),
            details: details.into(),
            timestamp: Utc::now(),
            context,
            suggestions: Vec::new(),
            retryable,
            retry_after_ms,
        }
    }

    /// Create a bad request error (400)
    pub fn bad_request(
        message: impl Into<String>,
        details: impl Into<String>,
        context: Option<super::ErrorContext>,
    ) -> Self {
        let mut error = Self::new(
            400,
            ErrorCategory::Client,
            ErrorSeverity::Medium,
            message,
            details,
            context.map(|c| c.to_hashmap()),
        );
        error
            .suggestions
            .push("Check your request parameters and try again".to_string());
        error
    }

    /// Create an unauthorized error (401)
    pub fn unauthorized(
        message: impl Into<String>,
        details: impl Into<String>,
        context: Option<super::ErrorContext>,
    ) -> Self {
        let mut error = Self::new(
            401,
            ErrorCategory::Auth,
            ErrorSeverity::High,
            message,
            details,
            context.map(|c| c.to_hashmap()),
        );
        error
            .suggestions
            .push("Check your authentication credentials".to_string());
        error
    }

    /// Create a forbidden error (403)
    pub fn forbidden(
        message: impl Into<String>,
        details: impl Into<String>,
        context: Option<super::ErrorContext>,
    ) -> Self {
        let mut error = Self::new(
            403,
            ErrorCategory::Auth,
            ErrorSeverity::High,
            message,
            details,
            context.map(|c| c.to_hashmap()),
        );
        error
            .suggestions
            .push("Check your permissions for this resource".to_string());
        error
    }

    /// Create a not found error (404)
    pub fn not_found(
        message: impl Into<String>,
        details: impl Into<String>,
        context: Option<super::ErrorContext>,
    ) -> Self {
        let mut error = Self::new(
            404,
            ErrorCategory::Client,
            ErrorSeverity::Medium,
            message,
            details,
            context.map(|c| c.to_hashmap()),
        );
        error
            .suggestions
            .push("Check the resource path and try again".to_string());
        error
    }

    /// Create a rate limit error (429)
    pub fn rate_limited(
        message: impl Into<String>,
        details: impl Into<String>,
        retry_after_ms: u64,
        context: Option<super::ErrorContext>,
    ) -> Self {
        let mut error = Self::new(
            429,
            ErrorCategory::RateLimit,
            ErrorSeverity::Medium,
            message,
            details,
            context.map(|c| c.to_hashmap()),
        );
        error.retry_after_ms = Some(retry_after_ms);
        error
            .suggestions
            .push(format!("Wait {} ms before retrying", retry_after_ms));
        error
    }

    /// Create an internal server error (500)
    pub fn internal_server_error(
        message: impl Into<String>,
        details: impl Into<String>,
        context: Option<super::ErrorContext>,
    ) -> Self {
        let mut error = Self::new(
            500,
            ErrorCategory::Server,
            ErrorSeverity::High,
            message,
            details,
            context.map(|c| c.to_hashmap()),
        );
        error
            .suggestions
            .push("Try again later or contact support if the problem persists".to_string());
        error
    }

    /// Create a service unavailable error (503)
    pub fn service_unavailable(
        message: impl Into<String>,
        details: impl Into<String>,
        context: Option<super::ErrorContext>,
    ) -> Self {
        let mut error = Self::new(
            503,
            ErrorCategory::External,
            ErrorSeverity::High,
            message,
            details,
            context.map(|c| c.to_hashmap()),
        );
        error.retry_after_ms = Some(5000); // 5 seconds
        error
            .suggestions
            .push("Service is temporarily unavailable, please try again later".to_string());
        error
    }

    /// Add a suggestion for error recovery
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }

    /// Add context information
    pub fn with_context(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        if self.context.is_none() {
            self.context = Some(HashMap::new());
        }
        self.context.as_mut().unwrap().insert(key.into(), value);
        self
    }

    /// Set retry delay
    pub fn with_retry_after(mut self, retry_after_ms: u64) -> Self {
        self.retry_after_ms = Some(retry_after_ms);
        self.retryable = true;
        self
    }

    /// Check if a status code indicates a retryable error
    fn is_retryable_status(status_code: u16) -> bool {
        matches!(status_code, 429 | 500 | 502 | 503 | 504)
    }

    /// Get the appropriate HTTP status code for this error
    pub fn status_code(&self) -> u16 {
        self.status_code
    }

    /// Check if this error should trigger alerts
    pub fn should_alert(&self) -> bool {
        self.severity >= ErrorSeverity::High
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}: {}", self.id, self.message, self.details)
    }
}

impl std::error::Error for ApiError {}

/// Convert ApiError to axum response
impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let status = axum::http::StatusCode::from_u16(self.status_code)
            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);

        (status, axum::Json(self)).into_response()
    }
}

/// Error type classification for retry strategies
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorType {
    NetworkError,
    ServiceUnavailable,
    InternalServerError,
    Timeout,
    RateLimit,
    Authentication,
    Validation,
    NotFound,
    Forbidden,
    BadRequest,
}

impl ErrorType {
    /// Create ErrorType from ContextNestError
    pub fn from_error(error: &ContextNestError) -> Self {
        match error {
            ContextNestError::NetworkError(_) => ErrorType::NetworkError,
            ContextNestError::ServiceUnavailable(_) => ErrorType::ServiceUnavailable,
            ContextNestError::InternalServerError(_) => ErrorType::InternalServerError,
            ContextNestError::Timeout(_) => ErrorType::Timeout,
            ContextNestError::Http(_) => ErrorType::NetworkError,
            ContextNestError::Database(_) => ErrorType::InternalServerError,
            ContextNestError::Parser(_) => ErrorType::Validation,
            ContextNestError::ParseError(_) => ErrorType::Validation,
            ContextNestError::Serialization(_) => ErrorType::Validation,
            ContextNestError::Regex(_) => ErrorType::Validation,
            ContextNestError::Query(_) => ErrorType::BadRequest,
            _ => ErrorType::InternalServerError,
        }
    }
}

/// Check if an error is retryable based on its type
impl ApiError {
    pub fn is_retryable(&self) -> bool {
        self.retryable
    }
}

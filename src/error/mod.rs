/// Error handling module for ContextNest
/// This module provides comprehensive error handling with:
/// - Context-aware error responses
/// - Structured error types
/// - Retry strategies
/// - Error classification and recovery
pub mod handler;
pub mod recovery;
pub mod retry;
pub mod types;

pub use handler::{ApiErrorHandler, ErrorContext, ErrorResponse};
pub use retry::{BackoffStrategy, RetryPolicy, RetryStrategy};
pub use types::ContextNestResult as Result;
pub use types::{
    ApiError, ContextNestError, ContextNestResult, ErrorCategory, ErrorSeverity, ErrorType,
};

/// Convert internal errors to API responses
pub fn into_api_error(error: ContextNestError, context: Option<ErrorContext>) -> ApiError {
    match error {
        ContextNestError::Parser(msg) => ApiError::bad_request("Invalid input", &msg, context),
        ContextNestError::Database(_) => ApiError::internal_server_error(
            "Database error",
            "Service temporarily unavailable",
            context,
        ),
        ContextNestError::Api(msg) => ApiError::internal_server_error("API error", &msg, context),
        ContextNestError::Config(_) => ApiError::internal_server_error(
            "Configuration error",
            "Service configuration error",
            context,
        ),
        ContextNestError::Configuration(msg) => {
            ApiError::internal_server_error("Configuration error", &msg, context)
        }
        ContextNestError::Io(_) => {
            ApiError::internal_server_error("IO error", "Service I/O error", context)
        }
        ContextNestError::Serialization(_) => {
            ApiError::bad_request("Serialization error", "Invalid data format", context)
        }
        ContextNestError::Http(_) => ApiError::service_unavailable(
            "External service error",
            "External service unavailable",
            context,
        ),
        ContextNestError::Regex(_) => {
            ApiError::bad_request("Pattern error", "Invalid pattern", context)
        }
        ContextNestError::ThreadPool(msg) => {
            ApiError::internal_server_error("Thread pool error", &msg, context)
        }
        ContextNestError::Validation(msg) => {
            ApiError::bad_request("Validation error", &msg, context)
        }
        ContextNestError::ParseError(msg) => ApiError::bad_request("Parse error", &msg, context),
        ContextNestError::SystemTime(_) => {
            ApiError::internal_server_error("System time error", "System time error", context)
        }
        ContextNestError::Query(msg) => ApiError::bad_request("Query error", &msg, context),
        ContextNestError::ServiceUnavailable(msg) => {
            ApiError::service_unavailable("Service unavailable", &msg, context)
        }
        ContextNestError::ServiceOverloaded(msg) => {
            ApiError::service_unavailable("Service overloaded", &msg, context)
        }
        ContextNestError::MaxRetriesExceeded(msg) => {
            ApiError::internal_server_error("Max retries exceeded", &msg, context)
        }
        ContextNestError::Timeout(msg) => {
            ApiError::internal_server_error("Request timeout", &msg, context)
        }
        ContextNestError::NetworkError(msg) => {
            ApiError::service_unavailable("Network error", &msg, context)
        }
        ContextNestError::InternalServerError(msg) => {
            ApiError::internal_server_error("Internal server error", &msg, context)
        }
        ContextNestError::Security(msg) => ApiError::forbidden("Security violation", &msg, context),
        ContextNestError::Crypto(msg) => {
            ApiError::internal_server_error("Cryptographic error", &msg, context)
        }
        ContextNestError::Communication(msg) => {
            ApiError::service_unavailable("Communication error", &msg, context)
        }
        ContextNestError::ResourceExhausted(msg) => {
            ApiError::service_unavailable("Resource exhausted", &msg, context)
        }
        ContextNestError::Execution(msg) => {
            ApiError::internal_server_error("Execution error", &msg, context)
        }
        ContextNestError::NotFound(msg) => ApiError::not_found("Not found", &msg, context),
        ContextNestError::Cancelled(msg) => {
            ApiError::bad_request("Operation cancelled", &msg, context)
        }
        ContextNestError::Authentication(msg) => {
            ApiError::unauthorized("Authentication failed", &msg, context)
        }
        ContextNestError::Parse(msg) => ApiError::bad_request("Parse error", &msg, context),
        ContextNestError::ProtocolError(msg) => {
            ApiError::internal_server_error("Protocol error", &msg, context)
        }
        ContextNestError::ExecutionTimeout(msg) => {
            ApiError::internal_server_error("Execution timeout", &msg, context)
        }
        ContextNestError::SecurityError(msg) => {
            ApiError::forbidden("Security error", &msg, context)
        }
        ContextNestError::ResourceError(msg) => {
            ApiError::service_unavailable("Resource error", &msg, context)
        }
        ContextNestError::ValidationError(msg) => {
            ApiError::bad_request("Validation error", &msg, context)
        }
        ContextNestError::ServiceError(msg) => {
            ApiError::service_unavailable("Service error", &msg, context)
        }
        ContextNestError::EventError(msg) => {
            ApiError::internal_server_error("Event error", &msg, context)
        }
        ContextNestError::AuthenticationError(msg) => {
            ApiError::unauthorized("Authentication error", &msg, context)
        }
    }
}

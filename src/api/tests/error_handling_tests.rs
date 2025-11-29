/// Tests for error handling functionality
#[cfg(test)]
mod tests {
    use crate::error::{ApiError, ApiErrorHandler, ErrorCategory, ErrorContext, ErrorSeverity};

    #[test]
    fn test_api_error_creation() {
        let error = ApiError::bad_request("Test error", "Test details", None);

        assert_eq!(error.status_code, 400);
        assert_eq!(error.category, ErrorCategory::Client);
        assert_eq!(error.severity, ErrorSeverity::Medium);
        assert_eq!(error.message, "Test error");
        assert_eq!(error.details, "Test details");
        assert!(!error.retryable);
    }

    #[test]
    fn test_retryable_errors() {
        let server_error = ApiError::internal_server_error("Server error", "Details", None);
        assert!(server_error.retryable);

        let client_error = ApiError::bad_request("Client error", "Details", None);
        assert!(!client_error.retryable);

        let rate_limit_error = ApiError::rate_limited("Rate limited", "Details", 1000, None);
        assert!(rate_limit_error.retryable);
        assert_eq!(rate_limit_error.retry_after_ms, Some(1000));
    }

    #[test]
    fn test_error_context() {
        let context = ErrorContext::new("test-123")
            .with_user_id("user-456")
            .with_endpoint("GET".to_string(), "/api/test".to_string());

        assert_eq!(context.request_id, Some("test-123".to_string()));
        assert_eq!(context.user_id, Some("user-456".to_string()));
        assert_eq!(context.method, Some("GET".to_string()));
        assert_eq!(context.endpoint, Some("/api/test".to_string()));
    }

    #[test]
    fn test_error_handler() {
        let handler = ApiErrorHandler::new()
            .with_debug_info(true)
            .with_logging(false); // Disable logging for tests

        let error = ApiError::internal_server_error("Test error", "Test details", None);
        let response = handler.handle_error(error);

        assert_eq!(response.error.status_code, 500);
        assert!(response.error.should_alert());
    }

    #[test]
    fn test_error_suggestions() {
        let error = ApiError::bad_request("Invalid input", "Missing required field", None)
            .with_suggestion("Check your request parameters")
            .with_suggestion("Refer to the API documentation");

        assert_eq!(error.suggestions.len(), 3); // 1 default + 2 custom suggestions
        assert!(error
            .suggestions
            .contains(&"Check your request parameters".to_string()));
    }

    #[test]
    fn test_sensitive_field_redaction() {
        let context = ErrorContext::new("test_sensitive")
            .with_parameter(
                "username".to_string(),
                serde_json::Value::String("test_user".to_string()),
            )
            .with_parameter(
                "password".to_string(),
                serde_json::Value::String("secret123".to_string()),
            )
            .with_parameter(
                "api_key".to_string(),
                serde_json::Value::String("key123".to_string()),
            );

        let hashmap = context.to_hashmap();

        // Check that sensitive fields are redacted
        if let Some(params) = hashmap.get("parameters") {
            if let serde_json::Value::Object(params_obj) = params {
                assert_eq!(
                    params_obj.get("username").unwrap(),
                    &serde_json::Value::String("test_user".to_string())
                );
                assert_eq!(
                    params_obj.get("password").unwrap(),
                    &serde_json::Value::String("[REDACTED]".to_string())
                );
                assert_eq!(
                    params_obj.get("api_key").unwrap(),
                    &serde_json::Value::String("[REDACTED]".to_string())
                );
            }
        }
    }
}

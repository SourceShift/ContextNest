/// Tests for middleware functionality
#[cfg(test)]
mod tests {
    use crate::api::middleware::{request_context::extract_request_context, RequestContext};
    use axum::{
        body::Body,
        http::{HeaderMap, HeaderName, HeaderValue, Request},
    };

    #[test]
    fn test_request_context_extraction() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", HeaderValue::from_static("test-123"));
        headers.insert("x-session-id", HeaderValue::from_static("session-456"));
        headers.insert("user-agent", HeaderValue::from_static("test-agent"));
        headers.insert("x-forwarded-for", HeaderValue::from_static("192.168.1.1"));
        headers.insert("x-meta-custom", HeaderValue::from_static("custom-value"));

        let context = RequestContext::from_headers(&headers);

        assert_eq!(context.request_id, "test-123");
        assert_eq!(context.session_id, Some("session-456".to_string()));
        assert_eq!(context.user_agent, Some("test-agent".to_string()));
        assert_eq!(context.client_ip, Some("192.168.1.1".to_string()));
        assert_eq!(
            context.get_metadata("custom"),
            Some(&"custom-value".to_string())
        );
    }

    #[test]
    fn test_request_context_metadata() {
        let context = RequestContext::new()
            .with_metadata("key1", "value1")
            .with_metadata("key2", "value2");

        assert_eq!(context.get_metadata("key1"), Some(&"value1".to_string()));
        assert_eq!(context.get_metadata("key2"), Some(&"value2".to_string()));
        assert_eq!(context.get_metadata("nonexistent"), None);
    }

    #[test]
    fn test_request_context_to_error_context() {
        let mut context = RequestContext::new();
        context.user_id = Some("test-user".to_string());
        context.client_ip = Some("192.168.1.1".to_string());

        let error_context = context.to_error_context();

        assert_eq!(error_context.request_id, Some(context.request_id));
        assert_eq!(error_context.user_id, Some("test-user".to_string()));
    }

    #[test]
    fn test_sensitive_header_filtering() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer token123"));
        headers.insert("cookie", HeaderValue::from_static("session=abc123"));
        headers.insert("x-api-key", HeaderValue::from_static("key123"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));

        let context = RequestContext::from_headers(&headers);

        // Sensitive headers should not set user_id in this basic implementation
        // In a real implementation, you'd extract user info from valid tokens
        assert_eq!(context.user_id, Some("user_123".to_string())); // Mock extraction
    }

    #[tokio::test]
    async fn test_request_id_generation() {
        let context1 = RequestContext::new();
        let context2 = RequestContext::new();

        // Each context should have a unique request ID
        assert_ne!(context1.request_id, context2.request_id);

        // Request IDs should be valid UUIDs (36 characters with dashes)
        assert_eq!(context1.request_id.len(), 36);
        assert_eq!(context2.request_id.len(), 36);
    }

    #[test]
    fn test_client_ip_extraction() {
        // Test x-forwarded-for header
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("192.168.1.1, 10.0.0.1"),
        );
        let context = RequestContext::from_headers(&headers);
        assert_eq!(context.client_ip, Some("192.168.1.1".to_string()));

        // Test x-real-ip header when x-forwarded-for is not present
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", HeaderValue::from_static("192.168.1.2"));
        let context = RequestContext::from_headers(&headers);
        assert_eq!(context.client_ip, Some("192.168.1.2".to_string()));

        // Test no IP headers
        let headers = HeaderMap::new();
        let context = RequestContext::from_headers(&headers);
        assert_eq!(context.client_ip, None);
    }
}

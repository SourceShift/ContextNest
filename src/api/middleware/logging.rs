use super::RequestContext;
use crate::error::{ContextNestError, ContextNestResult};
/// Request/response logging middleware
use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;
use tracing::{debug, info, warn, Instrument, Span};

/// Logging middleware configuration
#[derive(Debug, Clone)]
pub struct LoggingConfig {
    /// Whether to log request bodies
    pub log_request_body: bool,
    /// Whether to log response bodies
    pub log_response_body: bool,
    /// Maximum body size to log (in bytes)
    pub max_body_size: usize,
    /// Whether to log headers
    pub log_headers: bool,
    /// Headers to exclude from logging (for security)
    pub excluded_headers: Vec<String>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_request_body: false,  // Disabled by default for security
            log_response_body: false, // Disabled by default for performance
            max_body_size: 1024,      // 1KB
            log_headers: true,
            excluded_headers: vec![
                "authorization".to_string(),
                "cookie".to_string(),
                "x-api-key".to_string(),
                "x-auth-token".to_string(),
            ],
        }
    }
}

/// Logging middleware
pub struct LoggingMiddleware {
    config: LoggingConfig,
}

impl LoggingMiddleware {
    pub fn new(config: LoggingConfig) -> Self {
        Self { config }
    }

    pub fn default() -> Self {
        Self::new(LoggingConfig::default())
    }

    /// Log request headers (excluding sensitive ones)
    fn log_headers(&self, headers: &axum::http::HeaderMap, direction: &str) {
        if !self.config.log_headers {
            return;
        }

        let mut logged_headers = Vec::new();
        for (name, value) in headers.iter() {
            let name_str = name.as_str().to_lowercase();

            if !self.config.excluded_headers.contains(&name_str) {
                if let Ok(value_str) = value.to_str() {
                    logged_headers.push(format!("{}={}", name_str, value_str));
                }
            }
        }

        if !logged_headers.is_empty() {
            debug!(
                direction = direction,
                headers = logged_headers.join(", "),
                "HTTP headers"
            );
        }
    }
}

/// Middleware function for request/response logging
pub async fn log_requests(request: Request, next: Next) -> std::result::Result<Response, Response> {
    let start_time = Instant::now();
    let method = request.method().clone();
    let uri = request.uri().clone();
    let version = request.version();
    let request_context = request.extensions().get::<RequestContext>().cloned();

    // Create a tracing span for this request
    let span = tracing::info_span!(
        "http_request",
        method = %method,
        uri = %uri,
        version = ?version,
        request_id = request_context.as_ref().map(|ctx| ctx.request_id.as_str()).unwrap_or("unknown"),
        user_id = request_context.as_ref().and_then(|ctx| ctx.user_id.as_deref()).unwrap_or("anonymous"),
    );

    async move {
        // Log the incoming request
        info!(
            method = %method,
            uri = %uri,
            version = ?version,
            user_agent = request.headers().get("user-agent")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("unknown"),
            content_length = request.headers().get("content-length")
                .and_then(|h| h.to_str().ok())
                .unwrap_or("0"),
            "Incoming request"
        );

        // Log request headers (excluding sensitive ones)
        log_request_headers(request.headers());

        // Process the request
        let response = next.run(request).await;

        let duration = start_time.elapsed();
        let status = response.status();

        // Log the response
        if status.is_success() {
            info!(
                status = %status,
                duration_ms = duration.as_millis(),
                content_length = response.headers().get("content-length")
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or("unknown"),
                "Request completed"
            );
        } else if status.is_client_error() {
            warn!(
                status = %status,
                duration_ms = duration.as_millis(),
                content_length = response.headers().get("content-length")
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or("unknown"),
                "Request completed"
            );
        } else {
            tracing::error!(
                status = %status,
                duration_ms = duration.as_millis(),
                content_length = response.headers().get("content-length")
                    .and_then(|h| h.to_str().ok())
                    .unwrap_or("unknown"),
                "Request completed"
            );
        }

        // Log response headers (excluding sensitive ones)
        log_response_headers(response.headers());

        Ok(response)
    }
    .instrument(span)
    .await
}

/// Log request headers (excluding sensitive ones)
fn log_request_headers(headers: &axum::http::HeaderMap) {
    let excluded_headers = [
        "authorization",
        "cookie",
        "x-api-key",
        "x-auth-token",
        "x-session-token",
    ];

    let mut logged_headers = Vec::new();
    for (name, value) in headers.iter() {
        let name_str = name.as_str().to_lowercase();

        if !excluded_headers.contains(&name_str.as_str()) {
            if let Ok(value_str) = value.to_str() {
                logged_headers.push(format!("{}={}", name_str, value_str));
            }
        } else {
            logged_headers.push(format!("{}=[REDACTED]", name_str));
        }
    }

    if !logged_headers.is_empty() {
        debug!(headers = logged_headers.join(", "), "Request headers");
    }
}

/// Log response headers (excluding sensitive ones)
fn log_response_headers(headers: &axum::http::HeaderMap) {
    let excluded_headers = ["set-cookie", "x-auth-token", "x-session-token"];

    let mut logged_headers = Vec::new();
    for (name, value) in headers.iter() {
        let name_str = name.as_str().to_lowercase();

        if !excluded_headers.contains(&name_str.as_str()) {
            if let Ok(value_str) = value.to_str() {
                logged_headers.push(format!("{}={}", name_str, value_str));
            }
        } else {
            logged_headers.push(format!("{}=[REDACTED]", name_str));
        }
    }

    if !logged_headers.is_empty() {
        debug!(headers = logged_headers.join(", "), "Response headers");
    }
}

/// Performance metrics logging
pub async fn log_performance_metrics(
    request: Request,
    next: Next,
) -> std::result::Result<Response, Response> {
    let start_time = Instant::now();
    let method = request.method().clone();
    let uri = request.uri().clone();

    let response = next.run(request).await;

    let duration = start_time.elapsed();
    let status = response.status();

    // Log performance metrics
    info!(
        method = %method,
        uri = %uri,
        status = %status,
        duration_ms = duration.as_millis(),
        duration_us = duration.as_micros(),
        "Performance metrics"
    );

    // Warn about slow requests
    if duration.as_millis() > 1000 {
        warn!(
            method = %method,
            uri = %uri,
            duration_ms = duration.as_millis(),
            "Slow request detected"
        );
    }

    Ok(response)
}

/// Structured logging for API events
pub struct ApiEventLogger;

impl ApiEventLogger {
    /// Log authentication events
    pub fn log_auth_event(
        event_type: &str,
        user_id: Option<&str>,
        success: bool,
        details: Option<&str>,
        request_context: Option<&RequestContext>,
    ) {
        if success {
            info!(
                event_type = event_type,
                user_id = user_id.unwrap_or("unknown"),
                success = success,
                details = details.unwrap_or(""),
                request_id = request_context
                    .map(|ctx| ctx.request_id.as_str())
                    .unwrap_or("unknown"),
                client_ip = request_context
                    .and_then(|ctx| ctx.client_ip.as_deref())
                    .unwrap_or("unknown"),
                "Authentication event"
            );
        } else {
            warn!(
                event_type = event_type,
                user_id = user_id.unwrap_or("unknown"),
                success = success,
                details = details.unwrap_or(""),
                request_id = request_context
                    .map(|ctx| ctx.request_id.as_str())
                    .unwrap_or("unknown"),
                client_ip = request_context
                    .and_then(|ctx| ctx.client_ip.as_deref())
                    .unwrap_or("unknown"),
                "Authentication event"
            );
        }
    }

    /// Log API access events
    pub fn log_api_access(
        resource: &str,
        action: &str,
        user_id: Option<&str>,
        success: bool,
        request_context: Option<&RequestContext>,
    ) {
        info!(
            resource = resource,
            action = action,
            user_id = user_id.unwrap_or("anonymous"),
            success = success,
            request_id = request_context
                .map(|ctx| ctx.request_id.as_str())
                .unwrap_or("unknown"),
            "API access event"
        );
    }

    /// Log error events
    pub fn log_error_event(
        error_type: &str,
        error_message: &str,
        user_id: Option<&str>,
        request_context: Option<&RequestContext>,
    ) {
        warn!(
            error_type = error_type,
            error_message = error_message,
            user_id = user_id.unwrap_or("unknown"),
            request_id = request_context
                .map(|ctx| ctx.request_id.as_str())
                .unwrap_or("unknown"),
            "API error event"
        );
    }
}

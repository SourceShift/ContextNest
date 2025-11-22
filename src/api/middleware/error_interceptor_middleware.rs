use crate::error::ContextNestResult;
use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use serde_json::json;
use std::time::Instant;
use tracing::{error, info, warn};

/// Error interceptor configuration
#[derive(Debug, Clone)]
pub struct ErrorInterceptorConfig {
    pub include_stack_trace: bool,
    pub log_all_errors: bool,
    pub sanitize_errors: bool,
    pub custom_error_handlers: bool,
}

impl Default for ErrorInterceptorConfig {
    fn default() -> Self {
        Self {
            include_stack_trace: cfg!(debug_assertions),
            log_all_errors: true,
            sanitize_errors: true,
            custom_error_handlers: true,
        }
    }
}

/// Error information structure
#[derive(Debug, Clone)]
pub struct ErrorInfo {
    pub status: StatusCode,
    pub error_code: String,
    pub message: String,
    pub details: Option<String>,
    pub request_id: String,
    pub timestamp: String,
    pub path: String,
    pub method: String,
}

impl ErrorInfo {
    fn from_error(
        error: &dyn std::error::Error,
        status: StatusCode,
        request_id: String,
        path: String,
        method: String,
    ) -> Self {
        Self {
            status,
            error_code: error.to_string(),
            message: error.to_string(),
            details: error.source().map(|e| e.to_string()),
            request_id,
            timestamp: chrono::Utc::now().to_rfc3339(),
            path,
            method,
        }
    }
}

/// Error interceptor middleware
pub async fn error_interceptor_middleware(
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let start_time = Instant::now();

    // Extract request information before moving request
    let path = request.uri().path().to_string();
    let method = request.method().to_string();
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    // Process the request
    let response = next.run(request).await;

    let duration = start_time.elapsed();
    let status = response.status();

    if status.is_server_error() {
        error!(
            "Server error: {} {} - {} - {:?} - Request ID: {}",
            method, path, status, duration, request_id
        );
    } else if status.is_client_error() {
        warn!(
            "Client error: {} {} - {} - {:?} - Request ID: {}",
            method, path, status, duration, request_id
        );
    } else {
        info!(
            "Success: {} {} - {} - {:?} - Request ID: {}",
            method, path, status, duration, request_id
        );
    }

    Ok(response)
}

/// Global error handler for panics
pub async fn panic_recovery_middleware(
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let path = request.uri().path().to_string();
    let method = request.method().to_string();

    // Use std::panic::catch_unwind to catch panics
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tokio::runtime::Handle::current().block_on(async { next.run(request).await })
    }));

    match result {
        Ok(response) => Ok(response),
        Err(panic_info) => {
            error!(
                "Panic occurred in request {} {}: {:?}",
                method, path, panic_info
            );

            // Create error response
            let error_response = json!({
                "error": "Internal server error",
                "message": "An unexpected error occurred",
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "request_id": "panic-recovery"
            });

            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Performance monitoring middleware
pub async fn performance_monitoring_middleware(
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let start_time = Instant::now();
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    let response = next.run(request).await;

    let duration = start_time.elapsed();

    // Log performance metrics
    if duration.as_millis() > 1000 {
        warn!("Slow request: {} {} - took {:?}", method, path, duration);
    } else {
        info!("Request completed: {} {} - {:?}", method, path, duration);
    }

    // You could send these metrics to a monitoring system
    log_performance_metrics(method.as_str(), &path, duration, response.status());

    Ok(response)
}

fn log_performance_metrics(
    method: &str,
    path: &str,
    duration: std::time::Duration,
    status: StatusCode,
) {
    // This is where you would send metrics to your monitoring system
    // For now, we'll just log them
    info!(
        "Performance metric - Method: {}, Path: {}, Duration: {:?}, Status: {}",
        method, path, duration, status
    );
}

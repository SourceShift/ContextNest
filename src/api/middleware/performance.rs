/// Performance monitoring middleware
use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;
use tracing::{info, warn};
use crate::error::{ContextNestError, ContextNestResult};

/// Performance monitoring middleware
pub struct PerformanceMiddleware {
    slow_request_threshold_ms: u64,
}

impl PerformanceMiddleware {
    pub fn new(slow_request_threshold_ms: u64) -> Self {
        Self {
            slow_request_threshold_ms,
        }
    }
}

impl Default for PerformanceMiddleware {
    fn default() -> Self {
        Self::new(1000) // 1 second threshold
    }
}

/// Performance monitoring middleware function
pub async fn monitor_performance(
    request: Request,
    next: Next,
) -> std::result::Result<Response, Response> {
    let start_time = Instant::now();
    let method = request.method().clone();
    let uri = request.uri().clone();

    let response = next.run(request).await;
    
    let duration = start_time.elapsed();
    let duration_ms = duration.as_millis();

    // Log performance metrics
    info!(
        method = %method,
        uri = %uri,
        duration_ms = duration_ms,
        "Request performance"
    );

    // Warn about slow requests
    if duration_ms > 1000 {
        warn!(
            method = %method,
            uri = %uri,
            duration_ms = duration_ms,
            "Slow request detected"
        );
    }

    Ok(response)
}
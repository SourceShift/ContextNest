use crate::error::ContextNestResult;
use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use chrono::Utc;
use serde_json::json;
use std::time::Instant;
use tracing::{error, info, warn};

/// Logging middleware for ContextNest API
pub async fn logging_middleware(
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let start_time = Instant::now();
    let method = request.method().clone();
    let uri = request.uri().clone();
    let user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string(); // Clone the user_agent to avoid borrowing issues

    let timestamp = Utc::now().to_rfc3339();

    // Process the request
    let response = next.run(request).await;

    let duration = start_time.elapsed();
    let status = response.status();

    // Log the request
    match status.as_u16() {
        200..=299 => {
            info!(
                "Request completed: {} {} - {} - {:?} - {}",
                method, uri, status, duration, user_agent
            );
        }
        400..=499 => {
            warn!(
                "Client error: {} {} - {} - {:?} - {}",
                method, uri, status, duration, user_agent
            );
        }
        500..=599 => {
            error!(
                "Server error: {} {} - {} - {:?} - {}",
                method, uri, status, duration, user_agent
            );
        }
        _ => {
            info!(
                "Request: {} {} - {} - {:?} - {}",
                method, uri, status, duration, user_agent
            );
        }
    }

    // Store metrics for monitoring
    let log_entry = json!({
        "timestamp": timestamp,
        "method": method.to_string(),
        "uri": uri.to_string(),
        "status": status.as_u16(),
        "duration_ms": duration.as_millis(),
        "user_agent": user_agent
    });

    // Log structured data
    info!(
        "API Request: {}",
        serde_json::to_string(&log_entry).unwrap_or_default()
    );

    Ok(response)
}

pub struct LoggingConfig {
    pub include_body: bool,
    pub include_headers: bool,
    pub max_body_size: usize,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            include_body: false,
            include_headers: true,
            max_body_size: 1024,
        }
    }
}

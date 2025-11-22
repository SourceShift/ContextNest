use crate::error::{ContextNestError, ContextNestResult};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
    Json,
};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::{debug, error, info, warn};

/// Metrics configuration
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    pub enable_request_metrics: bool,
    pub enable_response_metrics: bool,
    pub enable_performance_metrics: bool,
    pub enable_error_metrics: bool,
    pub metrics_retention_duration: Duration,
    pub max_metrics_entries: usize,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enable_request_metrics: true,
            enable_response_metrics: true,
            enable_performance_metrics: true,
            enable_error_metrics: true,
            metrics_retention_duration: Duration::from_secs(3600), // 1 hour
            max_metrics_entries: 10000,
        }
    }
}

/// Request metrics
#[derive(Debug, Clone)]
pub struct RequestMetrics {
    pub timestamp: Instant,
    pub method: String,
    pub path: String,
    pub user_agent: String,
    pub client_ip: String,
    pub content_length: Option<usize>,
    pub request_id: String,
}

/// Response metrics
#[derive(Debug, Clone)]
pub struct ResponseMetrics {
    pub timestamp: Instant,
    pub status_code: u16,
    pub content_length: Option<usize>,
    pub duration: Duration,
    pub request_id: String,
}

/// Performance metrics
#[derive(Debug, Clone)]
pub struct PerformanceMetrics {
    pub timestamp: Instant,
    pub endpoint: String,
    pub duration: Duration,
    pub memory_usage: Option<usize>,
    pub cpu_usage: Option<f64>,
}

/// Error metrics
#[derive(Debug, Clone)]
pub struct ErrorMetrics {
    pub timestamp: Instant,
    pub error_type: String,
    pub error_message: String,
    pub endpoint: String,
    pub status_code: Option<u16>,
    pub request_id: String,
}

/// Metrics collector
#[derive(Debug)]
pub struct MetricsCollector {
    config: MetricsConfig,
    request_metrics: Arc<Mutex<Vec<RequestMetrics>>>,
    response_metrics: Arc<Mutex<Vec<ResponseMetrics>>>,
    performance_metrics: Arc<Mutex<Vec<PerformanceMetrics>>>,
    error_metrics: Arc<Mutex<Vec<ErrorMetrics>>>,
    aggregated_metrics: Arc<Mutex<HashMap<String, f64>>>,
}

impl MetricsCollector {
    pub fn new(config: MetricsConfig) -> Self {
        let collector = Self {
            request_metrics: Arc::new(Mutex::new(Vec::new())),
            response_metrics: Arc::new(Mutex::new(Vec::new())),
            performance_metrics: Arc::new(Mutex::new(Vec::new())),
            error_metrics: Arc::new(Mutex::new(Vec::new())),
            aggregated_metrics: Arc::new(Mutex::new(HashMap::new())),
            config,
        };

        // Start cleanup task
        Self::start_cleanup_task(
            collector.request_metrics.clone(),
            collector.response_metrics.clone(),
            collector.performance_metrics.clone(),
            collector.error_metrics.clone(),
            collector.config.metrics_retention_duration,
        );

        collector
    }

    pub fn record_request(&self, metrics: RequestMetrics) {
        if self.config.enable_request_metrics {
            let mut metrics_vec = self.request_metrics.lock().unwrap();
            metrics_vec.push(metrics);

            // Update aggregated metrics
            self.update_aggregated_metrics("total_requests", 1.0);
        }
    }

    pub fn record_response(&self, metrics: ResponseMetrics) {
        if self.config.enable_response_metrics {
            let status_code = metrics.status_code; // Extract before move
            let mut metrics_vec = self.response_metrics.lock().unwrap();
            metrics_vec.push(metrics);

            // Update aggregated metrics
            self.update_aggregated_metrics("total_responses", 1.0);

            if status_code >= 400 {
                self.update_aggregated_metrics("error_responses", 1.0);
            }
        }
    }

    pub fn record_performance(&self, metrics: PerformanceMetrics) {
        if self.config.enable_performance_metrics {
            let endpoint = metrics.endpoint.clone(); // Extract before move
            let duration = metrics.duration; // Extract before move
            let mut metrics_vec = self.performance_metrics.lock().unwrap();
            metrics_vec.push(metrics);

            // Update aggregated metrics
            self.update_aggregated_metrics(
                &format!("avg_duration_{}", endpoint),
                duration.as_millis() as f64,
            );
        }
    }

    pub fn record_error(&self, metrics: ErrorMetrics) {
        if self.config.enable_error_metrics {
            let mut metrics_vec = self.error_metrics.lock().unwrap();
            metrics_vec.push(metrics);

            // Update aggregated metrics
            self.update_aggregated_metrics("total_errors", 1.0);
        }
    }

    fn update_aggregated_metrics(&self, key: &str, value: f64) {
        let mut aggregated = self.aggregated_metrics.lock().unwrap();
        let entry = aggregated.entry(key.to_string()).or_insert(0.0);
        *entry += value;
    }

    pub fn get_metrics_summary(&self) -> serde_json::Value {
        let request_count = self.request_metrics.lock().unwrap().len();
        let response_count = self.response_metrics.lock().unwrap().len();
        let error_count = self.error_metrics.lock().unwrap().len();

        let avg_response_time = if response_count > 0 {
            let total_duration: Duration = self
                .response_metrics
                .lock()
                .unwrap()
                .iter()
                .map(|m| m.duration)
                .sum();
            total_duration.as_millis() as f64 / response_count as f64
        } else {
            0.0
        };

        let error_rate = if response_count > 0 {
            (error_count as f64 / response_count as f64) * 100.0
        } else {
            0.0
        };

        json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "total_requests": request_count,
            "total_responses": response_count,
            "total_errors": error_count,
            "average_response_time_ms": avg_response_time,
            "error_rate_percent": error_rate,
            "active_metrics": {
                "request_metrics": self.request_metrics.lock().unwrap().len(),
                "response_metrics": self.response_metrics.lock().unwrap().len(),
                "performance_metrics": self.performance_metrics.lock().unwrap().len(),
                "error_metrics": self.error_metrics.lock().unwrap().len()
            }
        })
    }

    fn start_cleanup_task(
        request_metrics: Arc<Mutex<Vec<RequestMetrics>>>,
        response_metrics: Arc<Mutex<Vec<ResponseMetrics>>>,
        performance_metrics: Arc<Mutex<Vec<PerformanceMetrics>>>,
        error_metrics: Arc<Mutex<Vec<ErrorMetrics>>>,
        retention_duration: Duration,
    ) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // Every 5 minutes

            loop {
                interval.tick().await;
                let now = Instant::now();

                // Clean up old metrics
                {
                    let mut req_metrics = request_metrics.lock().unwrap();
                    req_metrics.retain(|m| now.duration_since(m.timestamp) < retention_duration);
                }

                {
                    let mut resp_metrics = response_metrics.lock().unwrap();
                    resp_metrics.retain(|m| now.duration_since(m.timestamp) < retention_duration);
                }

                {
                    let mut perf_metrics = performance_metrics.lock().unwrap();
                    perf_metrics.retain(|m| now.duration_since(m.timestamp) < retention_duration);
                }

                {
                    let mut err_metrics = error_metrics.lock().unwrap();
                    err_metrics.retain(|m| now.duration_since(m.timestamp) < retention_duration);
                }

                debug!("Metrics cleanup completed");
            }
        });
    }
}

/// Metrics middleware
pub async fn metrics_middleware(
    State(metrics_collector): State<Arc<MetricsCollector>>,
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let start_time = Instant::now();
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_else(|| "unknown")
        .to_string();

    // Extract request information
    let user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let client_ip = request
        .headers()
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let content_length = request
        .headers()
        .get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok());

    // Record request metrics
    let request_metrics = RequestMetrics {
        timestamp: Instant::now(),
        method: method.clone(),
        path: path.clone(),
        user_agent: user_agent.clone(),
        client_ip: client_ip.clone(),
        content_length,
        request_id: request_id.clone(),
    };
    metrics_collector.record_request(request_metrics);

    // Process request
    let response = next.run(request).await;

    let end_time = Instant::now();
    let duration = end_time.duration_since(start_time);
    let status_code = response.status().as_u16();

    // Extract response information
    let response_content_length = response
        .headers()
        .get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok());

    // Record response metrics
    let response_metrics = ResponseMetrics {
        timestamp: end_time,
        status_code,
        content_length: response_content_length,
        duration,
        request_id: request_id.clone(),
    };
    metrics_collector.record_response(response_metrics.clone());

    // Record performance metrics
    let performance_metrics = PerformanceMetrics {
        timestamp: end_time,
        endpoint: format!("{} {}", method, path),
        duration,
        memory_usage: get_memory_usage(),
        cpu_usage: get_cpu_usage(),
    };
    metrics_collector.record_performance(performance_metrics.clone());

    // Record error metrics if applicable
    if status_code >= 400 {
        let error_metrics = ErrorMetrics {
            timestamp: end_time,
            error_type: match status_code {
                400..=499 => "client_error",
                500..=599 => "server_error",
                _ => "unknown_error",
            }
            .to_string(),
            error_message: format!("HTTP {}", status_code),
            endpoint: format!("{} {}", method, path),
            status_code: Some(status_code),
            request_id,
        };
        metrics_collector.record_error(error_metrics);
    }

    // Log based on performance
    match duration.as_millis() {
        0..=100 => {
            debug!(
                "Fast request: {} {} - {:?} - {}",
                method, path, duration, status_code
            );
        }
        101..=500 => {
            info!(
                "Normal request: {} {} - {:?} - {}",
                method, path, duration, status_code
            );
        }
        501..=2000 => {
            warn!(
                "Slow request: {} {} - {:?} - {}",
                method, path, duration, status_code
            );
        }
        _ => {
            error!(
                "Very slow request: {} {} - {:?} - {}",
                method, path, duration, status_code
            );
        }
    }

    Ok(response)
}

/// Get current process resident memory usage in bytes.
/// Not implemented in v0.1.0 — system-metrics readout is a v0.2 deliverable.
/// Returning `None` is the honest result; downstream metric formatters skip
/// the field when `None`. Wire via the `sysinfo` or `procfs` crate when
/// metrics-driven autoscaling lands.
fn get_memory_usage() -> Option<usize> {
    None
}

/// Get current process CPU usage as a fraction of one core.
/// Same posture as `get_memory_usage` — `None` until v0.2 metrics work.
fn get_cpu_usage() -> Option<f64> {
    None
}

/// Metrics endpoint handler
pub async fn get_metrics(
    State(metrics_collector): State<Arc<MetricsCollector>>,
) -> std::result::Result<Json<serde_json::Value>, StatusCode> {
    Ok(Json(metrics_collector.get_metrics_summary()))
}

/// Health check endpoint
pub async fn health_check(
    State(metrics_collector): State<Arc<MetricsCollector>>,
) -> std::result::Result<Json<serde_json::Value>, StatusCode> {
    let summary = metrics_collector.get_metrics_summary();

    Ok(Json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "metrics": summary
    })))
}

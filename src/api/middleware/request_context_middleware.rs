use crate::error::ContextNestResult;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Request context information
#[derive(Debug, Clone, Serialize)]
pub struct RequestContext {
    pub request_id: String,
    pub trace_id: String,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub client_ip: String,
    pub user_agent: String,
    pub correlation_id: String,
    #[serde(skip)]
    pub start_time: Instant,
    pub custom_attributes: HashMap<String, String>,
}

impl<'de> Deserialize<'de> for RequestContext {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, Deserialize, MapAccess, Visitor};
        use std::fmt;

        struct RequestContextVisitor;

        impl<'de> Visitor<'de> for RequestContextVisitor {
            type Value = RequestContext;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct RequestContext")
            }

            fn visit_map<M>(self, mut map: M) -> std::result::Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut request_id = None;
                let mut trace_id = None;
                let mut user_id = None;
                let mut session_id = None;
                let mut client_ip = None;
                let mut user_agent = None;
                let mut correlation_id = None;
                let mut custom_attributes = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        "request_id" => {
                            request_id = Some(map.next_value()?);
                        }
                        "trace_id" => {
                            trace_id = Some(map.next_value()?);
                        }
                        "user_id" => {
                            user_id = Some(map.next_value()?);
                        }
                        "session_id" => {
                            session_id = Some(map.next_value()?);
                        }
                        "client_ip" => {
                            client_ip = Some(map.next_value()?);
                        }
                        "user_agent" => {
                            user_agent = Some(map.next_value()?);
                        }
                        "correlation_id" => {
                            correlation_id = Some(map.next_value()?);
                        }
                        "custom_attributes" => {
                            custom_attributes = Some(map.next_value()?);
                        }
                        _ => {
                            // Skip unknown fields
                            let _: serde::de::IgnoredAny = map.next_value()?;
                        }
                    }
                }

                let request_id =
                    request_id.ok_or_else(|| de::Error::missing_field("request_id"))?;
                let trace_id = trace_id.ok_or_else(|| de::Error::missing_field("trace_id"))?;
                let client_ip = client_ip.ok_or_else(|| de::Error::missing_field("client_ip"))?;
                let user_agent =
                    user_agent.ok_or_else(|| de::Error::missing_field("user_agent"))?;
                let correlation_id =
                    correlation_id.ok_or_else(|| de::Error::missing_field("correlation_id"))?;
                let custom_attributes = custom_attributes.unwrap_or_default();

                Ok(RequestContext {
                    request_id,
                    trace_id,
                    user_id,
                    session_id,
                    client_ip,
                    user_agent,
                    correlation_id,
                    start_time: Instant::now(), // Always use current time during deserialization
                    custom_attributes,
                })
            }
        }

        deserializer.deserialize_map(RequestContextVisitor)
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        let request_id = Uuid::new_v4().to_string();
        let trace_id = generate_trace_id();
        let correlation_id = generate_correlation_id();

        Self {
            request_id,
            trace_id,
            user_id: None,
            session_id: None,
            client_ip: "127.0.0.1".to_string(),
            user_agent: "unknown".to_string(),
            correlation_id,
            start_time: Instant::now(),
            custom_attributes: HashMap::new(),
        }
    }
}

impl RequestContext {
    pub fn new(client_ip: String, user_agent: String) -> Self {
        let request_id = Uuid::new_v4().to_string();
        let trace_id = generate_trace_id();
        let correlation_id = generate_correlation_id();

        Self {
            request_id,
            trace_id,
            user_id: None,
            session_id: None,
            client_ip,
            user_agent,
            correlation_id,
            start_time: Instant::now(),
            custom_attributes: HashMap::new(),
        }
    }

    pub fn add_attribute(&mut self, key: String, value: String) {
        self.custom_attributes.insert(key, value);
    }

    pub fn get_duration(&self) -> Duration {
        self.start_time.elapsed()
    }
}

/// Context manager for tracking requests
#[derive(Debug)]
pub struct ContextManager {
    active_requests: Arc<std::sync::RwLock<HashMap<String, RequestContext>>>,
    config: ContextConfig,
}

impl ContextManager {
    pub fn new(config: ContextConfig) -> Self {
        Self {
            active_requests: Arc::new(std::sync::RwLock::new(HashMap::new())),
            config,
        }
    }

    pub fn create_context(&self, request: &Request) -> RequestContext {
        let client_ip = extract_client_ip(request);
        let user_agent = extract_user_agent(request);
        let mut context = RequestContext::new(client_ip, user_agent);

        // Extract existing context from headers
        if let Some(trace_id) = request.headers().get("x-trace-id") {
            if let Ok(trace_id_str) = trace_id.to_str() {
                context.trace_id = trace_id_str.to_string();
            }
        }

        if let Some(correlation_id) = request.headers().get("x-correlation-id") {
            if let Ok(correlation_id_str) = correlation_id.to_str() {
                context.correlation_id = correlation_id_str.to_string();
            }
        }

        if let Some(user_id) = request.headers().get("x-user-id") {
            if let Ok(user_id_str) = user_id.to_str() {
                context.user_id = Some(user_id_str.to_string());
            }
        }

        if let Some(session_id) = request.headers().get("x-session-id") {
            if let Ok(session_id_str) = session_id.to_str() {
                context.session_id = Some(session_id_str.to_string());
            }
        }

        context
    }

    pub fn register_context(&self, context: RequestContext) {
        let mut active = self.active_requests.write().unwrap();
        active.insert(context.request_id.clone(), context);
    }

    pub fn complete_context(&self, request_id: &str) {
        let mut active = self.active_requests.write().unwrap();
        if let Some(context) = active.remove(request_id) {
            let duration = context.get_duration();
            info!(
                "Request completed: {} - Duration: {:?} - User: {:?}",
                context.request_id, duration, context.user_id
            );
        }
    }

    pub fn get_active_count(&self) -> usize {
        self.active_requests.read().unwrap().len()
    }

    pub fn cleanup_expired_contexts(&self) {
        let mut active = self.active_requests.write().unwrap();
        let now = Instant::now();

        active.retain(|_, context| {
            now.duration_since(context.start_time) < self.config.request_timeout
        });
    }
}

/// Context configuration
#[derive(Debug, Clone)]
pub struct ContextConfig {
    pub request_timeout: Duration,
    pub include_performance_metrics: bool,
    pub include_user_tracking: bool,
    pub enable_distributed_tracing: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(300), // 5 minutes
            include_performance_metrics: true,
            include_user_tracking: true,
            enable_distributed_tracing: true,
        }
    }
}

/// Request context middleware
pub async fn request_context_middleware(
    State(context_manager): State<Arc<ContextManager>>,
    mut request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    // Create request context
    let context = context_manager.create_context(&request);

    // Add context headers to request
    request.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&context.request_id).unwrap(),
    );
    request.headers_mut().insert(
        "x-trace-id",
        HeaderValue::from_str(&context.trace_id).unwrap(),
    );
    request.headers_mut().insert(
        "x-correlation-id",
        HeaderValue::from_str(&context.correlation_id).unwrap(),
    );

    // Register context
    context_manager.register_context(context.clone());

    // Log request start
    info!(
        "Request started: {} {} - Request ID: {} - User: {:?}",
        request.method(),
        request.uri(),
        context.request_id,
        context.user_id
    );

    // Process request
    let response = next.run(request).await;

    // Add context headers to response
    let mut response_with_context = response;
    response_with_context.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&context.request_id).unwrap(),
    );
    response_with_context.headers_mut().insert(
        "x-trace-id",
        HeaderValue::from_str(&context.trace_id).unwrap(),
    );

    // Complete context
    context_manager.complete_context(&context.request_id);

    Ok(response_with_context)
}

/// Extract client IP from request
fn extract_client_ip(request: &Request) -> String {
    // Try X-Forwarded-For header first
    if let Some(forwarded_for) = request.headers().get("x-forwarded-for") {
        if let Ok(forwarded_str) = forwarded_for.to_str() {
            // Take the first IP in the chain
            if let Some(first_ip) = forwarded_str.split(',').next() {
                return first_ip.trim().to_string();
            }
        }
    }

    // Try X-Real-IP header
    if let Some(real_ip) = request.headers().get("x-real-ip") {
        if let Ok(real_ip_str) = real_ip.to_str() {
            return real_ip_str.to_string();
        }
    }

    // Fallback
    "127.0.0.1".to_string()
}

/// Extract user agent from request
fn extract_user_agent(request: &Request) -> String {
    request
        .headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

/// Generate a trace ID.
/// Returns `trace_<8-hex-chars>` derived from a v4 UUID. Functionally
/// equivalent to a short OpenTelemetry-style trace ID for in-process
/// correlation. For W3C-compatible `traceparent`-header propagation across
/// service boundaries, integrate `opentelemetry-otlp` via a follow-up
/// epic — the substrate is single-process in v0.1.0.
fn generate_trace_id() -> String {
    format!("trace_{}", Uuid::new_v4().to_string()[..8].to_string())
}

/// Generate correlation ID
fn generate_correlation_id() -> String {
    format!("corr_{}", Uuid::new_v4().to_string()[..8].to_string())
}

/// Cleanup task for expired contexts
pub async fn start_context_cleanup_task(context_manager: Arc<ContextManager>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));

        loop {
            interval.tick().await;
            context_manager.cleanup_expired_contexts();

            let active_count = context_manager.get_active_count();
            if active_count > 1000 {
                warn!("High number of active requests: {}", active_count);
            }
        }
    });
}

/// Performance monitoring middleware that works with request context
pub async fn context_performance_middleware(
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let start_time = Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    let response = next.run(request).await;

    let duration = start_time.elapsed();
    let status = response.status();

    // Log performance metrics
    match duration.as_millis() {
        0..=100 => {
            debug!(
                "Fast request: {} {} - {:?} - {}",
                method, path, duration, status
            );
        }
        101..=500 => {
            info!(
                "Normal request: {} {} - {:?} - {}",
                method, path, duration, status
            );
        }
        501..=2000 => {
            warn!(
                "Slow request: {} {} - {:?} - {}",
                method, path, duration, status
            );
        }
        _ => {
            error!(
                "Very slow request: {} {} - {:?} - {}",
                method, path, duration, status
            );
        }
    }

    Ok(response)
}

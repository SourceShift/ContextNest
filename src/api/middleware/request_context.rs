use crate::error::{ContextNestError, ContextNestResult};
/// Request context extraction and management
use axum::{
    extract::{FromRequestParts, Request},
    http::{header::HeaderName, HeaderMap, HeaderValue},
    middleware::Next,
    response::Response,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;
use uuid::Uuid;

/// Request context information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestContext {
    /// Unique request ID for tracking
    pub request_id: String,
    /// User ID if authenticated
    pub user_id: Option<String>,
    /// Session ID if available
    pub session_id: Option<String>,
    /// Client IP address
    pub client_ip: Option<String>,
    /// User agent string
    pub user_agent: Option<String>,
    /// Request timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl RequestContext {
    /// Create a new request context
    pub fn new() -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            user_id: None,
            session_id: None,
            client_ip: None,
            user_agent: None,
            timestamp: chrono::Utc::now(),
            metadata: HashMap::new(),
        }
    }

    /// Extract context from request headers
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let mut context = Self::new();

        // Check for existing request ID
        if let Some(request_id) = headers.get("x-request-id") {
            if let Ok(id) = request_id.to_str() {
                context.request_id = id.to_string();
            }
        }

        // Extract user ID from authorization context (if available)
        if let Some(auth_header) = headers.get("authorization") {
            if let Ok(auth_str) = auth_header.to_str() {
                context.user_id = extract_user_id_from_auth(auth_str);
            }
        }

        // Extract session ID
        if let Some(session_header) = headers.get("x-session-id") {
            if let Ok(session_str) = session_header.to_str() {
                context.session_id = Some(session_str.to_string());
            }
        }

        // Extract client IP
        context.client_ip = headers
            .get("x-forwarded-for")
            .and_then(|h| h.to_str().ok())
            .or_else(|| headers.get("x-real-ip").and_then(|h| h.to_str().ok()))
            .map(|ip| ip.split(',').next().unwrap_or(ip).trim().to_string());

        // Extract user agent
        if let Some(user_agent) = headers.get("user-agent") {
            if let Ok(ua_str) = user_agent.to_str() {
                context.user_agent = Some(ua_str.to_string());
            }
        }

        // Extract custom metadata headers
        for (name, value) in headers.iter() {
            if let Some(name_str) = name.as_str().strip_prefix("x-meta-") {
                if let Ok(value_str) = value.to_str() {
                    context
                        .metadata
                        .insert(name_str.to_string(), value_str.to_string());
                }
            }
        }

        context
    }

    /// Add metadata to the context
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// Convert to error context for error handling
    pub fn to_error_context(&self) -> crate::error::ErrorContext {
        crate::error::ErrorContext::new(&self.request_id)
            .with_user_id(self.user_id.as_deref().unwrap_or("anonymous"))
            .with_metadata(
                "client_ip".to_string(),
                serde_json::Value::String(self.client_ip.clone().unwrap_or_default()),
            )
            .with_metadata(
                "user_agent".to_string(),
                serde_json::Value::String(self.user_agent.clone().unwrap_or_default()),
            )
            .with_metadata(
                "timestamp".to_string(),
                serde_json::Value::String(self.timestamp.to_rfc3339()),
            )
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract user ID from authorization header.
/// the previous implementation returned mock user IDs
/// (`"user_123"` for `Bearer …` headers, `"api_user"` for `ApiKey …`) WITHOUT
/// verifying the token or key. Downstream code that treats a `Some(_)` as
/// authenticated was therefore trusting an unverified header.
/// v0.1.0 has no built-in auth (per — deploy
/// behind a reverse proxy that handles JWT/OAuth2/API-key validation).
/// This function now returns `None` unconditionally so callers cannot
/// silently rely on unverified identity. When auth re-enters the substrate
/// for the cloud-managed product (v0.5+), replace this with a real
/// `validate_jwt(token, &public_key) -> Option<Claims>` path.
fn extract_user_id_from_auth(_auth_header: &str) -> Option<String> {
    None
}

/// Axum extractor for request context
#[axum::async_trait]
impl<S> FromRequestParts<S> for RequestContext
where
    S: Send + Sync,
{
    type Rejection = ();

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> std::result::Result<Self, Self::Rejection> {
        // Try to get context from extensions first (set by middleware)
        if let Some(context) = parts.extensions.get::<RequestContext>() {
            Ok(context.clone())
        } else {
            // Fallback to extracting from headers
            Ok(RequestContext::from_headers(&parts.headers))
        }
    }
}

/// Request context layer for axum
pub struct RequestContextLayer;

impl<S> tower::Layer<S> for RequestContextLayer {
    type Service = RequestContextService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestContextService { inner }
    }
}

/// Request context service
#[derive(Clone)]
pub struct RequestContextService<S> {
    inner: S,
}

impl<S> tower::Service<Request> for RequestContextService<S>
where
    S: tower::Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut request: Request) -> Self::Future {
        let context = RequestContext::from_headers(request.headers());
        request.extensions_mut().insert(context.clone());

        // Add request ID to response headers
        let mut inner = self.inner.clone();
        let request_id = context.request_id.clone();

        Box::pin(async move {
            let mut response = inner.call(request).await?;

            // Add request ID to response headers
            if let Ok(header_value) = HeaderValue::from_str(&request_id) {
                response
                    .headers_mut()
                    .insert(HeaderName::from_static("x-request-id"), header_value);
            }

            Ok(response)
        })
    }
}

/// Middleware function to extract request context
pub async fn extract_request_context(
    mut request: Request,
    next: Next,
) -> std::result::Result<Response, Response> {
    let context = RequestContext::from_headers(request.headers());

    debug!(
        request_id = %context.request_id,
        user_id = ?context.user_id,
        client_ip = ?context.client_ip,
        "Extracted request context"
    );

    // Store context in request extensions
    request.extensions_mut().insert(context.clone());

    // Process the request
    let mut response = next.run(request).await;

    // Add request ID to response headers
    if let Ok(header_value) = HeaderValue::from_str(&context.request_id) {
        response
            .headers_mut()
            .insert(HeaderName::from_static("x-request-id"), header_value);
    }

    Ok(response)
}

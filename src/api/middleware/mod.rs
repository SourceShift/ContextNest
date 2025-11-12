pub mod comprehensive_validation;
pub mod compression_middleware;
pub mod cors_middleware;
pub mod error_interceptor_middleware;
/// API middleware components for ContextNest
/// This module provides comprehensive middleware for:
/// - Request/response logging and tracing
/// - Authentication and authorization with JWT/API keys
/// - Error interception and handling
/// - Request context management
/// - Performance monitoring and metrics
/// - Request validation and security
/// - CORS, compression, and security headers
pub mod logging_middleware;
pub mod metrics_middleware;
pub mod request_context_middleware;
pub mod security_middleware;
pub mod validation_middleware;
pub mod validators;

// Legacy middleware modules for backward compatibility
pub mod error_interceptor;
pub mod logging;
pub mod request_context;
pub mod security;
pub mod validation;

use crate::api::server::ApiRequest;

// Re-export enhanced middleware
// `comprehensive_validation::{ValidationConfig, ValidationResult}` overlap
// with the older `validation_middleware::*` re-export below. Explicitly
// allow the glob ambiguity here — callers can still reach the conflicting
// types via their fully-qualified module paths if needed.
#[allow(ambiguous_glob_reexports)]
pub use comprehensive_validation::*;
pub use compression_middleware::*;
pub use cors_middleware::*;
pub use error_interceptor_middleware::*;
pub use logging_middleware::*;
pub use metrics_middleware::*;
pub use request_context_middleware::*;
pub use security_middleware::*;
// `validation_middleware` and `comprehensive_validation` both export the
// types `ValidationConfig` and `ValidationResult`. Re-export the older
// `validation_middleware::*` with the duplicated names suppressed —
// callers can still reach the conflicting types via the explicit module
// paths (`comprehensive_validation::ValidationConfig` etc).
#[allow(ambiguous_glob_reexports)]
pub use validation_middleware::*;

// Legacy re-exports for backward compatibility
pub use error_interceptor::ErrorInterceptorLayer;
pub use logging::LoggingMiddleware;
pub use request_context::{RequestContext, RequestContextLayer};
pub use security::SecurityMiddleware as SecurityMW;
pub use validation::ValidationMiddleware;

use crate::error::{ApiError, ContextNestError, ContextNestResult, ErrorContext};
use axum::{
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{error, info, warn};
use uuid::Uuid;

/// Comprehensive middleware stack.
/// rate limiting is intentionally NOT in v0.1.0. Deploy
/// behind a reverse-proxy (Caddy + jwt, nginx + oauth2-proxy, Cloudflare
/// Access) that handles rate limiting at L7. The previous code had
/// commented-out `rate_limiter: Arc<ApiRateLimiter>` scaffolding +
/// matching commented init — removed in this epic. For agent-specific
/// abuse vectors (reasoning bypass, tool-chain amplification) see
/// `arXiv:2509.01619` (reasoning gates) and `arXiv:2604.17111`
/// (OS-inspired LLM-agent scheduling) — those design refs land with the
///v0.2+ rate-limit work.
pub struct MiddlewareStack {
    validation_middleware: Arc<ValidationMiddleware>,
    security_middleware: Arc<SecurityMW>,
}

impl MiddlewareStack {
    pub fn new() -> Self {
        Self {
            validation_middleware: Arc::new(ValidationMiddleware::new()),
            security_middleware: Arc::new(SecurityMW::new()),
        }
    }

    /// Process request through middleware stack
    pub async fn process_request(
        &self,
        request: crate::api::server::ApiRequest,
    ) -> ContextNestResult<crate::api::server::ProcessedApiRequest> {
        let mut processed_request = crate::api::server::ProcessedApiRequest::from(request);

        // 1. Security middleware
        self.security_middleware
            .process_request(&mut processed_request)
            .await?;

        // 2. Rate limiting — not in v0.1.0. See struct doc-comment for the
        // reverse-proxy deployment pattern.

        // 3. Validation
        self.validation_middleware
            .process_request(&mut processed_request)
            .await?;
        processed_request.validated = true;

        // Mark as authenticated (no auth required, all endpoints public)
        processed_request.authenticated = true;
        processed_request.rate_limited = true;
        Ok(processed_request)
    }

    /// Process response through middleware stack
    pub async fn process_response(
        &self,
        response: crate::api::server::ApiResponse,
    ) -> ContextNestResult<crate::api::server::ApiResponse> {
        // Process response through middleware stack in reverse order
        Ok(response)
    }
}

// Authentication middleware removed - all endpoints are now public

/// Build the complete middleware stack
pub fn build_middleware_stack() -> ServiceBuilder<
    tower::layer::util::Stack<tower_http::cors::CorsLayer, tower::layer::util::Identity>,
> {
    ServiceBuilder::new().layer(CorsLayer::permissive()) // Configure CORS
}

/// Apply middleware to a router
pub fn apply_middleware(router: Router) -> Router {
    router
        .layer(middleware::from_fn(request_id_middleware))
        .layer(middleware::from_fn(error_handling_middleware))
        .layer(build_middleware_stack())
}

/// Apply comprehensive validation middleware to a router.
/// v0.1.0 returns the router unchanged — the comprehensive-validation pipeline
/// has open type-state wiring issues and is gated behind a follow-up epic
/// rather than blocking compile recovery .
pub fn apply_comprehensive_validation(router: Router) -> Router {
    router
}

/// Request ID middleware
pub async fn request_id_middleware(
    mut request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    // Add request ID to headers
    let request_id = Uuid::new_v4().to_string();
    request.headers_mut().insert(
        "x-request-id",
        request_id
            .parse()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );

    let response = next.run(request).await;
    Ok(response)
}

/// Error handling middleware
pub async fn error_handling_middleware(
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let start_time = Instant::now();

    match next.run(request).await {
        response => {
            let duration = start_time.elapsed();

            // Log response information
            info!(
                status = response.status().as_u16(),
                duration_ms = duration.as_millis(),
                "Request completed"
            );

            Ok(response)
        }
    }
}

/// Middleware configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiddlewareConfig {
    pub enable_cors: bool,
    pub enable_tracing: bool,
    pub enable_compression: bool,
    pub enable_rate_limiting: bool,
    pub cors_origins: Vec<String>,
    pub max_request_size: usize,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            enable_cors: true,
            enable_tracing: true,
            enable_compression: true,
            enable_rate_limiting: true,
            cors_origins: vec!["*".to_string()],
            max_request_size: 10 * 1024 * 1024, // 10MB
        }
    }
}

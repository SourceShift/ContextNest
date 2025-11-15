use crate::error::ContextNestResult;
use axum::{
    extract::Request,
    http::{header::*, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};
use std::time::Instant;
use tracing::{info, warn};

/// CORS configuration
#[derive(Debug, Clone)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub exposed_headers: Vec<String>,
    pub allow_credentials: bool,
    pub max_age: Option<u64>,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "OPTIONS".to_string(),
                "PATCH".to_string(),
            ],
            allowed_headers: vec![
                "Content-Type".to_string(),
                "Authorization".to_string(),
                "X-Requested-With".to_string(),
                "Accept".to_string(),
                "Origin".to_string(),
                "X-Request-ID".to_string(),
            ],
            exposed_headers: vec!["X-Total-Count".to_string(), "X-Request-ID".to_string()],
            allow_credentials: false,
            max_age: Some(86400), // 24 hours
        }
    }
}

/// CORS middleware
pub async fn cors_middleware(
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let start_time = Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let config = CorsConfig::default();

    // Handle preflight OPTIONS requests
    if method == "OPTIONS" {
        return Ok(handle_preflight_request(&request, &config));
    }

    let mut response = next.run(request).await;

    // Add CORS headers to actual response
    add_cors_headers(&mut response, &config);

    let duration = start_time.elapsed();
    info!(
        "CORS middleware: {} {} - processed in {:?}",
        method, path, duration
    );

    Ok(response)
}

/// Handle CORS preflight requests
fn handle_preflight_request(request: &Request, config: &CorsConfig) -> Response {
    let mut response = Response::new(axum::body::Body::empty());

    // Set appropriate status code for preflight
    *response.status_mut() = StatusCode::NO_CONTENT;

    // Add preflight headers
    add_preflight_headers(&mut response, request, config);

    response
}

/// Add preflight-specific headers
fn add_preflight_headers(response: &mut Response, request: &Request, config: &CorsConfig) {
    let headers = response.headers_mut();

    // Access-Control-Allow-Origin
    if let Some(origin) = request.headers().get(ORIGIN) {
        if let Ok(origin_str) = origin.to_str() {
            if is_origin_allowed(origin_str, &config.allowed_origins) {
                headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
            }
        }
    }

    // Access-Control-Allow-Methods
    let methods_str = config.allowed_methods.join(", ");
    headers.insert(ACCESS_CONTROL_ALLOW_METHODS, methods_str.parse().unwrap());

    // Access-Control-Allow-Headers
    let headers_str = config.allowed_headers.join(", ");
    headers.insert(ACCESS_CONTROL_ALLOW_HEADERS, headers_str.parse().unwrap());

    // Access-Control-Max-Age
    if let Some(max_age) = config.max_age {
        headers.insert(ACCESS_CONTROL_MAX_AGE, max_age.to_string().parse().unwrap());
    }

    // Access-Control-Allow-Credentials
    if config.allow_credentials {
        headers.insert(ACCESS_CONTROL_ALLOW_CREDENTIALS, "true".parse().unwrap());
    }
}

/// Add CORS headers to response
fn add_cors_headers(response: &mut Response, config: &CorsConfig) {
    let headers = response.headers_mut();

    // For simplicity, we'll use wildcard for origin in actual responses
    // In production, you should validate the origin against the request
    headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());

    // Access-Control-Expose-Headers
    if !config.exposed_headers.is_empty() {
        let exposed_headers_str = config.exposed_headers.join(", ");
        headers.insert(
            ACCESS_CONTROL_EXPOSE_HEADERS,
            exposed_headers_str.parse().unwrap(),
        );
    }

    // Access-Control-Allow-Credentials
    if config.allow_credentials {
        headers.insert(ACCESS_CONTROL_ALLOW_CREDENTIALS, "true".parse().unwrap());
    }

    // Vary header for proper caching
    headers.insert(VARY, "Origin".parse().unwrap());
}

/// Check if origin is allowed
fn is_origin_allowed(origin: &str, allowed_origins: &[String]) -> bool {
    if allowed_origins.contains(&"*".to_string()) {
        return true;
    }

    allowed_origins.contains(&origin.to_string())
        || allowed_origins.iter().any(|allowed| {
            allowed.starts_with("http://")
                && origin.starts_with("http://")
                && allowed.split_once("://").unwrap().1 == origin.split_once("://").unwrap().1
        })
        || allowed_origins.iter().any(|allowed| {
            allowed.starts_with("https://")
                && origin.starts_with("https://")
                && allowed.split_once("://").unwrap().1 == origin.split_once("://").unwrap().1
        })
}

/// More restrictive CORS configuration for production
pub fn production_cors_config() -> CorsConfig {
    CorsConfig {
        allowed_origins: vec![
            "https://contextnest.com".to_string(),
            "https://app.contextnest.com".to_string(),
            "https://api.contextnest.com".to_string(),
        ],
        allowed_methods: vec![
            "GET".to_string(),
            "POST".to_string(),
            "PUT".to_string(),
            "DELETE".to_string(),
        ],
        allowed_headers: vec![
            "Content-Type".to_string(),
            "Authorization".to_string(),
            "X-Request-ID".to_string(),
        ],
        exposed_headers: vec!["X-Request-ID".to_string()],
        allow_credentials: true,
        max_age: Some(7200), // 2 hours
    }
}

/// Development CORS configuration
pub fn development_cors_config() -> CorsConfig {
    CorsConfig {
        allowed_origins: vec![
            "http://localhost:3000".to_string(),
            "http://localhost:3001".to_string(),
            "http://127.0.0.1:3000".to_string(),
            "http://127.0.0.1:3001".to_string(),
            "*".to_string(), // Allow all origins for development
        ],
        allowed_methods: vec![
            "GET".to_string(),
            "POST".to_string(),
            "PUT".to_string(),
            "DELETE".to_string(),
            "OPTIONS".to_string(),
            "PATCH".to_string(),
        ],
        allowed_headers: vec![
            "*".to_string(), // Allow all headers for development
        ],
        exposed_headers: vec!["*".to_string()],
        allow_credentials: false,
        max_age: Some(86400), // 24 hours
    }
}

/// CORS middleware with custom configuration
pub async fn cors_middleware_with_config(
    config: CorsConfig,
) -> impl Fn(
    Request,
    Next,
)
    -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, StatusCode>> + Send>> {
    move |request: Request, next: Next| {
        let config = config.clone();
        Box::pin(async move {
            let start_time = Instant::now();
            let method = request.method().clone();
            let path = request.uri().path().to_string();

            // Handle preflight OPTIONS requests
            if method == "OPTIONS" {
                return Ok(handle_preflight_request(&request, &config));
            }

            let mut response = next.run(request).await;

            // Add CORS headers to actual response
            add_cors_headers(&mut response, &config);

            let duration = start_time.elapsed();
            info!(
                "CORS middleware: {} {} - processed in {:?}",
                method, path, duration
            );

            Ok(response)
        })
    }
}

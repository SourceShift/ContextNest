use crate::error::ContextNestResult;
use axum::{
    extract::Request,
    http::{
        header::{
            CONTENT_SECURITY_POLICY, REFERRER_POLICY, STRICT_TRANSPORT_SECURITY, USER_AGENT,
            X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
        },
        HeaderName, HeaderValue, StatusCode,
    },
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

/// Security middleware configuration
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub enable_csp: bool,
    pub enable_hsts: bool,
    pub enable_x_frame_options: bool,
    pub enable_x_content_type_options: bool,
    pub enable_referrer_policy: bool,
    pub custom_security_headers: Vec<(String, String)>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            enable_csp: true,
            enable_hsts: true,
            enable_x_frame_options: true,
            enable_x_content_type_options: true,
            enable_referrer_policy: true,
            custom_security_headers: Vec::new(),
        }
    }
}

/// Security headers middleware
pub async fn security_middleware(
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let start_time = Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    // Check for potentially malicious requests
    if is_suspicious_request(&request) {
        warn!(
            "Suspicious request detected: {} {} from unknown client",
            method, path
        );
        return Err(StatusCode::FORBIDDEN);
    }

    let response = next.run(request).await;

    // Add security headers to response
    let mut response_with_security = add_security_headers(response);

    let duration = start_time.elapsed();
    info!(
        "Security middleware: {} {} - processed in {:?}",
        method, path, duration
    );

    Ok(response_with_security)
}

/// Add security headers to response
fn add_security_headers(response: Response) -> Response {
    let config = SecurityConfig::default();
    let mut response_with_headers = response;

    // Content Security Policy
    if config.enable_csp {
        response_with_headers.headers_mut().insert(
            CONTENT_SECURITY_POLICY,
            "default-src 'self'; script-src 'self' 'unsafe-inline' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; font-src 'self' data:; connect-src 'self'; frame-ancestors 'none';".parse().unwrap(),
        );
    }

    // HTTP Strict Transport Security
    if config.enable_hsts {
        response_with_headers.headers_mut().insert(
            STRICT_TRANSPORT_SECURITY,
            "max-age=31536000; includeSubDomains; preload"
                .parse()
                .unwrap(),
        );
    }

    // X-Frame-Options
    if config.enable_x_frame_options {
        response_with_headers
            .headers_mut()
            .insert(X_FRAME_OPTIONS, "DENY".parse().unwrap());
    }

    // X-Content-Type-Options
    if config.enable_x_content_type_options {
        response_with_headers
            .headers_mut()
            .insert(X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    }

    // Referrer Policy
    if config.enable_referrer_policy {
        response_with_headers.headers_mut().insert(
            REFERRER_POLICY,
            "strict-origin-when-cross-origin".parse().unwrap(),
        );
    }

    // X-XSS-Protection
    response_with_headers.headers_mut().insert(
        HeaderName::from_static("x-xss-protection"),
        HeaderValue::from_static("1; mode=block"),
    );

    // Permissions Policy
    response_with_headers.headers_mut().insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("geolocation=(), microphone=(), camera=()"),
    );

    // Custom security headers
    for (name, value) in config.custom_security_headers {
        if let (Ok(name_header), Ok(value_header)) = (
            HeaderName::try_from(name.as_str()),
            HeaderValue::try_from(value.as_str()),
        ) {
            response_with_headers
                .headers_mut()
                .insert(name_header, value_header);
        }
    }

    response_with_headers
}

/// Check for suspicious request patterns
fn is_suspicious_request(request: &Request) -> bool {
    let path = request.uri().path();
    let headers = request.headers();

    // Check for common attack patterns in path
    let suspicious_patterns = [
        "../",
        "..\\",
        "%2e%2e%2f",
        "%2e%2e%5c", // Directory traversal
        "<script",
        "</script",
        "javascript:", // XSS
        "union.*select",
        "drop.*table", // SQL injection (simplified)
        "cmd.exe",
        "/bin/sh",
        "powershell", // Command injection
    ];

    for pattern in &suspicious_patterns {
        if path.to_lowercase().contains(&pattern.to_lowercase()) {
            error!("Suspicious path detected: {}", path);
            return true;
        }
    }

    // Check for suspicious user agents
    if let Some(user_agent) = headers.get(USER_AGENT) {
        if let Ok(user_agent_str) = user_agent.to_str() {
            let suspicious_user_agents = [
                "sqlmap", "nikto", "nmap", "masscan", "zap", "burp", "scanner", "crawler", "bot",
            ];

            for suspicious in &suspicious_user_agents {
                if user_agent_str.to_lowercase().contains(suspicious) {
                    warn!("Suspicious user agent detected: {}", user_agent_str);
                    return true;
                }
            }
        }
    }

    // Check for unusual header patterns
    if headers.len() > 50 {
        warn!("Unusually high number of headers: {}", headers.len());
        return true;
    }

    false
}

/// Rate limiting for repeated violations
pub struct ViolationTracker {
    violations: std::collections::HashMap<String, Vec<Instant>>,
    max_violations: usize,
    window_duration: Duration,
}

impl ViolationTracker {
    pub fn new(max_violations: usize, window_duration: Duration) -> Self {
        Self {
            violations: std::collections::HashMap::new(),
            max_violations,
            window_duration,
        }
    }

    pub fn record_violation(&mut self, client_id: &str) -> bool {
        let now = Instant::now();
        let window_duration = self.window_duration;
        let violations = self
            .violations
            .entry(client_id.to_string())
            .or_insert_with(Vec::new);

        // Clean up old violations
        violations.retain(|&time| now.duration_since(time) < window_duration);

        violations.push(now);

        // Return true if client should be blocked
        violations.len() > self.max_violations
    }

    pub fn is_blocked(&self, client_id: &str) -> bool {
        if let Some(violations) = self.violations.get(client_id) {
            let now = Instant::now();
            let window_duration = self.window_duration;
            let recent_violations = violations
                .iter()
                .filter(|&&time| now.duration_since(time) < window_duration)
                .count();
            recent_violations > self.max_violations
        } else {
            false
        }
    }
}

/// Input validation middleware
pub async fn input_validation_middleware(
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let path = request.uri().path();

    // Validate path characters
    if !is_valid_path(path) {
        error!("Invalid path characters detected: {}", path);
        return Err(StatusCode::BAD_REQUEST);
    }

    // Validate query parameters
    if let Some(query) = request.uri().query() {
        if !is_valid_query(query) {
            error!("Invalid query parameters detected: {}", query);
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    let response = next.run(request).await;
    Ok(response)
}

fn is_valid_path(path: &str) -> bool {
    // Allow alphanumeric, hyphens, underscores, forward slashes, and dots
    path.chars()
        .all(|c| c.is_alphanumeric() || "-._/".contains(c))
}

fn is_valid_query(query: &str) -> bool {
    // Basic validation - no null bytes or control characters
    !query.contains('\0') && query.chars().all(|c| !c.is_control())
}

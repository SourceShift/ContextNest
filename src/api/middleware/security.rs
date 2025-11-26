/// Security middleware for ContextNest API
/// This module provides comprehensive security features including:
/// - Input sanitization
/// - CSRF protection
/// - Content Security Policy
/// - Request size limits
/// - IP filtering
use crate::error::{ContextNestError, ContextNestResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, warn};

/// Security middleware
pub struct SecurityMiddleware {
    /// Maximum request body size in bytes
    max_request_size: usize,
    /// Allowed origins for CORS
    allowed_origins: Vec<String>,
    /// Blocked IP addresses
    blocked_ips: Vec<String>,
    /// Content Security Policy rules
    csp_policy: String,
    /// Enable CSRF protection
    csrf_protection: bool,
}

impl SecurityMiddleware {
    pub fn new() -> Self {
        Self {
            max_request_size: 10 * 1024 * 1024, // 10MB
            allowed_origins: vec!["*".to_string()], // Configure appropriately for production
            blocked_ips: vec![],
            csp_policy: "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'".to_string(),
            csrf_protection: true,
        }
    }

    /// Configure maximum request size
    pub fn with_max_request_size(mut self, size: usize) -> Self {
        self.max_request_size = size;
        self
    }

    /// Configure allowed origins
    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = origins;
        self
    }

    /// Configure blocked IPs
    pub fn with_blocked_ips(mut self, ips: Vec<String>) -> Self {
        self.blocked_ips = ips;
        self
    }

    /// Configure CSP policy
    pub fn with_csp_policy(mut self, policy: String) -> Self {
        self.csp_policy = policy;
        self
    }

    /// Enable/disable CSRF protection
    pub fn with_csrf_protection(mut self, enabled: bool) -> Self {
        self.csrf_protection = enabled;
        self
    }

    /// Process request through security middleware
    pub async fn process_request(
        &self,
        request: &mut crate::api::server::ProcessedApiRequest,
    ) -> ContextNestResult<()> {
        // 1. Check IP blocklist
        self.check_ip_blocklist(request).await?;

        // 2. Validate request size
        self.validate_request_size(request).await?;

        // 3. Validate headers for security issues
        self.validate_security_headers(&request.inner.headers, request)
            .await?;

        // 4. Sanitize input data
        self.sanitize_input_data(request).await?;

        // 5. CSRF protection (for state-changing requests)
        if self.csrf_protection {
            self.check_csrf_protection(request).await?;
        }

        debug!(
            "Security validation passed for request: {}",
            request.inner.request_id
        );
        Ok(())
    }

    /// Check if request comes from blocked IP.
    /// Extracts the client IP from `X-Forwarded-For` (first hop), falling
    /// back to `X-Real-IP`, falling back to `127.0.0.1`. Trusts the
    /// reverse-proxy to set these headers correctly (per
    ///  the deployment posture is "behind a
    /// reverse proxy"). For untrusted-network deployments, layer a header-
    /// scrubbing rule in the proxy AND validate this method's source IP
    /// against the proxy's known address — that hardening work is v0.2+.
    async fn check_ip_blocklist(
        &self,
        request: &crate::api::server::ProcessedApiRequest,
    ) -> ContextNestResult<()> {
        let default_ip = "127.0.0.1".to_string();
        let client_ip = request
            .inner
            .headers
            .get("x-forwarded-for")
            .or_else(|| request.inner.headers.get("x-real-ip"))
            .unwrap_or(&default_ip)
            .split(',')
            .next()
            .unwrap_or("127.0.0.1")
            .trim();

        if self.blocked_ips.contains(&client_ip.to_string()) {
            warn!("Blocked IP attempted access: {}", client_ip);
            return Err(ContextNestError::Configuration(
                "Access denied from this IP address".to_string(),
            ));
        }

        Ok(())
    }

    /// Validate request size limits
    async fn validate_request_size(
        &self,
        request: &crate::api::server::ProcessedApiRequest,
    ) -> ContextNestResult<()> {
        if let Some(body) = &request.inner.body {
            let body_size = serde_json::to_string(body)?.len();
            if body_size > self.max_request_size {
                warn!("Request size exceeded limit: {} bytes", body_size);
                return Err(ContextNestError::Configuration(format!(
                    "Request too large: {} bytes (max: {})",
                    body_size, self.max_request_size
                )));
            }
        }

        Ok(())
    }

    /// Validate security-related headers
    async fn validate_security_headers(
        &self,
        headers: &HashMap<String, String>,
        request: &crate::api::server::ProcessedApiRequest,
    ) -> ContextNestResult<()> {
        // Check for suspicious headers that might indicate attacks
        let suspicious_patterns = [
            ("user-agent", vec!["sqlmap", "nikto", "nmap", "scanner"]),
            ("referer", vec!["<script", "javascript:", "data:"]),
            ("x-forwarded-for", vec!["../", "..\\", "<script"]),
        ];

        for (header_name, patterns) in &suspicious_patterns {
            if let Some(header_value) = headers.get(*header_name) {
                let header_lower = header_value.to_lowercase();
                for pattern in patterns {
                    if header_lower.contains(pattern) {
                        warn!(
                            "Suspicious header detected: {} = {}",
                            header_name, header_value
                        );
                        return Err(ContextNestError::Configuration(format!(
                            "Suspicious {} header detected",
                            header_name
                        )));
                    }
                }
            }
        }

        // Validate Content-Type for POST/PUT requests
        if ["POST", "PUT", "PATCH"].contains(&request.inner.method.as_str()) {
            if let Some(content_type) = headers.get("content-type") {
                if !content_type.starts_with("application/json")
                    && !content_type.starts_with("multipart/form-data")
                {
                    return Err(ContextNestError::Configuration(
                        "Unsupported content type".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Sanitize input data to prevent injection attacks
    async fn sanitize_input_data(
        &self,
        request: &mut crate::api::server::ProcessedApiRequest,
    ) -> ContextNestResult<()> {
        if let Some(body) = &mut request.inner.body {
            self.sanitize_json_value(body)?;
        }

        // Sanitize query parameters
        for (key, value) in &mut request.inner.query_params {
            let sanitized_value = self.sanitize_string(value)?;
            *value = sanitized_value;
        }

        Ok(())
    }

    /// Recursively sanitize JSON values
    fn sanitize_json_value(&self, value: &mut serde_json::Value) -> ContextNestResult<()> {
        match value {
            serde_json::Value::String(s) => {
                *s = self.sanitize_string(s)?;
            }
            serde_json::Value::Object(obj) => {
                for (_, v) in obj.iter_mut() {
                    self.sanitize_json_value(v)?;
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr.iter_mut() {
                    self.sanitize_json_value(v)?;
                }
            }
            _ => {} // Numbers, booleans, null don't need sanitization
        }
        Ok(())
    }

    /// Sanitize string input
    fn sanitize_string(&self, input: &str) -> ContextNestResult<String> {
        // Check for common injection patterns
        let dangerous_patterns = [
            "<script",
            "</script>",
            "javascript:",
            "data:text/html",
            "onload=",
            "onerror=",
            "onclick=",
            "eval(",
            "Function(",
            "setTimeout(",
            "setInterval(",
            "document.cookie",
            "drop table",
            "union select",
            "1=1--",
            "' or '1'='1",
            "; drop",
            "'; drop",
        ];

        let input_lower = input.to_lowercase();
        for pattern in &dangerous_patterns {
            if input_lower.contains(pattern) {
                warn!("Dangerous pattern detected in input: {}", pattern);
                return Err(ContextNestError::Configuration(format!(
                    "Input contains potentially dangerous content: {}",
                    pattern
                )));
            }
        }

        // Basic HTML encoding for safety
        let sanitized = input
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#x27;")
            .replace('&', "&amp;");

        // Check length after sanitization
        if sanitized.len() > 10000 {
            // 10KB limit per string
            return Err(ContextNestError::Configuration(
                "Input string too long after sanitization".to_string(),
            ));
        }

        Ok(sanitized)
    }

    /// Check CSRF protection for state-changing requests
    async fn check_csrf_protection(
        &self,
        request: &crate::api::server::ProcessedApiRequest,
    ) -> ContextNestResult<()> {
        // Only check CSRF for state-changing methods
        if !["POST", "PUT", "PATCH", "DELETE"].contains(&request.inner.method.as_str()) {
            return Ok(());
        }

        // Check for CSRF token in headers
        let csrf_token = request
            .inner
            .headers
            .get("x-csrf-token")
            .or_else(|| request.inner.headers.get("csrf-token"));

        if csrf_token.is_none() {
            return Err(ContextNestError::Configuration(
                "CSRF token required for this request".to_string(),
            ));
        }

        // In production, validate the token against a session store
        let token = csrf_token.unwrap();
        if token.is_empty() || token.len() < 16 {
            return Err(ContextNestError::Configuration(
                "Invalid CSRF token".to_string(),
            ));
        }

        Ok(())
    }

    /// Generate security headers for response
    pub fn generate_security_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();

        // Content Security Policy
        headers.insert(
            "Content-Security-Policy".to_string(),
            self.csp_policy.clone(),
        );

        // Other security headers
        headers.insert("X-Content-Type-Options".to_string(), "nosniff".to_string());
        headers.insert("X-Frame-Options".to_string(), "DENY".to_string());
        headers.insert("X-XSS-Protection".to_string(), "1; mode=block".to_string());
        headers.insert(
            "Referrer-Policy".to_string(),
            "strict-origin-when-cross-origin".to_string(),
        );
        headers.insert(
            "Strict-Transport-Security".to_string(),
            "max-age=31536000; includeSubDomains".to_string(),
        );

        // Remove server information
        headers.insert("Server".to_string(), "ContextNest".to_string());

        headers
    }
}

/// Security configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub max_request_size: usize,
    pub allowed_origins: Vec<String>,
    pub blocked_ips: Vec<String>,
    pub csp_policy: String,
    pub csrf_protection: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            max_request_size: 10 * 1024 * 1024, // 10MB
            allowed_origins: vec!["*".to_string()],
            blocked_ips: vec![],
            csp_policy: "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'".to_string(),
            csrf_protection: true,
        }
    }
}

/// Security audit logger
pub struct SecurityAuditLogger;

impl SecurityAuditLogger {
    pub fn log_security_event(
        &self,
        event_type: SecurityEventType,
        request_id: &str,
        details: &str,
    ) {
        match event_type {
            SecurityEventType::BlockedIP => {
                warn!(
                    event_type = "blocked_ip",
                    request_id = request_id,
                    details = details,
                    "Security event: Blocked IP access attempt"
                );
            }
            SecurityEventType::SuspiciousInput => {
                warn!(
                    event_type = "suspicious_input",
                    request_id = request_id,
                    details = details,
                    "Security event: Suspicious input detected"
                );
            }
            SecurityEventType::CSRFViolation => {
                warn!(
                    event_type = "csrf_violation",
                    request_id = request_id,
                    details = details,
                    "Security event: CSRF protection violation"
                );
            }
            SecurityEventType::RateLimitExceeded => {
                warn!(
                    event_type = "rate_limit_exceeded",
                    request_id = request_id,
                    details = details,
                    "Security event: Rate limit exceeded"
                );
            }
        }
    }
}

/// Types of security events
#[derive(Debug, Clone, Copy)]
pub enum SecurityEventType {
    BlockedIP,
    SuspiciousInput,
    CSRFViolation,
    RateLimitExceeded,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_string_sanitization() {
        let security = SecurityMiddleware::new();

        // Safe string
        let safe = "Hello world";
        assert_eq!(security.sanitize_string(safe).unwrap(), "Hello world");

        // String with HTML
        let html = "<script>alert('xss')</script>";
        assert!(security.sanitize_string(html).is_err());

        // String with SQL injection attempt
        let sql = "'; DROP TABLE users; --";
        assert!(security.sanitize_string(sql).is_err());
    }

    #[tokio::test]
    async fn test_request_size_validation() {
        let security = SecurityMiddleware::new().with_max_request_size(100);

        let mut request = crate::api::server::ProcessedApiRequest {
            inner: crate::api::server::ApiRequest {
                path: "/test".to_string(),
                method: "POST".to_string(),
                headers: HashMap::new(),
                body: Some(json!({"data": "x".repeat(200)})), // Too large
                query_params: HashMap::new(),
                user_context: None,
                request_id: "test".to_string(),
                timestamp: chrono::Utc::now(),
            },
            validated: false,
            authenticated: false,
            rate_limited: false,
        };

        assert!(security.validate_request_size(&request).await.is_err());
    }
}

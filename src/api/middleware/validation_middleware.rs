use crate::error::ContextNestResult;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Validation configuration
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    pub max_request_size: usize,
    pub allowed_content_types: Vec<String>,
    pub required_headers: Vec<String>,
    pub validate_json_schema: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_request_size: 10 * 1024 * 1024, // 10MB
            allowed_content_types: vec![
                "application/json".to_string(),
                "text/plain".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            ],
            required_headers: vec!["content-type".to_string()],
            validate_json_schema: true,
        }
    }
}

/// Validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub request_id: String,
}

impl ValidationResult {
    fn success(request_id: String) -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
            request_id,
        }
    }

    fn failure(errors: Vec<String>, request_id: String) -> Self {
        Self {
            is_valid: false,
            errors,
            warnings: Vec::new(),
            request_id,
        }
    }
}

/// Request validator
pub struct RequestValidator {
    config: ValidationConfig,
}

impl RequestValidator {
    pub fn new(config: ValidationConfig) -> Self {
        Self { config }
    }

    pub async fn validate_request(
        &self,
        request: &Request,
        request_id: String,
    ) -> ValidationResult {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Validate headers
        self.validate_headers(request.headers(), &mut errors, &mut warnings);

        // Validate content type
        if let Err(e) = self.validate_content_type(request.headers()) {
            errors.push(e);
        }

        // Validate request size
        if let Err(e) = self.validate_request_size(request) {
            errors.push(e);
        }

        // Validate body if present
        if let Err(e) = self.validate_body(request).await {
            errors.push(e);
        }

        if errors.is_empty() {
            ValidationResult::success(request_id)
        } else {
            ValidationResult::failure(errors, request_id)
        }
    }

    fn validate_headers(
        &self,
        headers: &HeaderMap,
        errors: &mut Vec<String>,
        warnings: &mut Vec<String>,
    ) {
        for required_header in &self.config.required_headers {
            if !headers.contains_key(required_header) {
                errors.push(format!("Missing required header: {}", required_header));
            }
        }

        // Check for security headers
        if !headers.contains_key("x-request-id") {
            warnings.push("Missing x-request-id header for request tracing".to_string());
        }
    }

    fn validate_content_type(&self, headers: &HeaderMap) -> std::result::Result<(), String> {
        if let Some(content_type) = headers.get("content-type") {
            if let Ok(content_type_str) = content_type.to_str() {
                let content_type_main = content_type_str.split(';').next().unwrap_or("").trim();

                if !self
                    .config
                    .allowed_content_types
                    .contains(&content_type_main.to_string())
                {
                    return Err(format!(
                        "Content type '{}' is not allowed",
                        content_type_main
                    ));
                }
            } else {
                return Err("Invalid content-type header format".to_string());
            }
        }
        Ok(())
    }

    /// Validate request size via the Content-Length header.
    /// Header-based validation is sufficient for HTTP/1.1 fixed-length bodies
    /// (the body has not been received yet at middleware time). Chunked
    /// transfer encoding is handled by axum's body-size limit layer.
    fn validate_request_size(&self, request: &Request) -> std::result::Result<(), String> {
        if let Some(content_length) = request.headers().get("content-length") {
            if let Ok(length_str) = content_length.to_str() {
                if let Ok(length) = length_str.parse::<usize>() {
                    if length > self.config.max_request_size {
                        return Err(format!(
                            "Request size {} exceeds maximum allowed size {}",
                            length, self.config.max_request_size
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Validate request body contents.
    /// this was a placeholder that logged "JSON request
    /// validation passed" without actually inspecting the body. Body-content
    /// validation is now the responsibility of the per-handler axum
    /// extractors (`Json<T>` does serde-driven schema validation; the
    /// seven-tool API uses typed request structs that fail-fast on malformed
    /// payloads). This method is a no-op kept so the call site in
    /// `ValidationMiddleware::process_request` doesn't need to change; remove
    /// when the call site is refactored.
    #[allow(clippy::unused_async)]
    async fn validate_body(&self, _request: &Request) -> std::result::Result<(), String> {
        Ok(())
    }
}

/// Validation middleware
pub async fn validation_middleware(
    State(validator): State<Arc<RequestValidator>>,
    mut request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    // Generate or extract request ID
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Add request ID to headers for downstream services
    request
        .headers_mut()
        .insert("x-request-id", request_id.parse().unwrap());

    // Validate the request
    let result = validator
        .validate_request(&request, request_id.clone())
        .await;

    if !result.is_valid {
        error!("Request validation failed: {:?}", result.errors);

        let error_response = json!({
            "error": "Validation failed",
            "errors": result.errors,
            "request_id": result.request_id,
            "timestamp": chrono::Utc::now().to_rfc3339()
        });

        return Err(StatusCode::BAD_REQUEST);
    }

    // Log warnings
    if !result.warnings.is_empty() {
        warn!("Request validation warnings: {:?}", result.warnings);
    }

    info!(
        "Request validation successful for request ID: {}",
        result.request_id
    );

    let response = next.run(request).await;
    Ok(response)
}

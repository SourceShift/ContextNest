use crate::error::ContextNestResult;
use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use flate2::{
    write::{GzEncoder, ZlibEncoder},
    Compression,
};
use std::{io::Write, time::Instant};
use tracing::{debug, info, warn};

/// Compression configuration
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    pub enable_gzip: bool,
    pub enable_deflate: bool,
    pub enable_brotli: bool,
    pub min_size_to_compress: usize,
    pub compression_level: u32,
    pub compressible_content_types: Vec<String>,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enable_gzip: true,
            enable_deflate: true,
            enable_brotli: false,       // Requires additional dependency
            min_size_to_compress: 1024, // 1KB
            compression_level: 6,       // Default compression level
            compressible_content_types: vec![
                "text/html".to_string(),
                "text/css".to_string(),
                "text/javascript".to_string(),
                "application/javascript".to_string(),
                "application/json".to_string(),
                "application/xml".to_string(),
                "text/xml".to_string(),
                "text/plain".to_string(),
            ],
        }
    }
}

/// Compression middleware
pub async fn compression_middleware(
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    let start_time = Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    // Check if the client supports compression
    let accepted_encodings = get_accepted_encodings(&request);

    if accepted_encodings.is_empty() {
        // Client doesn't support compression, proceed normally
        let response = next.run(request).await;
        let duration = start_time.elapsed();
        info!(
            "Compression middleware: {} {} - no compression supported, processed in {:?}",
            method, path, duration
        );
        return Ok(response);
    }

    let response = next.run(request).await;

    // Check if response should be compressed
    if should_compress_response(&response, &CompressionConfig::default()) {
        let compressed_response =
            compress_response(response, &accepted_encodings, &CompressionConfig::default()).await;
        let duration = start_time.elapsed();
        info!(
            "Compression middleware: {} {} - compressed in {:?}",
            method, path, duration
        );
        Ok(compressed_response)
    } else {
        let duration = start_time.elapsed();
        info!(
            "Compression middleware: {} {} - not compressible, processed in {:?}",
            method, path, duration
        );
        Ok(response)
    }
}

/// Get accepted encodings from request headers
fn get_accepted_encodings(request: &Request) -> Vec<String> {
    let mut encodings = Vec::new();

    if let Some(accept_encoding) = request.headers().get("accept-encoding") {
        if let Ok(encoding_str) = accept_encoding.to_str() {
            // Parse the Accept-Encoding header
            // Format: gzip, deflate, br;q=0.9, *;q=0.5
            for part in encoding_str.split(',') {
                let part = part.trim();
                let encoding = if let Some((enc, _params)) = part.split_once(';') {
                    enc.trim()
                } else {
                    part
                };

                if !encoding.is_empty() && encoding != "*" {
                    encodings.push(encoding.to_lowercase());
                }
            }
        }
    }

    encodings
}

/// Check if response should be compressed
fn should_compress_response(response: &Response, config: &CompressionConfig) -> bool {
    // Don't compress if response is already compressed
    if response.headers().contains_key("content-encoding") {
        return false;
    }

    // Don't compress if no content
    if response.status().as_u16() == 204 || response.status().as_u16() == 304 {
        return false;
    }

    // Check content type
    if let Some(content_type) = response.headers().get("content-type") {
        if let Ok(content_type_str) = content_type.to_str() {
            let content_type_main = content_type_str.split(';').next().unwrap_or("").trim();

            if !config
                .compressible_content_types
                .contains(&content_type_main.to_string())
            {
                return false;
            }
        } else {
            return false;
        }
    } else {
        return false;
    }

    // Check content length if available
    if let Some(content_length) = response.headers().get("content-length") {
        if let Ok(length_str) = content_length.to_str() {
            if let Ok(length) = length_str.parse::<usize>() {
                return length >= config.min_size_to_compress;
            }
        }
    }

    // If we can't determine size, assume it's worth compressing
    true
}

/// Compress response using the best available encoding
async fn compress_response(
    response: Response,
    accepted_encodings: &[String],
    config: &CompressionConfig,
) -> Response {
    // Try encodings in order of preference
    for encoding in accepted_encodings {
        match encoding.as_str() {
            "gzip" if config.enable_gzip => {
                return compress_with_gzip(response, config).await;
            }
            "deflate" if config.enable_deflate => {
                return compress_with_deflate(response, config).await;
            }
            "br" if config.enable_brotli => {
                // Brotli compression would require additional dependency
                debug!("Brotli compression requested but not enabled");
                continue;
            }
            _ => continue,
        }
    }

    // No suitable encoding found
    response
}

/// Compress response using gzip
async fn compress_with_gzip(response: Response, config: &CompressionConfig) -> Response {
    match extract_body(response).await {
        Ok((mut response_parts, body_bytes)) => {
            let compression = Compression::new(config.compression_level);
            let mut encoder = GzEncoder::new(Vec::new(), compression);

            match encoder.write_all(&body_bytes) {
                Ok(()) => match encoder.finish() {
                    Ok(compressed_data) => {
                        // Update headers
                        response_parts
                            .headers
                            .insert("content-encoding", HeaderValue::from_static("gzip"));
                        response_parts
                            .headers
                            .insert("content-length", HeaderValue::from(compressed_data.len()));
                        response_parts.headers.remove("content-length");

                        debug!(
                            "Gzip compression: {} -> {} bytes ({:.1}% reduction)",
                            body_bytes.len(),
                            compressed_data.len(),
                            (1.0 - compressed_data.len() as f64 / body_bytes.len() as f64) * 100.0
                        );

                        let mut new_response = Response::new(Body::from(compressed_data));
                        *new_response.status_mut() = response_parts.status;
                        *new_response.headers_mut() = response_parts.headers;
                        new_response
                    }
                    Err(e) => {
                        warn!("Failed to finish gzip compression: {}", e);
                        recreate_response(response_parts, body_bytes)
                    }
                },
                Err(e) => {
                    warn!("Failed to write gzip data: {}", e);
                    recreate_response(response_parts, body_bytes)
                }
            }
        }
        Err(response) => {
            warn!("Failed to extract response body for compression");
            response
        }
    }
}

/// Compress response using deflate
async fn compress_with_deflate(response: Response, config: &CompressionConfig) -> Response {
    match extract_body(response).await {
        Ok((mut response_parts, body_bytes)) => {
            let compression = Compression::new(config.compression_level);
            let mut encoder = ZlibEncoder::new(Vec::new(), compression);

            match encoder.write_all(&body_bytes) {
                Ok(()) => match encoder.finish() {
                    Ok(compressed_data) => {
                        // Update headers
                        response_parts
                            .headers
                            .insert("content-encoding", HeaderValue::from_static("deflate"));
                        response_parts
                            .headers
                            .insert("content-length", HeaderValue::from(compressed_data.len()));
                        response_parts.headers.remove("content-length");

                        debug!(
                            "Deflate compression: {} -> {} bytes ({:.1}% reduction)",
                            body_bytes.len(),
                            compressed_data.len(),
                            (1.0 - compressed_data.len() as f64 / body_bytes.len() as f64) * 100.0
                        );

                        let mut new_response = Response::new(Body::from(compressed_data));
                        *new_response.status_mut() = response_parts.status;
                        *new_response.headers_mut() = response_parts.headers;
                        new_response
                    }
                    Err(e) => {
                        warn!("Failed to finish deflate compression: {}", e);
                        recreate_response(response_parts, body_bytes)
                    }
                },
                Err(e) => {
                    warn!("Failed to write deflate data: {}", e);
                    recreate_response(response_parts, body_bytes)
                }
            }
        }
        Err(response) => {
            warn!("Failed to extract response body for compression");
            response
        }
    }
}

/// Extract body bytes from response
async fn extract_body(
    response: Response,
) -> std::result::Result<(axum::http::response::Parts, Vec<u8>), Response> {
    let (parts, body) = response.into_parts();

    match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => Ok((parts, bytes.to_vec())),
        Err(e) => {
            warn!("Failed to read response body: {}", e);
            Err(Response::from_parts(parts, axum::body::Body::empty()))
        }
    }
}

/// Recreate response from parts and body
fn recreate_response(parts: axum::http::response::Parts, body: Vec<u8>) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = parts.status;
    *response.headers_mut() = parts.headers;
    response
}

/// Streaming compression for large responses
pub async fn streaming_compression_middleware(
    request: Request,
    next: Next,
) -> std::result::Result<Response, StatusCode> {
    // For now, fall back to regular compression
    // In a full implementation, you would implement streaming compression
    compression_middleware(request, next).await
}

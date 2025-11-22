use super::RequestContext;
use crate::error::{ApiError, ApiErrorHandler, ContextNestResult, ErrorContext};
/// Error interception middleware for API requests
use axum::{
    extract::Request,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::time::Instant;
use tower::Layer;
use tracing::{debug, error, warn};

/// Error interceptor layer
#[derive(Clone)]
pub struct ErrorInterceptorLayer {
    handler: ApiErrorHandler,
}

impl ErrorInterceptorLayer {
    pub fn new(handler: ApiErrorHandler) -> Self {
        Self { handler }
    }
}

impl Default for ErrorInterceptorLayer {
    fn default() -> Self {
        Self::new(ApiErrorHandler::default())
    }
}

impl<S> Layer<S> for ErrorInterceptorLayer {
    type Service = ErrorInterceptorService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ErrorInterceptorService {
            inner,
            handler: self.handler.clone(),
        }
    }
}

/// Error interceptor service
#[derive(Clone)]
pub struct ErrorInterceptorService<S> {
    inner: S,
    handler: ApiErrorHandler,
}

impl<S> tower::Service<Request> for ErrorInterceptorService<S>
where
    S: tower::Service<Request, Response = Response> + Clone + Send + 'static,
    S::Error: std::fmt::Display + Send + Sync + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
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

    fn call(&mut self, request: Request) -> Self::Future {
        let mut inner = self.inner.clone();
        let handler = self.handler.clone();

        Box::pin(async move {
            let start_time = Instant::now();

            // Extract request context for error handling
            let request_context = request.extensions().get::<RequestContext>().cloned();
            let method = request.method().clone();
            let uri = request.uri().clone();

            match inner.call(request).await {
                Ok(response) => {
                    let status = response.status();
                    let duration = start_time.elapsed();

                    // Check if response indicates an error that wasn't properly handled
                    if status.is_client_error() || status.is_server_error() {
                        warn!(
                            method = %method,
                            uri = %uri,
                            status = %status,
                            duration_ms = duration.as_millis(),
                            "HTTP error response"
                        );
                    }

                    Ok(response)
                }
                Err(service_error) => {
                    let duration = start_time.elapsed();

                    // Convert service error to API error
                    let error_context = if let Some(req_ctx) = request_context {
                        req_ctx
                            .to_error_context()
                            .with_endpoint(method.to_string(), uri.to_string())
                            .with_metadata(
                                "duration_ms".to_string(),
                                serde_json::Value::Number(serde_json::Number::from(
                                    duration.as_millis() as u64,
                                )),
                            )
                    } else {
                        ErrorContext::new("middleware_error")
                            .with_endpoint(method.to_string(), uri.to_string())
                            .with_metadata(
                                "duration_ms".to_string(),
                                serde_json::Value::Number(serde_json::Number::from(
                                    duration.as_millis() as u64,
                                )),
                            )
                    };

                    let api_error = ApiError::internal_server_error(
                        "Service error",
                        service_error.to_string(),
                        Some(error_context),
                    );

                    let error_response = handler.handle_error(api_error);

                    error!(
                        method = %method,
                        uri = %uri,
                        error = %service_error,
                        duration_ms = duration.as_millis(),
                        "Service error intercepted"
                    );

                    // Convert error response to axum response
                    Ok(axum::Json(error_response).into_response())
                }
            }
        })
    }
}

/// Middleware function to intercept and handle errors
pub async fn intercept_errors(
    request: Request,
    next: Next,
) -> std::result::Result<Response, Response> {
    let start_time = Instant::now();
    let method = request.method().clone();
    let uri = request.uri().clone();
    let request_context = request.extensions().get::<RequestContext>().cloned();

    debug!(
        method = %method,
        uri = %uri,
        "Processing request"
    );

    let response = next.run(request).await;
    let status = response.status();
    let duration = start_time.elapsed();

    if status.is_success() {
        debug!(
            method = %method,
            uri = %uri,
            status = %status,
            duration_ms = duration.as_millis(),
            "Request completed successfully"
        );
    } else {
        warn!(
            method = %method,
            uri = %uri,
            status = %status,
            duration_ms = duration.as_millis(),
            "Request completed with error status"
        );
    }

    Ok(response)
}

/// Helper function to create error context from request information
pub fn create_error_context_from_request(
    method: &axum::http::Method,
    uri: &axum::http::Uri,
    request_context: Option<&RequestContext>,
) -> ErrorContext {
    let mut error_context =
        ErrorContext::new("request_error").with_endpoint(method.to_string(), uri.to_string());

    if let Some(req_ctx) = request_context {
        error_context = error_context.with_request_id(&req_ctx.request_id);

        if let Some(user_id) = &req_ctx.user_id {
            error_context = error_context.with_user_id(user_id);
        }

        if let Some(client_ip) = &req_ctx.client_ip {
            error_context = error_context.with_metadata(
                "client_ip".to_string(),
                serde_json::Value::String(client_ip.clone()),
            );
        }
    }

    error_context
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use tower::ServiceExt;

    async fn success_handler() -> &'static str {
        "success"
    }

    async fn error_handler() -> std::result::Result<&'static str, ApiError> {
        Err(ApiError::internal_server_error(
            "Test error",
            "This is a test error",
            None,
        ))
    }

    #[tokio::test]
    async fn test_error_interceptor_success() {
        let app = Router::new()
            .route("/success", get(success_handler))
            .layer(ErrorInterceptorLayer::default());

        let request = Request::builder()
            .uri("/success")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);
    }

    #[tokio::test]
    async fn test_error_interceptor_handles_errors() {
        let app = Router::new()
            .route("/error", get(error_handler))
            .layer(ErrorInterceptorLayer::default());

        let request = Request::builder()
            .uri("/error")
            .body(axum::body::Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 500);
    }
}

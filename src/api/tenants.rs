//! The v2 application API never accepts an unverified tenant/session selector.
//! All legacy data surfaces require the separate operator credential in tenant mode.
use crate::services::tenants::{types::StoreRequest, Error, Registry, Result};
use axum::{
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Invalid(_) => StatusCode::BAD_REQUEST,
            Error::Conflict(_) => StatusCode::CONFLICT,
            Error::Busy => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let message = if status == StatusCode::INTERNAL_SERVER_ERROR {
            "memory service unavailable".into()
        } else {
            self.to_string()
        };
        (status, Json(json!({"error":message}))).into_response()
    }
}
fn bearer(headers: &HeaderMap) -> Result<&str> {
    headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .filter(|s| !s.is_empty())
        .ok_or(Error::Unauthorized)
}

pub fn router(registry: Arc<Registry>) -> Router {
    Router::new()
        .route("/api/v2/sessions", post(register))
        .route("/api/v2/session/:action", post(transition))
        .route("/api/v2/memory/store", post(store))
        .route("/api/v2/memory/retrieve", post(retrieve))
        .route("/api/v2/memory/discard", post(discard))
        .route("/api/v2/memory/fragments", get(fragments))
        .route("/api/v2/memory/health", get(health))
        .layer(DefaultBodyLimit::max(96 * 1024))
        .layer(middleware::from_fn_with_state(registry.clone(), admission))
        .with_state(registry)
}
async fn admission(
    State(registry): State<Arc<Registry>>,
    request: Request,
    next: Next,
) -> Response {
    let _permit = match registry.admit() {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    match tokio::time::timeout(std::time::Duration::from_secs(3), next.run(request)).await {
        Ok(response) => response,
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"request deadline exceeded"})),
        )
            .into_response(),
    }
}
pub async fn operator_gate(
    State(registry): State<Arc<Registry>>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() == axum::http::Method::GET
        && ["/api/health", "/api/status"].contains(&request.uri().path())
    {
        return Json(json!({"status":"ok","healthy":true,"tenant_mode":true,"git_commit":env!("CONTEXTNEST_GIT_COMMIT"),"build_dirty":env!("CONTEXTNEST_BUILD_DIRTY")})).into_response();
    }
    match bearer(request.headers()) {
        Ok(token) if registry.is_operator(token) => next.run(request).await,
        _ => Error::Unauthorized.into_response(),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Registration {
    session_id: String,
    mode: String,
}
async fn register(
    State(r): State<Arc<Registry>>,
    headers: HeaderMap,
    Json(body): Json<Registration>,
) -> Result<impl IntoResponse> {
    Ok(Json(
        r.register(bearer(&headers)?, body.session_id, body.mode)
            .await?,
    ))
}
async fn store(
    State(r): State<Arc<Registry>>,
    headers: HeaderMap,
    Json(input): Json<StoreRequest>,
) -> Result<impl IntoResponse> {
    let (tenant, scope) = r.authenticate_session(bearer(&headers)?).await?;
    let acceptance = r.store(tenant, scope, input).await?;
    let status = if acceptance.indexing_status == "ready" {
        StatusCode::OK
    } else {
        StatusCode::ACCEPTED
    };
    Ok((status, Json(acceptance)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Query {
    query: String,
    #[serde(default = "top_k")]
    top_k: usize,
}
fn top_k() -> usize {
    4
}
async fn retrieve(
    State(r): State<Arc<Registry>>,
    headers: HeaderMap,
    Json(input): Json<Query>,
) -> Result<impl IntoResponse> {
    let (tenant, scope) = r.authenticate_session(bearer(&headers)?).await?;
    let hits = tokio::time::timeout(
        std::time::Duration::from_millis(1000),
        r.retrieve(tenant, scope, input.query, input.top_k),
    )
    .await
    .map_err(|_| Error::Busy)??;
    Ok(Json(json!({"hits":hits})))
}
async fn transition(
    State(r): State<Arc<Registry>>,
    headers: HeaderMap,
    Path(action): Path<String>,
) -> Result<impl IntoResponse> {
    let (tenant, scope) = r.authenticate_session(bearer(&headers)?).await?;
    tenant
        .with_db(move |db| db.transition(&scope, &action))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Discard {
    source_event_id: String,
}
async fn discard(
    State(r): State<Arc<Registry>>,
    headers: HeaderMap,
    Json(input): Json<Discard>,
) -> Result<impl IntoResponse> {
    let (tenant, scope) = r.authenticate_session(bearer(&headers)?).await?;
    tenant
        .with_db(move |db| db.discard(&scope, &input.source_event_id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn fragments(
    State(r): State<Arc<Registry>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    let (tenant, scope) = r.authenticate_session(bearer(&headers)?).await?;
    let (_, _, records) = tenant.with_db(move |db| db.read(&scope)).await?;
    Ok(Json(json!({"fragments":records})))
}
async fn health(State(r): State<Arc<Registry>>, headers: HeaderMap) -> Result<impl IntoResponse> {
    let (tenant, scope) = r.authenticate_session(bearer(&headers)?).await?;
    Ok(Json(tenant.with_db(move |db| db.stats(&scope)).await?))
}

//! HTTP surface for Concord P4 — the overlap-arbitration layer. Overlaps
//! are the tracked rows behind every stale/unrecorded/hot/owns/topic
//! notice; agents ack them, repeated unacknowledged notices escalate to
//! `human:operator`'s mailbox, and operator-authored `freezes` rows are
//! the one blocking `permissionDecision: "deny"` path (consumed by the
//! precheck).
//!
//! The store methods (upsert/ack/list/freeze CRUD) live in
//! [`coord_store`]; this module is the HTTP surface only, and its error
//! mapping mirrors `coord_principals.rs` (whose `err_to_response` is
//! private and therefore re-implemented locally).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response as AxumResponse},
    routing::{delete, get, post},
    Router,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use tracing::error;

use crate::services::coord_store::CoordStoreError;
use crate::services::ContextNestServices;

pub fn create_coord_overlaps_router() -> Router<ContextNestServices> {
    Router::new()
        .route(
            "/api/v1/coord/overlaps",
            get(list_overlaps).post(post_overlap),
        )
        .route("/api/v1/coord/overlaps/:id/ack", post(ack_overlap))
        .route(
            "/api/v1/coord/freezes",
            get(list_freezes).post(create_freeze),
        )
        .route("/api/v1/coord/freezes/:id", delete(delete_freeze))
}

// ──────────────────────── helpers ────────────────────────

/// Map `CoordStoreError` to the wire contract. Mirrors the private
/// `coord_principals::err_to_response`: the JSON side always carries an
/// `error` string and we never substitute a default value on failure.
fn err_to_response(e: CoordStoreError) -> AxumResponse {
    match e {
        CoordStoreError::NotFound => {
            (StatusCode::NOT_FOUND, Json(json!({"error": "not found"}))).into_response()
        }
        CoordStoreError::InvalidId(msg) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("invalid id: {msg}")})),
        )
            .into_response(),
        CoordStoreError::InvalidBody(msg) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("invalid body: {msg}")})),
        )
            .into_response(),
        CoordStoreError::Db(e) => {
            error!(error = %e, "coord_store database error");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "database error"})),
            )
                .into_response()
        }
        CoordStoreError::Json(e) => {
            error!(error = %e, "coord_store json error");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "json error"})),
            )
                .into_response()
        }
        CoordStoreError::Io(e) => {
            error!(error = %e, "coord_store io error");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "io error"})),
            )
                .into_response()
        }
    }
}

#[derive(Debug, Deserialize)]
struct ListOverlapsQuery {
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    since: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PostOverlapRequest {
    kind: String,
    subject: String,
    a: String,
    #[serde(default)]
    b: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AckOverlapRequest {
    principal: String,
    decision: String,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateFreezeRequest {
    glob: String,
    by: String,
    reason: String,
    ttl_secs: i64,
}

// ──────────────────────── handlers ────────────────────────

/// GET /api/v1/coord/overlaps[?state=open|acked|escalated|all][&since=<id>]
///
/// Newest-first, capped at 200. `since` is an incremental-polling cursor:
/// only rows with `id > since` are returned.
async fn list_overlaps(
    State(services): State<ContextNestServices>,
    Query(q): Query<ListOverlapsQuery>,
) -> AxumResponse {
    let state = q.state.unwrap_or_else(|| "all".to_string());
    let since = match q.since {
        Some(s) => match s.parse::<i64>() {
            Ok(n) => Some(n),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("invalid since cursor '{s}': must be an integer")
                    })),
                )
                    .into_response();
            }
        },
        None => None,
    };
    match services.coord_store.list_overlaps(&state, since, 200) {
        Ok(overlaps) => {
            let count = overlaps.len();
            (
                StatusCode::OK,
                Json(json!({ "count": count, "overlaps": overlaps })),
            )
                .into_response()
        }
        Err(e) => err_to_response(e),
    }
}

/// POST /api/v1/coord/overlaps — record one overlap notice manually.
/// Mirrors the store's `upsert_overlap` entry point (the notice sites in
/// the precheck/turn hooks call the store directly; this is the manual
/// operator path).
async fn post_overlap(
    State(services): State<ContextNestServices>,
    Json(body): Json<PostOverlapRequest>,
) -> AxumResponse {
    match services.coord_store.upsert_overlap(
        &body.kind,
        &body.subject,
        &body.a,
        body.b.as_deref(),
        Utc::now(),
    ) {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => err_to_response(e),
    }
}

/// POST /api/v1/coord/overlaps/:id/ack — ack one overlap (`proceed` or
/// `yield`).
async fn ack_overlap(
    State(services): State<ContextNestServices>,
    Path(id): Path<i64>,
    Json(body): Json<AckOverlapRequest>,
) -> AxumResponse {
    match services.coord_store.ack_overlap(
        id,
        &body.principal,
        &body.decision,
        body.note.as_deref(),
        Utc::now(),
    ) {
        Ok(item) => (StatusCode::OK, Json(item)).into_response(),
        Err(e) => err_to_response(e),
    }
}

/// POST /api/v1/coord/freezes — create a freeze on a path glob.
async fn create_freeze(
    State(services): State<ContextNestServices>,
    Json(body): Json<CreateFreezeRequest>,
) -> AxumResponse {
    match services.coord_store.create_freeze(
        &body.glob,
        &body.by,
        &body.reason,
        body.ttl_secs,
        Utc::now(),
    ) {
        Ok(freeze) => (StatusCode::CREATED, Json(freeze)).into_response(),
        Err(e) => err_to_response(e),
    }
}

/// DELETE /api/v1/coord/freezes/:id — remove a freeze by id.
async fn delete_freeze(
    State(services): State<ContextNestServices>,
    Path(id): Path<i64>,
) -> AxumResponse {
    match services.coord_store.delete_freeze(id) {
        Ok(()) => (StatusCode::OK, Json(json!({ "deleted": id }))).into_response(),
        Err(e) => err_to_response(e),
    }
}

/// GET /api/v1/coord/freezes — list live freezes (sweeping expired rows
/// on read).
async fn list_freezes(State(services): State<ContextNestServices>) -> AxumResponse {
    match services.coord_store.list_freezes(Utc::now()) {
        Ok(freezes) => {
            let count = freezes.len();
            (
                StatusCode::OK,
                Json(json!({ "count": count, "freezes": freezes })),
            )
                .into_response()
        }
        Err(e) => err_to_response(e),
    }
}

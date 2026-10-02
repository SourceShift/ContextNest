//! HTTP surface for the Concord P0 layer — principals, worker bindings,
//! per-principal mailbox. Routes mount at `/api/v1/coord/*` next to the
//! ephemeral lease plane (`api::coord`).
//!
//! The wire contract is the `concord-protocol.md` document in the
//! mini-ork `concord-p0-cli` worktree. Every handler here is one path
//! in that document; the helper `err_to_response` keeps storage errors
//! out of the handler bodies so the surface stays auditable.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response as AxumResponse},
    routing::{get, post, put},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::error;

use crate::services::coord_store::{
    validate_principal_id, CoordStoreError, Message, Principal, PrincipalUpsert,
};
use crate::services::ContextNestServices;

pub fn create_coord_principals_router() -> Router<ContextNestServices> {
    Router::new()
        .route(
            "/api/v1/coord/principals/:principal_id",
            put(put_principal)
                .get(get_principal)
                .delete(delete_principal),
        )
        .route("/api/v1/coord/principals", get(list_principals))
        .route(
            "/api/v1/coord/bindings/:worker_id",
            put(put_binding).get(get_binding),
        )
        .route(
            "/api/v1/coord/principals/:principal_id/messages",
            post(post_message).get(get_messages),
        )
        .route(
            "/api/v1/coord/principals/:principal_id/messages/:msg_id/ack",
            post(post_ack),
        )
}

// ──────────────────────── helpers ────────────────────────

/// Map `CoordStoreError` to the wire contract. The plan forbids
/// silent fallbacks, so the JSON side always carries an `error` string
/// and we never substitute a default value on failure.
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

#[derive(Debug, Serialize)]
struct UpsertPrincipalResponse {
    principal: Principal,
    unacked_messages: usize,
}

#[derive(Debug, Serialize)]
struct ListPrincipalsResponse {
    count: usize,
    principals: Vec<Principal>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PutBindingRequest {
    principal_id: String,
    #[serde(default)]
    pid: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct PostMessageRequest {
    from: String,
    body: String,
}

#[derive(Debug, Serialize)]
struct ListMessagesResponse {
    messages: Vec<Message>,
}

#[derive(Debug, Deserialize)]
struct AckRequest {
    by: String,
}

// ──────────────────────── handlers ────────────────────────

/// PUT /api/v1/coord/principals/:principal_id
async fn put_principal(
    State(services): State<ContextNestServices>,
    Path(principal_id): Path<String>,
    Json(body): Json<PrincipalUpsert>,
) -> AxumResponse {
    if validate_principal_id(&principal_id).is_err() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("invalid principal id: {principal_id}")})),
        )
            .into_response();
    }
    let principal = match services.coord_store.upsert_principal(&principal_id, body) {
        Ok(p) => p,
        Err(e) => return err_to_response(e),
    };
    let unacked = match services.coord_store.unacked_count(&principal_id) {
        Ok(n) => n,
        Err(e) => return err_to_response(e),
    };
    (
        StatusCode::OK,
        Json(UpsertPrincipalResponse {
            principal,
            unacked_messages: unacked,
        }),
    )
        .into_response()
}

/// GET /api/v1/coord/principals/:principal_id
async fn get_principal(
    State(services): State<ContextNestServices>,
    Path(principal_id): Path<String>,
) -> AxumResponse {
    match services.coord_store.get_principal(&principal_id) {
        Ok(Some(mut principal)) => {
            // Status is computed at read time — the stored value is
            // the last upsert's snapshot.
            principal.status =
                crate::services::coord_store::status_at(&principal, chrono::Utc::now())
                    .as_str()
                    .to_string();
            (StatusCode::OK, Json(principal)).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("principal not found: {principal_id}")})),
        )
            .into_response(),
        Err(e) => err_to_response(e),
    }
}

/// DELETE /api/v1/coord/principals/:principal_id
async fn delete_principal(
    State(services): State<ContextNestServices>,
    Path(principal_id): Path<String>,
) -> AxumResponse {
    match services.coord_store.end_principal(&principal_id) {
        Ok(principal) => (StatusCode::OK, Json(principal)).into_response(),
        Err(CoordStoreError::NotFound) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("principal not found: {principal_id}")})),
        )
            .into_response(),
        Err(e) => err_to_response(e),
    }
}

/// GET /api/v1/coord/principals[?status=active|all]
async fn list_principals(
    State(services): State<ContextNestServices>,
    Query(q): Query<ListQuery>,
) -> AxumResponse {
    let include_inactive = match q.status.as_deref() {
        None | Some("active") => false,
        Some("all") => true,
        Some(other) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("unknown status '{other}': use active or all")})),
            )
                .into_response();
        }
    };
    match services.coord_store.list_principals(include_inactive) {
        Ok(principals) => {
            let count = principals.len();
            (
                StatusCode::OK,
                Json(ListPrincipalsResponse { count, principals }),
            )
                .into_response()
        }
        Err(e) => err_to_response(e),
    }
}

/// PUT /api/v1/coord/bindings/:worker_id
async fn put_binding(
    State(services): State<ContextNestServices>,
    Path(worker_id): Path<String>,
    Json(body): Json<PutBindingRequest>,
) -> AxumResponse {
    match services
        .coord_store
        .bind(&worker_id, &body.principal_id, body.pid)
    {
        Ok(binding) => (StatusCode::OK, Json(binding)).into_response(),
        Err(e) => err_to_response(e),
    }
}

/// GET /api/v1/coord/bindings/:worker_id
async fn get_binding(
    State(services): State<ContextNestServices>,
    Path(worker_id): Path<String>,
) -> AxumResponse {
    match services.coord_store.get_binding(&worker_id) {
        Ok(Some(binding)) => (StatusCode::OK, Json(binding)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("binding not found: {worker_id}")})),
        )
            .into_response(),
        Err(e) => err_to_response(e),
    }
}

/// POST /api/v1/coord/principals/:principal_id/messages
async fn post_message(
    State(services): State<ContextNestServices>,
    Path(principal_id): Path<String>,
    Json(body): Json<PostMessageRequest>,
) -> AxumResponse {
    let body_bytes = body.body.len();
    if body_bytes == 0 || body_bytes > 8192 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("message body must be 1..=8192 bytes; got {body_bytes}")
            })),
        )
            .into_response();
    }
    match services
        .coord_store
        .post_message(&principal_id, &body.from, &body.body)
    {
        Ok(message) => (StatusCode::CREATED, Json(message)).into_response(),
        Err(e) => err_to_response(e),
    }
}

#[derive(Debug, Deserialize)]
struct ListMessagesQuery {
    #[serde(default)]
    unacked: Option<String>,
}

/// GET /api/v1/coord/principals/:principal_id/messages[?unacked=true|false]
async fn get_messages(
    State(services): State<ContextNestServices>,
    Path(principal_id): Path<String>,
    Query(q): Query<ListMessagesQuery>,
) -> AxumResponse {
    let unacked_only = match q.unacked.as_deref() {
        None | Some("false") | Some("0") => false,
        Some("true") | Some("1") => true,
        Some(other) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("unknown unacked value '{other}': use true/false")})),
            )
                .into_response();
        }
    };
    match services
        .coord_store
        .list_messages(&principal_id, unacked_only)
    {
        Ok(messages) => (StatusCode::OK, Json(ListMessagesResponse { messages })).into_response(),
        Err(e) => err_to_response(e),
    }
}

/// POST /api/v1/coord/principals/:principal_id/messages/:msg_id/ack
async fn post_ack(
    State(services): State<ContextNestServices>,
    Path((principal_id, msg_id)): Path<(String, String)>,
    Json(body): Json<AckRequest>,
) -> AxumResponse {
    match services
        .coord_store
        .ack_message(&principal_id, &msg_id, &body.by)
    {
        Ok(message) => (StatusCode::OK, Json(message)).into_response(),
        Err(e) => err_to_response(e),
    }
}

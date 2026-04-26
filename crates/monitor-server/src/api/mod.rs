mod sse;
mod tasks;
mod updates;
mod workstreams;

use std::sync::Arc;

use axum::{
    Router,
    http::StatusCode,
    middleware,
    response::{IntoResponse, Json, Response},
    routing::{get, patch, post},
};
use serde::Serialize;
use tower_http::trace::TraceLayer;

use crate::auth::{self, AuthState};
use crate::services::{AppService, ServiceError};

// ---------------------------------------------------------------------------
// Shared app state
// ---------------------------------------------------------------------------

pub type AppState = Arc<AppService>;

// ---------------------------------------------------------------------------
// Error response
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    message: String,
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let (status, error_kind, message) = match &self {
            ServiceError::NotFound(detail) => {
                tracing::warn!(%detail, "not found");
                (StatusCode::NOT_FOUND, "not_found", self.to_string())
            }
            ServiceError::InvalidInput(detail) => {
                (StatusCode::BAD_REQUEST, "invalid_input", detail.clone())
            }
            ServiceError::Db(db_err) => {
                tracing::error!(error = %db_err, "database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "An internal error occurred".to_string(),
                )
            }
        };

        let body = ErrorResponse {
            error: error_kind.to_string(),
            message,
        };

        (status, Json(body)).into_response()
    }
}

// ---------------------------------------------------------------------------
// Health endpoint
// ---------------------------------------------------------------------------

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok"}))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router(service: Arc<AppService>, auth_state: Arc<AuthState>) -> Router {
    // API routes (with auth middleware)
    let api = Router::new()
        .route("/health", get(health))
        .route(
            "/api/workstreams",
            post(workstreams::create).get(workstreams::list),
        )
        .route("/api/workstreams/{id}", patch(workstreams::update))
        .route("/api/tasks", post(tasks::create).get(tasks::list))
        .route("/api/tasks/{id}", patch(tasks::update))
        .route("/api/updates/manual", post(updates::ingest_manual))
        .route(
            "/api/updates/claude-hook",
            post(updates::ingest_claude_hook),
        )
        .route(
            "/api/updates/github-webhook",
            post(updates::ingest_github_webhook),
        )
        .route("/api/updates", get(updates::list))
        .route("/api/stream", get(sse::stream))
        .layer(middleware::from_fn_with_state(
            auth_state,
            auth::auth_middleware,
        ))
        .layer(TraceLayer::new_for_http());

    // Web UI routes (no auth — browser pages)
    let web = crate::web::router();

    // Merge at top level so web routes are NOT wrapped by API auth middleware
    Router::new()
        .merge(api)
        .merge(web)
        .with_state(service)
}

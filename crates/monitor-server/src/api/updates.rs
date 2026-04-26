use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use uuid::Uuid;

use monitor_common::api::{
    ClaudeHookUpdateRequest, GithubWebhookUpdateRequest, ListResponse, ManualUpdateRequest,
};

use super::AppState;
use crate::services::ServiceError;

// ---------------------------------------------------------------------------
// Query types
// ---------------------------------------------------------------------------

fn default_limit() -> i64 {
    50
}

#[derive(Deserialize)]
pub struct ListUpdatesQuery {
    task_id: Option<Uuid>,
    source: Option<String>,
    kind: Option<String>,
    #[serde(default, rename = "tag")]
    tags: Vec<String>,
    after_seq: Option<i64>,
    #[serde(default = "default_limit")]
    limit: i64,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn ingest_manual(
    State(service): State<AppState>,
    Json(req): Json<ManualUpdateRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    let update = service.ingest_manual_update(&req).await?;
    Ok((StatusCode::CREATED, Json(update)))
}

pub async fn ingest_claude_hook(
    State(service): State<AppState>,
    Json(req): Json<ClaudeHookUpdateRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    let update = service.ingest_claude_hook(&req).await?;
    Ok((StatusCode::CREATED, Json(update)))
}

pub async fn ingest_github_webhook(
    State(service): State<AppState>,
    Json(req): Json<GithubWebhookUpdateRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    let update = service.ingest_github_webhook(&req).await?;
    Ok((StatusCode::CREATED, Json(update)))
}

pub async fn list(
    State(service): State<AppState>,
    Query(params): Query<ListUpdatesQuery>,
) -> Result<impl IntoResponse, ServiceError> {
    let limit = params.limit.clamp(1, 200);
    let items = service
        .list_updates(
            params.task_id,
            params.source.as_deref(),
            params.kind.as_deref(),
            &params.tags,
            params.after_seq,
            limit,
        )
        .await?;
    let total = service
        .count_updates(
            params.task_id,
            params.source.as_deref(),
            params.kind.as_deref(),
            &params.tags,
            params.after_seq,
        )
        .await?;
    Ok(Json(ListResponse { items, total }))
}

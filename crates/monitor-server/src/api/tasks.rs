use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use monitor_common::api::{CreateTaskRequest, ListResponse, PatchTaskRequest};
use monitor_common::TaskStatus;

use super::AppState;
use crate::services::ServiceError;

// ---------------------------------------------------------------------------
// Query types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListTasksQuery {
    workstream_id: Option<Uuid>,
    status: Option<TaskStatus>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn create(
    State(service): State<AppState>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    let task = service.create_task(&req).await?;
    Ok((StatusCode::CREATED, Json(task)))
}

pub async fn list(
    State(service): State<AppState>,
    Query(params): Query<ListTasksQuery>,
) -> Result<impl IntoResponse, ServiceError> {
    let items = service
        .list_tasks(params.workstream_id, params.status)
        .await?;
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

pub async fn update(
    State(service): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchTaskRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    let task = service.update_task(id, &req).await?;
    Ok(Json(task))
}

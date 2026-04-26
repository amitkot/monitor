use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use monitor_common::api::{CreateWorkstreamRequest, ListResponse, PatchWorkstreamRequest};

use super::AppState;
use crate::services::ServiceError;

// ---------------------------------------------------------------------------
// Query types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ListWorkstreamsQuery {
    #[serde(default)]
    include_archived: bool,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn create(
    State(service): State<AppState>,
    Json(req): Json<CreateWorkstreamRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    let ws = service.create_workstream(&req).await?;
    Ok((StatusCode::CREATED, Json(ws)))
}

pub async fn list(
    State(service): State<AppState>,
    Query(params): Query<ListWorkstreamsQuery>,
) -> Result<impl IntoResponse, ServiceError> {
    let items = service.list_workstreams(params.include_archived).await?;
    let total = items.len() as i64;
    Ok(Json(ListResponse { items, total }))
}

pub async fn update(
    State(service): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchWorkstreamRequest>,
) -> Result<impl IntoResponse, ServiceError> {
    let ws = service.update_workstream(id, &req).await?;
    Ok(Json(ws))
}

use std::sync::Arc;

use askama::Template;
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use uuid::Uuid;

use monitor_common::{Task, Update, Workstream};

use crate::services::AppService;

// ---------------------------------------------------------------------------
// Askama → Axum IntoResponse adapter
// ---------------------------------------------------------------------------

struct HtmlTemplate<T: Template>(T);

impl<T: Template> IntoResponse for HtmlTemplate<T> {
    fn into_response(self) -> Response {
        match self.0.render() {
            Ok(html) => (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                html,
            )
                .into_response(),
            Err(e) => {
                tracing::error!("template render error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Template render error").into_response()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Template structs
// ---------------------------------------------------------------------------

/// A workstream with its tasks, used in the dashboard view.
struct WorkstreamWithTasks {
    workstream: Workstream,
    tasks: Vec<Task>,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    workstreams: Vec<WorkstreamWithTasks>,
}

#[derive(Template)]
#[template(path = "task_detail.html")]
struct TaskDetailTemplate {
    task: Task,
    status_str: String,
    workstream_name: String,
    updates: Vec<Update>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn index() -> Redirect {
    Redirect::permanent("/dashboard")
}

async fn dashboard(State(service): State<Arc<AppService>>) -> impl IntoResponse {
    let workstreams = match service.list_workstreams(false).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::error!("failed to list workstreams: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load dashboard").into_response();
        }
    };

    let all_tasks = match service.list_tasks(None, None).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("failed to list tasks: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load dashboard").into_response();
        }
    };

    let workstreams_with_tasks: Vec<WorkstreamWithTasks> = workstreams
        .into_iter()
        .map(|ws| {
            let tasks: Vec<Task> = all_tasks
                .iter()
                .filter(|t| t.workstream_id == ws.id)
                .cloned()
                .collect();
            WorkstreamWithTasks {
                workstream: ws,
                tasks,
            }
        })
        .collect();

    HtmlTemplate(DashboardTemplate {
        workstreams: workstreams_with_tasks,
    })
    .into_response()
}

async fn task_detail(
    State(service): State<Arc<AppService>>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    let task = match service.get_task(id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "Task not found").into_response();
        }
        Err(e) => {
            tracing::error!("failed to get task: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load task").into_response();
        }
    };

    // Get workstream name
    let workstreams = service.list_workstreams(true).await.unwrap_or_default();
    let workstream_name = workstreams
        .iter()
        .find(|ws| ws.id == task.workstream_id)
        .map(|ws| ws.name.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    // Get updates for this task (reverse chronological)
    let mut updates = service
        .list_updates(Some(id), None, None, &[], None, 200)
        .await
        .unwrap_or_default();
    updates.reverse();

    let status_str = task.status.to_string();

    HtmlTemplate(TaskDetailTemplate {
        task,
        status_str,
        workstream_name,
        updates,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router() -> Router<Arc<AppService>> {
    Router::new()
        .route("/", get(index))
        .route("/dashboard", get(dashboard))
        .route("/tasks/{id}", get(task_detail))
}

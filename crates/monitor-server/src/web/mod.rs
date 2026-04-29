use std::collections::HashMap;
use std::sync::Arc;

use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
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

struct FeedItem {
    update: Update,
    task: Option<Task>,
    workstream: Option<Workstream>,
}

struct TaskFeed {
    task: Task,
    workstream: Option<Workstream>,
    updates: Vec<Update>,
}

#[derive(Template)]
#[template(path = "stream.html")]
struct StreamTemplate {
    view: String,
    items: Vec<FeedItem>,
    task_feeds: Vec<TaskFeed>,
}

#[derive(Deserialize)]
struct StreamQuery {
    view: Option<String>,
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
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load dashboard",
            )
                .into_response();
        }
    };

    let all_tasks = match service.list_tasks(None, None).await {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("failed to list tasks: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to load dashboard",
            )
                .into_response();
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

async fn stream_page(
    State(service): State<Arc<AppService>>,
    Query(query): Query<StreamQuery>,
) -> impl IntoResponse {
    let view = match query.view.as_deref() {
        Some("grouped") => "grouped",
        Some("lanes") => "lanes",
        _ => "chrono",
    }
    .to_string();

    let mut updates = match service.list_updates(None, None, None, &[], None, 200).await {
        Ok(updates) => updates,
        Err(e) => {
            tracing::error!("failed to list updates: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load stream").into_response();
        }
    };
    updates.reverse();

    let tasks = match service.list_tasks(None, None).await {
        Ok(tasks) => tasks,
        Err(e) => {
            tracing::error!("failed to list tasks for stream: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load stream").into_response();
        }
    };
    let workstreams = match service.list_workstreams(true).await {
        Ok(workstreams) => workstreams,
        Err(e) => {
            tracing::error!("failed to list workstreams for stream: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to load stream").into_response();
        }
    };

    let tasks_by_id: HashMap<Uuid, Task> = tasks.into_iter().map(|task| (task.id, task)).collect();
    let workstreams_by_id: HashMap<Uuid, Workstream> = workstreams
        .into_iter()
        .map(|workstream| (workstream.id, workstream))
        .collect();

    let items: Vec<FeedItem> = updates
        .iter()
        .cloned()
        .map(|update| {
            let task = tasks_by_id.get(&update.task_id).cloned();
            let workstream = task
                .as_ref()
                .and_then(|task| workstreams_by_id.get(&task.workstream_id))
                .cloned();

            FeedItem {
                update,
                task,
                workstream,
            }
        })
        .collect();

    let mut grouped_updates: HashMap<Uuid, Vec<Update>> = HashMap::new();
    for update in updates {
        grouped_updates
            .entry(update.task_id)
            .or_default()
            .push(update);
    }

    let mut task_feeds: Vec<TaskFeed> = grouped_updates
        .into_iter()
        .filter_map(|(task_id, updates)| {
            let task = tasks_by_id.get(&task_id)?.clone();
            let workstream = workstreams_by_id.get(&task.workstream_id).cloned();

            Some(TaskFeed {
                task,
                workstream,
                updates,
            })
        })
        .collect();

    task_feeds.sort_by(|a, b| {
        let a_seq = a
            .updates
            .first()
            .map(|update| update.seq)
            .unwrap_or_default();
        let b_seq = b
            .updates
            .first()
            .map(|update| update.seq)
            .unwrap_or_default();
        b_seq.cmp(&a_seq)
    });

    HtmlTemplate(StreamTemplate {
        view,
        items,
        task_feeds,
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
        .route("/stream", get(stream_page))
        .route("/tasks/{id}", get(task_detail))
}

use tokio::sync::broadcast;
use uuid::Uuid;

use monitor_common::api::{
    ClaudeHookUpdateRequest, CreateTaskRequest, CreateWorkstreamRequest,
    GithubWebhookUpdateRequest, ManualUpdateRequest, PatchTaskRequest, PatchWorkstreamRequest,
};
use monitor_common::{Task, TaskStatus, Update, Workstream};

use crate::adapters::{claude_hook, github_webhook};
use crate::db::{Db, DbError};

#[derive(Debug, Clone)]
pub enum LiveEvent {
    UpdateCreated(Update),
    TaskCreated(Task),
    TaskUpdated(Task),
    WorkstreamCreated(Workstream),
    WorkstreamUpdated(Workstream),
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("not found: {0}")]
    NotFound(String),

    #[allow(dead_code)]
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error(transparent)]
    Db(DbError),
}

impl From<DbError> for ServiceError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::NotFound(msg) => ServiceError::NotFound(msg),
            other => ServiceError::Db(other),
        }
    }
}

pub type Result<T> = std::result::Result<T, ServiceError>;

// ---------------------------------------------------------------------------
// AppService
// ---------------------------------------------------------------------------

pub struct AppService {
    db: Db,
    tx: broadcast::Sender<LiveEvent>,
}

impl AppService {
    pub fn new(db: Db) -> Self {
        let (tx, _rx) = broadcast::channel(256);
        Self { db, tx }
    }

    // -----------------------------------------------------------------------
    // Workstreams
    // -----------------------------------------------------------------------

    pub async fn create_workstream(&self, req: &CreateWorkstreamRequest) -> Result<Workstream> {
        let ws = self.db.create_workstream(req).await?;
        tracing::info!(id = %ws.id, name = %ws.name, "workstream created");
        let _ = self.tx.send(LiveEvent::WorkstreamCreated(ws.clone()));
        Ok(ws)
    }

    pub async fn update_workstream(
        &self,
        id: Uuid,
        req: &PatchWorkstreamRequest,
    ) -> Result<Workstream> {
        let ws = self.db.update_workstream(id, req).await?;
        tracing::info!(id = %ws.id, "workstream updated");
        let _ = self.tx.send(LiveEvent::WorkstreamUpdated(ws.clone()));
        Ok(ws)
    }

    pub async fn list_workstreams(&self, include_archived: bool) -> Result<Vec<Workstream>> {
        Ok(self.db.list_workstreams(include_archived).await?)
    }

    // -----------------------------------------------------------------------
    // Tasks
    // -----------------------------------------------------------------------

    pub async fn create_task(&self, req: &CreateTaskRequest) -> Result<Task> {
        // Validate: workstream must exist
        if self.db.get_workstream(req.workstream_id).await?.is_none() {
            return Err(ServiceError::NotFound(format!(
                "workstream {}",
                req.workstream_id
            )));
        }

        let task = self.db.create_task(req).await?;
        tracing::info!(id = %task.id, name = %task.name, workstream_id = %task.workstream_id, "task created");
        let _ = self.tx.send(LiveEvent::TaskCreated(task.clone()));
        Ok(task)
    }

    pub async fn update_task(&self, id: Uuid, req: &PatchTaskRequest) -> Result<Task> {
        if let Some(workstream_id) = req.workstream_id {
            if self.db.get_workstream(workstream_id).await?.is_none() {
                return Err(ServiceError::NotFound(format!(
                    "workstream {}",
                    workstream_id
                )));
            }
        }

        let task = self.db.update_task(id, req).await?;
        tracing::info!(id = %task.id, "task updated");
        let _ = self.tx.send(LiveEvent::TaskUpdated(task.clone()));
        Ok(task)
    }

    pub async fn list_tasks(
        &self,
        workstream_id: Option<Uuid>,
        status: Option<TaskStatus>,
    ) -> Result<Vec<Task>> {
        Ok(self.db.list_tasks(workstream_id, status).await?)
    }

    pub async fn get_task(&self, id: Uuid) -> Result<Option<Task>> {
        Ok(self.db.get_task(id).await?)
    }

    // -----------------------------------------------------------------------
    // Updates
    // -----------------------------------------------------------------------

    pub async fn ingest_manual_update(&self, req: &ManualUpdateRequest) -> Result<Update> {
        // Validate: task must exist
        let task = self.db.get_task(req.task_id).await?;
        if task.is_none() {
            return Err(ServiceError::NotFound(format!("task {}", req.task_id)));
        }

        let update = self
            .db
            .insert_update(
                req.task_id,
                "manual",
                &req.message,
                req.kind.as_deref(),
                req.level.clone(),
                &req.tags,
                req.data.as_ref(),
            )
            .await?;

        tracing::info!(seq = update.seq, task_id = %update.task_id, source = %update.source, "update ingested");

        // Broadcast — ignore errors (no active receivers is fine)
        let _ = self.tx.send(LiveEvent::UpdateCreated(update.clone()));

        Ok(update)
    }

    pub async fn ingest_claude_hook(&self, req: &ClaudeHookUpdateRequest) -> Result<Update> {
        // Validate: task must exist
        let task = self.db.get_task(req.task_id).await?;
        if task.is_none() {
            return Err(ServiceError::NotFound(format!("task {}", req.task_id)));
        }

        let (source, message, kind, level, tags, data) =
            claude_hook::normalize_claude_hook(req.task_id, &req.payload);

        let update = self
            .db
            .insert_update(
                req.task_id,
                &source,
                &message,
                kind.as_deref(),
                level,
                &tags,
                data.as_ref(),
            )
            .await?;

        tracing::info!(seq = update.seq, task_id = %update.task_id, source = %update.source, "update ingested");

        let _ = self.tx.send(LiveEvent::UpdateCreated(update.clone()));
        Ok(update)
    }

    pub async fn ingest_github_webhook(&self, req: &GithubWebhookUpdateRequest) -> Result<Update> {
        // Validate: task must exist
        let task = self.db.get_task(req.task_id).await?;
        if task.is_none() {
            return Err(ServiceError::NotFound(format!("task {}", req.task_id)));
        }

        let (source, message, kind, level, tags, data) = github_webhook::normalize_github_webhook(
            req.task_id,
            req.headers.as_ref(),
            &req.payload,
        );

        let update = self
            .db
            .insert_update(
                req.task_id,
                &source,
                &message,
                kind.as_deref(),
                level,
                &tags,
                data.as_ref(),
            )
            .await?;

        tracing::info!(seq = update.seq, task_id = %update.task_id, source = %update.source, "update ingested");

        let _ = self.tx.send(LiveEvent::UpdateCreated(update.clone()));
        Ok(update)
    }

    pub async fn list_updates(
        &self,
        task_id: Option<Uuid>,
        source: Option<&str>,
        kind: Option<&str>,
        tags: &[String],
        after_seq: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Update>> {
        Ok(self
            .db
            .list_updates(task_id, source, kind, tags, after_seq, limit)
            .await?)
    }

    pub async fn count_updates(
        &self,
        task_id: Option<Uuid>,
        source: Option<&str>,
        kind: Option<&str>,
        tags: &[String],
        after_seq: Option<i64>,
    ) -> Result<i64> {
        Ok(self
            .db
            .count_updates(task_id, source, kind, tags, after_seq)
            .await?)
    }

    // -----------------------------------------------------------------------
    // Broadcast
    // -----------------------------------------------------------------------

    pub fn subscribe(&self) -> broadcast::Receiver<LiveEvent> {
        self.tx.subscribe()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;
    use monitor_common::api::{
        CreateTaskRequest, CreateWorkstreamRequest, ManualUpdateRequest, PatchTaskRequest,
        PatchWorkstreamRequest,
    };
    use monitor_common::{TaskStatus, WorkstreamStatus};

    async fn setup_service() -> AppService {
        let pool = init_db("sqlite::memory:").await.unwrap();
        let db = Db::new(pool);
        AppService::new(db)
    }

    // -- Workstream CRUD --

    #[tokio::test]
    async fn test_create_and_list_workstreams() {
        let svc = setup_service().await;

        let ws = svc
            .create_workstream(&CreateWorkstreamRequest {
                name: "Alpha".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        assert_eq!(ws.name, "Alpha");
        assert_eq!(ws.status, WorkstreamStatus::Active);

        let all = svc.list_workstreams(false).await.unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, ws.id);
    }

    #[tokio::test]
    async fn test_update_workstream() {
        let svc = setup_service().await;

        let ws = svc
            .create_workstream(&CreateWorkstreamRequest {
                name: "Beta".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let updated = svc
            .update_workstream(
                ws.id,
                &PatchWorkstreamRequest {
                    name: Some("Beta v2".to_string()),
                    status: Some(WorkstreamStatus::Archived),
                    metadata: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.name, "Beta v2");
        assert_eq!(updated.status, WorkstreamStatus::Archived);

        // Archived workstream excluded from default list
        let active = svc.list_workstreams(false).await.unwrap();
        assert!(active.is_empty());

        // But included when requested
        let all = svc.list_workstreams(true).await.unwrap();
        assert_eq!(all.len(), 1);
    }

    // -- Task CRUD --

    #[tokio::test]
    async fn test_create_and_list_tasks() {
        let svc = setup_service().await;

        let ws = svc
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = svc
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task One".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        assert_eq!(task.name, "Task One");
        assert_eq!(task.status, TaskStatus::Active);

        let tasks = svc.list_tasks(Some(ws.id), None).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task.id);
    }

    #[tokio::test]
    async fn test_update_task_rejects_unknown_workstream() {
        let svc = setup_service().await;

        let ws = svc
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = svc
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task One".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let err = svc
            .update_task(
                task.id,
                &PatchTaskRequest {
                    name: None,
                    workstream_id: Some(Uuid::new_v4()),
                    status: None,
                    summary_text: None,
                    summary_source: None,
                    metadata: None,
                },
            )
            .await
            .unwrap_err();

        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_get_task() {
        let svc = setup_service().await;

        let ws = svc
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = svc
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Find me".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let found = svc.get_task(task.id).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Find me");

        let missing = svc.get_task(Uuid::new_v4()).await.unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn test_update_task() {
        let svc = setup_service().await;

        let ws = svc
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = svc
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Original".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let updated = svc
            .update_task(
                task.id,
                &PatchTaskRequest {
                    name: Some("Renamed".to_string()),
                    workstream_id: None,
                    status: Some(TaskStatus::Done),
                    summary_text: None,
                    summary_source: None,
                    metadata: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.status, TaskStatus::Done);
    }

    // -- FK validation --

    #[tokio::test]
    async fn test_create_task_with_nonexistent_workstream_fails() {
        let svc = setup_service().await;

        let result = svc
            .create_task(&CreateTaskRequest {
                workstream_id: Uuid::new_v4(),
                name: "Orphan".to_string(),
                metadata: None,
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, ServiceError::NotFound(_)),
            "expected NotFound, got: {err:?}"
        );
    }

    #[tokio::test]
    async fn test_ingest_update_for_nonexistent_task_fails() {
        let svc = setup_service().await;

        let result = svc
            .ingest_manual_update(&ManualUpdateRequest {
                task_id: Uuid::new_v4(),
                message: "Ghost update".to_string(),
                level: None,
                kind: None,
                tags: vec![],
                data: None,
            })
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, ServiceError::NotFound(_)),
            "expected NotFound, got: {err:?}"
        );
    }

    // -- Updates CRUD --

    #[tokio::test]
    async fn test_ingest_and_list_updates() {
        let svc = setup_service().await;

        let ws = svc
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = svc
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let update = svc
            .ingest_manual_update(&ManualUpdateRequest {
                task_id: task.id,
                message: "Progress report".to_string(),
                level: None,
                kind: Some("note".to_string()),
                tags: vec!["v1".to_string()],
                data: None,
            })
            .await
            .unwrap();

        assert_eq!(update.source, "manual");
        assert_eq!(update.message, "Progress report");

        let updates = svc
            .list_updates(Some(task.id), None, None, &[], None, 100)
            .await
            .unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].id, update.id);
    }

    // -- Broadcast --

    #[tokio::test]
    async fn test_broadcast_on_ingest() {
        let svc = setup_service().await;

        let ws = svc
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = svc
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        // Subscribe BEFORE ingesting
        let mut rx = svc.subscribe();

        let update = svc
            .ingest_manual_update(&ManualUpdateRequest {
                task_id: task.id,
                message: "Broadcast me".to_string(),
                level: None,
                kind: None,
                tags: vec![],
                data: None,
            })
            .await
            .unwrap();

        let received = rx.recv().await.unwrap();
        match received {
            LiveEvent::UpdateCreated(received_update) => {
                assert_eq!(received_update.id, update.id);
                assert_eq!(received_update.message, "Broadcast me");
            }
            other => panic!("expected UpdateCreated event, got {other:?}"),
        }
    }
}

use chrono::{DateTime, Utc};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions, SqliteRow};
use sqlx::{migrate::MigrateDatabase, Row, Sqlite};
use uuid::Uuid;

use monitor_common::api::{
    CreateTaskRequest, CreateWorkstreamRequest, PatchTaskRequest, PatchWorkstreamRequest,
};
use monitor_common::{Task, TaskStatus, Update, UpdateLevel, Workstream, WorkstreamStatus};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlx error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid data: {0}")]
    InvalidData(String),
}

pub type Result<T> = std::result::Result<T, DbError>;

// ---------------------------------------------------------------------------
// Db struct
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // -----------------------------------------------------------------------
    // Workstreams
    // -----------------------------------------------------------------------

    pub async fn create_workstream(&self, req: &CreateWorkstreamRequest) -> Result<Workstream> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let id_str = id.to_string();
        let status_str = "active";
        let metadata_str = req
            .metadata
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| "{}".to_string());
        let now_str = now.to_rfc3339();

        sqlx::query(
            "INSERT INTO workstreams (id, name, status, metadata, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id_str)
        .bind(&req.name)
        .bind(status_str)
        .bind(&metadata_str)
        .bind(&now_str)
        .bind(&now_str)
        .execute(&self.pool)
        .await?;

        Ok(Workstream {
            id,
            name: req.name.clone(),
            status: WorkstreamStatus::Active,
            metadata: req
                .metadata
                .clone()
                .unwrap_or(serde_json::Value::Object(Default::default())),
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn list_workstreams(&self, include_archived: bool) -> Result<Vec<Workstream>> {
        let rows = if include_archived {
            sqlx::query("SELECT * FROM workstreams ORDER BY created_at")
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query("SELECT * FROM workstreams WHERE status = 'active' ORDER BY created_at")
                .fetch_all(&self.pool)
                .await?
        };

        rows.iter().map(row_to_workstream).collect()
    }

    pub async fn get_workstream(&self, id: Uuid) -> Result<Option<Workstream>> {
        let row = sqlx::query("SELECT * FROM workstreams WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(ref r) => Ok(Some(row_to_workstream(r)?)),
            None => Ok(None),
        }
    }

    pub async fn update_workstream(
        &self,
        id: Uuid,
        req: &PatchWorkstreamRequest,
    ) -> Result<Workstream> {
        let id_str = id.to_string();
        let now_str = Utc::now().to_rfc3339();

        // Build dynamic update
        let mut set_clauses = Vec::new();
        let mut binds: Vec<String> = Vec::new();

        if let Some(ref name) = req.name {
            set_clauses.push("name = ?");
            binds.push(name.clone());
        }
        if let Some(ref status) = req.status {
            set_clauses.push("status = ?");
            binds.push(status_to_str(status).to_string());
        }
        if let Some(ref metadata) = req.metadata {
            set_clauses.push("metadata = ?");
            binds.push(serde_json::to_string(metadata).unwrap_or_else(|_| "{}".to_string()));
        }

        set_clauses.push("updated_at = ?");
        binds.push(now_str);

        let sql = format!(
            "UPDATE workstreams SET {} WHERE id = ?",
            set_clauses.join(", ")
        );
        binds.push(id_str.clone());

        let mut query = sqlx::query(&sql);
        for b in &binds {
            query = query.bind(b);
        }
        let result = query.execute(&self.pool).await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound(format!("workstream {id}")));
        }

        // Fetch updated row
        let row = sqlx::query("SELECT * FROM workstreams WHERE id = ?")
            .bind(&id_str)
            .fetch_one(&self.pool)
            .await?;

        row_to_workstream(&row)
    }

    // -----------------------------------------------------------------------
    // Tasks
    // -----------------------------------------------------------------------

    pub async fn create_task(&self, req: &CreateTaskRequest) -> Result<Task> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let id_str = id.to_string();
        let ws_id_str = req.workstream_id.to_string();
        let status_str = "active";
        let metadata_str = req
            .metadata
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| "{}".to_string());
        let now_str = now.to_rfc3339();

        sqlx::query(
            "INSERT INTO tasks (id, workstream_id, name, status, metadata, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id_str)
        .bind(&ws_id_str)
        .bind(&req.name)
        .bind(status_str)
        .bind(&metadata_str)
        .bind(&now_str)
        .bind(&now_str)
        .execute(&self.pool)
        .await?;

        Ok(Task {
            id,
            workstream_id: req.workstream_id,
            name: req.name.clone(),
            status: TaskStatus::Active,
            summary_text: None,
            summary_updated_at: None,
            summary_source: None,
            metadata: req
                .metadata
                .clone()
                .unwrap_or(serde_json::Value::Object(Default::default())),
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn list_tasks(
        &self,
        workstream_id: Option<Uuid>,
        status: Option<TaskStatus>,
    ) -> Result<Vec<Task>> {
        let mut sql = "SELECT * FROM tasks WHERE 1=1".to_string();
        let mut binds: Vec<String> = Vec::new();

        if let Some(ws_id) = workstream_id {
            sql.push_str(" AND workstream_id = ?");
            binds.push(ws_id.to_string());
        }
        if let Some(ref st) = status {
            sql.push_str(" AND status = ?");
            binds.push(task_status_to_str(st).to_string());
        }

        sql.push_str(" ORDER BY created_at");

        let mut query = sqlx::query(&sql);
        for b in &binds {
            query = query.bind(b);
        }

        let rows = query.fetch_all(&self.pool).await?;
        rows.iter().map(row_to_task).collect()
    }

    pub async fn get_task(&self, id: Uuid) -> Result<Option<Task>> {
        let id_str = id.to_string();
        let row = sqlx::query("SELECT * FROM tasks WHERE id = ?")
            .bind(&id_str)
            .fetch_optional(&self.pool)
            .await?;

        match row {
            Some(ref r) => Ok(Some(row_to_task(r)?)),
            None => Ok(None),
        }
    }

    pub async fn update_task(&self, id: Uuid, req: &PatchTaskRequest) -> Result<Task> {
        let id_str = id.to_string();
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        let mut set_clauses = Vec::new();
        let mut binds: Vec<String> = Vec::new();

        if let Some(ref name) = req.name {
            set_clauses.push("name = ?");
            binds.push(name.clone());
        }
        if let Some(ref workstream_id) = req.workstream_id {
            set_clauses.push("workstream_id = ?");
            binds.push(workstream_id.to_string());
        }
        if let Some(ref status) = req.status {
            set_clauses.push("status = ?");
            binds.push(task_status_to_str(status).to_string());
        }
        if let Some(ref summary_text) = req.summary_text {
            set_clauses.push("summary_text = ?");
            binds.push(summary_text.clone());
            set_clauses.push("summary_updated_at = ?");
            binds.push(now_str.clone());
            set_clauses.push("summary_source = ?");
            binds.push(
                req.summary_source
                    .as_deref()
                    .unwrap_or("manual")
                    .to_string(),
            );
        }
        if let Some(ref metadata) = req.metadata {
            set_clauses.push("metadata = ?");
            binds.push(serde_json::to_string(metadata).unwrap_or_else(|_| "{}".to_string()));
        }

        set_clauses.push("updated_at = ?");
        binds.push(now_str);

        let sql = format!("UPDATE tasks SET {} WHERE id = ?", set_clauses.join(", "));
        binds.push(id_str.clone());

        let mut query = sqlx::query(&sql);
        for b in &binds {
            query = query.bind(b);
        }
        let result = query.execute(&self.pool).await?;

        if result.rows_affected() == 0 {
            return Err(DbError::NotFound(format!("task {id}")));
        }

        let row = sqlx::query("SELECT * FROM tasks WHERE id = ?")
            .bind(&id_str)
            .fetch_one(&self.pool)
            .await?;

        row_to_task(&row)
    }

    // -----------------------------------------------------------------------
    // Updates
    // -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_update(
        &self,
        task_id: Uuid,
        source: &str,
        message: &str,
        kind: Option<&str>,
        level: Option<UpdateLevel>,
        tags: &[String],
        data: Option<&serde_json::Value>,
    ) -> Result<Update> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let id_str = id.to_string();
        let task_id_str = task_id.to_string();
        let now_str = now.to_rfc3339();
        let level_str = level.as_ref().map(update_level_to_str);
        let tags_str = serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string());
        let data_str =
            data.map(|d| serde_json::to_string(d).unwrap_or_else(|_| "null".to_string()));

        sqlx::query(
            "INSERT INTO updates (id, task_id, source, timestamp, message, kind, level, tags, data)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id_str)
        .bind(&task_id_str)
        .bind(source)
        .bind(&now_str)
        .bind(message)
        .bind(kind)
        .bind(level_str)
        .bind(&tags_str)
        .bind(&data_str)
        .execute(&self.pool)
        .await?;

        // Read back the row to get the auto-generated seq
        let row = sqlx::query("SELECT * FROM updates WHERE id = ?")
            .bind(&id_str)
            .fetch_one(&self.pool)
            .await?;

        row_to_update(&row)
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
        let mut sql = "SELECT * FROM updates WHERE 1=1".to_string();
        // Track which bind positions are i64 vs String
        enum Bind {
            Str(String),
            Int(i64),
        }
        let mut binds: Vec<Bind> = Vec::new();

        if let Some(tid) = task_id {
            sql.push_str(" AND task_id = ?");
            binds.push(Bind::Str(tid.to_string()));
        }
        if let Some(src) = source {
            sql.push_str(" AND source = ?");
            binds.push(Bind::Str(src.to_string()));
        }
        if let Some(k) = kind {
            sql.push_str(" AND kind = ?");
            binds.push(Bind::Str(k.to_string()));
        }
        if let Some(seq) = after_seq {
            sql.push_str(" AND seq > ?");
            binds.push(Bind::Int(seq));
        }
        if !tags.is_empty() {
            sql.push_str(" AND EXISTS (SELECT 1 FROM json_each(updates.tags) WHERE value IN (");
            for idx in 0..tags.len() {
                if idx > 0 {
                    sql.push_str(", ");
                }
                sql.push('?');
            }
            sql.push_str("))");
            for tag in tags {
                binds.push(Bind::Str(tag.clone()));
            }
        }

        sql.push_str(" ORDER BY seq ASC LIMIT ?");
        binds.push(Bind::Int(limit));

        // We need to bind dynamically with mixed types.
        // sqlx doesn't have a great way to do this with query(), so we'll use a manual approach.
        let mut query = sqlx::query(&sql);
        for b in &binds {
            match b {
                Bind::Str(s) => query = query.bind(s),
                Bind::Int(i) => query = query.bind(i),
            }
        }

        let rows = query.fetch_all(&self.pool).await?;
        rows.iter().map(row_to_update).collect()
    }

    pub async fn count_updates(
        &self,
        task_id: Option<Uuid>,
        source: Option<&str>,
        kind: Option<&str>,
        tags: &[String],
        after_seq: Option<i64>,
    ) -> Result<i64> {
        let mut sql = "SELECT COUNT(*) AS count FROM updates WHERE 1=1".to_string();
        enum Bind {
            Str(String),
            Int(i64),
        }
        let mut binds: Vec<Bind> = Vec::new();

        if let Some(tid) = task_id {
            sql.push_str(" AND task_id = ?");
            binds.push(Bind::Str(tid.to_string()));
        }
        if let Some(src) = source {
            sql.push_str(" AND source = ?");
            binds.push(Bind::Str(src.to_string()));
        }
        if let Some(k) = kind {
            sql.push_str(" AND kind = ?");
            binds.push(Bind::Str(k.to_string()));
        }
        if let Some(seq) = after_seq {
            sql.push_str(" AND seq > ?");
            binds.push(Bind::Int(seq));
        }
        if !tags.is_empty() {
            sql.push_str(" AND EXISTS (SELECT 1 FROM json_each(updates.tags) WHERE value IN (");
            for idx in 0..tags.len() {
                if idx > 0 {
                    sql.push_str(", ");
                }
                sql.push('?');
            }
            sql.push_str("))");
            for tag in tags {
                binds.push(Bind::Str(tag.clone()));
            }
        }

        let mut query = sqlx::query(&sql);
        for b in &binds {
            match b {
                Bind::Str(s) => query = query.bind(s),
                Bind::Int(i) => query = query.bind(i),
            }
        }

        let row = query.fetch_one(&self.pool).await?;
        row.try_get("count").map_err(DbError::from)
    }
}

// ---------------------------------------------------------------------------
// Database initialization
// ---------------------------------------------------------------------------

pub async fn init_db(database_url: &str) -> Result<SqlitePool> {
    // Create the database file if it doesn't exist
    if !Sqlite::database_exists(database_url).await.unwrap_or(false) {
        Sqlite::create_database(database_url).await?;
    }

    let pool = SqlitePoolOptions::new()
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                sqlx::query("PRAGMA foreign_keys=ON")
                    .execute(&mut *conn)
                    .await?;
                sqlx::query("PRAGMA journal_mode=WAL")
                    .execute(&mut *conn)
                    .await?;
                Ok(())
            })
        })
        .max_connections(5)
        .connect(database_url)
        .await?;

    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

// ---------------------------------------------------------------------------
// Row mapping helpers
// ---------------------------------------------------------------------------

fn row_to_workstream(row: &SqliteRow) -> Result<Workstream> {
    let id_str: String = row.try_get("id")?;
    let id =
        Uuid::parse_str(&id_str).map_err(|e| DbError::InvalidData(format!("invalid uuid: {e}")))?;

    let status_str: String = row.try_get("status")?;
    let status = parse_workstream_status(&status_str)?;

    let metadata_str: String = row.try_get("metadata")?;
    let metadata: serde_json::Value = serde_json::from_str(&metadata_str)
        .unwrap_or(serde_json::Value::Object(Default::default()));

    let created_at_str: String = row.try_get("created_at")?;
    let created_at = parse_datetime(&created_at_str)?;

    let updated_at_str: String = row.try_get("updated_at")?;
    let updated_at = parse_datetime(&updated_at_str)?;

    Ok(Workstream {
        id,
        name: row.try_get("name")?,
        status,
        metadata,
        created_at,
        updated_at,
    })
}

fn row_to_task(row: &SqliteRow) -> Result<Task> {
    let id_str: String = row.try_get("id")?;
    let id =
        Uuid::parse_str(&id_str).map_err(|e| DbError::InvalidData(format!("invalid uuid: {e}")))?;

    let ws_id_str: String = row.try_get("workstream_id")?;
    let workstream_id = Uuid::parse_str(&ws_id_str)
        .map_err(|e| DbError::InvalidData(format!("invalid uuid: {e}")))?;

    let status_str: String = row.try_get("status")?;
    let status = parse_task_status(&status_str)?;

    let metadata_str: String = row.try_get("metadata")?;
    let metadata: serde_json::Value = serde_json::from_str(&metadata_str)
        .unwrap_or(serde_json::Value::Object(Default::default()));

    let created_at_str: String = row.try_get("created_at")?;
    let created_at = parse_datetime(&created_at_str)?;

    let updated_at_str: String = row.try_get("updated_at")?;
    let updated_at = parse_datetime(&updated_at_str)?;

    let summary_updated_at_str: Option<String> = row.try_get("summary_updated_at")?;
    let summary_updated_at = summary_updated_at_str
        .as_deref()
        .map(parse_datetime)
        .transpose()?;

    Ok(Task {
        id,
        workstream_id,
        name: row.try_get("name")?,
        status,
        summary_text: row.try_get("summary_text")?,
        summary_updated_at,
        summary_source: row.try_get("summary_source")?,
        metadata,
        created_at,
        updated_at,
    })
}

fn row_to_update(row: &SqliteRow) -> Result<Update> {
    let seq: i64 = row.try_get("seq")?;

    let id_str: String = row.try_get("id")?;
    let id =
        Uuid::parse_str(&id_str).map_err(|e| DbError::InvalidData(format!("invalid uuid: {e}")))?;

    let task_id_str: String = row.try_get("task_id")?;
    let task_id = Uuid::parse_str(&task_id_str)
        .map_err(|e| DbError::InvalidData(format!("invalid uuid: {e}")))?;

    let timestamp_str: String = row.try_get("timestamp")?;
    let timestamp = parse_datetime(&timestamp_str)?;

    let level_str: Option<String> = row.try_get("level")?;
    let level = level_str.as_deref().map(parse_update_level).transpose()?;

    let tags_str: String = row.try_get("tags")?;
    let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();

    let data_str: Option<String> = row.try_get("data")?;
    let data = data_str
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .unwrap_or(None);

    Ok(Update {
        seq,
        id,
        task_id,
        source: row.try_get("source")?,
        timestamp,
        message: row.try_get("message")?,
        kind: row.try_get("kind")?,
        level,
        tags,
        data,
    })
}

// ---------------------------------------------------------------------------
// Enum parsing helpers
// ---------------------------------------------------------------------------

fn parse_workstream_status(s: &str) -> Result<WorkstreamStatus> {
    match s {
        "active" => Ok(WorkstreamStatus::Active),
        "archived" => Ok(WorkstreamStatus::Archived),
        _ => Err(DbError::InvalidData(format!(
            "unknown workstream status: {s}"
        ))),
    }
}

fn status_to_str(s: &WorkstreamStatus) -> &'static str {
    match s {
        WorkstreamStatus::Active => "active",
        WorkstreamStatus::Archived => "archived",
    }
}

fn parse_task_status(s: &str) -> Result<TaskStatus> {
    match s {
        "active" => Ok(TaskStatus::Active),
        "blocked" => Ok(TaskStatus::Blocked),
        "done" => Ok(TaskStatus::Done),
        "cancelled" => Ok(TaskStatus::Cancelled),
        _ => Err(DbError::InvalidData(format!("unknown task status: {s}"))),
    }
}

fn task_status_to_str(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Active => "active",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Done => "done",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn parse_update_level(s: &str) -> Result<UpdateLevel> {
    match s {
        "info" => Ok(UpdateLevel::Info),
        "warn" => Ok(UpdateLevel::Warn),
        "error" => Ok(UpdateLevel::Error),
        _ => Err(DbError::InvalidData(format!("unknown update level: {s}"))),
    }
}

fn update_level_to_str(l: &UpdateLevel) -> &'static str {
    match l {
        UpdateLevel::Info => "info",
        UpdateLevel::Warn => "warn",
        UpdateLevel::Error => "error",
    }
}

fn parse_datetime(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| DbError::InvalidData(format!("invalid datetime: {e}")))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_common::api::{
        CreateTaskRequest, CreateWorkstreamRequest, PatchTaskRequest, PatchWorkstreamRequest,
    };

    async fn setup_db() -> Db {
        let pool = init_db("sqlite::memory:").await.unwrap();
        Db::new(pool)
    }

    // -- Workstream tests --

    #[tokio::test]
    async fn test_create_and_list_workstreams() {
        let db = setup_db().await;

        let req = CreateWorkstreamRequest {
            name: "Test Workstream".to_string(),
            metadata: Some(serde_json::json!({"env": "dev"})),
        };
        let ws = db.create_workstream(&req).await.unwrap();
        assert_eq!(ws.name, "Test Workstream");
        assert_eq!(ws.status, WorkstreamStatus::Active);
        assert_eq!(ws.metadata["env"], "dev");

        let list = db.list_workstreams(false).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, ws.id);
    }

    #[tokio::test]
    async fn test_list_workstreams_excludes_archived() {
        let db = setup_db().await;

        let _ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "Active WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        db.create_workstream(&CreateWorkstreamRequest {
            name: "To Archive".to_string(),
            metadata: None,
        })
        .await
        .unwrap();

        // Archive the second one (we need its ID, so let's list all first)
        let all = db.list_workstreams(true).await.unwrap();
        let archived_ws = all.iter().find(|w| w.name == "To Archive").unwrap();

        db.update_workstream(
            archived_ws.id,
            &PatchWorkstreamRequest {
                name: None,
                status: Some(WorkstreamStatus::Archived),
                metadata: None,
            },
        )
        .await
        .unwrap();

        let active_only = db.list_workstreams(false).await.unwrap();
        assert_eq!(active_only.len(), 1);
        assert_eq!(active_only[0].name, "Active WS");

        let all = db.list_workstreams(true).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_update_workstream() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "Original".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let updated = db
            .update_workstream(
                ws.id,
                &PatchWorkstreamRequest {
                    name: Some("Renamed".to_string()),
                    status: None,
                    metadata: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.name, "Renamed");
        assert_eq!(updated.status, WorkstreamStatus::Active);
        assert!(updated.updated_at >= ws.updated_at);
    }

    #[tokio::test]
    async fn test_update_nonexistent_workstream() {
        let db = setup_db().await;

        let result = db
            .update_workstream(
                Uuid::new_v4(),
                &PatchWorkstreamRequest {
                    name: Some("Nope".to_string()),
                    status: None,
                    metadata: None,
                },
            )
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), DbError::NotFound(_)));
    }

    // -- Task tests --

    #[tokio::test]
    async fn test_create_and_list_tasks() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "My Task".to_string(),
                metadata: Some(serde_json::json!({"priority": 1})),
            })
            .await
            .unwrap();

        assert_eq!(task.name, "My Task");
        assert_eq!(task.status, TaskStatus::Active);
        assert_eq!(task.workstream_id, ws.id);

        let tasks = db.list_tasks(Some(ws.id), None).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, task.id);
    }

    #[tokio::test]
    async fn test_get_task() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let fetched = db.get_task(task.id).await.unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().name, "Task");

        let not_found = db.get_task(Uuid::new_v4()).await.unwrap();
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_list_tasks_filter_by_status() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let _t1 = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Active Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let t2 = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Done Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        db.update_task(
            t2.id,
            &PatchTaskRequest {
                name: None,
                workstream_id: None,
                status: Some(TaskStatus::Done),
                summary_text: None,
                summary_source: None,
                metadata: None,
            },
        )
        .await
        .unwrap();

        let active = db.list_tasks(None, Some(TaskStatus::Active)).await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].name, "Active Task");

        let done = db.list_tasks(None, Some(TaskStatus::Done)).await.unwrap();
        assert_eq!(done.len(), 1);
        assert_eq!(done[0].name, "Done Task");
    }

    #[tokio::test]
    async fn test_update_task() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let updated = db
            .update_task(
                task.id,
                &PatchTaskRequest {
                    name: Some("Updated Task".to_string()),
                    workstream_id: None,
                    status: Some(TaskStatus::Blocked),
                    summary_text: Some("Blocked by deps".to_string()),
                    summary_source: Some("agent".to_string()),
                    metadata: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.name, "Updated Task");
        assert_eq!(updated.status, TaskStatus::Blocked);
        assert_eq!(updated.summary_text, Some("Blocked by deps".to_string()));
        assert!(updated.summary_updated_at.is_some());
        assert_eq!(updated.summary_source, Some("agent".to_string()));
    }

    #[tokio::test]
    async fn test_create_task_fk_constraint() {
        let db = setup_db().await;

        let result = db
            .create_task(&CreateTaskRequest {
                workstream_id: Uuid::new_v4(), // non-existent workstream
                name: "Orphan Task".to_string(),
                metadata: None,
            })
            .await;

        assert!(result.is_err());
    }

    // -- Update tests --

    #[tokio::test]
    async fn test_insert_and_list_updates() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let u1 = db
            .insert_update(
                task.id,
                "claude:session-1",
                "Started work",
                Some("tool_use"),
                Some(UpdateLevel::Info),
                &["ci".to_string()],
                Some(&serde_json::json!({"tool": "bash"})),
            )
            .await
            .unwrap();

        let u2 = db
            .insert_update(task.id, "manual", "Progress note", None, None, &[], None)
            .await
            .unwrap();

        assert!(u2.seq > u1.seq, "seq should increase monotonically");

        let updates = db
            .list_updates(Some(task.id), None, None, &[], None, 100)
            .await
            .unwrap();
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].message, "Started work");
        assert_eq!(updates[1].message, "Progress note");
    }

    #[tokio::test]
    async fn test_seq_increases_monotonically() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let mut last_seq = 0i64;
        for i in 0..5 {
            let u = db
                .insert_update(
                    task.id,
                    "test",
                    &format!("Update {i}"),
                    None,
                    None,
                    &[],
                    None,
                )
                .await
                .unwrap();
            assert!(u.seq > last_seq);
            last_seq = u.seq;
        }
    }

    #[tokio::test]
    async fn test_list_updates_after_seq() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let u1 = db
            .insert_update(task.id, "test", "First", None, None, &[], None)
            .await
            .unwrap();

        let _u2 = db
            .insert_update(task.id, "test", "Second", None, None, &[], None)
            .await
            .unwrap();

        let _u3 = db
            .insert_update(task.id, "test", "Third", None, None, &[], None)
            .await
            .unwrap();

        let after_first = db
            .list_updates(None, None, None, &[], Some(u1.seq), 100)
            .await
            .unwrap();
        assert_eq!(after_first.len(), 2);
        assert_eq!(after_first[0].message, "Second");
        assert_eq!(after_first[1].message, "Third");
    }

    #[tokio::test]
    async fn test_list_updates_filter_by_source() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        db.insert_update(task.id, "claude", "From Claude", None, None, &[], None)
            .await
            .unwrap();

        db.insert_update(task.id, "github", "From GitHub", None, None, &[], None)
            .await
            .unwrap();

        let claude_updates = db
            .list_updates(None, Some("claude"), None, &[], None, 100)
            .await
            .unwrap();
        assert_eq!(claude_updates.len(), 1);
        assert_eq!(claude_updates[0].source, "claude");
    }

    #[tokio::test]
    async fn test_list_updates_filter_by_kind() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        db.insert_update(
            task.id,
            "test",
            "Tool use",
            Some("tool_use"),
            None,
            &[],
            None,
        )
        .await
        .unwrap();

        db.insert_update(task.id, "test", "Plain", None, None, &[], None)
            .await
            .unwrap();

        let tool_updates = db
            .list_updates(None, None, Some("tool_use"), &[], None, 100)
            .await
            .unwrap();
        assert_eq!(tool_updates.len(), 1);
        assert_eq!(tool_updates[0].kind, Some("tool_use".to_string()));
    }

    #[tokio::test]
    async fn test_list_updates_limit() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        for i in 0..5 {
            db.insert_update(
                task.id,
                "test",
                &format!("Update {i}"),
                None,
                None,
                &[],
                None,
            )
            .await
            .unwrap();
        }

        let limited = db
            .list_updates(None, None, None, &[], None, 2)
            .await
            .unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[tokio::test]
    async fn test_list_updates_filter_by_tags_match_any() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        db.insert_update(
            task.id,
            "test",
            "Claude event",
            None,
            None,
            &["claude".to_string()],
            None,
        )
        .await
        .unwrap();

        db.insert_update(
            task.id,
            "test",
            "GitHub event",
            None,
            None,
            &["github".to_string()],
            None,
        )
        .await
        .unwrap();

        let tags = vec!["manual".to_string(), "github".to_string()];
        let filtered = db
            .list_updates(None, None, None, &tags, None, 100)
            .await
            .unwrap();

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].message, "GitHub event");
        assert_eq!(
            db.count_updates(None, None, None, &tags, None)
                .await
                .unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn test_update_fk_constraint() {
        let db = setup_db().await;

        let result = db
            .insert_update(
                Uuid::new_v4(), // non-existent task
                "test",
                "Orphan update",
                None,
                None,
                &[],
                None,
            )
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_workstream_default_metadata() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "No Meta".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        assert_eq!(ws.metadata, serde_json::json!({}));
    }

    #[tokio::test]
    async fn test_update_with_tags_and_data() {
        let db = setup_db().await;

        let ws = db
            .create_workstream(&CreateWorkstreamRequest {
                name: "WS".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTaskRequest {
                workstream_id: ws.id,
                name: "Task".to_string(),
                metadata: None,
            })
            .await
            .unwrap();

        let data = serde_json::json!({"attempt": 3, "duration_ms": 1200});
        let u = db
            .insert_update(
                task.id,
                "ci",
                "Retry succeeded",
                Some("ci_result"),
                Some(UpdateLevel::Warn),
                &["ci".to_string(), "retry".to_string()],
                Some(&data),
            )
            .await
            .unwrap();

        assert_eq!(u.tags, vec!["ci", "retry"]);
        assert_eq!(u.data, Some(data));
        assert_eq!(u.level, Some(UpdateLevel::Warn));
        assert_eq!(u.kind, Some("ci_result".to_string()));
    }
}

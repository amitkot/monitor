use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Task, TaskStatus, Update, UpdateLevel, Workstream, WorkstreamStatus};

// ---------------------------------------------------------------------------
// Workstream requests
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkstreamRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchWorkstreamRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<WorkstreamStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Task requests
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub workstream_id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchTaskRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workstream_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Update requests
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualUpdateRequest {
    pub task_id: Uuid,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<UpdateLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Source-specific update requests (shapes defined now, logic in later milestones)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeHookUpdateRequest {
    pub task_id: Uuid,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubWebhookUpdateRequest {
    pub task_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<serde_json::Value>,
    pub payload: serde_json::Value,
}

// ---------------------------------------------------------------------------
// List / query responses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResponse<T> {
    pub items: Vec<T>,
    pub total: i64,
}

pub type WorkstreamListResponse = ListResponse<Workstream>;
pub type TaskListResponse = ListResponse<Task>;
pub type UpdateListResponse = ListResponse<Update>;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_workstream_request_round_trip() {
        let req = CreateWorkstreamRequest {
            name: "My Workstream".to_string(),
            metadata: Some(serde_json::json!({"priority": "high"})),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: CreateWorkstreamRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, req.name);
    }

    #[test]
    fn create_workstream_request_without_metadata() {
        let json = r#"{"name":"Minimal"}"#;
        let req: CreateWorkstreamRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "Minimal");
        assert!(req.metadata.is_none());
    }

    #[test]
    fn patch_workstream_request_partial() {
        let json = r#"{"status":"archived"}"#;
        let req: PatchWorkstreamRequest = serde_json::from_str(json).unwrap();
        assert!(req.name.is_none());
        assert_eq!(req.status, Some(WorkstreamStatus::Archived));
        assert!(req.metadata.is_none());
    }

    #[test]
    fn create_task_request_round_trip() {
        let req = CreateTaskRequest {
            workstream_id: Uuid::new_v4(),
            name: "Implement SSE".to_string(),
            metadata: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: CreateTaskRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, req.name);
        assert_eq!(deserialized.workstream_id, req.workstream_id);
    }

    #[test]
    fn patch_task_request_partial() {
        let json = r#"{"status":"blocked","summary_text":"Waiting on dependency"}"#;
        let req: PatchTaskRequest = serde_json::from_str(json).unwrap();
        assert!(req.name.is_none());
        assert!(req.workstream_id.is_none());
        assert_eq!(req.status, Some(TaskStatus::Blocked));
        assert_eq!(
            req.summary_text,
            Some("Waiting on dependency".to_string())
        );
        assert!(req.metadata.is_none());
    }

    #[test]
    fn manual_update_request_round_trip() {
        let req = ManualUpdateRequest {
            task_id: Uuid::new_v4(),
            message: "Progress update".to_string(),
            level: Some(UpdateLevel::Info),
            kind: Some("manual_note".to_string()),
            tags: vec!["frontend".to_string()],
            data: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: ManualUpdateRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.message, req.message);
        assert_eq!(deserialized.level, req.level);
        assert_eq!(deserialized.tags, req.tags);
    }

    #[test]
    fn manual_update_request_minimal() {
        let json = r#"{"task_id":"a1a2a3a4-b1b2-c1c2-d1d2-e1e2e3e4e5e6","message":"Hello"}"#;
        let req: ManualUpdateRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.message, "Hello");
        assert!(req.level.is_none());
        assert!(req.kind.is_none());
        assert!(req.tags.is_empty());
        assert!(req.data.is_none());
    }

    #[test]
    fn claude_hook_update_request_round_trip() {
        let req = ClaudeHookUpdateRequest {
            task_id: Uuid::new_v4(),
            payload: serde_json::json!({"event": "tool_use", "tool": "bash"}),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: ClaudeHookUpdateRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task_id, req.task_id);
        assert_eq!(deserialized.payload, req.payload);
    }

    #[test]
    fn github_webhook_update_request_round_trip() {
        let req = GithubWebhookUpdateRequest {
            task_id: Uuid::new_v4(),
            headers: Some(serde_json::json!({"x-github-event": "workflow_job"})),
            payload: serde_json::json!({"action": "completed"}),
        };
        let json = serde_json::to_string(&req).unwrap();
        let deserialized: GithubWebhookUpdateRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.task_id, req.task_id);
        assert!(deserialized.headers.is_some());
    }

    #[test]
    fn github_webhook_update_request_without_headers() {
        let json = r#"{"task_id":"a1a2a3a4-b1b2-c1c2-d1d2-e1e2e3e4e5e6","payload":{"action":"in_progress"}}"#;
        let req: GithubWebhookUpdateRequest = serde_json::from_str(json).unwrap();
        assert!(req.headers.is_none());
        assert_eq!(req.payload["action"], "in_progress");
    }
}

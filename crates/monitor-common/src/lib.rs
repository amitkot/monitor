pub mod api;

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Workstream
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkstreamStatus {
    Active,
    Archived,
}

impl fmt::Display for WorkstreamStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Archived => write!(f, "archived"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workstream {
    pub id: Uuid,
    pub name: String,
    pub status: WorkstreamStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Active,
    Blocked,
    Done,
    Cancelled,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Blocked => write!(f, "blocked"),
            Self::Done => write!(f, "done"),
            Self::Cancelled => write!(f, "cancelled"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub workstream_id: Uuid,
    pub name: String,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_updated_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_source: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateLevel {
    Info,
    Warn,
    Error,
}

impl fmt::Display for UpdateLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warn => write!(f, "warn"),
            Self::Error => write!(f, "error"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Update {
    pub seq: i64,
    pub id: Uuid,
    pub task_id: Uuid,
    pub source: String,
    pub timestamp: DateTime<Utc>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<UpdateLevel>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn workstream_round_trip() {
        let ws = Workstream {
            id: Uuid::new_v4(),
            name: "Test Workstream".to_string(),
            status: WorkstreamStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            metadata: serde_json::json!({"key": "value"}),
        };
        let json = serde_json::to_string(&ws).unwrap();
        let deserialized: Workstream = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, ws.name);
        assert_eq!(deserialized.status, ws.status);
    }

    #[test]
    fn workstream_status_serializes_as_lowercase() {
        let active = serde_json::to_string(&WorkstreamStatus::Active).unwrap();
        let archived = serde_json::to_string(&WorkstreamStatus::Archived).unwrap();
        assert_eq!(active, "\"active\"");
        assert_eq!(archived, "\"archived\"");
    }

    #[test]
    fn task_round_trip() {
        let task = Task {
            id: Uuid::new_v4(),
            workstream_id: Uuid::new_v4(),
            name: "Test Task".to_string(),
            status: TaskStatus::Active,
            summary_text: Some("A summary".to_string()),
            summary_updated_at: Some(Utc::now()),
            summary_source: Some("manual".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            metadata: serde_json::json!({}),
        };
        let json = serde_json::to_string(&task).unwrap();
        let deserialized: Task = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, task.name);
        assert_eq!(deserialized.status, task.status);
        assert_eq!(deserialized.summary_text, task.summary_text);
    }

    #[test]
    fn task_optional_fields_omitted_when_none() {
        let task = Task {
            id: Uuid::new_v4(),
            workstream_id: Uuid::new_v4(),
            name: "Minimal Task".to_string(),
            status: TaskStatus::Blocked,
            summary_text: None,
            summary_updated_at: None,
            summary_source: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            metadata: serde_json::json!({}),
        };
        let json = serde_json::to_string(&task).unwrap();
        assert!(!json.contains("summary_text"));
        assert!(!json.contains("summary_updated_at"));
        assert!(!json.contains("summary_source"));
    }

    #[test]
    fn task_status_serializes_as_lowercase() {
        assert_eq!(
            serde_json::to_string(&TaskStatus::Active).unwrap(),
            "\"active\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Blocked).unwrap(),
            "\"blocked\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Done).unwrap(),
            "\"done\""
        );
        assert_eq!(
            serde_json::to_string(&TaskStatus::Cancelled).unwrap(),
            "\"cancelled\""
        );
    }

    #[test]
    fn update_round_trip() {
        let update = Update {
            seq: 42,
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            source: "claude:session-abc".to_string(),
            timestamp: Utc::now(),
            message: "Implemented retry logic".to_string(),
            kind: Some("tool_use".to_string()),
            level: Some(UpdateLevel::Info),
            tags: vec!["ci".to_string(), "retry".to_string()],
            data: Some(serde_json::json!({"attempt": 3})),
        };
        let json = serde_json::to_string(&update).unwrap();
        let deserialized: Update = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.seq, update.seq);
        assert_eq!(deserialized.message, update.message);
        assert_eq!(deserialized.level, update.level);
        assert_eq!(deserialized.tags, update.tags);
    }

    #[test]
    fn update_optional_fields_omitted_when_none() {
        let update = Update {
            seq: 1,
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            source: "manual".to_string(),
            timestamp: Utc::now(),
            message: "Simple note".to_string(),
            kind: None,
            level: None,
            tags: vec![],
            data: None,
        };
        let json = serde_json::to_string(&update).unwrap();
        assert!(!json.contains("\"kind\""));
        assert!(!json.contains("\"level\""));
        assert!(!json.contains("\"data\""));
    }

    #[test]
    fn update_level_serializes_as_lowercase() {
        assert_eq!(
            serde_json::to_string(&UpdateLevel::Info).unwrap(),
            "\"info\""
        );
        assert_eq!(
            serde_json::to_string(&UpdateLevel::Warn).unwrap(),
            "\"warn\""
        );
        assert_eq!(
            serde_json::to_string(&UpdateLevel::Error).unwrap(),
            "\"error\""
        );
    }
}

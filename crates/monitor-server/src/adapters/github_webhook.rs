use monitor_common::UpdateLevel;
use uuid::Uuid;

/// Normalize a GitHub webhook payload into the standard update fields.
///
/// Returns `(source, message, kind, level, tags, data)`.
pub fn normalize_github_webhook(
    _task_id: Uuid,
    headers: Option<&serde_json::Value>,
    payload: &serde_json::Value,
) -> (
    String,
    String,
    Option<String>,
    Option<UpdateLevel>,
    Vec<String>,
    Option<serde_json::Value>,
) {
    // Source: github:<full_name>
    let full_name = payload["repository"]["full_name"]
        .as_str()
        .unwrap_or("unknown/unknown");
    let source = format!("github:{full_name}");

    // Extract the event type from headers
    let event_type = headers
        .and_then(|h| h["x-github-event"].as_str())
        .unwrap_or("unknown");

    let action = payload["action"].as_str().unwrap_or("unknown");

    let (message, kind, level) = match event_type {
        "workflow_run" => {
            let wf_name = payload["workflow_run"]["name"]
                .as_str()
                .unwrap_or("unknown");
            let message = format!("Workflow '{wf_name}' {action}");
            let level = determine_workflow_level(action, &payload["workflow_run"]);
            (message, Some("ci_run".to_string()), Some(level))
        }
        "workflow_job" => {
            let job_name = payload["workflow_job"]["name"]
                .as_str()
                .unwrap_or("unknown");
            let message = format!("Job '{job_name}' {action}");
            let level = determine_workflow_level(action, &payload["workflow_job"]);
            (message, Some("ci_job".to_string()), Some(level))
        }
        other => {
            let message = format!("GitHub event: {other}");
            (
                message,
                Some("external_update".to_string()),
                Some(UpdateLevel::Info),
            )
        }
    };

    // Tags: event type and action
    let mut tags = vec![event_type.to_string()];
    if action != "unknown" {
        tags.push(action.to_string());
    }

    // Curated data subset
    let event_object_key = match event_type {
        "workflow_run" => Some("workflow_run"),
        "workflow_job" => Some("workflow_job"),
        _ => None,
    };

    let mut data = serde_json::Map::new();
    data.insert("action".to_string(), serde_json::json!(action));
    if let Some(name) = payload["repository"]["full_name"].as_str() {
        data.insert(
            "repository_full_name".to_string(),
            serde_json::json!(name),
        );
    }
    if let Some(login) = payload["sender"]["login"].as_str() {
        data.insert("sender_login".to_string(), serde_json::json!(login));
    }
    if let Some(key) = event_object_key {
        if let Some(obj) = payload.get(key) {
            data.insert(key.to_string(), obj.clone());
        }
    }
    if let Some(delivery) = headers.and_then(|h| h["x-github-delivery"].as_str()) {
        data.insert("delivery_id".to_string(), serde_json::json!(delivery));
    }

    (
        source,
        message,
        kind,
        level,
        tags,
        Some(serde_json::Value::Object(data)),
    )
}

/// Determine the update level based on the action and conclusion fields.
fn determine_workflow_level(action: &str, event_obj: &serde_json::Value) -> UpdateLevel {
    if action == "completed" {
        let conclusion = event_obj["conclusion"].as_str().unwrap_or("");
        match conclusion {
            "success" => UpdateLevel::Info,
            "failure" => UpdateLevel::Error,
            _ => UpdateLevel::Info,
        }
    } else {
        UpdateLevel::Info
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn workflow_run_completed_success() {
        let task_id = Uuid::new_v4();
        let headers = json!({"x-github-event": "workflow_run"});
        let payload = json!({
            "action": "completed",
            "repository": {"full_name": "org/repo"},
            "sender": {"login": "octocat"},
            "workflow_run": {
                "name": "CI",
                "conclusion": "success",
                "status": "completed"
            }
        });

        let (source, message, kind, level, tags, data) =
            normalize_github_webhook(task_id, Some(&headers), &payload);

        assert_eq!(source, "github:org/repo");
        assert_eq!(message, "Workflow 'CI' completed");
        assert_eq!(kind.as_deref(), Some("ci_run"));
        assert_eq!(level, Some(UpdateLevel::Info));
        assert!(tags.contains(&"workflow_run".to_string()));
        assert!(tags.contains(&"completed".to_string()));

        let data = data.unwrap();
        assert_eq!(data["action"], "completed");
        assert_eq!(data["repository_full_name"], "org/repo");
        assert_eq!(data["sender_login"], "octocat");
        assert!(data.get("workflow_run").is_some());
    }

    #[test]
    fn workflow_run_completed_failure() {
        let task_id = Uuid::new_v4();
        let headers = json!({"x-github-event": "workflow_run"});
        let payload = json!({
            "action": "completed",
            "repository": {"full_name": "org/repo"},
            "sender": {"login": "octocat"},
            "workflow_run": {
                "name": "CI",
                "conclusion": "failure",
                "status": "completed"
            }
        });

        let (_source, message, _kind, level, _tags, _data) =
            normalize_github_webhook(task_id, Some(&headers), &payload);

        assert_eq!(message, "Workflow 'CI' completed");
        assert_eq!(level, Some(UpdateLevel::Error));
    }

    #[test]
    fn workflow_job_normalizes_correctly() {
        let task_id = Uuid::new_v4();
        let headers = json!({"x-github-event": "workflow_job"});
        let payload = json!({
            "action": "completed",
            "repository": {"full_name": "org/repo"},
            "sender": {"login": "octocat"},
            "workflow_job": {
                "name": "build",
                "conclusion": "success",
                "status": "completed"
            }
        });

        let (source, message, kind, level, tags, data) =
            normalize_github_webhook(task_id, Some(&headers), &payload);

        assert_eq!(source, "github:org/repo");
        assert_eq!(message, "Job 'build' completed");
        assert_eq!(kind.as_deref(), Some("ci_job"));
        assert_eq!(level, Some(UpdateLevel::Info));
        assert!(tags.contains(&"workflow_job".to_string()));
        assert!(tags.contains(&"completed".to_string()));

        let data = data.unwrap();
        assert!(data.get("workflow_job").is_some());
    }

    #[test]
    fn workflow_job_failure() {
        let task_id = Uuid::new_v4();
        let headers = json!({"x-github-event": "workflow_job"});
        let payload = json!({
            "action": "completed",
            "repository": {"full_name": "org/repo"},
            "sender": {"login": "octocat"},
            "workflow_job": {
                "name": "test",
                "conclusion": "failure",
                "status": "completed"
            }
        });

        let (_source, _message, _kind, level, _tags, _data) =
            normalize_github_webhook(task_id, Some(&headers), &payload);

        assert_eq!(level, Some(UpdateLevel::Error));
    }

    #[test]
    fn unknown_event_normalizes_to_external_update() {
        let task_id = Uuid::new_v4();
        let headers = json!({"x-github-event": "push"});
        let payload = json!({
            "action": "created",
            "repository": {"full_name": "org/repo"},
            "sender": {"login": "octocat"}
        });

        let (source, message, kind, level, tags, _data) =
            normalize_github_webhook(task_id, Some(&headers), &payload);

        assert_eq!(source, "github:org/repo");
        assert_eq!(message, "GitHub event: push");
        assert_eq!(kind.as_deref(), Some("external_update"));
        assert_eq!(level, Some(UpdateLevel::Info));
        assert!(tags.contains(&"push".to_string()));
    }

    #[test]
    fn missing_headers_defaults_to_unknown() {
        let task_id = Uuid::new_v4();
        let payload = json!({
            "action": "completed",
            "repository": {"full_name": "org/repo"}
        });

        let (_source, message, kind, _level, _tags, _data) =
            normalize_github_webhook(task_id, None, &payload);

        assert_eq!(message, "GitHub event: unknown");
        assert_eq!(kind.as_deref(), Some("external_update"));
    }

    #[test]
    fn in_progress_action_level_is_info() {
        let task_id = Uuid::new_v4();
        let headers = json!({"x-github-event": "workflow_run"});
        let payload = json!({
            "action": "in_progress",
            "repository": {"full_name": "org/repo"},
            "sender": {"login": "octocat"},
            "workflow_run": {
                "name": "Deploy",
                "status": "in_progress"
            }
        });

        let (_source, message, _kind, level, _tags, _data) =
            normalize_github_webhook(task_id, Some(&headers), &payload);

        assert_eq!(message, "Workflow 'Deploy' in_progress");
        assert_eq!(level, Some(UpdateLevel::Info));
    }
}

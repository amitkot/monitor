use monitor_common::UpdateLevel;
use uuid::Uuid;

/// Normalize a Claude Code hook payload into the standard update fields.
///
/// Returns `(source, message, kind, level, tags, data)`.
pub fn normalize_claude_hook(
    _task_id: Uuid,
    payload: &serde_json::Value,
) -> (
    String,
    String,
    Option<String>,
    Option<UpdateLevel>,
    Vec<String>,
    Option<serde_json::Value>,
) {
    // Source: claude:<session_id>
    let session_id = payload["session_id"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let source = format!("claude:{session_id}");

    let hook_event = payload["hook_event_name"].as_str().unwrap_or("");
    let tool_name = payload["tool_name"].as_str().unwrap_or("unknown");

    let (message, kind, level) = match hook_event {
        "PostToolUse" => (
            format!("Tool `{tool_name}` completed"),
            Some("tool_use".to_string()),
            Some(UpdateLevel::Info),
        ),
        "PostToolUseFailure" => {
            let error = payload["error"].as_str().unwrap_or("unknown error");
            (
                format!("Tool `{tool_name}` failed: {error}"),
                Some("tool_failure".to_string()),
                Some(UpdateLevel::Error),
            )
        }
        "PreToolUse" => (
            format!("Tool `{tool_name}` starting"),
            Some("tool_use".to_string()),
            Some(UpdateLevel::Info),
        ),
        other => (
            format!("Claude hook event: {other}"),
            Some("assistant_message".to_string()),
            Some(UpdateLevel::Info),
        ),
    };

    // Tags: include tool_name if present in the payload
    let mut tags = Vec::new();
    if payload.get("tool_name").is_some() {
        tags.push(tool_name.to_string());
    }

    // Store full payload as data
    let data = Some(payload.clone());

    (source, message, kind, level, tags, data)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn post_tool_use_normalizes_correctly() {
        let task_id = Uuid::new_v4();
        let payload = json!({
            "session_id": "abc123",
            "hook_event_name": "PostToolUse",
            "tool_name": "Write",
            "tool_input": {"file_path": "/tmp/test.txt", "content": "hello"},
            "tool_response": {"filePath": "/tmp/test.txt", "success": true},
            "tool_use_id": "toolu_01ABC123"
        });

        let (source, message, kind, level, tags, data) = normalize_claude_hook(task_id, &payload);

        assert_eq!(source, "claude:abc123");
        assert_eq!(message, "Tool `Write` completed");
        assert_eq!(kind.as_deref(), Some("tool_use"));
        assert_eq!(level, Some(UpdateLevel::Info));
        assert_eq!(tags, vec!["Write"]);
        assert!(data.is_some());
        assert_eq!(data.unwrap()["session_id"], "abc123");
    }

    #[test]
    fn post_tool_use_failure_normalizes_correctly() {
        let task_id = Uuid::new_v4();
        let payload = json!({
            "session_id": "abc123",
            "hook_event_name": "PostToolUseFailure",
            "tool_name": "Bash",
            "tool_input": {"command": "npm test"},
            "tool_use_id": "toolu_01ABC123",
            "error": "Command exited with non-zero status code 1",
            "is_interrupt": false
        });

        let (source, message, kind, level, tags, _data) = normalize_claude_hook(task_id, &payload);

        assert_eq!(source, "claude:abc123");
        assert_eq!(
            message,
            "Tool `Bash` failed: Command exited with non-zero status code 1"
        );
        assert_eq!(kind.as_deref(), Some("tool_failure"));
        assert_eq!(level, Some(UpdateLevel::Error));
        assert_eq!(tags, vec!["Bash"]);
    }

    #[test]
    fn pre_tool_use_normalizes_correctly() {
        let task_id = Uuid::new_v4();
        let payload = json!({
            "session_id": "sess-42",
            "hook_event_name": "PreToolUse",
            "tool_name": "Read",
            "tool_input": {"file_path": "/tmp/foo.txt"},
            "tool_use_id": "toolu_99"
        });

        let (source, message, kind, level, tags, _data) = normalize_claude_hook(task_id, &payload);

        assert_eq!(source, "claude:sess-42");
        assert_eq!(message, "Tool `Read` starting");
        assert_eq!(kind.as_deref(), Some("tool_use"));
        assert_eq!(level, Some(UpdateLevel::Info));
        assert_eq!(tags, vec!["Read"]);
    }

    #[test]
    fn unknown_event_normalizes_to_assistant_message() {
        let task_id = Uuid::new_v4();
        let payload = json!({
            "session_id": "sess-99",
            "hook_event_name": "SomeNewEvent"
        });

        let (source, message, kind, level, tags, _data) = normalize_claude_hook(task_id, &payload);

        assert_eq!(source, "claude:sess-99");
        assert_eq!(message, "Claude hook event: SomeNewEvent");
        assert_eq!(kind.as_deref(), Some("assistant_message"));
        assert_eq!(level, Some(UpdateLevel::Info));
        assert!(tags.is_empty());
    }

    #[test]
    fn missing_session_id_defaults_to_unknown() {
        let task_id = Uuid::new_v4();
        let payload = json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash"
        });

        let (source, _message, _kind, _level, _tags, _data) =
            normalize_claude_hook(task_id, &payload);

        assert_eq!(source, "claude:unknown");
    }
}

# Source Event Shapes

This note captures reusable facts about upstream event payloads that the monitor system may ingest and normalize.

## Claude Code Hooks

Claude Code hooks provide structured JSON input.

Common fields include:

- `session_id`
- `transcript_path`
- `cwd`
- `permission_mode`
- `hook_event_name`

Event-specific fields depend on the hook type.

### `PreToolUse`

`PreToolUse` fires before a tool executes and includes:

- `tool_name`
- `tool_input`
- `tool_use_id`

Example shape:

```json
{
  "tool_name": "Bash",
  "tool_input": {
    "command": "rm -rf /tmp/build"
  }
}
```

### `PostToolUse`

`PostToolUse` fires after a successful tool execution and includes:

- `tool_name`
- `tool_input`
- `tool_response`
- `tool_use_id`

Example shape:

```json
{
  "session_id": "abc123",
  "transcript_path": "/Users/.../.claude/projects/.../00893aaf-19fa-41d2-8238-13269b9b3ca0.jsonl",
  "cwd": "/Users/...",
  "permission_mode": "default",
  "hook_event_name": "PostToolUse",
  "tool_name": "Write",
  "tool_input": {
    "file_path": "/path/to/file.txt",
    "content": "file content"
  },
  "tool_response": {
    "filePath": "/path/to/file.txt",
    "success": true
  },
  "tool_use_id": "toolu_01ABC123"
}
```

### `PostToolUseFailure`

`PostToolUseFailure` fires after a failed tool execution and includes:

- `tool_name`
- `tool_input`
- `tool_use_id`
- `error`
- `is_interrupt`

Example shape:

```json
{
  "session_id": "abc123",
  "transcript_path": "/Users/.../.claude/projects/.../00893aaf-19fa-41d2-8238-13269b9b3ca0.jsonl",
  "cwd": "/Users/...",
  "permission_mode": "default",
  "hook_event_name": "PostToolUseFailure",
  "tool_name": "Bash",
  "tool_input": {
    "command": "npm test",
    "description": "Run test suite"
  },
  "tool_use_id": "toolu_01ABC123",
  "error": "Command exited with non-zero status code 1",
  "is_interrupt": false
}
```

### Notes

- Claude hooks can send structured JSON to the monitor system without lossy parsing.
- `tool_input` and `tool_response` schemas vary by tool.
- A normalized monitor update should preserve raw hook data in an optional structured field rather than forcing the UI to understand every upstream schema.

Source:

- https://code.claude.com/docs/en/hooks

## GitHub Workflow Webhooks

GitHub webhooks deliver JSON over HTTP POST.

Useful headers include:

- `X-GitHub-Event`
- `X-GitHub-Delivery`

For GitHub Actions, the most relevant webhook events are usually:

- `workflow_run`
- `workflow_job`

Typical payloads include:

- `action`
- `repository`
- `sender`
- `installation` when using a GitHub App
- event-specific objects such as `workflow_run` or `workflow_job`

### `workflow_run`

This event represents activity on a workflow run.

The payload includes:

- `action`
- `workflow_run`
- `repository`
- `sender`

Examples of action types include completed activity on a run.

### `workflow_job`

This event represents activity on an individual job inside a workflow run.

It is typically more useful than `workflow_run` when the monitor should display step-by-step execution progress for CI work.

### Notes

- GitHub webhook bodies are already structured JSON and can be stored in a normalized update's optional `data` field.
- The monitor should extract a stable human-readable message from the webhook while preserving the raw webhook body for deeper inspection.

Source:

- https://docs.github.com/en/webhooks/webhook-events-and-payloads
- https://docs.github.com/en/actions/writing-workflows/choosing-when-your-workflow-runs/events-that-trigger-workflows

## Codex Note

I did not find an official documented Codex hook/event payload format comparable to Claude Code hooks. There is at least an open feature request for event hooks in the public `openai/codex` repository, so the monitor design should not assume Codex-native hooks exist yet.

This is an inference from currently available public information, not an official product guarantee.

## Normalized Update Kind Guidance

The monitor's internal `Update.kind` field should remain an optional freeform string in v1, but producers and server-side adapters should prefer a small, documented vocabulary for consistency.

Expected v1 values include:

- `manual_note`
- `assistant_message`
- `tool_use`
- `tool_failure`
- `file_change`
- `test_result`
- `build_result`
- `ci_run`
- `ci_job`
- `deployment`
- `error`
- `status_note`
- `decision`
- `external_update`
- `plan_update`

Usage guidance:

- `kind` is for lightweight categorization, filtering, and UI hints.
- `kind` is not authoritative workflow state.
- `kind` may be omitted when a producer cannot classify an event cleanly.
- New values may be added over time without a schema migration.

Suggested mapping examples:

- Claude hook `PostToolUse` for `Bash` -> `tool_use`
- Claude hook `PostToolUseFailure` -> `tool_failure`
- Claude hook affecting files -> `file_change`
- GitHub `workflow_run` -> `ci_run`
- GitHub `workflow_job` -> `ci_job`
- UI-entered note -> `manual_note`
- Human decision or conclusion from the UI -> `decision`

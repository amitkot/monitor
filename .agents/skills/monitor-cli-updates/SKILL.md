---
name: monitor-cli-updates
description: "Use when an agent needs to record development work in the Monitor app through the monitor-cli: create or find workstreams, create or update tasks, send factual progress updates, record blockers/tests/decisions, or post manual updates for external meat-space events."
---

# Monitor CLI Updates

Use the Monitor CLI to keep development work visible while you work. Prefer small, factual updates tied to a task over long end-of-session summaries.

## CLI Setup

Use the standalone CLI binary, not `cargo run`, for normal monitor updates:

```bash
monitor-cli <command>
```

If `monitor-cli` is not available, install it from the repository once:

```bash
cargo install --path crates/monitor-cli
```

If the environment is offline but dependencies are already cached, add `--offline`. Ensure Cargo's bin directory, usually `~/.cargo/bin`, is on `PATH`.

For non-local servers, set:

```bash
export MONITOR_SERVER=http://127.0.0.1:3000
export MONITOR_TOKEN=<bearer-token>
```

You can also pass connection options per command:

```bash
monitor-cli --server http://127.0.0.1:3000 --token <bearer-token> task list
```

The CLI prints JSON. Use the returned `id` values for later commands. If `jq` is available, capture IDs with `jq -r .id`; otherwise read the JSON output directly.

## Output Modes

Use the default output mode when you need returned IDs or diagnostics.

Use `--quiet`, or its alias `--silent`, when output would be noisy but the caller should still see a non-zero exit status on failure:

```bash
monitor-cli --quiet task list
```

For hooks and background integrations where monitor delivery must never block the calling tool, combine `--quiet` with shell-level error suppression:

```bash
monitor-cli --quiet update manual \
  --task <TASK_ID> \
  --message "Claude hook observed a session event." \
  --kind claude_hook \
  --tags claude,hook \
  || true
```

Prefer `|| true` over a pipe to `true`; it preserves the intended command structure and avoids pipe-related surprises.

## Help Discovery

Use `--help` whenever command shape, option names, or allowed values are uncertain. Start broad, then drill into the exact subcommand:

```bash
monitor-cli --help
monitor-cli workstream --help
monitor-cli workstream create --help
monitor-cli task --help
monitor-cli task create --help
monitor-cli task update --help
monitor-cli update --help
monitor-cli update manual --help
```

Do not guess flags if a command fails. Run the relevant `--help`, correct the command, and retry.

## Workflow

1. Find or create a workstream for the larger effort.
2. Find or create a task for the specific unit of work.
3. Send updates as meaningful events happen.
4. Update task status when state changes.

Do not create a new workstream for every task. A workstream is a collection of related tasks. A task is the unit that receives updates.

## Workstreams

List active workstreams:

```bash
monitor-cli workstream list
```

Include archived workstreams when trying to avoid duplicates:

```bash
monitor-cli workstream list --include-archived
```

Create a workstream:

```bash
monitor-cli workstream create "Monitor UI polish"
```

Attach small JSON metadata when it helps later filtering or context:

```bash
monitor-cli workstream create "Monitor UI polish" \
  --metadata '{"area":"ui","repo":"monitor"}'
```

Archive or reactivate a workstream:

```bash
monitor-cli workstream update <WORKSTREAM_ID> --status archived
monitor-cli workstream update <WORKSTREAM_ID> --status active
```

Allowed workstream statuses are `active` and `archived`.

## Tasks

List tasks, optionally filtered:

```bash
monitor-cli task list
monitor-cli task list --workstream <WORKSTREAM_ID>
monitor-cli task list --status active
```

Create a task in a workstream:

```bash
monitor-cli task create "Make stream rows expandable" \
  --workstream <WORKSTREAM_ID>
```

Attach metadata for stable machine-readable context:

```bash
monitor-cli task create "Make stream rows expandable" \
  --workstream <WORKSTREAM_ID> \
  --metadata '{"area":"stream","surface":"web-ui"}'
```

Update task status:

```bash
monitor-cli task update <TASK_ID> --status blocked
monitor-cli task update <TASK_ID> --status done
```

Allowed task statuses are `active`, `blocked`, `done`, and `cancelled`.

Update the task summary only when there is a useful current-status summary to maintain:

```bash
monitor-cli task update <TASK_ID> \
  --summary "Rows are now clickable; verification is pending." \
  --summary-source agent
```

Use `summary-source` values such as `agent`, `ui`, `human`, or a specific tool/session name. Keep summaries short and current; do not duplicate the full update history.

## Updates

Send updates with `update manual`. Every update must attach to a task and include a clear message:

```bash
monitor-cli update manual \
  --task <TASK_ID> \
  --message "Replaced the visible Details disclosure with row-click expansion."
```

Add optional `level`, `kind`, `tags`, and structured `data` when they add value:

```bash
monitor-cli update manual \
  --task <TASK_ID> \
  --message "cargo check passed for monitor-server." \
  --level info \
  --kind test_passed \
  --tags agent,verification \
  --data '{"command":"cargo check -p monitor-server --quiet"}'
```

Allowed levels are `info`, `warn`, and `error`.

Use `kind` as a freeform category. Prefer stable snake_case values, for example:

- `progress`
- `decision`
- `blocker`
- `test_passed`
- `test_failed`
- `implementation_note`
- `review_note`
- `manual_note`
- `external_event`

Use tags as comma-separated short labels for filtering, for example `agent`, `ui`, `backend`, `verification`, `manual`, `github`, `claude`, `codex`, or a feature area.

Use `data` for compact JSON details that should remain machine-readable, such as command names, file paths, URLs, commit hashes, CI run IDs, or error snippets. Keep large logs out of `data`; summarize them in `message` and link or reference the artifact.

## Update Quality

Send an update when one of these happens:

- You start work on a task.
- You make a meaningful implementation change.
- You hit or clear a blocker.
- You make a decision that affects future work.
- A test, check, review, or deployment succeeds or fails.
- Something happens outside the codebase that affects the task.

Avoid noisy updates for every command. Do not claim a task is done until the requested verification has run or you explicitly state what was not verified.

Prefer messages that answer: what changed, what matters, and what is next.

Good:

```text
Implemented row-click expansion for stream updates; task links still navigate normally. Running cargo check next.
```

Bad:

```text
Working on it.
```

## Practical Examples

Create a workstream, create a task, and send a first update:

```bash
WORKSTREAM_ID=$(monitor-cli workstream create "Monitor UI polish" | jq -r .id)
TASK_ID=$(monitor-cli task create "Make stream rows expandable" --workstream "$WORKSTREAM_ID" | jq -r .id)
monitor-cli update manual --task "$TASK_ID" --kind progress --tags agent,ui --message "Starting stream row interaction change."
```

Record a blocker:

```bash
monitor-cli task update <TASK_ID> --status blocked
monitor-cli update manual \
  --task <TASK_ID> \
  --level warn \
  --kind blocker \
  --tags agent,blocked \
  --message "Blocked because the server endpoint is not reachable at MONITOR_SERVER."
```

Record completion:

```bash
monitor-cli update manual \
  --task <TASK_ID> \
  --level info \
  --kind test_passed \
  --tags agent,verification \
  --message "cargo check passed after the stream row interaction change."
monitor-cli task update <TASK_ID> --status done
```

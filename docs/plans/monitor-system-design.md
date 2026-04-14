# Monitor: Real-time Development Task Monitoring System

## Context

Build a local-first monitoring system that aggregates progress updates from multiple sources such as Claude Code sessions, GitHub Actions, and manual UI input into a task-centric dashboard. The system must be consumable by both humans (web UI) and AI agents (REST API + SSE subscriptions). It starts local and should be deployable to an internet-accessible server later.

## Architecture Overview

```text
┌─────────────┐  HTTP POST   ┌──────────────────┐  SSE   ┌──────────────┐
│ Sender CLI  │─────────────▶│                  │───────▶│   Web UI     │
│ (monitor)   │              │  Monitor Server  │        │ (HTMX/JS +   │
└─────────────┘              │  (monitor-server)│        │   Askama)     │
                             │                  │        └──────────────┘
┌─────────────┐  HTTP POST   │  ┌────────────┐  │  REST
│ GitHub      │─────────────▶│  │  SQLite    │  │◀──────▶ Agents / CLI
│ Webhooks    │              │  └────────────┘  │
└─────────────┘              │                  │
┌─────────────┐  HTTP POST   │ Source-specific  │
│ Manual UI   │─────────────▶│ normalization    │
│ Updates     │              │ adapters         │
└─────────────┘              └──────────────────┘
```

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Core organization model | Workstreams contain Tasks | Workstreams group related development tasks without implying workflow columns. |
| Task activity model | Append-only task Updates | Each task has a live activity thread of updates from hooks, CI, or humans. |
| Workstream status | `Active`, `Archived` | Workstream lifecycle controls visibility, not execution state. |
| Task status | `Active`, `Blocked`, `Done`, `Cancelled` | Explicit task state without overcommitting to a larger workflow engine. |
| Summary model | Optional mutable best-effort summary | UI, agents, or system jobs can update it; it is never authoritative. |
| Transport (ingest) | HTTP POST | Simple, works for CLI, UI, and GitHub webhooks. |
| Transport (push to clients) | SSE + replay via sequence number | Live updates plus durable catch-up for reconnecting clients. |
| Persistence | SQLite (via sqlx) | Embedded, zero-config, file-based. |
| Update payload model | Message-first + optional structured data | Guarantees renderable updates while preserving upstream JSON. |
| Update source | Freeform string | Keeps schema simple while allowing stable adapter-defined provenance. |
| Normalization boundary | Server-side source adapters | Keeps CLI/hooks lightweight and centralizes source-specific logic. |
| Web UI | Askama + HTMX with small JS where needed | Mostly server-rendered, but flexible enough for authenticated/browser streaming later. |
| Agent interface | REST API + SSE | Agents can read current state, write updates, and subscribe to changes. |
| Auth primitive | Bearer token | Good fit for API clients and deployable later. |
| Auth policy | Config-driven local vs strict remote | Local mode can relax reads; remote mode requires bearer auth for reads and writes. |
| Framework | Axum + Tokio | Good Rust async web stack with first-class SSE support. |
| Workspace | Multi-crate | `monitor-common`, `monitor-server`, `monitor-cli`. |

## Workspace Structure

```text
monitor/
├── Cargo.toml
├── ai_docs/
│   └── source-event-shapes.md
├── crates/
│   ├── monitor-common/
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── monitor-server/
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── main.rs
│   │   │   ├── config.rs
│   │   │   ├── db.rs
│   │   │   ├── services.rs      # core application services
│   │   │   ├── adapters/        # source-specific normalization
│   │   │   ├── api/
│   │   │   │   ├── mod.rs
│   │   │   │   ├── workstreams.rs
│   │   │   │   ├── tasks.rs
│   │   │   │   ├── updates.rs
│   │   │   │   └── sse.rs
│   │   │   └── web/
│   │   │       ├── mod.rs
│   │   │       └── templates/
│   │   └── migrations/
│   └── monitor-cli/
│       ├── Cargo.toml
│       └── src/main.rs
└── docs/
    └── plans/
```

## Domain Model

### Workstream

A workstream is a collection of related development tasks.

```rust
enum WorkstreamStatus {
    Active,
    Archived,
}

struct Workstream {
    id: Uuid,
    name: String,
    status: WorkstreamStatus,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    metadata: serde_json::Value,
}
```

Notes:

- Workstreams are grouping containers, not workflow columns.
- Archiving a workstream is allowed even if some tasks remain active.
- Archived workstreams are hidden by default but retained.

### Task

A task is the actionable unit of work. Updates belong to tasks.

```rust
enum TaskStatus {
    Active,
    Blocked,
    Done,
    Cancelled,
}

struct Task {
    id: Uuid,
    workstream_id: Uuid,
    name: String,
    status: TaskStatus,
    summary_text: Option<String>,
    summary_updated_at: Option<DateTime<Utc>>,
    summary_source: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    metadata: serde_json::Value,
}
```

Notes:

- `workstream_id` is mutable; tasks may move between workstreams.
- Task status is authoritative current workflow state.
- The summary is optional, mutable, and best-effort.
- Summary may be written by the UI, agents, or system jobs.
- Task status transitions are freely patchable in v1.

### Update

An update is an append-only activity item attached to a task.

```rust
enum UpdateLevel {
    Info,
    Warn,
    Error,
}

struct Update {
    seq: i64,
    id: Uuid,
    task_id: Uuid,
    source: String,
    timestamp: DateTime<Utc>,
    message: String,
    kind: Option<String>,
    level: Option<UpdateLevel>,
    tags: Vec<String>,
    data: Option<serde_json::Value>,
}
```

Notes:

- `seq` is a monotonic durable sequence used for replay and SSE resume.
- `message` is required and is the canonical display string.
- `kind` is an optional freeform category such as `tool_use`, `ci_job`, or `manual_note`.
- `data` may hold the raw or near-raw source payload.
- `source` is a freeform string such as `ui`, `claude:session-abc`, or `github:owner/repo`.

## Source Adapters and Normalization

The server is the normalization boundary.

Each source-specific ingest path:

1. validates the incoming payload shape
2. normalizes it into the internal `Update` model
3. persists it durably
4. publishes it to SSE subscribers

This keeps the sender CLI and hooks lightweight while centralizing formatting and source-specific logic.

Planned server-side adapters:

- `manual`
- `claude_hook`
- `github_webhook`

Reference payload shapes and `kind` guidance are documented in `ai_docs/source-event-shapes.md`.

## API Authentication

Bearer token is the API authentication primitive.

Policy is configuration-driven:

- local relaxed mode:
  - read endpoints may be open
  - write endpoints require bearer token
- strict mode:
  - read and write endpoints require bearer token

Remote deployment should use strict mode.

Browser session auth is explicitly out of scope for v1. If remote browser UI is added later, it can layer a session/login flow on top of the same server without changing the core API model.

## REST API

### State Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/workstreams` | Create a workstream |
| GET | `/api/workstreams` | List workstreams |
| PATCH | `/api/workstreams/:id` | Update workstream name, status, metadata |
| POST | `/api/tasks` | Create a task |
| GET | `/api/tasks` | List tasks |
| PATCH | `/api/tasks/:id` | Update task name, status, summary, metadata, or move workstream |
| GET | `/api/updates` | Query stored updates |
| GET | `/api/stream` | SSE stream of new updates |

### Ingest Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/updates/manual` | Manual UI or API-submitted update |
| POST | `/api/updates/claude-hook` | Claude Code hook payload |
| POST | `/api/updates/github-webhook` | GitHub webhook payload |

Optional later:

- `POST /api/updates/normalized`

### Ingest Request Shapes

`POST /api/updates/manual` minimal shape:

```json
{
  "task_id": "uuid",
  "message": "Vendor confirmed replacement will ship tomorrow",
  "level": "info",
  "kind": "manual_note",
  "tags": ["manual", "external"],
  "data": {
    "notes": "Optional structured details"
  }
}
```

Rules:

- required: `task_id`, `message`
- optional: `level`, `kind`, `tags`, `data`
- server sets `source` for this endpoint, for example `ui` or `manual`

`POST /api/updates/claude-hook` shape:

```json
{
  "task_id": "uuid",
  "payload": { "raw": "claude hook json" }
}
```

Rules:

- required: `task_id`, `payload`
- server normalizes `payload` into `message`, `kind`, `level`, `source`, and `data`

`POST /api/updates/github-webhook` shape:

```json
{
  "task_id": "uuid",
  "headers": {
    "x-github-event": "workflow_job",
    "x-github-delivery": "..."
  },
  "payload": { "raw": "github webhook json" }
}
```

Rules:

- required: `task_id`, `payload`
- optional: selected headers needed for normalization or auditing
- server normalizes the webhook into `message`, `kind`, `level`, `source`, and `data`

### Query Semantics

`GET /api/tasks` supports filters such as:

- `workstream_id`
- `status`
- `include_archived_workstreams`

`GET /api/updates` supports filters such as:

- `task_id`
- `source`
- `kind`
- `tags`
- `after_seq`
- `since`
- `until`
- pagination limits

## SSE and Catch-up Model

SSE is for new updates. It is not the only source of truth.

Clients should use this startup pattern:

1. fetch current state with `GET /api/workstreams` and `GET /api/tasks`
2. optionally fetch recent history with `GET /api/updates`
3. connect to `GET /api/stream` for new updates

Reconnect/catch-up options:

- resume by last seen `seq` using `GET /api/updates?after_seq=...`
- or refetch the current snapshot if that is simpler for the client

SSE events should emit the durable `seq` as the SSE event id.

Resume semantics:

- clients should send `Last-Event-ID` when reconnecting
- the server should treat `Last-Event-ID` as the preferred resume cursor
- if `Last-Event-ID` is not present, the server may also accept `after_seq` as a query parameter
- if the requested resume point is too old or unavailable, the server may require the client to refetch current state and then reconnect

## Web UI

The UI is task-centric rather than kanban-column-centric.

Default view:

- active workstreams
- each workstream contains tasks
- each task shows:
  - name
  - explicit task status
  - optional summary
  - lightweight task metadata only, not the full update thread

Task detail view:

- shows the full update thread in reverse chronological order
- includes manual update entry
- includes summary editing

Supported UI actions:

- create workstream
- archive/unarchive workstream
- create task
- move task between workstreams
- change task status
- edit task summary
- add manual update to a task
- filter updates by source, kind, tags, or status

## Sender CLI

`monitor-cli` is a lightweight sender and query tool. It should not be the primary normalization boundary for source-specific event schemas.

Initial CLI responsibilities:

- send manual updates
- create and update workstreams/tasks
- query state
- forward source-specific payloads when useful

Example commands:

```text
monitor update manual --task <task-id> --message "Implemented retry logic"
monitor workstream create "Monitor MVP"
monitor task create --workstream <id> "SSE replay support"
monitor task update <task-id> --status blocked
monitor task update <task-id> --summary "Waiting on confirmation from CI maintainers"
```

## Application Service Layer

The core abstraction should be application services, not a generic ingest trait.

Examples:

- `create_workstream`
- `update_workstream`
- `create_task`
- `update_task`
- `ingest_manual_update`
- `ingest_claude_hook`
- `ingest_github_webhook`

This keeps HTTP routes, future background sources, and normalization logic separate and composable.

If a future background source such as NATS is added, it should feed these services rather than force HTTP and background subscriptions into the same abstraction.

## Implementation Phases

### Phase 1: Foundation

1. Set up Cargo workspace with 3 crates
2. Define `Workstream`, `Task`, `Update`, request/response types in `monitor-common`
3. Set up SQLite schema and migrations in `monitor-server`
4. Implement basic Axum server with health endpoint
5. Implement workstream/task CRUD and `GET /api/updates`

### Phase 2: Ingest and Streaming

6. Implement durable update sequence numbers
7. Implement `POST /api/updates/manual`
8. Implement SSE endpoint and reconnect/catch-up flow
9. Add bearer auth policy middleware

### Phase 3: Source Adapters

10. Implement server-side Claude hook normalization
11. Implement GitHub webhook normalization
12. Add `kind` and filtering support

### Phase 4: Web UI

13. Set up Askama templates with base layout
14. Build workstream/task dashboard
15. Add manual updates and summary editing
16. Wire live updates via SSE

### Phase 5: CLI and Polish

17. Implement `monitor-cli` commands for manual updates and state changes
18. Add pagination and bulk query improvements
19. Improve logging, tracing, error handling, graceful shutdown
20. Add configuration file support

## Verification

- `cargo run -p monitor-server` starts on localhost:3000
- `POST /api/workstreams` creates a workstream
- `POST /api/tasks` creates a task inside a workstream
- `POST /api/updates/manual` appends a task update
- `GET /api/tasks` returns current task state
- `GET /api/updates?task_id=...` returns update history
- `GET /api/stream` emits new updates with durable sequence ids
- `cargo run -p monitor-cli -- update manual --task <id> --message "test"` sends a manual update

## Future Considerations

- remote deployment with strict auth and optional browser login/session layer
- additional task states such as `Waiting`
- richer source adapters
- NATS or another background source feeding the service layer
- optional MCP server for tighter agent integration

## Raw Payload Retention

`Update.data` should retain enough structured source data to support debugging and future reprocessing, but v1 does not need to preserve every upstream payload byte indefinitely.

Recommendation:

- manual updates: store full `data` as provided
- Claude hook updates: store the full hook payload in `data`
- GitHub webhook updates: store a near-raw curated subset plus relevant delivery metadata instead of blindly persisting every field

Why:

- GitHub payloads can be substantially larger than manual or Claude updates
- a curated subset keeps the database smaller while preserving the fields most likely to matter for display, debugging, and normalization improvements
- the normalized top-level `message`, `kind`, `level`, and `source` remain the primary read path

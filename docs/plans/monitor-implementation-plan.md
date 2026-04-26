# Monitor Implementation Plan

## Goal

Implement the monitoring system described in `docs/plans/monitor-system-design.md` as a local-first Rust workspace with:

- durable SQLite-backed state
- REST APIs for workstreams, tasks, and updates
- source-specific ingest endpoints
- SSE for live updates
- a server-rendered web UI
- a lightweight CLI

This plan is ordered to get the core state model and manual-update workflow working first, then layer live delivery, source adapters, and UI.

## Milestone 1: Workspace and Core Types

### Deliverables

- Cargo workspace with:
  - `crates/monitor-common`
  - `crates/monitor-server`
  - `crates/monitor-cli`
- shared Rust types in `monitor-common`
- server and CLI binaries that compile and start

### Tasks

1. Create workspace `Cargo.toml` and crate manifests.
2. Add shared dependencies to the workspace where appropriate:
   - `serde`
   - `serde_json`
   - `uuid`
   - `chrono`
   - `thiserror`
3. Define core domain types in `monitor-common`:
   - `WorkstreamStatus`
   - `TaskStatus`
   - `UpdateLevel`
   - `Workstream`
   - `Task`
   - `Update`
4. Define request/response types in `monitor-common`:
   - `CreateWorkstreamRequest`
   - `PatchWorkstreamRequest`
   - `CreateTaskRequest`
   - `PatchTaskRequest`
   - `ManualUpdateRequest`
   - `ClaudeHookUpdateRequest`
   - `GithubWebhookUpdateRequest`
   - list/query response shapes
5. Add serde defaults and validation-friendly field shapes.

### Notes

- Keep request/response types distinct from DB row structs where useful.
- Keep `Update.kind` optional and freeform.
- Keep `Update.data` as `Option<serde_json::Value>`.

### Verification

- `cargo check` succeeds for the workspace
- `monitor-common` types serialize and deserialize cleanly in unit tests

## Milestone 2: SQLite Schema and Data Access

### Deliverables

- initial SQL migrations
- `sqlx`-based DB layer in `monitor-server`
- durable `seq` for updates

### Schema

Create tables:

1. `workstreams`
   - `id`
   - `name`
   - `status`
   - `metadata`
   - `created_at`
   - `updated_at`

2. `tasks`
   - `id`
   - `workstream_id`
   - `name`
   - `status`
   - `summary_text`
   - `summary_updated_at`
   - `summary_source`
   - `metadata`
   - `created_at`
   - `updated_at`

3. `updates`
   - `seq INTEGER PRIMARY KEY AUTOINCREMENT`
   - `id`
   - `task_id`
   - `source`
   - `timestamp`
   - `message`
   - `kind`
   - `level`
   - `tags`
   - `data`

### Tasks

1. Add migrations under `crates/monitor-server/migrations/`.
2. Decide SQLite encoding details:
   - UUID as text
   - enum values as text
   - tags as JSON text
   - metadata/data as JSON text
3. Implement repository/query helpers in `db.rs` for:
   - create/list/update workstreams
   - create/list/update tasks
   - insert update
   - list updates with filters
4. Add indexes:
   - `tasks(workstream_id)`
   - `tasks(status)`
   - `updates(task_id, seq)`
   - `updates(source, seq)`
   - optional `updates(kind, seq)`

### Verification

- migrations apply on a fresh DB
- insert/select round-trips work for all three tables
- `updates.seq` increases monotonically

## Milestone 3: Application Service Layer

### Deliverables

- service layer that owns business operations
- HTTP handlers thinly delegate to services

### Tasks

1. Implement service methods in `services.rs`:
   - `create_workstream`
   - `update_workstream`
   - `list_workstreams`
   - `create_task`
   - `update_task`
   - `list_tasks`
   - `ingest_manual_update`
   - `list_updates`
2. Ensure service methods enforce only v1 rules:
   - enum validation
   - task must reference an existing workstream
   - updates must reference an existing task
   - no task/workstream transition graph enforcement beyond enum membership
3. When inserting updates:
   - persist first
   - return the stored row including `seq`
   - publish to live subscribers after commit

### Verification

- service-layer tests pass without HTTP
- invalid foreign-key references fail cleanly

## Milestone 4: Core REST API

### Deliverables

- Axum routes for workstreams, tasks, and manual updates
- JSON request/response contracts implemented

### Tasks

1. Add routes:
   - `POST /api/workstreams`
   - `GET /api/workstreams`
   - `PATCH /api/workstreams/:id`
   - `POST /api/tasks`
   - `GET /api/tasks`
   - `PATCH /api/tasks/:id`
   - `GET /api/updates`
   - `POST /api/updates/manual`
2. Implement filtering on:
   - tasks by `workstream_id`, `status`
   - updates by `task_id`, `source`, `kind`, repeated `tag` query params, `after_seq`, time bounds
3. Define pagination and filter semantics:
   - default page size: `50`
   - maximum page size: `200`
   - repeated `?tag=a&tag=b` means match-any across the supplied tags
4. Standardize error responses:
   - validation errors
   - not found
   - auth failures
   - internal errors
5. Add health endpoint:
   - `GET /health`

### Verification

- basic CRUD works via `curl`
- manual updates can be inserted and queried
- patching task summary and status works

## Milestone 5: Live Streaming

### Deliverables

- SSE endpoint
- catch-up semantics documented and implemented

### Tasks

1. Add in-process fanout using `tokio::sync::broadcast` or equivalent.
2. Implement `GET /api/stream`.
3. Emit SSE event ids from `Update.seq`.
4. Support reconnect behavior:
   - honor `Last-Event-ID`
   - optionally support `after_seq` query param
5. Define event payload shape sent over SSE:
   - likely the normalized `Update` JSON
6. Decide behavior when resume cursor cannot be served:
   - return an error that instructs clients to refetch snapshot
   - or start live-only and rely on client catch-up via `GET /api/updates`
7. Keep SSE update-only in v1:
   - SSE emits normalized `Update` events only
   - task/workstream mutations are not independently broadcast unless they also create an update
   - UI should refetch affected partials after its own task/workstream mutations

### Verification

- two concurrent clients receive the same new updates
- reconnect after a dropped connection resumes from the last seen `seq`

## Milestone 6: Authentication and Config

### Deliverables

- config file/env loading
- bearer token auth policy middleware

### Tasks

1. Define config model in `config.rs`:
   - bind address
   - database path
   - auth mode
   - API tokens
2. Implement auth policy:
   - relaxed local mode: reads may be open, writes require token
   - strict mode: reads and writes require token
3. Apply auth consistently to:
   - state endpoints
   - ingest endpoints
   - SSE endpoint according to read policy
4. Ensure UI routes remain separable from API policy if needed later.

### Verification

- unauthorized writes fail in relaxed mode
- unauthorized reads fail in strict mode
- authorized SSE connections work

## Milestone 7: Web UI

### Deliverables

- server-rendered dashboard
- task detail page
- live update refresh

### Tasks

1. Add Askama templates and page routes:
   - `/`
   - `/workstreams/:id`
   - `/tasks/:id`
2. Dashboard page:
   - active workstreams
   - tasks with status and optional summary
3. Task detail page:
   - full update thread in reverse chronological order
   - manual update form
   - summary edit UI
   - status change UI
4. Use HTMX for:
   - create/update forms
   - partial refreshes after the initiating client's own mutations
5. Add minimal JS for:
   - SSE connection
   - targeting task or thread partial refresh after new updates

### Verification

- dashboard renders active workstreams and tasks
- task detail shows history newest-first
- posting a manual update refreshes the task thread
- live updates appear without full page reload

## Milestone 8: Source Adapters

### Deliverables

- Claude hook ingest
- GitHub webhook ingest

### Tasks

1. Implement adapter modules under `src/adapters/`:
   - `manual.rs`
   - `claude_hook.rs`
   - `github_webhook.rs`
2. Claude hook adapter:
   - accept raw hook payload
   - require `task_id` routing from the caller rather than inferring it from the hook payload
   - document the expected routing source for local hooks, for example an env var supplied by the hook script
   - derive `message`, `kind`, `level`, `source`
   - store raw payload in `data`
3. GitHub webhook adapter:
   - accept relevant headers plus raw payload
   - derive `message`, `kind`, `level`, `source`
   - store curated near-raw subset plus delivery metadata in `data`
4. Add routes:
   - `POST /api/updates/claude-hook`
   - `POST /api/updates/github-webhook`

### Verification

- sample Claude payload normalizes as expected
- Claude hook ingest succeeds when `task_id` is supplied by the caller
- sample GitHub `workflow_job` and `workflow_run` payloads normalize as expected

## Milestone 9: CLI

### Deliverables

- lightweight CLI for manual updates and state changes

### Tasks

1. Implement commands:
   - `monitor workstream create`
   - `monitor workstream update`
   - `monitor task create`
   - `monitor task update`
   - `monitor update manual`
2. Add server URL and token configuration:
   - flags
   - env vars
3. Keep CLI normalization minimal:
   - manual updates may be sent already normalized
   - source-specific events should mostly be forwarded to server-specific endpoints

### Verification

- CLI can create workstreams/tasks
- CLI can add manual updates
- CLI works against auth-protected server

## Milestone 10: Hardening and Fit-and-Finish

### Deliverables

- polished error handling
- logs and tracing
- stable verification path

### Tasks

1. Add structured tracing.
2. Improve error messages and response bodies.
3. Add pagination defaults and limits for update queries and enforce:
   - default `50`
   - max `200`
4. Add retention/size guardrails for stored GitHub payload subsets.
5. Add graceful shutdown handling.
6. Add smoke tests for core flows.

### Verification

- happy-path smoke test covers:
  - create workstream
  - create task
  - send manual update
  - query state
  - receive SSE event

## Suggested Build Order

Implement in this exact order:

1. `monitor-common` types
2. SQLite schema and DB helpers
3. service layer
4. workstream/task/manual update API
5. SSE + `seq`
6. auth/config
7. web UI
8. CLI
9. Claude adapter
10. GitHub adapter

This order gets the core product working before source-specific integrations.

## Out of Scope for This Plan

- browser session auth
- multi-user identity model
- NATS integration
- MCP server integration
- advanced task workflow rules
- full-text search or analytics

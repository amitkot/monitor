# Source-Agnostic Monitor Ingestion

## Summary

Monitor should not need a new server-side adapter for every source that can report
progress. The server should be a stable sink for normalized task updates, while
source-specific integrations translate upstream events into Monitor's update model
before sending them.

Codex session monitoring should be the first integration built this way. Codex-specific
parsing and task mapping can live in `monitor-cli`, leaving the server responsible for
validation, persistence, SSE fan-out, and rendering.

## Design Direction

Add a generic update ingestion API:

```text
POST /api/updates
```

The request should accept the normalized update fields that Monitor already stores:

- `task_id`
- `source`
- `message`
- optional `kind`
- optional `level`
- optional `tags`
- optional `data`

The `source` field is supplied by the trusted producer and remains freeform. Examples:

- `codex:<session_id>`
- `claude:<session_id>`
- `github:<owner>/<repo>`
- `manual`
- `slack`

The existing `/api/updates/manual` endpoint can remain as compatibility sugar that
sets `source = "manual"`. Source-specific server endpoints can still exist when a
source must post directly to Monitor, but they should be optional adapters rather than
the default integration pattern.

## Codex Integration

Add a Codex integration to `monitor-cli` instead of teaching the server Codex event
semantics.

Proposed command:

```bash
monitor-cli codex ingest-hook
```

Behavior:

- read Codex hook JSON from stdin
- derive or reuse one Monitor task per Codex `session_id`
- create or reuse a workstream for the repository or working directory
- normalize Codex events into Monitor updates
- post updates through the generic `POST /api/updates` endpoint
- preserve raw or near-raw Codex payloads in `data`
- stay quiet and non-blocking when used from hooks

No LLM is needed for the first version. The integration should produce deterministic
messages from structured event fields.

Suggested normalization:

| Codex event | Monitor kind | Notes |
|-------------|--------------|-------|
| session start | `session_start` | Include cwd, model, and session id in `data`. |
| user prompt | `user_prompt` | Use a compact prompt excerpt as the message. |
| assistant output | `assistant_message` | Use a compact response/status excerpt when available. |
| tool start/completion | `tool_use` | Include tool name in tags. |
| tool failure | `tool_failure` | Use `level = error`. |
| permission request | `permission_request` | Use `level = warn` when user action is needed. |
| turn complete | `turn_summary` | Record deterministic turn completion metadata. |

The task metadata should store stable linkage information:

- `source: "codex"`
- `session_id`
- `cwd`
- optional transcript or session file path
- optional model and originator fields

## Future Sources

New integrations should usually be implemented as producers that call the generic
API after normalization. This keeps the Monitor server small and avoids changing
server code every time a new source appears.

Use a server-side adapter only when the source cannot reasonably run `monitor-cli` or
another local producer. GitHub webhooks are a good example: GitHub posts directly to
Monitor, so a server adapter can still be useful there.

## Implementation Notes

- Add a common request type for normalized update ingestion, likely alongside the
  existing API request structs.
- The generic endpoint should validate task existence exactly like manual updates do.
- Auth behavior should match existing write endpoints.
- SSE event shape does not need to change because stored `Update` values are unchanged.
- UI changes are not required for v1, though filters by `source`, `kind`, and `tags`
  become more useful as more integrations arrive.

## Test Plan

- Server tests for generic update ingestion, validation, auth, storage, list filtering,
  and SSE broadcast.
- CLI tests for Codex hook parsing and deterministic normalization.
- Integration test that multiple Codex events with the same `session_id` reuse the same
  Monitor task.
- Workspace verification with:

```bash
cargo test --workspace
```

## Assumptions

- `monitor-cli` is the right default home for source-specific integrations that can run
  local commands.
- Monitor's existing `Update` data model is sufficient for v1.
- Codex progress should be tool-level initially, not only turn-level.
- LLM-generated summaries can be added later as a separate summarization feature.

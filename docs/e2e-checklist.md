# Manual E2E Checklist

Run this outside the sandbox on your machine.

## 1. Start the server

Relaxed local mode:

```bash
cargo run -p monitor-server
```

Strict mode:

```bash
export MONITOR_AUTH_MODE=strict
export MONITOR_API_TOKENS=dev-secret-token
cargo run -p monitor-server
```

## 2. Create a workstream

```bash
monitor-cli workstream create "E2E Workstream"
```

Save the returned `id`.

## 3. Create a task

```bash
monitor-cli task create "E2E Task" --workstream <WORKSTREAM_ID>
```

Save the returned `id`.

## 4. Open the UI

Open:

```text
http://127.0.0.1:3000/dashboard
```

Verify:

- the workstream appears
- the task appears under it
- task detail loads at `/tasks/<TASK_ID>`

Open the global stream:

```text
http://127.0.0.1:3000/stream
```

Verify:

- updates across all tasks appear newest-first
- each update links back to its task

## 5. Open an SSE stream

In another terminal:

```bash
curl -N http://127.0.0.1:3000/api/stream?task_id=<TASK_ID>
```

Or, in strict mode:

```bash
curl -N \
  -H "Authorization: Bearer dev-secret-token" \
  "http://127.0.0.1:3000/api/stream?task_id=<TASK_ID>"
```

## 6. Send a manual update

```bash
monitor-cli update manual \
  --task <TASK_ID> \
  --message "Manual E2E update" \
  --kind manual_note \
  --level info \
  --tags e2e,manual
```

Verify:

- the SSE terminal receives an `update` event
- the task detail page shows the new update at the top

## 7. Update task state

```bash
monitor-cli task update <TASK_ID> --status blocked
monitor-cli task update <TASK_ID> --summary "Blocked during e2e verification"
```

Verify:

- task detail reflects the new status and summary
- dashboard reflects the updated summary/status after refresh or UI-initiated partial update

## 8. Check replay/catch-up

Send two more updates:

```bash
monitor-cli update manual --task <TASK_ID> --message "Update one"
monitor-cli update manual --task <TASK_ID> --message "Update two"
```

Then query history:

```bash
curl "http://127.0.0.1:3000/api/updates?task_id=<TASK_ID>&limit=50"
```

Verify:

- updates are returned with increasing `seq`
- `total` reflects all matches, not just the page size

## 9. Check tag filtering

```bash
curl "http://127.0.0.1:3000/api/updates?tag=e2e&tag=manual"
```

Verify:

- results include updates matching either tag

## 10. Check auth behavior

Strict mode only:

- without token:
  - `GET /api/workstreams` should return `401`
  - `POST /api/updates/manual` should return `401`
- with token:
  - both should succeed

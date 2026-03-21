# Web API Contract (Local UI)

Version: v1 (prefix: `/api/v1`)
Scope: local, single-user, no auth.

## Conventions

- All responses are JSON unless noted.
- Errors use a shared envelope (see below).
- Coordinator data structures align with engine/service types:
  - `core/src/service/coordinator_workflow.rs` (`CoordinatorStatus`, `CoordinatorCommandResult`)
  - `core/src/service/diagnostic.rs` (`FailureReport`)
  - `docs/schemas/coordinator-event.v1.schema.json` (event payload)

## Error Envelope

All non-2xx responses return:

```json
{
  "error": {
    "code": "MACC-WEB-0000",
    "category": "Internal",
    "message": "Human-readable message",
    "context": {
      "field": "value"
    },
    "cause": "optional root cause summary"
  }
}
```

Fields:
- `code` (string, required): MACC error code.
- `category` (string, required): `Validation | Auth | Dependency | Conflict | NotFound | Internal`.
- `message` (string, required).
- `context` (object, optional): structured details.
- `cause` (string, optional).

## Endpoints

### GET `/api/v1/health`

Purpose: liveness probe.

Response 200:
```json
{ "status": "ok" }
```

### GET `/api/v1/status`

Purpose: coordinator status + latest failure diagnostics.

Response 200 (shape mirrors `CoordinatorStatus`):
```json
{
  "total": 10,
  "todo": 3,
  "active": 2,
  "blocked": 1,
  "merged": 4,
  "paused": false,
  "pause_reason": null,
  "pause_task_id": null,
  "pause_phase": null,
  "latest_error": null,
  "failure_report": {
    "message": "Coordinator paused due to a blocking error.",
    "task_id": "WEB-BACKEND-001",
    "phase": "review",
    "source": "event",
    "blocking": true,
    "event_type": "task_blocked",
    "kind": "InternalError",
    "suggested_fixes": ["Run macc coordinator unlock --all"]
  }
}
```

Notes:
- `failure_report` may be `null` when no diagnostic is available.
- `latest_error` mirrors the most recent coordinator error string when present.

### POST `/api/v1/coordinator/{action}`

Purpose: trigger coordinator actions.

Path parameter `action` (string, required):
- `run`
- `stop`
- `resume`
- `dispatch`
- `advance`
- `reconcile`
- `cleanup`

Request body: none.

Response 200 (shape mirrors `CoordinatorCommandResult`):
```json
{
  "status": {
    "total": 10,
    "todo": 3,
    "active": 2,
    "blocked": 1,
    "merged": 4,
    "paused": false,
    "pause_reason": null,
    "pause_task_id": null,
    "pause_phase": null,
    "latest_error": null,
    "failure_report": null
  },
  "resumed": true,
  "aggregated_performer_logs": 2,
  "runtime_status": "running",
  "exported_events_path": ".macc/log/coordinator/events.jsonl",
  "removed_worktrees": 0,
  "selected_task": {
    "task_id": "WEB-BACKEND-001",
    "phase": "review"
  }
}
```

Notes:
- Fields are optional and may be `null` depending on the action.

### GET `/api/v1/registry/tasks`

Purpose: list coordinator registry tasks with operator-facing metadata.

Response 200:
```json
[
  {
    "id": "WEB2-BE-REG-001",
    "title": "Implement registry task list and action endpoints",
    "priority": "P1",
    "state": "blocked",
    "tool": "codex",
    "attempts": 2,
    "heartbeat": "2026-03-20T12:00:00Z",
    "delayedUntil": null,
    "currentPhase": "review",
    "lastError": "performer failed",
    "lastErrorCode": "E901",
    "assignee": null,
    "worktree": null,
    "events": [],
    "updatedAt": "2026-03-20T12:05:00Z"
  }
]
```

Notes:
- `events` contains coordinator event excerpts already present in the storage snapshot.
- `attempts` reflects the task runtime attempt counter when present.

### POST `/api/v1/registry/tasks/{id}/{action}`

Purpose: apply operator actions to a single registry task.

Path parameters:
- `id` (string, required): registry task ID.
- `action` (string, required): `requeue | reassign`.

Request body for `requeue`:
```json
{
  "kind": "requeue",
  "justification": "optional operator note"
}
```

Request body for `reassign`:
```json
{
  "kind": "reassign",
  "tool": "gemini",
  "justification": "optional operator note"
}
```

Response 200:
- Updated `ApiRegistryTask` payload for the mutated task.

Notes:
- `requeue` resets blocked/failed tasks back to `todo`.
- `reassign` updates the task's assigned tool and rejects active or merged tasks.
- Invalid actions use the standard error envelope instead of falling through to SPA routing.

### GET `/api/v1/events` (SSE)

Purpose: stream coordinator events.

Headers:
- `Accept: text/event-stream`

Response 200:
- `Content-Type: text/event-stream`

SSE envelope:
- `event`: logical event name (e.g., `coordinator_event`, `heartbeat`).
- `id`: event id (use coordinator `event_id` when available, otherwise a monotonic cursor).
- `data`: JSON payload.

Payload for `coordinator_event`:
- Coordinator event record following `docs/schemas/coordinator-event.v1.schema.json`.

Heartbeat:
- Emit `event: heartbeat` at a regular cadence.
- `data` should be a coordinator event record with `type: "heartbeat"` and a current `ts`.

Example (wire format):
```
id: 7f9c1c
event: coordinator_event
data: {"schema_version":"1","event_id":"7f9c1c","seq":42,"ts":"2024-10-11T12:01:02Z","source":"coordinator","type":"task_transition","status":"ok"}

event: heartbeat
data: {"schema_version":"1","event_id":"hb-42","seq":43,"ts":"2024-10-11T12:01:07Z","source":"coordinator","type":"heartbeat","status":"ok"}

```

### GET `/api/v1/logs`

Purpose: list browsable coordinator and performer log files under `.macc/log/`.

Response 200:
```json
[
  {
    "path": "coordinator/events.jsonl",
    "category": "coordinator",
    "size": 2048,
    "modified": "2026-03-20T12:00:00Z"
  },
  {
    "path": "performer/worker-01--TASK-001-.md",
    "category": "performer",
    "size": 1024,
    "modified": "2026-03-20T12:05:00Z"
  }
]
```

Notes:
- Only files under `.macc/log/coordinator` and `.macc/log/performer` are listed.

### GET `/api/v1/logs/{path}`

Purpose: read a log file under `.macc/log/` with optional pagination and line filtering.

Path parameter:
- `path` (string, required): URL-encoded relative path such as `coordinator/events.jsonl`.

Query parameters:
- `offset` (number, optional): zero-based filtered line offset. Default `0`.
- `limit` (number, optional): maximum filtered lines returned. Default `200`, capped server-side.
- `search` (string, optional): substring filter applied before pagination.

Response 200:
```json
{
  "path": "coordinator/events.jsonl",
  "lines": [
    "{\"type\":\"task_started\"}",
    "{\"type\":\"task_finished\"}"
  ],
  "total": 12,
  "hasMore": true
}
```

Notes:
- The server rejects absolute paths and traversal attempts such as `..`.
- `total` reflects the number of lines after `search` filtering when `search` is provided.

### GET `/api/v1/logs/tail` (SSE)

Purpose: tail a log file under `.macc/log/` and stream newly appended lines.

Headers:
- `Accept: text/event-stream`
- `Last-Event-ID` (optional): resume from a previous SSE event id.

Query parameters:
- `path` (string, required): URL-encoded relative path such as `coordinator/events.jsonl`.

Response 200:
- `Content-Type: text/event-stream`

SSE envelope:
- `event: log_line` for each newly completed line appended to the target file.
- `id`: byte-offset cursor in the form `off-<offset>`.
- `data`: JSON payload with `path`, `timestamp`, and `content`.

Heartbeat:
- Emit `event: heartbeat` every 15 seconds while the stream is open.
- Heartbeat `id` values use the form `hb-<offset>-<timestamp_ms>`.
- Heartbeat `data` includes `path`, `timestamp`, `offset`, `type: "heartbeat"`, and `status: "ok"`.

Reconnect and rotation:
- `Last-Event-ID` accepts both `off-<offset>` and heartbeat ids.
- On reconnect the server resumes from the provided byte offset, capped to the current file size.
- If the file is truncated or replaced, the cursor resets to the beginning of the current file and streaming continues.

Concurrency:
- A single client may hold up to 25 concurrent log-tail streams.

Example (wire format):
```
id: off-128
event: log_line
data: {"path":"coordinator/events.jsonl","timestamp":"2026-03-20T12:01:02Z","content":"worker started"}

id: hb-128-1760000000000
event: heartbeat
data: {"path":"coordinator/events.jsonl","timestamp":"2026-03-20T12:01:17Z","offset":128,"type":"heartbeat","status":"ok"}
```

### POST `/api/v1/worktrees/{id}/run`

Purpose: trigger a performer run for a managed worktree without blocking the HTTP request.

Path parameter:
- `id` (string, required): worktree id such as `feature-web-01`.

Response 202:
```json
{
  "status": "started",
  "worktreeId": "feature-web-01",
  "path": "/repo/.macc/worktree/feature-web-01"
}
```

Notes:
- The endpoint rejects unknown worktrees with the standard error envelope.
- If the same worktree is already running through this endpoint, the server returns a worktree conflict error.

### GET `/api/v1/worktrees/{id}/logs` (SSE)

Purpose: stream performer log lines for a single worktree.

Headers:
- `Accept: text/event-stream`
- `Last-Event-ID` (optional): resume after the last delivered line number or heartbeat cursor.

Response 200:
- `Content-Type: text/event-stream`

SSE envelope:
- `event: log_line` for performer output lines.
- `id`: 1-based line number.
- `data`: JSON payload with `worktree_id`, `line`, `timestamp`, `level`, `message`.

Heartbeat:
- Emit `event: heartbeat` every 15 seconds.
- `id` uses the format `hb-{line}-{timestamp_ms}` so reconnects can resume from the most recent delivered line.

Example (wire format):
```
id: 1
event: log_line
data: {"worktree_id":"feature-web-01","line":1,"timestamp":"2026-03-20T12:00:00Z","level":"info","message":"boot"}

id: hb-1-1760000000000
event: heartbeat
data: {"event_id":"hb-1-1760000000000","type":"heartbeat","status":"ok","line":1,"timestamp":"2026-03-20T12:00:15Z"}
```

Notes:
- The stream tails the newest performer log available for the worktree.
- When a log line starts with RFC3339 timestamp + log level, the server normalizes those fields into the JSON payload and sends the remaining text as `message`.

### GET `/api/v1/coordinator/tool-cooldown`

Purpose: list all active tool cooldowns.

Response 200 (shape mirrors `ApiCoordinatorCommandResult`):
```json
{
  "tool_cooldowns": [
    {
      "tool_id": "gemini",
      "throttled_until": 1711000000,
      "remaining_seconds": 3600,
      "backoff_seconds": 60
    }
  ]
}
```

### POST `/api/v1/coordinator/tool-cooldown`

Purpose: manually set a tool cooldown.

Request body:
```json
{
  "tool": "gemini",
  "duration_seconds": 3600
}
```

Response 200 (shape mirrors `ApiCoordinatorCommandResult`):
```json
{
  "tool_cooldowns": [...]
}
```

### DELETE `/api/v1/coordinator/tool-cooldown/{tool}`

Purpose: manually clear a tool cooldown.

Path parameter:
- `tool` (string, required): tool ID to clear.

Response 200 (shape mirrors `ApiCoordinatorCommandResult`):
```json
{
  "tool_cooldowns": [...]
}
```

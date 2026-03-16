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

# Web API Contract (Local UI)

Version: v1 (prefix: `/api/v1`)
Scope: local, single-user, no auth.

## Conventions

- All responses are JSON unless noted.
- Errors use a shared envelope.
- `EventSource` endpoints use `text/event-stream`.
- `GET /api/v1/prd` and `PUT /api/v1/prd` accept an optional `?path=` query string for worktree-specific PRDs.
- The sections below cover the current local web surface end to end: config, PRD, plan, apply, worktrees, registry, logs, doctor, backups, terminal, and supporting coordinator endpoints.

### Error Envelope

All non-2xx responses return:

```json
{
  "error": {
    "code": "MACC-WEB-0000",
    "category": "Internal",
    "message": "Human-readable message",
    "retryable": false,
    "recommended_action": "optional operator hint",
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
- `retryable` (boolean, required): whether the client should retry automatically.
- `recommended_action` (string, optional): operator hint.
- `context` (object, optional): structured details.
- `cause` (string, optional).

## Core State

### GET `/api/v1/health`

Purpose: liveness probe.

Response 200:
```json
{ "status": "ok" }
```

### GET `/api/v1/status`

Purpose: coordinator status plus latest failure diagnostics.

Response 200:
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
  },
  "throttled_tools": [
    {
      "tool_id": "gemini",
      "throttled_until": "2026-03-20T12:00:00Z",
      "consecutive_count": 2
    }
  ],
  "effective_max_parallel": 4
}
```

### GET `/api/v1/git/graph`

Purpose: repository graph view for the Git page.

Query parameters:
- `limit` (number, optional)
- `since` (string, optional SHA cursor)

Response 200:
```json
{
  "commits": [
    {
      "sha": "abc123",
      "shortSha": "abc123",
      "subject": "feat: WEB2-DOCS-001 - Update web UI documentation",
      "author": "Brand",
      "timestamp": 1711000000,
      "parentShas": ["def456"],
      "branchRefs": ["main"],
      "taskId": "WEB2-DOCS-001"
    }
  ],
  "branches": ["main"],
  "head": "abc123"
}
```

## Coordinator

### POST `/api/v1/coordinator/{action}`

Purpose: trigger coordinator actions.

Path parameter `action`:
- `run`
- `stop`
- `resume`
- `dispatch`
- `advance`
- `reconcile`
- `cleanup`
- `sync`

> **Removed**: `audit-prd` has been removed from this endpoint. Use `POST /api/v1/prd/audit` instead.

Request body: none.

Response 200:
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
    "id": "WEB-BACKEND-001",
    "title": "Implement registry task list and action endpoints",
    "tool": "codex",
    "base_branch": "main"
  }
}
```

Notes:
- Fields are optional and may be `null` depending on the action.

### GET `/api/v1/coordinator/tool-cooldown`

Purpose: list active tool cooldowns.

Response 200:
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

Purpose: set a tool cooldown manually.

Request body:
```json
{
  "tool": "gemini",
  "duration_seconds": 3600
}
```

Response 200:
```json
{ "tool_cooldowns": [] }
```

### DELETE `/api/v1/coordinator/tool-cooldown/{tool}`

Purpose: clear a tool cooldown manually.

Response 200:
```json
{ "tool_cooldowns": [] }
```

### GET `/api/v1/events` (SSE)

Purpose: stream coordinator events.

Headers:
- `Accept: text/event-stream`

SSE envelope:
- `event`: `coordinator_event` or `heartbeat`
- `id`: coordinator `event_id` when available
- `data`: JSON payload matching the coordinator event schema

Example:
```text
id: 7f9c1c
event: coordinator_event
data: {"schema_version":"1","event_id":"7f9c1c","seq":42,"ts":"2024-10-11T12:01:02Z","source":"coordinator","type":"task_transition","status":"ok"}

event: heartbeat
data: {"schema_version":"1","event_id":"hb-42","seq":43,"ts":"2024-10-11T12:01:07Z","source":"coordinator","type":"heartbeat","status":"ok"}
```

## Configuration and PRD

### GET `/api/v1/config`

Purpose: read the effective canonical configuration.

Response 200:
```json
{
  "version": "v1",
  "enabledTools": ["codex", "claude"],
  "toolConfig": {},
  "toolSettings": {},
  "standardsPath": null,
  "standardsInline": {},
  "selectedSkills": [],
  "selectedAgents": [],
  "selectedMcp": [],
  "quiet": false,
  "offline": false,
  "webPort": 3450,
  "webAssets": "dist",
  "coordinatorTool": "codex",
  "referenceBranch": "main",
  "prdFile": "worktree.prd.json",
  "taskRegistryFile": ".macc/automation/task/task_registry.json",
  "requirementsDetected": false,
  "managedEnvironmentWarnings": []
}
```

### PUT `/api/v1/config`

Purpose: update and persist the canonical configuration.

Request body:
```json
{
  "quiet": true,
  "offline": false,
  "webPort": 3450,
  "selectedSkills": ["macc-prd-planner"]
}
```

Response 200: same shape as `GET /api/v1/config`.

### POST `/api/v1/config/standards-preview`

Purpose: render the tool standards preview cards used by the Config page.

Request body:
```json
{
  "standardsPath": "docs/standards.md",
  "standardsInline": {
    "codex": "# Codex notes"
  }
}
```

Response 200:
```json
{
  "cards": [
    {
      "id": "codex",
      "title": "Codex - AGENTS.md (rendered)",
      "content": "# Codex notes"
    }
  ]
}
```

### GET `/api/v1/prd`

Purpose: read the active PRD file.

Query parameters:
- `path` (string, optional): relative file path or worktree path.

Response 200:
```json
{
  "tasks": [
    {
      "id": "WEB2-DOCS-001",
      "title": "Update web UI documentation",
      "priority": "4",
      "category": "documentation",
      "scope": null,
      "baseBranch": null,
      "coordinatorTool": null,
      "description": "Documentation needs to cover all new web UI features.",
      "objective": "Keep documentation current with the implementation.",
      "result": "Updated README.md, docs/WEB_API_CONTRACT.md, CHANGELOG.md.",
      "dependencies": [],
      "exclusiveResources": ["README.md", "docs/WEB_API_CONTRACT.md", "docs/ERRORS.md", "CHANGELOG.md"],
      "steps": ["Update README.md Web UI section.", "Update docs/WEB_API_CONTRACT.md.", "Update CHANGELOG.md."],
      "notes": null,
      "metadata": {}
    }
  ],
  "metadata": {
    "version": "v1"
  }
}
```

### PUT `/api/v1/prd`

Purpose: replace the PRD content and persist it.

Request body:
```json
{
  "tasks": [
    {
      "id": "WEB2-DOCS-001",
      "title": "Update web UI documentation",
      "priority": "4",
      "category": "documentation",
      "scope": null,
      "baseBranch": null,
      "coordinatorTool": null,
      "description": "Documentation needs to cover all new web UI features.",
      "objective": "Keep documentation current with the implementation.",
      "result": "Updated README.md, docs/WEB_API_CONTRACT.md, CHANGELOG.md.",
      "dependencies": [],
      "exclusiveResources": [],
      "steps": [],
      "notes": null,
      "metadata": {}
    }
  ],
  "metadata": {
    "version": "v1"
  }
}
```

Response 200: same shape as `GET /api/v1/prd`.

## Planning and Apply

### POST `/api/v1/plan`

Purpose: preview the plan without writing files.

Request body:
```json
{
  "scope": "project",
  "tools": ["codex"],
  "worktrees": ["feature-web-01"],
  "allowUserScope": false,
  "offline": false,
  "includeDiff": true,
  "explain": true
}
```

Response 200:
```json
{
  "summary": {
    "totalActions": 3,
    "filesWrite": 2,
    "filesMerge": 1,
    "consentRequired": 0,
    "backupRequired": 1,
    "backupPath": ".macc/backups"
  },
  "files": [
    {
      "path": "README.md",
      "kind": "write",
      "scope": "project",
      "consentRequired": false,
      "backupRequired": false,
      "setExecutable": false,
      "riskLevel": "safe",
      "contentPreview": "# MACC",
      "explain": "Write updated web UI docs."
    }
  ],
  "diffs": [],
  "risks": [
    {
      "level": "safe",
      "message": "No elevated risks detected for this plan preview."
    }
  ],
  "consents": []
}
```

### POST `/api/v1/apply`

Purpose: execute the apply workflow. Use `dryRun: true` for preview mode.

Request body:
```json
{
  "scope": "project",
  "tools": ["codex"],
  "allowUserScope": false,
  "dryRun": false,
  "confirmed": true
}
```

Response:
- When `dryRun` is `true`, the response matches `ApiPlanResponse` from `POST /api/v1/plan`.
- When `dryRun` is `false`, the response is:

```json
{
  "dryRun": false,
  "appliedActions": 3,
  "changedFiles": 2,
  "backupLocations": [".macc/backups"],
  "results": [
    {
      "path": "README.md",
      "kind": "write",
      "success": true,
      "message": "updated",
      "backupLocation": ".macc/backups/README.md"
    }
  ],
  "warnings": []
}
```

## Worktrees and Terminal

### GET `/api/v1/worktrees`

Purpose: list managed worktrees.

Response 200:
```json
[
  {
    "id": "feature-web-01",
    "slug": "feature-web",
    "branch": "feature/web-01",
    "tool": "codex",
    "status": "active",
    "path": "/repo/.macc/worktree/feature-web-01",
    "baseBranch": "main",
    "head": "abc123",
    "scope": "project",
    "feature": "web-ui",
    "locked": false,
    "prunable": false,
    "sessionLabel": "codex#1"
  }
]
```

### POST `/api/v1/worktrees`

Purpose: create one or more managed worktrees.

Request body:
```json
{
  "slug": "feature-web",
  "tool": "codex",
  "count": 1,
  "base": "main",
  "scope": "project",
  "feature": "web-ui",
  "skipApply": false,
  "allowUserScope": false
}
```

Response 200: array of `ApiWorktree`.

### DELETE `/api/v1/worktrees/{id}`

Purpose: remove a managed worktree.

Request body:
```json
{
  "confirmed": true,
  "force": false
}
```

Response 200:
```json
{
  "status": "ok",
  "message": "Removed worktree 'feature-web-01'",
  "id": "feature-web-01",
  "path": "/repo/.macc/worktree/feature-web-01",
  "force": false,
  "removeBranch": false
}
```

### POST `/api/v1/worktrees/{id}/run`

Purpose: start a performer run for a managed worktree.

Response 202:
```json
{
  "status": "started",
  "worktreeId": "feature-web-01",
  "path": "/repo/.macc/worktree/feature-web-01"
}
```

### GET `/api/v1/worktrees/{id}/logs` (SSE)

Purpose: stream performer logs for a single worktree.

Headers:
- `Accept: text/event-stream`
- `Last-Event-ID` (optional): resume after the last delivered line number or heartbeat cursor

Response 200:
- `Content-Type: text/event-stream`

Example:
```text
id: 1
event: log_line
data: {"worktree_id":"feature-web-01","line":1,"timestamp":"2026-03-20T12:00:00Z","level":"info","message":"boot"}

id: hb-1-1760000000000
event: heartbeat
data: {"event_id":"hb-1-1760000000000","type":"heartbeat","status":"ok","line":1,"timestamp":"2026-03-20T12:00:15Z"}
```

### POST `/api/v1/terminal`

Purpose: create a terminal session for the project root or a worktree.

Request body:
```json
{
  "terminalType": "worktree",
  "worktreeId": "feature-web-01"
}
```

Response 201:
```json
{
  "sessionId": "term-...",
  "terminalType": "worktree",
  "path": "/repo/.macc/worktree/feature-web-01",
  "worktreeId": "feature-web-01"
}
```

Notes:
- Returns `MACC-WEB-3003` when the terminal session limit is reached or an existing session conflicts with the request.
- Returns `MACC-WEB-4002` when the local PTY or shell startup fails.

### GET `/api/v1/terminal/{session}`

Purpose: attach to a terminal session over WebSocket.

Response:
- `101 Switching Protocols`

## Logs, Doctor, Backups

### GET `/api/v1/logs`

Purpose: list browsable coordinator and performer log files under `.macc/log/`.

Response 200:
```json
[
  {
    "path": "coordinator/events.jsonl",
    "size": 2048,
    "modified": "2026-03-20T12:00:00Z"
  },
  {
    "path": "performer/worker-01--TASK-001-.md",
    "size": 1024,
    "modified": "2026-03-20T12:05:00Z"
  }
]
```

### GET `/api/v1/logs/{path}`

Purpose: read a log file under `.macc/log/` with optional pagination and line filtering.

Query parameters:
- `offset` (number, optional): zero-based filtered line offset, default `0`
- `limit` (number, optional): maximum filtered lines returned, default `200`
- `search` (string, optional): substring filter applied before pagination

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

### GET `/api/v1/logs/tail` (SSE)

Purpose: tail a log file under `.macc/log/` and stream newly appended lines.

Query parameters:
- `path` (string, required): relative path such as `coordinator/events.jsonl`

Response 200:
- `Content-Type: text/event-stream`

### GET `/api/v1/doctor`

Purpose: run diagnostics for tools, paths, and environment health.

Response 200:
```json
{
  "healthScore": 90,
  "issuesBySeverity": {
    "warning": 1,
    "error": 0
  },
  "issues": [
    {
      "name": "Git is missing",
      "toolId": null,
      "target": "git",
      "severity": "warning",
      "kind": "which",
      "status": "missing",
      "message": "Install git to enable repo operations"
    }
  ]
}
```

### POST `/api/v1/doctor/fix`

Purpose: run safe automated fixes for doctor issues.

Request body: optional `ApiDoctorFixRequest` payload. The current UI sends an empty body to fix all safe issues.

Response 200:
```json
{
  "status": "ok",
  "message": "Doctor fix resolved 1 issue(s).",
  "attempted_count": 1,
  "fixed_count": 1,
  "failed_count": 0,
  "backup_location": null,
  "results": [],
  "report": {
    "healthScore": 100,
    "issuesBySeverity": {},
    "issues": []
  }
}
```

### GET `/api/v1/backups`

Purpose: list backup sets under `.macc/backups/`.

Response 200:
```json
[
  {
    "id": "20260320-120000",
    "timestamp": "20260320-120000",
    "files": 2,
    "entries": [
      { "path": "README.md", "size": 1200 }
    ],
    "totalSize": 1200,
    "path": "/repo/.macc/backups/20260320-120000",
    "userScope": false
  }
]
```

### POST `/api/v1/backups/{id}/restore`

Purpose: restore a backup set.

Request body:
```json
{ "confirmed": true }
```

Response 200:
```json
{
  "status": "ok",
  "message": "Restored 2 file(s) from backup '20260320-120000' after creating restore backup '20260320-130000'",
  "backupId": "20260320-120000",
  "restoreBackupId": "20260320-130000",
  "restoredFiles": 2
}
```

### GET `/api/v1/trust`

Purpose: retrieve the dynamic trust status and safety parameters of the project workspace.

Response 200:
```json
{
  "state": "trusted",
  "local_only": true,
  "terminal_enabled": false,
  "user_level_writes": 0,
  "backups_ready": true,
  "catalog_pinned": true,
  "secrets_redacted": true,
  "server_exposure": "127.0.0.1:3450",
  "allowed_roots": [
    "/repo"
  ],
  "audit_log": "/repo/.macc/log/coordinator/coordinator.log"
}
```

## Registry

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
    "description": null,
    "objective": null,
    "result": null,
    "dependencies": [],
    "exclusiveResources": [],
    "steps": [],
    "notes": null,
    "assignee": null,
    "worktree": null,
    "events": [],
    "updatedAt": "2026-03-20T12:05:00Z"
  }
]
```

### POST `/api/v1/registry/tasks/{id}/{action}`

Purpose: apply operator actions to a single registry task.

Path parameters:
- `id` (string, required): registry task ID
- `action` (string, required): `requeue | reassign | abandon`

Request body for `requeue`:
```json
{ "kind": "requeue", "justification": "optional operator note" }
```

Request body for `reassign`:
```json
{ "kind": "reassign", "tool": "gemini", "justification": "optional operator note" }
```

Request body for `abandon`:
```json
{ "kind": "abandon", "justification": "optional operator note" }
```

Response 200: updated `ApiRegistryTask`.

Notes:
- `requeue` resets blocked/failed tasks back to `todo`.
- `reassign` updates the task's assigned tool and rejects active or merged tasks.
- `abandon` transitions the task to a terminal abandoned state.

### GET `/api/v1/registry/tasks/{id}`

Purpose: retrieve details of a single registry task, including its historical events.

Path parameters:
- `id` (string, required): registry task ID

Response 200: `ApiRegistryTask` object.

### GET `/api/v1/registry/tasks/{id}/events`

Purpose: retrieve the list of raw coordinator events captured for a single task.

Path parameters:
- `id` (string, required): registry task ID

Response 200:
```json
[
  {
    "eventId": "evt_12345",
    "eventType": "task_claimed",
    "ts": "2026-03-20T12:00:00Z",
    "status": "claimed",
    "severity": "info",
    "message": "Task WEB2-BE-REG-001 claimed by worker-1"
  }
]
```

### GET `/api/v1/registry/tasks/{id}/logs`

Purpose: retrieve the current stdout and stderr logs for a single task.

Path parameters:
- `id` (string, required): registry task ID

Response 200:
```json
{
  "taskId": "WEB2-BE-REG-001",
  "stdout": "Running cargo build...\nCompilation successful.",
  "stderr": "warning: unused variable: `foo`"
}
```

### GET `/api/v1/registry/tasks/{id}/diff`

Purpose: retrieve the current git diff of the task's active worktree.

Path parameters:
- `id` (string, required): registry task ID

Query parameters:
- `format` (string, optional): `patch` (default) or `stat`

Response 200:
```json
{
  "taskId": "WEB2-BE-REG-001",
  "format": "patch",
  "diff": "diff --git a/src/main.rs b/src/main.rs\n..."
}
```

### GET `/api/v1/registry/tasks/{id}/explain`

Purpose: retrieve a structured timeline explanation of a task's lifecycle events.

Path parameters:
- `id` (string, required): registry task ID

Response 200:
```json
{
  "taskId": "WEB2-BE-REG-001",
  "timeline": [
    {
      "timestamp": "2026-03-20T12:00:00Z",
      "severity": "info",
      "phase": "implementing",
      "eventType": "task_claimed",
      "message": "Task claimed by worker-1"
    }
  ]
}
```

### GET `/api/v1/tasks/{id}/stream`

Purpose: open a server-sent events (SSE) stream to receive realtime events for a task.

Path parameters:
- `id` (string, required): registry task ID

Query parameters:
- `lastEventId` (string, optional): resume stream cursor
- `webClientId` (string, optional)

Response 200: `text/event-stream` returning structured event payloads.

### POST `/api/v1/registry/tasks/{id}/retry`

Purpose: requeue a failed or blocked task back to `todo` state to trigger a retry.

Path parameters:
- `id` (string, required): registry task ID

Response 200: updated `ApiRegistryTask` object.

### POST `/api/v1/registry/tasks/{id}/stop`

Purpose: send a stop/kill signal to cancel the performer execution for a running task.

Path parameters:
- `id` (string, required): registry task ID

Response 200: updated `ApiRegistryTask` object.

### POST `/api/v1/registry/tasks/{id}/run-testing`

Purpose: force/manually trigger the testing phase for a task.

Path parameters:
- `id` (string, required): registry task ID

Response 200: updated `ApiRegistryTask` object.

### POST `/api/v1/registry/tasks/{id}/run-review`

Purpose: force/manually trigger the review phase for a task.

Path parameters:
- `id` (string, required): registry task ID

Response 200: updated `ApiRegistryTask` object.

---

## UX Observability Endpoints (spec §4.21, §8)

### GET `/api/v1/snapshot`

Purpose: return the full `RuntimeSnapshot` — the shared runtime model consumed by CLI (`macc status --json`), TUI observer, and Web Mission Control.

Response 200: full `RuntimeSnapshot` JSON with fields: `generated_at`, `project`, `coordinator`, `queue`, `workers`, `tasks`, `throttled_tools`, `recent_events`, `git`, `diagnostics`.

### GET `/api/v1/search?q=<query>`

Purpose: search across tasks, skills, worktrees, and error codes. Response 200: array of `{ kind, id, label, meta }`.

### GET `/api/v1/skills`

Purpose: list available skills from `.macc/skills/` and built-ins. Response 200: array of `{ id, title, kind, risk, description }`.

### GET `/api/v1/skills/{id}`

Purpose: full skill definition. Response 200: skill JSON. Response 404: not found.

### POST `/api/v1/skills/{id}/dry-run`

Purpose: dry-run preview (no execution). Response 200: `{ skillId, title, kind, tool, risk, commands, writes, logsPath }`.

### POST `/api/v1/skills/{id}/run`

Purpose: execute a skill. Request body: `{ tool?, task_id?, yes? }`. Response 200: `{ skillId, status, tool, startedAt, durationMs, stdout, stderr, exitCode }`.

### GET `/api/v1/runs`

Purpose: list recent skill run log entries (up to 50). Response 200: array of `{ id, skill_id, started_at, status }`.

### GET `/api/v1/runs/{id}`

Purpose: run events. Response 200: `{ run_id, events }`.

### GET `/api/v1/runs/{id}/logs`

Purpose: raw JSONL log for a skill run. Response 200: JSONL text.

### GET `/api/v1/failures/recent`

Purpose: last 20 failure events from the coordinator log. Response 200: array of `{ task_id, tool, error_code, retryable, excerpt, ts }`.

### GET `/api/v1/workers/{id}/snapshot`

Purpose: per-worker runtime snapshot. Response 200: `WorkerRuntime` JSON. Response 404: not found.


## PRD Generation (spec §8)

### POST `/api/v1/prd/generate`

Purpose: build the fixed `macc-prd-planner` prompt from a brief file. In `dry_run` mode returns the assembled prompt without invoking any tool; otherwise returns the prompt for the caller to route.

Request body:
```json
{
  "fromPath": "brief.md",
  "tool": "claude",
  "modelSelection": { "mode": "auto" },
  "instructions": "Split coordinator, adapter, TUI, and Web work into separate tasks.",
  "dryRun": false,
  "promote": false,
  "yes": false
}
```

Response 200:
```json
{
  "status": "dry_run | prompt_ready",
  "runId": "2026-05-26-143012",
  "targetDir": ".macc/generated/prd/macc-prd-planner/2026-05-26-143012",
  "tool": "claude",
  "prompt": "..."
}
```

### POST `/api/v1/prd/audit`

Purpose: enrich an existing PRD from commit history and delivered code. Replaces the removed `POST /api/v1/coordinator/audit-prd` endpoint.

Request body:
```json
{
  "prdPath": "prd.json",
  "tool": "claude",
  "modelSelection": { "mode": "auto" },
  "referenceBranch": "main",
  "diffStat": true,
  "dryRun": false
}
```

Response 200:
```json
{
  "completedWithContext": 4,
  "todoTasks": 3,
  "promptGenerated": true,
  "prompt": "...",
  "dispatched": false
}
```

### POST `/api/v1/prd/promote`

Purpose: promote a generated PRD file to the active `prd.json`. Creates a backup before overwriting.

Request body:
```json
{
  "sourcePath": ".macc/generated/prd/macc-prd-planner/2026-05-26-143012/prd.json",
  "destPath": "prd.json",
  "yes": false
}
```

Response 200: `{ promoted, source, destination, backupCreated }`.

### POST `/api/v1/prd/validate`

Purpose: run lightweight validation on a PRD file.

Request body: `{ "prdPath": "prd.json" }`.

Response 200: `{ ok, warnings: [], errors: [] }`.

### GET `/api/v1/prd/generation-runs`

Purpose: list all PRD generation runs under `.macc/generated/prd/macc-prd-planner/`. Response 200: array of `{ runId, path }`.

### GET `/api/v1/prd/generation-runs/{run_id}`

Purpose: show metadata and file list for a specific run. Response 200: `{ runId, path, files, metadata }`. Response 404: run not found.

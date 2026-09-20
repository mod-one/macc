# Error Catalog

This catalog defines error code naming and the current web/API codes.
Keep it synchronized with `cli/src/commands/web/errors.rs`.

## Naming

Format: `MACC-<DOMAIN>-<4 digits>`

- `DOMAIN` examples: `WEB`, `CORE`, `COORD`.
- Codes are stable once published.

## Categories

The API error envelope uses these categories:

- `Validation`
- `Auth`
- `Dependency`
- `Conflict`
- `NotFound`
- `Internal`

## Web/API Codes

- `MACC-WEB-0000`: Unspecified web API failure (fallback when no specific mapping is available).

### Validation (1000 range)

- `MACC-WEB-1000`: Generic validation failure (`MaccError::Validation`).
- `MACC-WEB-1001`: Operator confirmation required before destructive web actions.
- `MACC-WEB-1002`: Secret scan validation failure (`MaccError::SecretDetected`).
- `MACC-WEB-1003`: Configuration parse/validation failure (`MaccError::Config`).
- `MACC-WEB-1004`: Catalog operation validation failure (`MaccError::Catalog`).
- `MACC-WEB-1005`: Registry action payload or path validation failure.
- `MACC-WEB-1006`: Tool specification validation failure (`MaccError::ToolSpec`).
- `MACC-WEB-1007`: Log API path or query validation failure.

### NotFound (2000 range)

- `MACC-WEB-2000`: Project root cannot be resolved (`MaccError::ProjectRootNotFound`).
- `MACC-WEB-2001`: User home directory cannot be resolved (`MaccError::HomeDirNotFound`).
- `MACC-WEB-2002`: Registry task was not found for the requested operator action.
- `MACC-WEB-2003`: Backup set was not found for the requested restore action.
- `MACC-WEB-2004`: Worktree was not found for the requested web action.
- `MACC-WEB-2005`: Requested log file was not found under `.macc/log/`.
- `MACC-WEB-2006`: Terminal session was not found or is no longer available.

### Conflict / Auth (3000 range)

- `MACC-WEB-3000`: User-scope operation denied in current mode (`MaccError::UserScopeNotAllowed`).
- `MACC-WEB-3001`: Registry operator action conflicts with the task's current state/runtime.
- `MACC-WEB-3002`: Worktree action conflicts with the current git/worktree state.
- `MACC-WEB-3003`: Terminal action conflicts with the current terminal session state.

### Dependency / Engine (4000 range)

- `MACC-WEB-4000`: Local I/O dependency failed (`MaccError::Io`).
- `MACC-WEB-4001`: Remote fetch dependency failed (`MaccError::Fetch`).
- `MACC-WEB-4002`: Terminal session creation or PTY startup failed.

### Internal (5000 range)

- `MACC-WEB-5000`: Coordinator workflow failure (`MaccError::Coordinator`).
- `MACC-WEB-5001`: Coordinator storage backend failure (`MaccError::Storage`).
- `MACC-WEB-5002`: Git subsystem failure (`MaccError::Git`).

## Coordinator Engine Error Codes (E-Series)

These are internal error codes emitted by the coordinator engine (E-series codes). They categorize task failures and determine recovery/retry paths.

- `E410` (Coordinator lease conflict): Another coordinator process holds the lease. **Not retryable**.
- `E411` (Runtime ledger write failed): Failed to write to the durable runtime ledger. **Not retryable**.
- `E412` (Recovery classification failed): Failed to analyze prior state for recovery. **Not retryable**.
- `E413` (Performer heartbeat stale): Performer failed to report heartbeats within the stale window. **Conditional retry**.
- `E414` (Performer process dead): Performer process exited or was terminated unexpectedly. **Conditional retry**.
- `E415` (Orphaned performer detected): Untracked performer process found running. **Not retryable**.
- `E416` (Force termination failed): Failed to kill a performer process/group during force-stop. **Not retryable**.
- `E417` (Dirty worktree blocks recovery): Worktree has uncommitted local changes blocking clean checkout. **Not retryable**.
- `E418` (Stale event rejected): Received an out-of-order or expired event/epoch. **Not retryable**.
- `E902` (Retry budget exhausted): A task that reported `error_with_changes` used up its same-worktree retry budget. The task is set to `blocked`, and its worktree and branch stay attached so the committed work can be found and merged or discarded. **Not retryable** — requires operator action. See `automation.coordinator.phase_runner_max_attempts`.

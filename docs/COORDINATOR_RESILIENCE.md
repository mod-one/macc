# MACC Coordinator Resilience & Recovery Manual

This document details the crash-consistent runtime design, process-group termination, stop semantics, and automatic recovery procedures for the MACC Coordinator.

---

## 1. Crash-Consistent Runtime Ledger

The MACC coordinator utilizes a local SQLite database (`.macc/state/ledger.db` or configured state path) to durably ledger all state transitions *before* and *after* execution.

### Schema Foundations
- **`coordinator_runs`**: Persists coordinator process instances, run IDs, hostnames, timestamps, run status (`running`, `draining`, `stopping`, `force_stopping`, `stopped`, `crashed`, `recovered`), and incrementing epoch fencing tokens.
- **`coordinator_control`**: Records operator-directed modes (`running`, `draining`, `graceful_stopping`, `force_stopping`, `stopped`), force grace limits, and cleanup configurations.
- **`task_runtime`**: Isolates runtime state (`dispatched`, `running`, `phase_done`, `failed`, `stale`, `orphaned`, `adopted`, `blocked`) from high-level workflow state (`todo`, `claimed`, etc.), capturing process identifiers (PID, PGID), heartbeat metrics, and task attempt counts.

---

## 2. Process-Group based Termination

When a performer task is launched, it is spawned in a new session or process group (`setsid` or `process_group(0)`). Consequently, the PGID is identical to the performer's PID.

This allows the coordinator to robustly clean up the entire process tree of a task (including vendor binaries, shells, and nested children) using process group signaling:
- **Graceful Stop**: `SIGTERM` is sent to the process group (`kill -TERM -<PGID>`). The coordinator waits up to a specified grace period (default 10s).
- **Force Stop**: If processes survive the grace period, or if the operator triggers a force-kill, the coordinator issues `SIGKILL` to the process group (`kill -KILL -<PGID>`).

---

## 3. Explicit Stop Semantics

MACC supports four distinct operator stop modes, available via CLI, Web API, and TUI:

1. **Drain & Stop (`drain`)**
   - Disables new task dispatching.
   - Allows currently running performer tasks to execute to completion.
   - Shuts down the coordinator once all running tasks finish.
2. **Graceful Stop (`graceful`)**
   - Disables new task dispatching.
   - Signals active performer process groups with `SIGTERM`.
   - Shuts down the coordinator immediately without waiting or escalating to `SIGKILL`.
3. **Force Stop (`force`)**
   - Signals active performer process groups with `SIGTERM` followed by `SIGKILL` after a grace period.
   - Clears active database claims.
   - Shuts down the coordinator immediately.
4. **Force Stop & Cleanup (`force + cleanup`)**
   - Force-stops all performer process groups.
   - Automatically prunes and removes all active task git worktrees and temporary branches.
   - Shuts down the coordinator.

---

## 4. Automatic Startup & Recovery Logic

Upon startup or when the `/api/v1/coordinator/recover` endpoint is triggered, the coordinator runs a state reconciliation cycle to detect and heal anomalies:

### Stale heartbeats
If a performer is marked as running in the database but its last heartbeat is older than 180 seconds:
- If the performer process is still alive, status is updated to `stale` (allowing grace time).
- If the performer process is dead, the task is requeued/blocked depending on the worktree state.

### Process Liveness checks
The coordinator checks whether the recorded performer PID/PGID is active:
- **Process dead, no commits ahead**: Requeues the task cleanly (resets to `todo` state) and releases worktree locks.
- **Process dead, commits ahead**: Marks the task as `phase_done` or blocks it to prevent discarding completed work.
- **Process dead, merge in progress**: Blocks the task and requests operator intervention (preventing corrupted repository state).

### Orphaned process detection
The coordinator scans the system for processes whose cwd starts with the repository path and whose binary matches `macc` or `performer`, but are not registered in the active database. These are flagged as `orphaned` so the operator can inspect and terminate them.

---

## 5. Web REST API Reference

### Stop Coordinator
- **Endpoint**: `POST /api/v1/coordinator/stop`
- **Request Body**:
  ```json
  {
    "mode": "drain",
    "cleanup_worktrees": false,
    "force_grace_seconds": 10,
    "reason": "operator requested shutdown"
  }
  ```

### Recover Coordinator
- **Endpoint**: `POST /api/v1/coordinator/recover`
- **Request Body**:
  ```json
  {
    "dry_run": false
  }
  ```

### Active Processes Listing
- **Endpoint**: `GET /api/v1/coordinator/processes`
- **Response Body**:
  ```json
  {
    "processes": [
      {
        "task_id": "T-042",
        "claim_id": "claim_abc",
        "pid": 1292224,
        "process_group_id": 1292224,
        "tool": "codex",
        "status": "alive",
        "last_heartbeat_at": "2026-05-26T12:00:00Z"
      }
    ]
  }
  ```

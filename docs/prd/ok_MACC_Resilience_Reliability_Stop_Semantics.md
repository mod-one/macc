# MACC Resilience & Reliability Motif

**Crash-Consistent Coordinator, Runtime Recovery, Client Control, Drain Stop, and Force Termination**

**Project:** MACC — Multi-Assistant Code Config  
**Status:** Proposed design amendment  
**Date:** 2026-05-26  
**Language:** English  
**Target location:** `docs/COORDINATOR_RESILIENCE.md` or a new section under the coordinator/runtime documentation

---

## 1. Executive Summary

The MACC coordinator is the control plane responsible for selecting ready tasks, assigning worktree slots, launching performers, consuming runtime events, advancing task phases, merging branches, and cleaning up. In the current model, the coordinator is operationally powerful but still fragile in one important way: if the coordinator process crashes or is stopped incorrectly, launched tool processes can remain alive, active task claims can become ambiguous, and manual recovery may be required.

This motif upgrades the coordinator from a best-effort process supervisor into a **crash-consistent runtime controller**.

The core idea is simple:

> MACC should not merely detect coordinator failure. Coordinator failure should become boring.

To achieve this, MACC should add:

1. A durable SQLite **Coordinator Runtime Ledger**.
2. Restart-safe **checkpoint/resume** at every state transition.
3. A strict separation between **workflow state** and **runtime process state**.
4. Performer heartbeat and process liveness monitoring.
5. Startup recovery before any new dispatch.
6. Fencing tokens to prevent double-dispatch and stale event writes.
7. Health/readiness/status endpoints visible from all clients.
8. Explicit coordinator stop modes: `drain`, `graceful`, `force`, and `force + cleanup`.
9. Process-group based termination for launched tool processes.
10. Full accessibility from CLI, TUI, Web UI, and API.

---

## 2. Context

MACC already has the foundations required for this design:

- a coordinator full-cycle loop,
- task registry state,
- worktree slot reuse,
- performer heartbeats,
- structured error codes,
- `macc coordinator status`, `sync-prd`, `reconcile`, `unlock`, and `cleanup`,
- local Web API endpoints for health/status/coordinator actions,
- a future direction that separates task workflow state from runtime process lifecycle.

This proposal consolidates those existing pieces into a more robust runtime model.

---

## 3. Problems to Solve

### 3.1 Coordinator as a Single Point of Failure

The coordinator currently owns critical in-memory runtime knowledge:

- which tasks are actively claimed,
- which worktree slots are locked,
- which performer processes were launched,
- which phase a task is actually executing,
- when the last heartbeat was observed,
- whether a task is safe to requeue, adopt, block, or merge.

If the coordinator exits unexpectedly, this state may be partially lost unless every important transition is persisted durably.

### 3.2 Orphaned Performer Processes

A stopped coordinator may leave launched tool processes running. Example:

```text
brand    1292224  0.0  0.2 1406624 47604 pts/2   Sl   00:35   0:00 node .../codex --model gpt-5.4 --yolo exec -
brand    1292225  0.0  0.0   3136  1788 pts/2    S    00:35   0:00 tee /tmp/tmp.LwmAS0rilP
brand    1292252  4.0  0.6 426584 110984 pts/2   Sl   00:35   0:11 .../codex-linux-x64/vendor/x
```

Killing only the coordinator process is not sufficient. Killing only the top-level PID may also be insufficient because child processes, `tee`, shell wrappers, vendor binaries, and background sleeps can survive.

### 3.3 Ambiguous Stop Semantics

Users need several distinct stop behaviors:

- stop after all current work finishes,
- stop soon without dispatching new tasks,
- kill everything because a tool is hung,
- kill everything and clean generated worktrees.

A single `stop` command is too ambiguous.

### 3.4 Manual Recovery Burden

Commands such as `macc coordinator unlock --task <id>` are useful, but they should be operator overrides, not the standard recovery path. MACC should automatically classify stale, orphaned, adopted, and blocked tasks whenever possible.

### 3.5 Client Visibility Gap

The same controls must be available through:

- CLI,
- TUI,
- Web UI,
- local REST API,
- scripts and monitoring.

A stop mode or health state that exists only in the CLI is not enough.

---

## 4. Design Goals

### 4.1 Reliability Goals

- Persist critical runtime state before and after every state transition.
- Resume safely after coordinator crash or host reboot.
- Prevent duplicate task dispatch.
- Detect stale performers automatically.
- Detect orphaned processes and expose them clearly.
- Preserve potentially useful worktree changes by default.
- Provide forceful termination when the operator explicitly requests it.

### 4.2 UX Goals

- Make stop behavior explicit and predictable.
- Expose the same actions across CLI, TUI, Web UI, and API.
- Provide clear labels: **Drain & Stop**, **Graceful Stop**, **Force Stop**, **Force Stop + Cleanup**.
- Show current coordinator mode in every client.
- Avoid requiring users to understand PIDs, process groups, and runtime state internals during normal operation.

### 4.3 Operational Goals

- Make `/health` script-friendly.
- Make `/status` rich enough for dashboards.
- Support external monitors without requiring log parsing.
- Make recovery dry-runs possible.
- Keep destructive actions behind explicit confirmation gates.

---

## 5. Non-Goals

This design does **not** turn MACC into a distributed CI/CD orchestrator or a multi-node cluster scheduler.

It does not require:

- remote public hosting,
- a Kubernetes-like control plane,
- distributed consensus,
- remote worker fleets,
- automatic deletion of partial work after failure.

The design remains local-first and project-scoped.

---

## 6. Core Concept: Crash-Consistent Runtime Control

The coordinator should treat runtime state as a durable ledger, not as transient memory.

Every transition that changes task ownership, worktree assignment, lock state, process identity, or runtime phase must be written to SQLite.

The coordinator should be able to restart and answer:

1. Which tasks were active?
2. Which performer processes were launched?
3. Which process groups should still exist?
4. Which worktree slots were locked?
5. Which resources were reserved?
6. Which phase was each task in?
7. Which events were already consumed?
8. Which tasks can be adopted, requeued, blocked, or marked complete?

---

## 7. Durable Coordinator Runtime Ledger

### 7.1 Recommended SQLite Tables

#### `coordinator_runs`

Tracks coordinator process instances.

```text
coordinator_runs
- run_id TEXT PRIMARY KEY
- pid INTEGER NOT NULL
- hostname TEXT NOT NULL
- started_at TEXT NOT NULL
- last_tick_at TEXT
- stopped_at TEXT
- status TEXT NOT NULL
  -- running | draining | stopping | force_stopping | stopped | crashed | recovered
- epoch INTEGER NOT NULL
- version TEXT NOT NULL
- stop_reason TEXT
```

#### `coordinator_control`

Stores the operator-requested control mode.

```text
coordinator_control
- id INTEGER PRIMARY KEY CHECK (id = 1)
- mode TEXT NOT NULL
  -- running | draining | graceful_stopping | force_stopping | stopped
- requested_at TEXT
- requested_by TEXT
  -- cli | tui | web | api | recovery | system
- drain_snapshot_json TEXT
- force_grace_seconds INTEGER
- cleanup_after_force INTEGER
- reason TEXT
```

#### `task_runtime`

Stores runtime lifecycle state separately from workflow state.

```text
task_runtime
- task_id TEXT NOT NULL
- claim_id TEXT PRIMARY KEY
- run_id TEXT NOT NULL
- coordinator_epoch INTEGER NOT NULL
- workflow_state TEXT NOT NULL
  -- todo | claimed | in_progress | pr_open | changes_requested | queued | merged | abandoned
- runtime_status TEXT NOT NULL
  -- dispatched | running | phase_done | failed | stale | orphaned | adopted | blocked | aborted_by_operator
- phase TEXT
  -- dev | review | fix | merge | cleanup
- tool TEXT
- worktree_slot_id TEXT
- worktree_path TEXT
- branch TEXT
- base_branch TEXT
- pid INTEGER
- process_group_id INTEGER
- started_at TEXT
- updated_at TEXT
- last_heartbeat_at TEXT
- heartbeat_seq INTEGER DEFAULT 0
- lease_expires_at TEXT
- attempt INTEGER DEFAULT 0
- locked_resources_json TEXT
- last_error_code TEXT
- last_error_message TEXT
```

#### `worktree_slots`

Tracks reusable worktree slots.

```text
worktree_slots
- slot_id TEXT PRIMARY KEY
- path TEXT NOT NULL
- base_branch TEXT NOT NULL
- current_branch TEXT
- assigned_task_id TEXT
- assigned_claim_id TEXT
- tool TEXT
- status TEXT NOT NULL
  -- idle | assigned | dirty | cleanup_pending | broken | blocked
- last_checked_at TEXT
- cleanup_error TEXT
```

#### `resource_locks`

Tracks exclusive resources.

```text
resource_locks
- resource TEXT NOT NULL
- task_id TEXT NOT NULL
- claim_id TEXT NOT NULL
- acquired_at TEXT NOT NULL
- expires_at TEXT
- PRIMARY KEY(resource, claim_id)
```

#### `event_cursor`

Stores event consumption progress.

```text
event_cursor
- stream TEXT PRIMARY KEY
- last_event_id TEXT
- last_read_at TEXT
```

#### `process_registry`

Optional but useful for inspection and force termination.

```text
process_registry
- process_id TEXT PRIMARY KEY
- task_id TEXT
- claim_id TEXT
- pid INTEGER NOT NULL
- process_group_id INTEGER
- parent_pid INTEGER
- command TEXT
- launched_at TEXT
- last_seen_at TEXT
- status TEXT
  -- alive | exited | killed | unknown
```

---

## 8. Transactional State Transitions

The coordinator should update runtime state using explicit transition transactions.

### 8.1 Claim and Dispatch Transaction

When dispatching a task:

```text
BEGIN TRANSACTION
1. verify task is dispatchable
2. create claim_id
3. acquire resource locks
4. assign worktree slot
5. create task_runtime row with runtime_status=dispatched
6. write spawn intent
COMMIT
7. spawn performer process group
8. update task_runtime with pid/process_group_id and runtime_status=running
```

If the coordinator crashes before step 7, the task is classified on restart as:

```text
dispatched_without_process
```

and can be safely requeued or blocked.

If the coordinator crashes after step 7 but before step 8, startup recovery should inspect OS process state and event logs to adopt or classify the process.

### 8.2 Advance Transaction

When a phase completes:

```text
BEGIN TRANSACTION
1. validate claim_id and coordinator_epoch
2. consume event
3. update runtime_status=phase_done
4. update workflow_state if needed
5. release or retain locks depending on next phase
6. update event_cursor
COMMIT
```

### 8.3 Merge Transaction

Merges should be represented as a phase, not an invisible side effect.

```text
runtime_status=running
phase=merge
```

If a crash happens during merge, startup recovery must inspect Git state:

- merge in progress,
- conflict markers,
- partial index state,
- merge commit already created,
- task commit already present on base branch.

---

## 9. Workflow State vs Runtime State

MACC should maintain two separate concepts.

### 9.1 Workflow State

Workflow state describes the logical product/task lifecycle:

```text
todo
claimed
in_progress
pr_open
changes_requested
queued
merged
abandoned
```

### 9.2 Runtime Status

Runtime status describes the process/control-plane lifecycle:

```text
dispatched
running
phase_done
failed
heartbeat_stale
process_dead
stale
orphaned
adopted
blocked_dirty_worktree
blocked_merge_recovery
aborted_by_operator
requeued_after_stale
```

### 9.3 Why This Matters

A task can be logically `in_progress` while its runtime process is `heartbeat_stale`.

A task can be logically `merged` while an old performer process is still `orphaned`.

A task can be logically `claimed` but runtime-classified as `dispatched_without_process`.

This separation prevents false assumptions and makes recovery deterministic.

---

## 10. Fencing Tokens and Anti-Split-Brain Protection

Every coordinator run should have:

- `run_id`,
- `epoch`,
- `claim_id` per active task.

Every performer event should include:

```json
{
  "schema_version": 1,
  "event_id": "evt_...",
  "run_id": "run_...",
  "coordinator_epoch": 12,
  "claim_id": "claim_...",
  "task_id": "T-042",
  "phase": "dev",
  "type": "heartbeat",
  "seq": 17,
  "pid": 12345,
  "process_group_id": 12345,
  "worktree": ".macc/worktree/slot-02",
  "tool": "codex",
  "timestamp": "2026-05-26T12:00:00Z"
}
```

The coordinator must ignore events from stale `claim_id`s or older epochs.

This prevents:

- duplicate dispatch,
- stale coordinator writes,
- events from a previous run corrupting current runtime state,
- standby takeover races.

---

## 11. Startup Recovery Sweep

Before dispatching any new task, `macc coordinator` must run a recovery sweep.

### 11.1 Recovery Algorithm

```text
recover_startup()
1. Acquire coordinator singleton lease.
2. Increment coordinator epoch.
3. Load active task_runtime rows.
4. Replay unread events from event_cursor.
5. Verify OS process/process group liveness.
6. Inspect worktree state.
7. Inspect Git branch and merge state.
8. Run deterministic PRD reconciliation from commit history.
9. Classify every active task.
10. Persist recovery decisions.
11. Only then enable scheduling.
```

### 11.2 Recovery Classifications

| Situation | Classification | Default Action |
|---|---|---|
| Performer alive and heartbeat fresh | `adopted` | Continue monitoring. |
| Performer alive but heartbeat stale | `heartbeat_stale` | Wait grace period, then block/requeue. |
| Performer process group dead, no result | `process_dead` | Requeue or block depending on policy. |
| Performer dead, commit exists | `phase_done` | Continue FSM advancement. |
| Task commit already on base branch | `merged` | Close runtime claim. |
| Worktree dirty, no phase result | `blocked_dirty_worktree` | Require operator review. |
| Git merge in progress | `blocked_merge_recovery` | Resume, abort, or manual intervention. |
| Claim exists but no process was spawned | `dispatched_without_process` | Requeue safely. |
| Process exists but claim is stale | `orphaned` | Surface and optionally force terminate. |

### 11.3 Dry Run Mode

```bash
macc coordinator recover --dry-run
```

Should print the proposed classifications and actions without mutating state.

---

## 12. Performer Liveness Monitoring

The coordinator should include a continuous runtime monitor.

### 12.1 Heartbeat Events

Performers should emit heartbeat events at a fixed interval.

Recommended default:

```yaml
automation:
  coordinator:
    performer_heartbeat_interval_seconds: 15
    performer_stale_seconds: 180
    performer_dead_grace_seconds: 30
```

### 12.2 Liveness Sources

The monitor should combine:

1. heartbeat freshness,
2. process group existence,
3. event stream progress,
4. worktree file changes,
5. tool-specific runner state when available.

### 12.3 Stale Action Policy

```yaml
automation:
  coordinator:
    stale_action: block
    # block | requeue | reset | kill_and_requeue
```

Recommended default:

```text
block
```

Rationale: preserving partial work is safer than automatically killing or resetting unknown tool output.

---

## 13. Coordinator Stop Semantics

MACC should support explicit stop modes across all clients.

### 13.1 Stop Mode Matrix

| Mode | CLI | Dispatch New Tasks | Running Performers | Worktrees | Use Case |
|---|---|---:|---|---|---|
| Drain | `macc coordinator stop --drain` | No | Let current active tasks finish through merge/cleanup | Cleaned normally | Best clean shutdown. |
| Graceful | `macc coordinator stop --graceful` | No | Ask performers to stop at next safe boundary if supported | Preserved by default | Stop soon, avoid killing. |
| Force | `macc coordinator stop --force` | No | SIGTERM then SIGKILL process groups | Preserved by default | Hung or orphaned tools. |
| Force + Cleanup | `macc coordinator stop --force --cleanup-worktrees` | No | Kill process groups | Cleanup attempted | Emergency reset. |

---

## 14. Drain Mode: Stop After Current Tasks Finish

### 14.1 User Goal

The user wants to say:

> Once the tasks currently in progress have been committed and merged, stop.

### 14.2 Semantics

Drain mode should:

1. set coordinator mode to `draining`,
2. snapshot all currently active task claims,
3. disable new task dispatch,
4. continue advancing only the snapshotted tasks,
5. allow those tasks to complete dev/review/fix/merge/cleanup,
6. stop the coordinator once the drain set is empty.

Important: “currently running tasks” should mean active task claims, not merely currently running PIDs. A task may still need review, fix, merge, reconciliation, or cleanup after a performer process exits.

### 14.3 CLI

```bash
macc coordinator stop --drain
```

Optional alias:

```bash
macc coordinator drain
```

### 14.4 API

```http
POST /api/v1/coordinator/stop
Content-Type: application/json

{
  "mode": "drain"
}
```

### 14.5 TUI Label

```text
Drain & Stop
```

### 14.6 Web UI Label

```text
Stop after current tasks
```

### 14.7 Status Output

```json
{
  "coordinator_mode": "draining",
  "new_dispatch_enabled": false,
  "drain_started_at": "2026-05-26T12:00:00Z",
  "active_drain_tasks": 3,
  "will_exit_when_idle": true
}
```

### 14.8 Scheduler Rule

```rust
if coordinator_control.mode == CoordinatorMode::Draining {
    // Do not claim new tasks.
    // Continue only tasks in drain_snapshot.
    return DispatchDecision::DisabledByDrainMode;
}
```

---

## 15. Graceful Stop

### 15.1 Semantics

Graceful stop should:

1. disable new dispatch,
2. persist `mode=graceful_stopping`,
3. signal active performers to stop at a safe boundary when supported,
4. allow current phase result to be recorded if possible,
5. preserve worktrees by default,
6. exit the coordinator after active phases stop or timeout.

### 15.2 Difference Between Drain and Graceful

Drain means:

> Finish currently active tasks all the way through merge and cleanup.

Graceful means:

> Stop soon without killing, preferably at a safe phase boundary.

---

## 16. Force Stop

### 16.1 User Goal

The user has stopped the coordinator but tool processes remain alive. They need a reliable way to terminate all MACC-launched runtime processes.

### 16.2 Recommended Command

```bash
macc coordinator stop --force
```

### 16.3 Semantics

Force stop should:

1. set coordinator mode to `force_stopping`,
2. stop dispatch immediately,
3. enumerate active process groups from the runtime ledger,
4. send `SIGTERM` to each process group,
5. wait `force_grace_seconds`,
6. send `SIGKILL` to remaining process groups,
7. mark affected task runtime rows as `aborted_by_operator`,
8. preserve worktrees by default,
9. show next suggested recovery actions.

### 16.4 Why Process Groups, Not Single PIDs

A launched tool may create child processes:

- shell wrappers,
- `node` processes,
- `tee`,
- vendor binaries,
- sleeping helper processes,
- subprocesses spawned by the tool itself.

Killing only one PID can leave descendants alive.

Therefore MACC should launch every performer in its own process group/session and terminate the whole group.

### 16.5 Unix Implementation

When spawning performers:

```rust
use std::os::unix::process::CommandExt;

let mut cmd = std::process::Command::new("performer.sh");
unsafe {
    cmd.pre_exec(|| {
        libc::setsid();
        Ok(())
    });
}
```

Store:

```text
pid
process_group_id
session_id
```

Force stop:

```text
killpg(process_group_id, SIGTERM)
wait force_grace_seconds
killpg(process_group_id, SIGKILL)
```

### 16.6 Windows Implementation Direction

On Windows, use a Job Object for each performer tree.

Recommended behavior:

- create a Job Object per performer,
- assign the process to the Job Object,
- configure kill-on-job-close when force stopping,
- persist enough metadata to identify the launched process tree.

### 16.7 Force Stop Should Preserve Worktrees by Default

Do **not** automatically delete worktrees after force stop.

Rationale:

- the tool may have produced useful partial changes,
- the operator may need logs and diffs,
- automatic deletion can destroy recoverable work.

Recommended follow-up:

```bash
macc coordinator status
macc coordinator recover --dry-run
macc coordinator cleanup
```

### 16.8 Force + Cleanup

For emergency reset:

```bash
macc coordinator stop --force --cleanup-worktrees
```

This should require a stronger confirmation gate in TUI/Web because it combines process termination and filesystem cleanup.

---

## 17. Client Accessibility

All stop and recovery controls should be accessible from every client.

### 17.1 CLI

```bash
macc coordinator status
macc coordinator recover
macc coordinator recover --dry-run
macc coordinator ps
macc coordinator stop --drain
macc coordinator stop --graceful
macc coordinator stop --force
macc coordinator stop --force --cleanup-worktrees
macc coordinator unlock --task <id> --reason "operator reviewed stale worktree"
macc coordinator kill --task <id>
macc coordinator adopt --task <id>
```

### 17.2 TUI

Coordinator screen actions:

```text
Run
Pause Dispatch
Drain & Stop
Graceful Stop
Force Stop
Force Stop + Cleanup
Recover
Recover Dry Run
Open Logs
Inspect Processes
Unlock Task
Adopt Task
Kill Task
```

### 17.3 Web UI

Coordinator Console should include:

- current mode badge,
- active tasks table,
- stale tasks table,
- orphaned process table,
- drain countdown/progress,
- buttons for stop modes,
- force stop confirmation dialog,
- recovery dry-run view,
- links to task logs and worktree diffs.

Suggested labels:

```text
Stop after current tasks
Stop gracefully
Force stop tools
Force stop and cleanup
```

### 17.4 REST API

#### Stop

```http
POST /api/v1/coordinator/stop
```

Request:

```json
{
  "mode": "drain",
  "cleanup_worktrees": false,
  "force_grace_seconds": 10,
  "reason": "operator requested shutdown"
}
```

Response:

```json
{
  "ok": true,
  "coordinator_mode": "draining",
  "new_dispatch_enabled": false,
  "active_drain_tasks": 3
}
```

#### Recovery

```http
POST /api/v1/coordinator/recover
```

Request:

```json
{
  "dry_run": true
}
```

#### Process Listing

```http
GET /api/v1/coordinator/processes
```

Response:

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

---

## 18. Health, Readiness, and Status Endpoints

### 18.1 `/api/v1/health`

Purpose: cheap liveness/readiness check for scripts and monitoring.

```http
GET /api/v1/health
```

Response:

```json
{
  "status": "degraded",
  "version": "0.4.0",
  "uptime_seconds": 921,
  "db_ok": true,
  "coordinator": {
    "active": true,
    "mode": "draining",
    "run_id": "run_abc",
    "last_tick_at": "2026-05-26T12:00:00Z",
    "last_heartbeat_at": "2026-05-26T11:59:55Z",
    "active_tasks": 4,
    "stale_tasks": 1,
    "blocked_tasks": 0,
    "orphaned_processes": 0
  }
}
```

Recommended statuses:

```text
ok
starting
draining
degraded
unhealthy
stopped
```

### 18.2 `/api/v1/status`

Purpose: rich UI/dashboard state.

```http
GET /api/v1/status
```

Response:

```json
{
  "coordinator_mode": "draining",
  "new_dispatch_enabled": false,
  "tasks": {
    "todo": 12,
    "running": 4,
    "stale": 1,
    "blocked": 0,
    "merged": 28
  },
  "worktrees": {
    "idle": 2,
    "assigned": 4,
    "dirty": 1,
    "cleanup_pending": 0
  },
  "recovery": {
    "last_recovery_at": "2026-05-26T11:45:00Z",
    "last_recovery_result": "1 stale task blocked"
  },
  "processes": {
    "alive": 4,
    "stale": 1,
    "orphaned": 0
  },
  "throttled_tools": [],
  "effective_max_parallel": 4
}
```

---

## 19. Configuration Additions

Recommended additions to `.macc/macc.yaml`:

```yaml
automation:
  coordinator:
    # Runtime ledger and recovery
    runtime_ledger_enabled: true
    recovery_on_start: auto
    coordinator_lease_ttl_seconds: 45
    event_replay_max_events: 10000

    # Heartbeats and liveness
    performer_heartbeat_interval_seconds: 15
    performer_stale_seconds: 180
    performer_dead_grace_seconds: 30
    stale_action: block

    # Stop behavior
    default_stop_mode: drain
    force_grace_seconds: 10
    preserve_worktrees_on_force: true
    allow_force_cleanup: true

    # Process supervision
    launch_performers_in_process_group: true
    kill_process_group_on_force: true

    # Client/API visibility
    expose_processes_endpoint: true
    health_include_runtime_summary: true
```

---

## 20. Recommended Error Codes

Extend the existing coordinator/runtime error model with:

| Code | Name | Retryable | Meaning |
|---|---|---:|---|
| `E410` | Coordinator lease conflict | No | Another active coordinator owns the lease. |
| `E411` | Runtime ledger write failed | No | SQLite transition could not be persisted. |
| `E412` | Recovery classification failed | No | Startup recovery could not safely classify runtime state. |
| `E413` | Performer heartbeat stale | Depends | Performer has not emitted heartbeat within threshold. |
| `E414` | Performer process dead | Depends | Recorded process/process group no longer exists. |
| `E415` | Orphaned performer detected | No | Tool process exists without a valid active claim. |
| `E416` | Force termination failed | No | SIGTERM/SIGKILL or Windows Job termination failed. |
| `E417` | Dirty worktree blocks recovery | No | Partial changes require operator review. |
| `E418` | Stale event rejected | No | Event has old epoch or stale claim ID. |

---

## 21. Security and Safety Considerations

### 21.1 Force Stop Confirmation

Force stop is destructive at the process level and should require confirmation in TUI/Web.

Suggested confirmation copy:

```text
This will terminate all active MACC-launched tool processes.
Worktrees will be preserved for inspection.
```

For `--force --cleanup-worktrees`, require a stronger confirmation:

```text
Type FORCE CLEANUP to terminate active tools and clean MACC-managed worktrees.
```

### 21.2 Preserve Work by Default

The safest default is:

```text
kill processes, preserve worktrees
```

Cleanup should be an explicit second step or explicit flag.

### 21.3 Audit Logging

Every mutating action should be logged:

```text
.macc/log/ops.jsonl
```

Fields:

```json
{
  "timestamp": "2026-05-26T12:00:00Z",
  "client": "web",
  "action": "coordinator.stop",
  "mode": "force",
  "cleanup_worktrees": false,
  "status": "ok",
  "duration_ms": 1420
}
```

---

## 22. Suggested CLI UX

### 22.1 Status

```bash
macc coordinator status
```

Example:

```text
Coordinator: draining
Run ID: run_abc
Dispatch: disabled
Active drain tasks: 3
Running performers: 3
Stale performers: 0
Orphaned processes: 0
Will stop when current tasks are merged and cleaned.
```

### 22.2 Process Inspection

```bash
macc coordinator ps
```

Example:

```text
TASK    CLAIM      TOOL   PID      PGID     STATUS   HEARTBEAT    WORKTREE
T-042   claim_abc  codex  1292224  1292224  alive    12s ago      slot-02
T-043   claim_def  gemini 1292400  1292400  stale    4m ago       slot-03
```

### 22.3 Drain Stop

```bash
macc coordinator stop --drain
```

Output:

```text
Drain requested.
New dispatch is disabled.
3 active task claims will continue through merge and cleanup.
Coordinator will stop when the drain set is empty.
```

### 22.4 Force Stop

```bash
macc coordinator stop --force
```

Output:

```text
Force stop requested.
Dispatch disabled.
Sent SIGTERM to 3 process groups.
2 exited within 10s.
Sent SIGKILL to 1 remaining process group.
Marked 3 task runtimes as aborted_by_operator.
Worktrees were preserved for inspection.
Next: run `macc coordinator recover --dry-run`.
```

---

## 23. Implementation Plan

### Milestone 1 — Runtime Ledger

Deliverables:

- SQLite tables for coordinator runtime state.
- Transactional task claim and worktree assignment.
- Persisted event cursor.
- Runtime status separate from workflow state.

Acceptance criteria:

- Coordinator can restart and list previously active runtime claims.
- Claims include `claim_id`, `run_id`, `epoch`, worktree, PID, and phase.
- Dispatch cannot happen without a durable runtime row.

### Milestone 2 — Startup Recovery

Deliverables:

- `macc coordinator recover`.
- `macc coordinator recover --dry-run`.
- Recovery classification matrix.
- Git/worktree/process inspection.

Acceptance criteria:

- Coordinator does not dispatch before recovery completes.
- Dead performers are classified.
- Dirty worktrees are blocked, not deleted.
- Already-merged tasks are reconciled from commit history.

### Milestone 3 — Runtime Monitor

Deliverables:

- Heartbeat freshness checks.
- Process/process-group liveness checks.
- Stale task surfacing in CLI/TUI/Web/API.
- `stale_action` policy enforcement.

Acceptance criteria:

- Missing heartbeat marks task `heartbeat_stale`.
- Dead process marks task `process_dead`.
- Operator sees stale/orphaned tasks without reading logs.

### Milestone 4 — Stop Modes

Deliverables:

- `stop --drain`.
- `stop --graceful`.
- `stop --force`.
- `stop --force --cleanup-worktrees`.
- Coordinator mode persistence.
- Drain snapshot logic.

Acceptance criteria:

- Drain disables new dispatch but lets active tasks complete through merge.
- Graceful stop does not kill performers by default.
- Force stop kills process groups.
- Worktrees are preserved by default.

### Milestone 5 — Client and API Integration

Deliverables:

- API stop endpoint with modes.
- API process listing.
- Expanded `/health` and `/status`.
- TUI buttons.
- Web UI controls and confirmation dialogs.

Acceptance criteria:

- Same stop modes are available from CLI, TUI, Web UI, and API.
- `/health` reports degraded/stale/orphaned states.
- Web UI can trigger drain and force stop safely.

### Milestone 6 — Optional Standby Mode

Deliverables:

- Coordinator lease TTL.
- Standby loop.
- Epoch increment on takeover.
- Fencing validation.

Acceptance criteria:

- Only one active coordinator dispatches.
- Standby takes over after lease expiry.
- Stale coordinator events are rejected.

---

## 24. Acceptance Criteria

### 24.1 Crash Recovery

- If the coordinator crashes while performers are running, restart adopts or classifies them before dispatching new tasks.
- If the coordinator crashes after claiming a task but before spawning, the task is safely requeued or blocked.
- If the host reboots, previous runtime processes are marked dead and worktrees are inspected.
- If a task was already merged, reconciliation marks it merged and closes runtime state.

### 24.2 Liveness

- Performers emit heartbeats.
- Missing heartbeats are detected within configured thresholds.
- Stale tasks are visible in CLI, TUI, Web UI, `/health`, and `/status`.

### 24.3 Stop Modes

- `stop --drain` stops dispatch and exits only after current tasks finish through merge/cleanup.
- `stop --graceful` stops new dispatch and requests safe stop without killing by default.
- `stop --force` terminates all MACC-launched process groups.
- `stop --force --cleanup-worktrees` performs guarded cleanup after termination.

### 24.4 Orphan Handling

- MACC records performer PID and process group ID.
- MACC can list active/orphaned performer processes.
- MACC can terminate process groups, not just parent PIDs.
- Worktrees are preserved after force stop unless cleanup is explicitly requested.

### 24.5 Client Accessibility

- CLI, TUI, Web UI, and REST API expose equivalent coordinator actions.
- Web UI and TUI require confirmation for force stop and force cleanup.
- API responses return structured status and recommended next actions.

---

## 25. Recommended Spec Insertion

```md
### Coordinator Resilience & Stop Semantics

MACC coordinator must be crash-consistent. A coordinator crash must not imply task loss, double-dispatch, or permanent manual recovery.

#### Durable runtime ledger

The coordinator persists all runtime-critical state to SQLite at every state transition:

- active task claims,
- claim IDs and coordinator epoch,
- assigned worktree slots,
- resource locks,
- performer PID/process group,
- current phase,
- last heartbeat,
- event cursor,
- retry/recovery metadata.

SQLite writes must be transactional. A task claim, worktree assignment, lock acquisition, and performer spawn intent are recorded as one transition. On restart, MACC reconstructs runtime state from this ledger, event logs, Git history, and worktree inspection.

#### Startup recovery

Before dispatching new tasks, `macc coordinator` runs a recovery sweep:

1. acquire singleton coordinator lease,
2. load active runtime rows,
3. replay unread events,
4. verify performer process liveness,
5. inspect worktree cleanliness and branch state,
6. run deterministic PRD reconciliation,
7. classify active tasks as adopted, stale, failed, blocked, or complete.

#### Performer liveness

Performers emit heartbeat events every configured interval. The coordinator marks tasks as stale when no heartbeat is received within `performer_stale_seconds`. Stale tasks are surfaced in CLI, TUI, and Web UI, and handled according to `stale_action`.

#### Stop modes

MACC supports explicit coordinator stop modes across CLI, TUI, Web UI, and API:

- `stop --drain`: disable new dispatch and stop after currently active tasks complete through merge/cleanup.
- `stop --graceful`: disable new dispatch and request active performers to stop at a safe boundary without killing by default.
- `stop --force`: terminate all active MACC-launched performer process groups, preserving worktrees by default.
- `stop --force --cleanup-worktrees`: terminate performers and run guarded worktree cleanup.

#### Health and readiness

The Web API exposes:

- `GET /api/v1/health` for cheap liveness/readiness checks,
- `GET /api/v1/status` for detailed coordinator, task, worktree, process, throttle, and recovery state.

#### Optional standby

MACC may run a standby coordinator that takes over only after the active coordinator lease expires. Fencing tokens prevent stale coordinators from updating active claims.
```

---

## 26. Final Recommendation

Implement this motif in the following order:

1. **Runtime ledger first** — without durable state, every other feature remains best-effort.
2. **Startup recovery second** — never dispatch before classifying previous runtime state.
3. **Drain mode third** — this is the safest daily shutdown behavior.
4. **Process-group force stop fourth** — essential for real-world orphaned tools.
5. **Client/API integration fifth** — make the same controls available everywhere.
6. **Standby mode last** — useful, but only after fencing and recovery are reliable.

The recommended default operator behavior should be:

```bash
# Normal clean shutdown
macc coordinator stop --drain

# Hung/orphaned tools
macc coordinator stop --force
macc coordinator recover --dry-run

# Emergency reset
macc coordinator stop --force --cleanup-worktrees
```

The most important product decision is this:

> `--drain` should be the default safe shutdown path. `--force` should exist, but it should be explicit, process-group based, audited, and preserve worktrees by default.

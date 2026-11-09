# Performer monitoring and phase execution

The native coordinator keeps tests, reviews and correction phases in an owned
blocking worker, separate from its control-plane loop. Phase execution remains
serialized; each task still follows the configured validation and review order.
The loop continues consuming performer exits, runtime heartbeats and merge
results while a phase runs. It applies outcomes to a fresh registry snapshot,
rather than retaining a mutable snapshot throughout an external process call.

The idle wait listens for performer and merge completion channels, the active
phase's join handle, and an OS-exit notification. The existing 500 ms / 2 second
timers are maintenance fallbacks, not mandatory delays after a process exits.
A received completion is retained until its normal monitor applies it exactly
once. Exiting does not itself mean a task succeeded: terminal IPC evidence and
all existing validation, integrity and publication gates still apply.

Missing terminal IPC evidence is reconciled asynchronously, checking first at
50 ms and using increasing delays within the existing four-second grace window.
The OS exit wakes the loop before that reconciliation; one missing result does
not block monitoring another performer. Missing evidence after the window keeps
the existing protocol-failure behavior.

A phase is launched after the coordinator commits its current registry changes,
so its context preparation cannot be overwritten by the launcher's old snapshot.
Context preparation stays on the control plane; the process worker never saves a
whole registry snapshot. Testing/reviewing tasks retain their concurrency capacity
and exclusive worktree ownership while the loop continues dispatching other work.
A pending phase prevents completion, drain completion and dispatch-budget exit.
If the task's state, branch, base or worktree changes, its worker is cancelled and
the obsolete result is discarded. Force stop, signal shutdown and integrity or
merge pauses also cancel outstanding phases. Cancellation kills the phase's own
process group and reaps the child before worktree cleanup can proceed. Both
output pipes are drained concurrently to avoid blocking on large outputs.

This removes deliberate idle polling delays and phase-process waits from the
monitoring path. It is not a hard real-time guarantee: short registry operations,
Git commands, OS scheduling and client refresh intervals can still add latency.

Regression coverage lives in `control_plane/phase_job.rs` and
`control_plane/phase_runner.rs`: waking and retaining success/error/merge events,
processing a precondition failure during a held review, retaining that outcome
when the review finishes, cancelling stale phases, reaping process groups, and
draining large stdout/stderr output. These tests use local fixtures and no AI
provider or user project.

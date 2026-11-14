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
The running phase is persisted before context preparation. If a native pending
decision blocks the same task/claim/tool/branch/worktree, the worker is suspended
and emits `phase_suspended`. Its source files, phase, decision, raw output and
validation artifacts remain available. No interrupted result grants acceptance.
Other state/assignment changes still discard the obsolete outcome and emit
`phase_result_stale`. Force stop, signal shutdown and integrity or merge pauses
also cancel outstanding phases.

Cancellation first signals the runner so its cleanup trap can stop the provider
and finalize its fenced session lease. A three-second deadline bounds cleanup;
remaining processes in its group are then killed and reaped. Both output pipes
are drained concurrently, including on interruption. Raw evidence is captured
before a cancelled phase returns. A runner already exited cannot leave output
pipes holding the loop indefinitely.

Resolution resumes the retained phase with fresh native context. Older records
whose `current_phase` still names the preceding phase recover the interrupted
phase from the native decision-context pin. Resumed corrections run configured
tests and require a new independent review, including at the optional review
cycle limit. External evidence and normal publication gates remain mandatory.

Checkpoint previews distinguish blocking leases from dead owners whose leases
have expired. Recovery and checkpoint preparation quarantine only the latter;
live owners, PID reuse and unexpired leases cannot be evicted. Lease readiness
changes wake supervisor reevaluation. Delivered checkpoints are retired without
deleting their payloads or hashes, and are omitted from active drift diagnostics.
Queue completion retires them before replacing the current registry.

One native decision can block its owner and transitive consumers. Diagnostics
group these consumers below the causal root even when each carries E903. They
retain the requested action and unavailable capability. Supervisor handoff points
to the retained task worktree and exact decision; an unused diagnostic workspace
is not described as repaired work.

This removes deliberate idle polling delays and phase-process waits from the
monitoring path. It is not a hard real-time guarantee: short registry operations,
Git commands, OS scheduling and client refresh intervals can still add latency.

Regression coverage lives in `control_plane/phase_job.rs` and
`control_plane/phase_runner.rs`: waking and retaining success/error/merge events,
processing a precondition failure during a held review, retaining that outcome
when the review finishes, cancelling stale phases, reaping process groups, and
draining large stdout/stderr output. These tests use local fixtures and no AI
provider or user project.

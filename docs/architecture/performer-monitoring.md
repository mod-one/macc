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

## Decision context refresh across review and correction

A native `DM_STALE` receipt is a request for updated invocation context, not a
human question. Phase workers pass that fact through an owned atomic flag; a
model-written marker alone cannot schedule automatic recovery. The control plane
keeps a refresh cycle for fix/test/review just as for development. The existing
per-revision retry bound still applies, and a rejected recovery stops explicitly.

Dependency failures (`E905`) do not own a refresh or human wait. The coordinator
removes inherited `context_refresh` markers from these records and ignores them
when deciding whether the causal root needs another native sync cycle. Decision
sync cannot requeue a dependent or consume its refresh budget merely because its
dependency explanation quotes `DM_STALE`. A separate
unresolved external prerequisite still stops the engine for supervisor handoff;
rejected root recovery cannot keep an otherwise idle run alive indefinitely.

Decision-only recovery selects the latest checkpoint for the retained phase and
role. It requires the same worker, branch, HEAD, index/content digests, task
sources, plan and contract, with no unresolved native request or active lease.
Ambiguous multi-role legacy records, a missing/mismatched phase checkpoint and
uncheckpointed edits remain blocked with a checkpoint/phase/role/field diagnostic.
General task readiness also scopes its checks to the active role when known.

In particular, a review made before a correction must not be compared with the
corrected working tree to decide whether the correction can resume. Its payload,
hash and objections remain historical evidence. Recovery invalidates that
historical review for acceptance before updating the fix context, then the fresh
review invocation recaptures current code under the unchanged contract and a new
provider context. This does not reuse an old acknowledgement or approval. Source,
ownership, contract and lease guards remain in force; mandatory tests and the
fresh independent review still precede publication.

Clients distinguish an internal `context_refresh` sentinel from a decision ID.
Checkpoint recovery failures point to the incident evidence and native reconcile
preview, not to a device/approval request. Both the shared Web run summary and the
TUI pause overlay describe a retained phase needing context refresh.

The supervisor's automatic restart limit is scoped to semantic recovery evidence
for native context refresh incidents: interrupted task/role/phase, retained
worker, branch, HEAD, index/content and source hashes, and the **current applicable**
decision binding. A new applicable decision can therefore recover even before a
checkpoint is reconciled. Neither a regenerated checkpoint UUID, reconciliation
metadata, a new run/epoch nor an unrelated decision renews this budget. Updating
the stored binding to the current one does not grant a second restart for the
same decision. Other incident types keep their existing crash fingerprint limit.
Existing ledger entries are retained; the versioned native fingerprint permits
one migration recovery and then enforces the same one-restart bound.

When that bound is exhausted, the supervisor still runs the native exact-state
preview and records `checkpoint-recovery.json`, without reconciling checkpoints,
copying retained files or spending an AI diagnosis call. A refused preview records
`checkpoint-recovery-blocked.json` with the guard that failed. A valid but recurring
refresh reports `MACC-CONTEXT-REFRESH-RESTART-LIMIT` and an explicit retry action.
No recovery marks development complete or skips tests, independent review or
delivery validation.

Regression tests in `decisions/waiting.rs`, `decisions/tests_delegation.rs`,
checkpoint `recovery/tests.rs` and CLI
`supervisor_runtime/checkpoint_recovery.rs` cover legacy dependency markers,
current versus stored decision bindings, unrelated decisions, regenerated
checkpoints, retained development progress and non-mutating budget inspection.

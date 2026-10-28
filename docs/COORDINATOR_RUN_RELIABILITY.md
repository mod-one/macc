# Runner finalization and retained worktrees

## Streamed protocol and usage

A checkpoint acknowledgement proves the current task/phase was presented to a
fresh session. It is not interchangeable with a success marker. Output adapters
must preserve assistant protocol text across all turns, not just the last answer.
`UsageSpec.text_arrays` declares event selectors, a collection pointer, item
selectors and text pointers relative to each collection item. User, tool and
thinking blocks are not assistant protocol text. Scalar text projections remain
compatible for existing adapters. Numeric parsing still uses only the terminal
aggregate selected by `event_match`, never the concatenated assistant text.

An adapter may declare `required_for_checkpoint: true`. A live checkpoint then
requests its declared protocol output even in passive usage mode. This is a
protocol requirement, not an inferred token estimate. The current multi-turn
adapter uses stream JSON and verbose assistant events. The format declarations
live in ToolSpec, with no provider-name dispatch in the collector. The output
profile change conservatively rotates incompatible native sessions; retained Git
work stays on the same task branch. See [usage telemetry](USAGE_TELEMETRY.md).

## Checkpoint size and finalization

Initial checkpoints retain the strict `max_content_bytes` bound and fail before
provider execution when mandatory criteria cannot fit. Resumed checkpoints also
validate their mandatory context against that limit, while retained execution
evidence uses the current finish budget. The separate
`session_management.checkpoints.max_finish_content_bytes` defaults to 65536 bytes,
allows 2048..262144 and must be at least the start limit. It reserves space for
changed paths, retained objections and outcome evidence accumulated during work.
The starting checkpoint pins this finish limit, so an in-flight policy edit does
not shrink an invocation's allowance. Old starting checkpoints without this field
use the effective finish limit. Both limits are configurable through CLI session
policy, TUI Sessions settings and Web Setup → Sessions via the shared catalog.
No schema migration, silent limit override or mandatory-criteria truncation occurs.
An actual overflow remains an explicit persistence failure and preserves Git work.

## Retry and terminal events

A transient internal execution attempt with another attempt available emits
`progress/running`, not `phase_result/failed`. A terminal phase failure stops the
performer's retry loop, including when IPC acknowledgement fails. This prevents
an internal retry racing the coordinator's failure-grace process cleanup.

Output/checkpoint failures append their specific `MACC_TASK_RESULT_EXP` and a
`MACC_RUNNER_ERROR` code. Missing protocol output uses E103; infrastructure
finalization failures use E901. These do not become success merely because the
provider printed a success marker. Existing quota, precondition and tool-switch
semantics remain intact. Static prompt instructions use a quoted heredoc; task
paths and other dynamic context are printed as data outside it.

## Worktree activity

The shared read model distinguishes:

- `in_progress`: an assigned execution state with a live performer PID.
- `blocked`: a task reserves this slot; its unfinished work is retained.
- `waiting`: an assigned active workflow state without a live performer.
- `retained`: uncommitted work without a current blocking/active assignment.
- `idle`: a clean unassigned slot; not a claim that it may bypass reuse guards.
- `supervisor` / `integration`: supporting workspaces, not task performers.

CLI `macc worktree list`, TUI Home and Web worktrees use this interpretation.
Directory count is not concurrency. A blocked dirty slot must not be recycled for
a different task merely to maintain one directory per tool. No retention,
cleanup, commit reconciliation or task-acceptance rule is relaxed.

## Supervisor destinations and progress

The diagnosis has optional `macc_repair_required` and `external_prerequisites`
fields. Non-blocking MACC improvement hints do not prevent application repair.
A required product update instead publishes a stable incident report under
`.macc/reports/macc-defects/` with status `reported_unfixed`, the original evidence
location and the separate MACC source/runtime destination. It does not spend
application repair rounds trying to patch an installed MACC binary. Missing
devices/authority remain explicit waits. These diagnoses reuse the existing durable
wait policy; no automatic repeated diagnosis occurs without relevant changes.
Installing/restarting a different MACC executable invalidates the cached diagnosis,
including reinstalls of uncommitted builds. See [supervisor recovery](SUPERVISOR.md).

While an AI subprocess runs, the supervisor records PID, elapsed seconds and
stdout/stderr sizes every five seconds in `agent-status.json`, its intervention
report and live health. Empty output is explicitly described as awaiting first
output with unverified progress. Liveness/output volume never proves a successful
repair. Existing configured deadlines, process-group cleanup, tests and AI
verification gates remain in effect; no live project settings are changed.

Provider output reference: [Claude Code programmatic output](https://code.claude.com/docs/en/headless#stream-responses).

## Supervisor handoff after prerequisite repair

An authorized unblock reconciles retained worker content with the repaired reference,
then explicitly permits one dispatch into that same branch/worktree. Full content
and the original index are pinned in private recovery refs before any changes;
conflicts preserve the original worker and remain blocked. A supervisor repair
cannot copy an unfinished task into a generic repair commit or attest its delivery.
See [supervisor handoff](SUPERVISOR.md#retained-task-handoff-and-repair-traceability)
for recovery artifacts, retry semantics and the distinction between
`repair_integrated`, `restart_requested` and verified `recovered` execution.

## Revision-checked unblock commands

`macc coordinator unblock-task --task <id> --evidence <proof>` uses a control
request when the coordinator is alive. It carries a unique request ID and the
fingerprint of the task inspected by the client. The owning coordinator processes
it between lifecycle phases (also while paused), rechecks the fingerprint and
blocked state, reconciles the retained worktree, saves the state and then returns
an acknowledgement. Offline unblocks use the same repair/validation operation.
A live endpoint failure does not silently fall back to a competing registry write.

A repeated request ID with identical contents is acknowledged without applying the
transition again, even if the task has since advanced. A changed payload or stale
fingerprint is rejected. `task_unblocked` records remain informational audit events;
replaying them cannot reopen delivered work, erase a later failure or reset a PID.
The task retains a small `unblock_control` receipt; the next claim consumes the
existing `repair_resume` authorization as before.

The IPC transport has one five-second connect/write/read deadline, bounded replies
and numeric loopback addresses only. The server discards queued commands whose acknowledgement wait was cancelled
or whose queue deadline expired. A client timeout does not establish cancellation:
inspect the task before retrying, as an accepted repair may still complete. Async callers use `control::send().await`
or `spawn_blocking` for the synchronous service API so the listener and owner loop
remain schedulable. No provider or model is involved in an unblock request.

Checkpoint migration is automatic only between existing `worker-<digits>` or
`slot-<digits>` worktrees immediately under the project's `.macc/worktree/` pool.
Both must belong to the same repository and contain the same task in their scoped
PRD. Present MACC metadata must match the real branch and slot identity. Both slots
must have no active session lease. Moving to the main checkout or an unrelated
worktree requires explicit reconciliation, rather than a slot-name heuristic.

## Deterministic CLI regression fixtures

Adapter-only Apply/catalogue tests configure fixture tools explicitly and disable
native preparation/network checks in their temporary project. Tests of indexing
and provider preparation use their own initialized Git fixtures and controlled
processes; real provider installation remains opt-in. Local HTTP fixtures own and
join their server threads, bound request reads and I/O, and serve multiple requests
without leaking an accept loop. Tests should not depend on tools or cached GitHub
availability installed on the developer's machine.

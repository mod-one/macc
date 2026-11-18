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

## Negative reviews and retained recovery

An acknowledged, current review with `REVIEW_VERDICT: CHANGES_REQUESTED` and
explicit `pass`/`fail` evidence for every non-advisory binding completes as
`changes_requested`. Its findings remain in the native receipt and raw phase
artifacts. Both positive and negative reviews receive unchanged-source/clean-tree
postchecks and commit attestation. Negative receipts never authorize merge.
Missing evidence, missing/incorrect acknowledgements, stale context and
`needs_human_review` remain blocking, with distinct diagnostics.

The correction runs in the retained worker, followed by configured tests and
a new independent compliance review even at the ordinary review-cycle limit.
That final review still counts toward the bound: a further rejection preserves
the objection and stops at the correction budget instead of looping or merging.
After a runtime update or explicit retry, legacy native `compliance_failed`
receipts with exact task/attempt/digest and explicit failure evidence may request
a fresh review; neither the old verdict nor passing compliance is synthesized.

Startup recovery preserves the workflow phase, worker, branch and claim of tasks
between phases, including without a PID. It only clears dead process state; no
delivery result is inferred. Recorded blocks remain blocked for native recovery.
Review postchecks retain unexpected untracked files and reject the dirty review
instead of deleting those files.

Supervisor repair agents write only in the isolated repair worktree. Native
snapshots detect changes to the project or incident worker worktrees after both
repair and verification; all original files and indexes remain available on
refusal. Empty isolated diffs cannot become a repair commit or recovered result.
`repair_evidence` in the intervention report and `verified-repair.json` bind
verification to the actual worker, branch, HEAD, index tree, full content tree
and changed paths. Integration rechecks this snapshot before publishing. Model
prose about a repair elsewhere is insufficient. Such a failure names the native
fix/review handoff rather than treating the unchanged supervisor base as delivery.

## Source prerequisites and compliance objections

Native phase preconditions use the common coordinator protocol projection and
deduplicate repeated final answers. Missing conditions are reported as missing,
without inventing a checkpoint problem. A correction/test/review failure records
the same `out_of_scope_fix` paths as development and preserves its phase and
worktree. Test prerequisites block from both pre-test and testing states instead
of becoming an unrelated FSM error or an unchanged retry. After the source is
repaired, native unblock resumes the retained fix/test/review phase; a resumed
correction still needs a fresh independent review. The scope event names that
phase; source prerequisites point to repair
logs instead of an unrelated decision form.

Independent compliance objections retain decision IDs, versions, resolution
origins and concrete findings. The control plane projects a structured
`review_compliance_block` only from an acknowledged, current native review
receipt. Wrong task, attempt, plan, digest, stale context and accepted receipts
cannot supply that block. A subsequent failure or successful phase clears the
obsolete active projection; the underlying native receipt remains historical
evidence.

The supervisor rereads this authoritative receipt before diagnosing a governed
review block. When independent review requires governance, it records
`review-compliance-block.json` without spending a provider call, modifying source
or claiming an external device/credential is missing. Shared run summaries and
clients preserve the finding and route to the exact decision/version and its
governed revision. A negative check still cannot authorize compliance or merge.

Mandatory project requirements remain above implementation choices, including
delegated choices. Reviewer prompts distinguish implementation failures from
conflicts in the choice itself and describe the existing request/supersedes
protocol for proposing a compliant governed replacement. `never_ask` does not
waive requirements or fabricate an independent pass. Human-origin authority and
external approvals keep their existing safeguards.

Investigation reports distinguish checked source bytes from semantic validation.
No configured authoritative source evidence means no native source check was
performed; a list of investigated files is an agent declaration. Repeated policy
references to the same checked source consume the byte budget once. No
application-specific business rule is embedded in MACC's generic diagnostics.

## Provider process lifetime and checkpoint finalization

All shared adapter invocations, including the JSON session backend, pass through
the internal `runner-process` service. Its stdio and provider exit status remain
unchanged. The shell transports prompts/output; process ownership and shutdown
live in one native module rather than duplicated shell cleanup paths.

On Linux this dedicated, synchronous subprocess is a child subreaper. It adopts
orphaned tool processes even after `setsid` or a double fork. After provider exit
or cancellation it sends TERM, escalates to KILL after a bounded grace period,
and reaps every owned child before returning. It never signals a saved project
PID, an unrelated sibling or another invocation. The supervisor has its own
session and observes parent loss, so a killed runner shell does not abandon its
tools. Other Unix platforms retain process-group cleanup; detached adoption is
a Linux capability.

Only a completed lifetime boundary writes its per-call completion receipt.
The shared shell waits for that boundary, drains output, then saves the final
checkpoint and releases its fenced session lease. Missing receipts (including
a crashed or forcibly killed boundary) refuse checkpoint finalization with E901;
they cannot silently turn a moving worktree into a verified checkpoint.

`MACC-CHECKPOINT-WORKTREE-DRIFT` distinguishes retained content/index changes
from changed ownership or requirements. The diagnostic asks to stop invocation
tools and inspect late or external writes before reviewed reconciliation. An
old drift alone does not prove its writer, authorize automatic acceptance of
new bytes, waive requirements, or establish a missing device/approval. Existing
exact-state and contract guards remain in force.

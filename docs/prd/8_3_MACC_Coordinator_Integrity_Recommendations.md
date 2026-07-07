# MACC Coordinator Integrity & Recovery Recommendations

**Target branch:** `feat/coordinator`  
**Scope:** MACC coordinator, performer terminal events, IPC reliability, worktree reuse, unmerged commit protection, TUI/Web visibility, and recovery workflows.

---

## 1. Executive Summary

This document consolidates the recommendations for hardening MACC's coordinator workflow around one critical failure mode:

> A task can produce useful commits, fail to persist a valid terminal `phase_result` event, be classified as `failed` or `E901`, and then have its worktree reused or its branch abandoned automatically even though the work was never confirmed as merged.

The root issue is not only a JSON serialization bug. It is a broken trust chain between:

- the performer;
- the coordinator;
- the IPC terminal event contract;
- the task registry;
- the local worktree PRD state;
- the Git merge state;
- and the TUI/Web operator feedback loop.

The recommended default behavior is:

> If a task produces commits but MACC cannot confirm the terminal result through IPC, MACC must pause the coordinator, protect the commit, display a blocking alert, and wait for an explicit operator decision before continuing.

This favors integrity over silent progress.

---

## 2. Core Design Principle

MACC should prefer an explicit pause over silent loss of trust.

Avoiding deadlock is valuable, but avoiding silent loss or misclassification of unmerged work is more important.

Therefore:

1. Continue automatically only when integrity is certain.
2. Protect and pause when integrity is uncertain.
3. Require explicit operator action for destructive or potentially destructive recovery.
4. Make every protected, unconfirmed, or abandoned commit visible in the coordinator summary and TUI/Web.

---

## 3. Problem Chain

The observed failure chain can be summarized as follows:

1. The performer locally succeeds or produces commits.
2. The terminal `phase_result` payload is malformed, incomplete, or collapses to `{}`.
3. The coordinator rejects the terminal event, for example because `payload.result_kind` is missing.
4. The performer may still exit successfully or mark the local PRD as passed.
5. The coordinator does not have a valid terminal event and may classify the task as `E901`.
6. The task can be treated as `failed`.
7. The reuse logic may consider `failed` worktree slots safe to reset.
8. The branch may be tagged as abandoned and the worktree reused.
9. The TUI/Web may not clearly surface the root cause or the protected commits.
10. The final run status may appear successful or less severe than the actual integrity risk.

This creates a split-brain state:

- the local PRD may suggest the task passed;
- the registry may classify it as failed;
- Git may still contain unmerged useful work;
- the operator may not see the problem in the live UI.

---

## 4. Priority P0 Recommendations

### 4.1. Make Terminal Payload Construction Robust

#### Problem

The performer currently risks building invalid or degenerate JSON payloads when optional fields are constructed with fragile `jq` expressions such as:

```jq
field: ($value | select(length > 0))
```

If the selected value is empty, the parent object may collapse or omit required data in unexpected ways.

For a successful terminal event, `payload.result_kind` is not optional. A `phase_result status=done` event without `payload.result_kind` must never be accepted.

#### Recommendation

Build payloads with explicit conditional object merges instead of fragile inline `select` filters.

Recommended `jq` pattern:

```jq
{
  attempt: ($attempt | tonumber?),
  changed: $changed
}
+ (if $result_kind != "" then { result_kind: $result_kind } else {} end)
+ (if $result_exp != "" then { result_exp: $result_exp } else {} end)
```

Then enforce a strict invariant:

```text
phase_result status=done requires payload.result_kind
```

If the payload is missing `result_kind`, `emit_performer_event` must return failure. It must not replace the payload with `{}` and continue.

#### Acceptance Criteria

- No successful terminal event can be emitted without `payload.result_kind`.
- Empty optional fields do not remove required fields.
- A performer-side test verifies that an empty `result_exp` does not remove `result_kind`.
- Invalid terminal payloads fail loudly before being sent to the coordinator.

---

### 4.2. Fail Loudly When a Terminal IPC Event Is Rejected

#### Problem

`must_emit_performer_event` can correctly return non-zero when the coordinator rejects an IPC event. However, if the caller ignores that return code, the performer can continue and mark the task as passed locally.

This produces a dangerous split-brain:

- the performer thinks the task completed;
- the local PRD can be marked as passed;
- the coordinator rejects the terminal event;
- the registry records the task as failed or incomplete.

#### Recommendation

Every mandatory terminal event emission must be checked and treated as fatal on failure.

Recommended shell pattern:

```bash
if ! must_emit_performer_event "phase_result" "$CURRENT_PHASE" "done" "$payload"; then
  echo "Error: failed to persist terminal phase_result event" >&2
  exit 1
fi
```

Also apply this to failed terminal events when they are part of the coordinator contract.

The performer must not call `mark_task_passed` unless the terminal `phase_result` has been accepted by the coordinator or persisted through an explicitly valid fallback path.

#### Acceptance Criteria

- A rejected terminal IPC event causes a non-zero performer exit.
- `mark_task_passed` is never executed after a rejected terminal event.
- The local PRD cannot say `passed` while the coordinator rejected completion.
- The task log includes the IPC rejection cause.

---

### 4.3. Treat E901 as an Integrity Condition, Not a Routine Task Failure

#### Problem

`E901` means the performer exited without persisting a valid terminal `phase_result` event.

This is qualitatively different from a normal task failure. It indicates a contract failure between performer and coordinator, often caused by:

- a protocol mismatch;
- a serialization bug;
- an IPC rejection;
- a version mismatch;
- or a regression in the performer wrapper.

If `E901` occurs after a task produced commits, the coordinator must assume the work may be valuable and unconfirmed.

#### Recommendation

Classify `E901` as a blocking integrity condition when all of the following are true:

```text
task status is failed or blocked
+ last_error_code is E901
+ worktree has commits ahead of base
+ branch is not merged into the reference branch
```

When this condition is met, the coordinator must:

1. pause the run;
2. stop dispatching new tasks;
3. protect the commit;
4. surface a blocking notification in the TUI/Web;
5. require operator review.

Recommended user-facing message:

```text
Task produced commits but terminal result was not persisted; work was protected and requires operator review.
```

#### Acceptance Criteria

- `E901` with unmerged commits pauses the coordinator.
- No new task is dispatched after this condition is detected.
- The task appears in a `Needs operator review` section.
- The final run status cannot be `success` while unresolved `E901` tasks with unmerged commits exist.

---

### 4.4. Do Not Silently Reuse Worktrees That Contain Unmerged Commits

#### Problem

A failed worktree slot is not always safe to reset.

There is a critical difference between:

- a task that failed cleanly without producing commits;
- a task that failed after producing useful commits;
- a task that succeeded locally but failed to persist its terminal event.

Treating all `failed` tasks as non-blocking and reusable can silently discard or obscure useful work.

#### Recommendation

Introduce explicit failure categories:

```text
failed_clean
failed_with_unmerged_commits
protocol_failed_with_commits
```

Before reusing a worktree slot, the coordinator must calculate:

```text
has_commits_ahead_of_base
is_branch_merged_into_base
terminal_event_confirmed
abandonment_authorized
```

Automatic reuse must be forbidden when:

```text
has_commits_ahead_of_base == true
and is_branch_merged_into_base == false
and abandonment_authorized == false
```

In this case, the coordinator should block the slot and ask for operator review instead of resetting the worktree.

#### Acceptance Criteria

- A worktree with unmerged commits is never reset as a side effect of dispatching another task.
- A failed worktree without commits may still be reused automatically.
- A failed worktree with commits requires explicit recovery or abandonment.
- The coordinator summary lists blocked worktrees and protected commits.

---

### 4.5. Gate Branch Abandonment Behind the Destructive Actions Policy

#### Problem

Automatically tagging and resetting a branch with unmerged commits is a destructive or potentially destructive action, even if the commit is technically protected by a tag.

A protected tag reduces data loss risk, but it does not make the operation safe or obvious.

#### Recommendation

Abandoning or resetting any branch with unmerged commits must honor the existing destructive action policy.

For example:

```yaml
coordinator:
  destructive_actions: double_confirm
```

The coordinator should require explicit confirmation for:

- abandoning a branch with commits ahead of base;
- resetting a worktree containing unmerged commits;
- deleting or overwriting recovery branches;
- resuming after an integrity pause if the operator chooses to discard work.

#### Acceptance Criteria

- Automatic dispatch cannot implicitly abandon unmerged commits.
- Abandonment requires operator action unless explicitly configured otherwise.
- The action is logged with reason, SHA, branch, task ID, and operator decision.

---

## 5. Priority P1 Recommendations

### 5.1. Use Git Reachability as the Only Authority for `merged`

#### Problem

A task must not be considered merged based on local marks, PRD status, task IDs in commit messages, or other heuristics.

Only Git reachability should determine whether task work is merged.

#### Recommendation

A task may enter `merged` only if its branch or task commit is verified as reachable from the reference branch.

Invariant:

```text
A task may only reach merged if its branch or commit is a verified ancestor of the reference branch.
```

A function such as `is_branch_merged_into_base` should be the final authority.

The coordinator should reconcile the local worktree PRD from the registry after each job, not allow local PRD marks to override the registry.

#### Acceptance Criteria

- A task ID in a commit message is not enough to mark the task as merged.
- Local `passed` status is not enough to mark the task as merged.
- `merged` requires Git reachability from the configured reference branch.

---

### 5.2. Make IPC Rejections Visible in Coordinator Events

#### Problem

When the IPC listener rejects a performer event, the error can remain only in the performer log. The live coordinator feed, TUI, and Web UI may not clearly expose the root cause.

This makes the operator discover the issue too late, often only after noticing abandoned tags or inconsistent task states.

#### Recommendation

When the coordinator rejects a performer event, it should emit a coordinator-side diagnostic event.

Recommended event shape:

```json
{
  "type": "coordinator_diagnostic",
  "severity": "blocking",
  "code": "IPC_TERMINAL_EVENT_REJECTED",
  "message": "Rejected terminal performer event: payload.result_kind is required"
}
```

This event should appear in:

- the event feed;
- the TUI live screen;
- the Web live page;
- the final run summary.

#### Acceptance Criteria

- A terminal IPC rejection is visible without opening raw log files.
- The TUI/Web displays a persistent warning for blocking IPC errors.
- The event includes task ID, event type, phase, status, and rejection reason.

---

### 5.3. Rename Ambiguous Abandonment Tags

#### Problem

The tag prefix `macc/abandoned/...` may be technically correct in some cases, but it is misleading for protocol failures or unconfirmed work.

If the task produced commits and the terminal event failed, the work is not necessarily abandoned. It is unconfirmed or protected.

#### Recommendation

Use more precise tag categories:

```text
macc/protected/<task-id>-<timestamp>
macc/unconfirmed/<task-id>-<timestamp>
macc/recovery/<task-id>-<timestamp>
```

Recommended semantics:

- `protected`: work was preserved because automatic continuation was unsafe;
- `unconfirmed`: work exists but terminal completion was not accepted;
- `recovery`: work is part of an explicit recovery flow;
- `abandoned`: operator intentionally abandoned the work.

Each tag event should include:

```text
SHA
source branch
task ID
reason
merge status
recommended next action
```

#### Acceptance Criteria

- Protocol failure cases do not use `abandoned` by default.
- The operator can distinguish protected work from intentionally abandoned work.
- The final summary includes all protected/unconfirmed/recovery tags.

---

### 5.4. Improve TUI/Web Visibility

#### Problem

The live interface must show integrity problems as soon as they occur. A blocking issue should not be discoverable only by reading task logs or Git tags.

#### Recommendation

Add a visible `Needs operator review` section in the TUI/Web.

This section should include tasks with:

```text
last_error_code = E901
or severity = blocking
or status = failed with unmerged commits
or protected/unconfirmed commits
```

The live overview should show counters such as:

```text
blocked: 1
needs_review: 1
protected_commits: 1
```

Each task should provide quick access to:

- task details;
- performer logs;
- coordinator diagnostic events;
- protected commit diff;
- recovery actions.

#### Acceptance Criteria

- Blocking coordinator events trigger persistent notifications.
- `E901` appears in a dedicated review section.
- Protected commits are listed with SHA and branch.
- The operator can navigate directly from the alert to the task detail.

---

### 5.5. Clarify Final Run Status

#### Problem

A run should not finish with `result: success` if there are unresolved blocked tasks, unconfirmed commits, or protected branches.

#### Recommendation

Use explicit final statuses:

```text
success
paused
blocked
partial_failure
needs_operator_review
```

Recommended meanings:

- `success`: all required tasks are confirmed and merged or intentionally classified;
- `paused`: execution stopped waiting for operator input;
- `blocked`: execution cannot continue safely;
- `partial_failure`: some tasks failed, but no integrity-sensitive work is at risk;
- `needs_operator_review`: work was protected but not confirmed.

#### Acceptance Criteria

- `success` is impossible if unresolved `E901` tasks with unmerged commits remain.
- The final summary lists all protected and unconfirmed branches.
- The final summary explains the next required operator action.

---

### 5.6. Add Conservative Continuation Policies

#### Recommendation

Add explicit coordinator configuration flags, enabled by default:

```yaml
coordinator:
  pause_on_unconfirmed_commits: true
  stop_on_unmerged_task_failure: true
```

Recommended activation condition:

```text
task failed or blocked
+ commits ahead of base
+ branch not merged
+ terminal result missing or rejected
```

Optionally introduce an integrity mode:

```yaml
coordinator:
  integrity_mode: strict # strict | balanced | permissive
```

Recommended behavior:

- `strict`: pause immediately on terminal contract failure;
- `balanced`: pause only if unmerged commits exist;
- `permissive`: continue, but still protect work and surface warnings.

Default recommendation:

```yaml
coordinator:
  integrity_mode: balanced
  pause_on_unconfirmed_commits: true
  stop_on_unmerged_task_failure: true
```

#### Acceptance Criteria

- Default configuration prevents silent worktree reuse with unmerged commits.
- Advanced users can opt into more permissive behavior explicitly.
- The selected integrity policy appears in the run summary.

---

## 6. Priority P2 Recommendations

### 6.1. Add Guided Recovery Actions

#### Recommendation

The TUI/Web should provide guided recovery actions for tasks in `Needs operator review`.

Recommended actions:

```text
Inspect protected commit
Show diff
Merge protected commit
Retry terminal event from log
Mark task accepted manually
Retry task on same worktree
Retry task on new worktree
Abandon intentionally
Resume coordinator
```

#### Action Semantics

##### Inspect protected commit

Show metadata:

- SHA;
- source branch;
- task ID;
- produced files;
- commit message;
- reason for protection.

##### Show diff

Display the diff between the protected commit/branch and the reference branch.

##### Merge protected commit

Attempt to merge the protected commit or branch into the reference branch, subject to normal merge gates.

##### Retry terminal event from log

Reconstruct and retry the terminal event only if enough validated information exists in the log.

##### Mark task accepted manually

Allow operator override only with explicit reason and audit trail.

##### Retry task on same worktree

Reuse the same worktree only if no uncommitted or unprotected changes will be lost.

##### Retry task on new worktree

Start from the reference branch while preserving the original worktree/branch.

##### Abandon intentionally

Requires destructive action confirmation and should create an audit event.

##### Resume coordinator

Allowed only after the blocking condition is resolved or explicitly overridden.

#### Acceptance Criteria

- Every recovery action writes an audit event.
- Destructive actions require confirmation.
- The operator can resolve `needs_operator_review` without manually inspecting hidden files.

---

### 6.2. Add Tests and Runtime Invariants

#### Performer Tests

1. **Terminal payload keeps `result_kind` when `result_exp` is empty**

Input:

```text
result_kind = success_with_changes
result_exp = ""
```

Expected:

```json
{
  "payload": {
    "result_kind": "success_with_changes"
  }
}
```

2. **Terminal IPC rejection exits non-zero**

Simulate coordinator negative ACK.

Expected:

```text
performer exit code != 0
```

3. **Rejected terminal event does not mark PRD as passed**

Expected:

```text
mark_task_passed is not called
```

#### Coordinator Tests

1. **E901 with commits ahead pauses the run**

Input:

```text
last_error_code = E901
has_commits_ahead_of_base = true
is_branch_merged_into_base = false
```

Expected:

```text
run state = paused or needs_operator_review
no further dispatch
```

2. **Failed clean worktree can be reused**

Input:

```text
state = failed
has_commits_ahead_of_base = false
```

Expected:

```text
worktree reusable = true
```

3. **Failed worktree with unmerged commits cannot be reused automatically**

Input:

```text
state = failed
has_commits_ahead_of_base = true
is_branch_merged_into_base = false
```

Expected:

```text
worktree reusable = false
requires operator review
```

4. **Merged requires Git reachability**

Input:

```text
task contains ID in commit message
but branch is not reachable from reference branch
```

Expected:

```text
task state != merged
```

#### Runtime Invariants

```text
A successful terminal phase_result must contain payload.result_kind.
```

```text
A task may only reach merged if its branch or commit is a verified ancestor of the reference branch.
```

```text
A task with commits ahead of base may only leave a worktree through merge or explicitly authorized abandonment.
```

```text
A rejected terminal event cannot lead to local PRD passed status.
```

```text
A run cannot finish as success while protected or unconfirmed commits remain unresolved.
```

---

## 7. Recommended Implementation Plan

### Step 1 — Performer Hardening

- Replace fragile `jq` payload construction with conditional object merges.
- Validate `payload.result_kind` for successful terminal events.
- Make terminal IPC rejection fatal.
- Prevent `mark_task_passed` after terminal event failure.
- Add performer tests for empty optional fields and IPC rejection.

### Step 2 — Coordinator Integrity Guard

- Detect `E901` with commits ahead of base.
- Reclassify this condition as `needs_operator_review` or `paused`.
- Stop dispatching new tasks after integrity failure.
- Prevent automatic worktree reuse when unmerged commits exist.

### Step 3 — Git Merge Authority

- Make Git reachability the only authority for `merged`.
- Remove or downgrade commit-message matching heuristics.
- Reconcile local PRD state from the registry after jobs.

### Step 4 — Protected Commit Flow

- Replace ambiguous `abandoned` tags for protocol failure cases.
- Add `protected`, `unconfirmed`, and `recovery` tag categories.
- Include SHA, branch, task ID, reason, and next action in events.

### Step 5 — TUI/Web Visibility

- Emit coordinator-side diagnostic events for IPC rejection.
- Add persistent blocking notifications.
- Add `Needs operator review` section.
- Display blocked/protected/unconfirmed counters.

### Step 6 — Guided Recovery

- Add recovery actions in the TUI/Web.
- Gate destructive recovery actions behind `destructive_actions`.
- Log every operator decision.

### Step 7 — Final Status Semantics

- Prevent `success` when unresolved protected work exists.
- Add `paused`, `blocked`, `partial_failure`, and `needs_operator_review` statuses where appropriate.
- Include recovery summary in final run output.

---

## 8. Recommended Default Behavior

The safest default behavior is:

```text
If a task produced commits but MACC cannot confirm the terminal result through IPC:

1. protect the commit;
2. pause the coordinator;
3. stop further dispatch;
4. display a blocking TUI/Web alert;
5. classify the task as needs_operator_review;
6. require explicit operator recovery or abandonment;
7. prevent the run from ending as success until resolved.
```

Recommended configuration defaults:

```yaml
coordinator:
  integrity_mode: balanced
  pause_on_unconfirmed_commits: true
  stop_on_unmerged_task_failure: true
  destructive_actions: double_confirm
```

---

## 9. Final Checklist

### Performer Payload & IPC

- [ ] Replace fragile `jq select(length > 0)` payload fields with conditional object merges.
- [ ] Ensure `phase_result status=done` always includes `payload.result_kind`.
- [ ] Reject successful terminal events with missing or empty `result_kind`.
- [ ] Ensure `MACC_TASK_RESULT: success_with_changes` injects `result_kind = success_with_changes`.
- [ ] Make terminal `must_emit_performer_event` failure fatal.
- [ ] Prevent `mark_task_passed` if the terminal event was rejected.
- [ ] Log IPC rejection details in the task log.
- [ ] Add tests for empty `result_exp` preserving `result_kind`.
- [ ] Add tests for IPC negative ACK causing non-zero performer exit.

### Coordinator Integrity Handling

- [ ] Detect `E901` as a performer/coordinator contract failure.
- [ ] Detect commits ahead of base for failed or blocked tasks.
- [ ] Detect whether the task branch is merged into the reference branch.
- [ ] Pause on `E901 + commits ahead + not merged`.
- [ ] Stop dispatching new tasks after this integrity condition.
- [ ] Add `needs_operator_review` or equivalent state.
- [ ] Prevent final `success` while unresolved integrity conditions exist.

### Worktree Reuse & Branch Protection

- [ ] Distinguish `failed_clean` from `failed_with_unmerged_commits`.
- [ ] Distinguish `protocol_failed_with_commits` from normal task failure.
- [ ] Forbid automatic worktree reuse when unmerged commits exist.
- [ ] Require explicit operator authorization before abandoning unmerged work.
- [ ] Apply `destructive_actions` to branch abandonment and destructive reset.
- [ ] Preserve protected work with a clear tag or recovery branch.
- [ ] Include SHA, branch, task ID, reason, and next action in protection events.

### Merge Authority

- [ ] Make Git reachability the only authority for `merged`.
- [ ] Prevent task IDs in commit messages from marking tasks as merged.
- [ ] Prevent local PRD `passed` from marking tasks as merged.
- [ ] Reconcile local PRD state from the registry after each job.
- [ ] Add invariant tests for `merged` requiring reference branch reachability.

### TUI/Web Visibility

- [ ] Emit coordinator-side diagnostic events when IPC rejects performer events.
- [ ] Add persistent TUI/Web notifications for blocking severity.
- [ ] Add a `Needs operator review` section.
- [ ] Display counters for `blocked`, `needs_review`, and `protected_commits`.
- [ ] Show protected commit SHA and source branch in the live UI.
- [ ] Link alerts directly to task details, logs, diffs, and recovery actions.

### Guided Recovery

- [ ] Add `Inspect protected commit` action.
- [ ] Add `Show diff` action.
- [ ] Add `Merge protected commit` action.
- [ ] Add `Retry terminal event from log` action.
- [ ] Add `Mark task accepted manually` action with audit reason.
- [ ] Add `Retry task on same worktree` action.
- [ ] Add `Retry task on new worktree` action.
- [ ] Add `Abandon intentionally` action gated by `destructive_actions`.
- [ ] Add `Resume coordinator` action after resolution.
- [ ] Audit every recovery action.

### Final Run Status & Summary

- [ ] Add or use explicit final statuses: `success`, `paused`, `blocked`, `partial_failure`, `needs_operator_review`.
- [ ] Prevent `success` if protected or unconfirmed commits remain.
- [ ] Include protected/unconfirmed/recovery tags in the final summary.
- [ ] Include recommended next operator action in the final summary.
- [ ] Show selected `integrity_mode` and continuation policy in the run summary.

### Configuration

- [ ] Add `pause_on_unconfirmed_commits: true`.
- [ ] Add `stop_on_unmerged_task_failure: true`.
- [ ] Optionally add `integrity_mode: strict | balanced | permissive`.
- [ ] Default to `balanced` or `strict`, not permissive.
- [ ] Ensure permissive mode still protects work and surfaces warnings.

---

## 10. Recommended First Pull Request Scope

For a first focused implementation PR, prioritize:

1. robust performer payload construction;
2. fatal terminal IPC failure;
3. no `mark_task_passed` after rejected terminal event;
4. coordinator pause on `E901 + commits ahead + not merged`;
5. visible coordinator diagnostic event;
6. tests for the above.

This first PR would address the core integrity bug without requiring the full guided recovery UI to be completed immediately.

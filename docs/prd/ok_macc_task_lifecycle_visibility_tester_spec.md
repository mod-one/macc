# MACC Task Lifecycle & Visibility Specification

> **Document status:** Proposal  
> **Scope:** Coordinator, TUI, Web UI, task registry, performer/tester/reviewer roles  
> **Language:** English  
> **Related motif:** Task Lifecycle & Visibility  
> **Primary goal:** Make ongoing task execution understandable without forcing the user to tail raw logs.

---

## 1. Executive Summary

MACC currently exposes task progress mainly through coarse lifecycle states such as:

```text
todo → claimed → done/failed
```

This is not sufficient for a multi-agent, multi-worktree coding system. A task may be claimed but not actually running, running but blocked, testing, retrying, rate-limited, waiting for review, or stale. Users need to know what is happening now, who owns the task, where it is running, and what the next action is.

This proposal introduces a structured **Task Lifecycle & Visibility Layer** with four major improvements:

1. **A clearer runtime model**
   - Separate durable workflow state from live runtime status and current phase.

2. **A streamlined Live Tasks TUI**
   - Replace raw timestamp-heavy rows with compact operator-friendly rows.

3. **A first-class optional Tester role**
   - Add a dedicated testing/verification phase between Performer and Reviewer, controlled by configuration.

4. **Explainability commands and streams**
   - Add `macc explain <task-id>`, `macc diff <task-id>`, task log streaming, and structured timeline events.

The recommended lifecycle becomes:

```text
Performer → [optional Tester] → [optional Reviewer] → Merge
```

Testing and review must both be independently configurable.

---

## 2. Problem Statement

The current Live Tasks output is too close to raw registry/log data.

Example of current style:

```text
\ NOY-L5-WINWIDGET-001 [claimed|running|dev] tool=claude hb=2026-05-24T23:33:59Z updated=2026-05-24T23:28:26Z
```

Alternative example:

```text
\ worker-03 : NOY-L5-WINWIDGET-001 [status] tool=claude Started=2026-05-24T23:33:59Z updated=2026-05-24T23:28:26Z
```

Problems:

- The row is log-like instead of dashboard-like.
- Full ISO timestamps are visually expensive.
- `updated` is ambiguous.
- `[claimed|running|dev]` exposes internal structure but is hard to scan.
- Worker ownership is not visually prominent enough.
- There is no human-readable “current activity.”
- Users still need logs to understand what the performer is doing.
- Testing and review phases are not treated as independently configurable pipeline stages.

---

## 3. Design Goals

### 3.1 User-facing goals

MACC should let the user answer these questions immediately:

- Which worker is handling this task?
- What is the task ID?
- Is the task healthy?
- Is it running, waiting, blocked, stale, rate-limited, or failed?
- What phase is it in?
- Which tool is being used?
- How long has it been running?
- Is the heartbeat fresh?
- What is the current activity?
- Where are the logs, diff, timeline, and worktree?

### 3.2 System goals

The implementation should:

- Preserve the existing coordinator architecture.
- Avoid overloading task workflow state with transient runtime details.
- Make task execution replayable through structured events.
- Support both TUI and Web UI.
- Support future SQLite-backed task/event storage.
- Keep raw logs out of the task registry.
- Allow testing and review to be enabled, disabled, or made policy-driven independently.

---

## 4. Core Model: Three Layers of Task State

MACC should separate task state into three distinct layers.

```text
task.state           = durable workflow lifecycle
task_runtime.status  = live process/runtime condition
task_runtime.phase   = current work phase
```

### 4.1 Durable workflow state

The workflow state answers:

> Where is this task in the business process?

Recommended values:

```text
todo
queued
claimed
in_progress
testing
changes_requested
reviewing
pr_open
merged
failed
abandoned
```

Notes:

- This field should be durable.
- It should survive process restarts.
- It should be safe to reconcile from PRD, registry, and commit history.
- It should not change for every small runtime event.

### 4.2 Runtime status

The runtime status answers:

> What is the execution process doing right now?

Recommended values:

```text
idle
dispatched
starting
running
waiting
blocked
phase_done
retry_scheduled
rate_limited
failed
stale
completed
```

This field is operational and may change frequently.

### 4.3 Runtime phase

The phase answers:

> What type of work is currently being performed?

Recommended values:

```text
reading_context
planning
implementing
editing
testing
fixing
reviewing
committing
opening_pr
waiting_ci
merging
cleanup
```

For compact TUI display, use shorter aliases:

```text
ctx
plan
dev
edit
test
fix
review
commit
pr
ci
merge
clean
```

---

## 5. Proposed Task Runtime Object

Each task should have a runtime object stored in the registry or derived from runtime storage.

```json
{
  "id": "NOY-L5-WINWIDGET-001",
  "state": "claimed",
  "task_runtime": {
    "status": "running",
    "phase": "dev",
    "message": "Editing WinWidget client shell",
    "progress": 42,
    "worker_id": "worker-03",
    "tool": "claude",
    "worktree": ".macc/worktree/worker-03",
    "branch": "ai/claude/noy-l5-winwidget-001",
    "run_id": "run-001",
    "stdout_log": ".macc/log/performer/NOY-L5-WINWIDGET-001/run-001.stdout.log",
    "stderr_log": ".macc/log/performer/NOY-L5-WINWIDGET-001/run-001.stderr.log",
    "events_log": ".macc/log/performer/NOY-L5-WINWIDGET-001/run-001.events.jsonl",
    "started_at": "2026-05-24T23:33:59Z",
    "last_heartbeat": "2026-05-24T23:34:07Z",
    "last_event_at": "2026-05-24T23:34:12Z",
    "registry_updated_at": "2026-05-24T23:34:12Z"
  }
}
```

### 5.1 Field semantics

| Field | Meaning |
|---|---|
| `status` | Live process condition: running, waiting, stale, failed, etc. |
| `phase` | Current work phase: dev, test, review, merge, etc. |
| `message` | Human-readable current activity. |
| `progress` | Optional approximate progress, 0–100. |
| `worker_id` | Worker slot currently handling the task. |
| `tool` | Tool assigned to the current phase. |
| `worktree` | Worktree path for inspection and diff. |
| `branch` | Task branch. |
| `run_id` | Unique execution attempt identifier. |
| `stdout_log` | Path to performer stdout log. |
| `stderr_log` | Path to performer stderr log. |
| `events_log` | Path to structured task events. |
| `started_at` | Runtime start timestamp. |
| `last_heartbeat` | Last heartbeat emitted by worker/tool. |
| `last_event_at` | Last meaningful task event. |
| `registry_updated_at` | Last registry write time. |

### 5.2 Avoid ambiguous fields

Avoid displaying a generic field named:

```text
updated
```

Prefer explicit names:

```text
last_event_at
last_status_update
registry_updated_at
last_heartbeat
```

In the compact TUI, use relative time:

```text
age 5m
hb 8s
last 12s
```

Full timestamps belong in the expanded task details view.

---

## 6. Live Tasks TUI Redesign

### 6.1 Recommended compact row

Canonical compact row:

```text
● worker-03  NOY-L5-WINWIDGET-001  RUN dev  claude  age 5m  hb 8s  Editing WinWidget client shell
```

Column structure:

```text
<health> <worker>  <task-id>  <runtime> <phase>  <tool>  <age>  <heartbeat>  <current-message>
```

Example table:

```text
LIVE TASKS
Health  Worker     Task ID                  Status      Tool     Age    HB     Current activity
●       worker-03  NOY-L5-WINWIDGET-001    RUN dev     claude   5m     8s     Editing WinWidget client shell
●       worker-01  API-AUTH-014            RUN test    codex    12m    3s     Running pnpm test
▲       worker-02  DB-MIGRATION-003        ERR test    gemini   18m    2m     Test failed: migration snapshot mismatch
◐       worker-04  UI-NAV-009              WAIT review claude   31m    44s    Waiting for reviewer
!       worker-05  UI-FORM-021             STALE dev   claude   42m    9m     No heartbeat beyond stale threshold
```

### 6.2 Narrow terminal format

For narrow terminals:

```text
● worker-03 NOY-L5-WINWIDGET-001 RUN/dev claude age 5m hb 8s — Editing WinWidget client shell
```

### 6.3 Expanded row

When the user selects a task:

```text
Task:      NOY-L5-WINWIDGET-001
Worker:    worker-03
Tool:      claude
State:     claimed
Runtime:   running
Phase:     dev
Started:   2026-05-24T23:33:59Z
Heartbeat: 8s ago
Last event: 12s ago
Worktree:  .macc/worktree/worker-03
Branch:    ai/claude/noy-l5-winwidget-001
Message:   Editing WinWidget client shell
```

### 6.4 Full internal state in details only

The compact row should not display:

```text
[claimed|running|dev]
```

Instead, show:

```text
RUN dev
```

Then expose the full split in details:

```text
state=claimed
runtime=running
phase=dev
```

This preserves correctness while reducing visual load.

---

## 7. Health and Status Display Conventions

### 7.1 Health symbols

| Symbol | Meaning |
|---|---|
| `●` | Healthy active task |
| `◐` | Waiting or paused |
| `▲` | Warning or failed phase |
| `!` | Stale heartbeat or blocked |
| `✓` | Completed |
| `·` | Idle or no active runtime |

Color may be used in the TUI, but the symbols must remain meaningful in monochrome terminals.

### 7.2 Compact runtime labels

| Label | Runtime status |
|---|---|
| `RUN` | running |
| `WAIT` | waiting |
| `BLK` | blocked |
| `RETRY` | retry scheduled |
| `RATE` | rate-limited |
| `STALE` | stale |
| `ERR` | failed |
| `DONE` | completed |

### 7.3 Compact phase labels

| Label | Phase |
|---|---|
| `ctx` | reading context |
| `plan` | planning |
| `dev` | implementation |
| `edit` | editing |
| `test` | testing |
| `fix` | fixing |
| `review` | reviewing |
| `commit` | committing |
| `pr` | opening pull request |
| `ci` | waiting for CI |
| `merge` | merging |
| `clean` | cleanup |

---

## 8. TUI Interaction Model

Recommended Live Tasks shortcuts:

```text
Enter  Open task details
l      Toggle live logs pane
d      Show task diff
e      Explain task timeline
r      Retry failed task
s      Stop task
f      Filter tasks
/      Search task ID or message
```

### 8.1 Split-pane layout

Recommended layout:

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ LIVE TASKS                                                                    │
├────────┬───────────┬───────────────────────┬──────────┬───────┬─────┬──────┤
│ Health │ Worker    │ Task                  │ Status   │ Tool  │ Age │ HB   │
├────────┼───────────┼───────────────────────┼──────────┼───────┼─────┼──────┤
│ ●      │ worker-03 │ NOY-L5-WINWIDGET-001  │ RUN dev  │ claude│ 5m  │ 8s   │
│ ▲      │ worker-02 │ DB-MIGRATION-003      │ ERR test │ gemini│ 18m │ 2m   │
└────────┴───────────┴───────────────────────┴──────────┴───────┴─────┴──────┘
┌──────────────────────────────────────────────────────────────────────────────┐
│ SELECTED TASK DETAIL                                                          │
│ Editing WinWidget client shell                                                │
│ Worktree: .macc/worktree/worker-03                                            │
│ Branch: ai/claude/noy-l5-winwidget-001                                        │
└──────────────────────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────────────────────┐
│ LIVE LOGS                                                                     │
│ 23:34:03 Running focused validation...                                        │
│ 23:34:07 Updated src/widgets/winwidget.ts                                     │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 8.2 Display priority

The compact row should prioritize:

1. Health
2. Worker
3. Task ID
4. Runtime status
5. Phase
6. Tool
7. Age
8. Heartbeat freshness
9. Current activity

Full timestamps, worktree paths, branch names, and raw registry fields should be shown only in detail views.

---

## 9. Structured Task Events

MACC should use structured task events as the canonical visibility layer.

### 9.1 Event schema

```json
{
  "id": "evt_01HYZ...",
  "task_id": "NOY-L5-WINWIDGET-001",
  "run_id": "run-001",
  "worker_id": "worker-03",
  "timestamp": "2026-05-24T23:34:12Z",
  "severity": "info",
  "source": "performer",
  "event_type": "phase_progress",
  "phase": "dev",
  "message": "Editing WinWidget client shell",
  "metadata": {
    "files_changed": 2,
    "tool": "claude"
  }
}
```

### 9.2 Recommended event types

```text
task_claimed
phase_started
phase_progress
phase_completed
phase_skipped
command_started
command_completed
command_failed
file_summary
test_summary
commit_created
pr_created
merge_started
merge_conflict
retry_scheduled
heartbeat
status_message
artifact_created
task_failed
task_completed
```

### 9.3 Recommended severities

```text
debug
info
notice
warn
error
fatal
```

### 9.4 Recommended sources

```text
coordinator
performer
tester
reviewer
merge_worker
git
doctor
web
tui
operator
```

### 9.5 Event storage

Initial storage may be JSONL:

```text
.macc/log/events.jsonl
.macc/log/performer/<task-id>/<run-id>.events.jsonl
```

Future storage should be SQLite-backed:

```text
.macc/state/macc.db
```

JSONL should remain available for human debugging and simple tailing.

---

## 10. Performer Streaming

Each performer run should produce stable log files.

Recommended structure:

```text
.macc/log/performer/
  NOY-L5-WINWIDGET-001/
    run-001.stdout.log
    run-001.stderr.log
    run-001.events.jsonl
    run-001.summary.json
```

A worker-centric alias may also be maintained:

```text
.macc/log/performer/
  worker-03/
    current.stdout.log
    current.stderr.log
    current.events.jsonl
```

### 10.1 Log registry pointers

The task runtime should point to the active logs:

```json
{
  "task_id": "NOY-L5-WINWIDGET-001",
  "runtime": {
    "stdout_log": ".macc/log/performer/NOY-L5-WINWIDGET-001/run-001.stdout.log",
    "stderr_log": ".macc/log/performer/NOY-L5-WINWIDGET-001/run-001.stderr.log",
    "events_log": ".macc/log/performer/NOY-L5-WINWIDGET-001/run-001.events.jsonl"
  }
}
```

The registry should store paths and summaries, not raw logs.

---

## 11. `macc explain <task-id>`

`macc explain` should be the flight recorder for a task.

```bash
macc explain NOY-L5-WINWIDGET-001
```

Example output:

```text
NOY-L5-WINWIDGET-001 — Implement WinWidget client shell

State: claimed
Runtime: running
Phase: dev
Tool: claude
Worker: worker-03
Worktree: .macc/worktree/worker-03
Branch: ai/claude/noy-l5-winwidget-001

Timeline
23:33:59  info   claimed          Assigned to worker-03
23:34:01  info   ctx              Loaded PRD, standards, and scope
23:34:16  info   plan             Generated implementation plan
23:35:04  info   dev              Editing WinWidget client shell
23:39:22  info   test             Running focused validation
23:40:11  warn   test             Validation failed: snapshot mismatch
23:40:18  info   fix              Returning to performer for fix
```

### 11.1 Recommended options

```bash
macc explain <task-id> --json
macc explain <task-id> --since 30m
macc explain <task-id> --severity warn,error
macc explain <task-id> --logs
macc explain <task-id> --artifacts
macc explain <task-id> --compact
```

### 11.2 Data sources

The command should read from:

```text
.macc/state/task_registry.json or SQLite
.macc/log/events.jsonl
.macc/log/coordinator/*.jsonl
.macc/log/performer/<task-id>/*.events.jsonl
.macc/log/performer/<task-id>/*.log
git log / MACC commit trailers
```

---

## 12. `macc diff <task-id>`

`macc diff` should show a task’s changes without requiring the user to `cd` into the worktree.

```bash
macc diff NOY-L5-WINWIDGET-001
```

Recommended options:

```bash
macc diff <task-id>
macc diff <task-id> --stat
macc diff <task-id> --name-only
macc diff <task-id> --cached
macc diff <task-id> --base main
macc diff <task-id> --format patch
macc diff <task-id> --open
```

### 12.1 Active worktree diff

For active tasks:

```bash
git -C <task_worktree> diff <base_branch>...HEAD
```

### 12.2 Merged or cleaned-up task diff

If the worktree no longer exists but the task has commits:

```bash
git diff <base> <task_commit_sha>
```

### 12.3 Example output

```text
NOY-L5-WINWIDGET-001 — Implement WinWidget client shell

Worktree: .macc/worktree/worker-03
Branch: ai/claude/noy-l5-winwidget-001
Base: main

Diff stat
 src/widgets/winwidget.ts        | 42 +++++++++++++++++++++
 src/widgets/winwidget.test.ts   | 31 +++++++++++++++
 src/client/window-shell.tsx     | 12 +++++-
 3 files changed, 82 insertions(+), 3 deletions(-)
```

---

## 13. Optional Tester Role

### 13.1 Recommendation

MACC should introduce a first-class **Tester** role, but it should not be mandatory for every task.

The preferred lifecycle is:

```text
Performer → [optional Tester] → [optional Reviewer] → Merge
```

The Tester phase should have its own prompt, output contract, permissions, and phase configuration.

### 13.2 Role responsibilities

#### Performer

```text
Implements the task.
May run quick validation.
Produces code changes and commits.
```

#### Tester

```text
Validates behavior.
Runs project validation commands.
Adds or improves tests when permitted.
Reproduces failures.
Produces structured test evidence.
Returns PASS, FAIL, or BLOCKED.
```

#### Reviewer

```text
Reviews architecture, quality, maintainability, security, and PRD alignment.
Uses Tester evidence as input when available.
Approves or requests changes.
```

### 13.3 Why Tester should be separate from Reviewer

A Reviewer asks:

```text
Is this implementation correct, maintainable, secure, idiomatic, and aligned with the PRD?
```

A Tester asks:

```text
Can this implementation be proven to work, and what breaks when we exercise it?
```

These are related but distinct forms of quality assurance.

---

## 14. Tester Prompt Contract

The Tester prompt should instruct the agent to:

```text
1. Read the task, PRD, acceptance criteria, changed files, and implementation summary.
2. Identify expected behavior and likely regression zones.
3. Run the project's standard validation commands.
4. Run focused tests for the changed area.
5. Add missing tests only when permitted by configuration.
6. Avoid unrelated refactors or broad source changes.
7. Produce a structured test report.
8. Return PASS, FAIL, or BLOCKED.
```

### 14.1 Tester write permissions

By default, the Tester should be constrained.

Allowed:

```text
- Run tests.
- Inspect logs.
- Inspect diffs.
- Add missing tests when permitted.
- Produce failure reports.
- Request fixes.
```

Restricted by default:

```text
- Large source edits.
- Architecture changes.
- Broad refactors.
- Dependency changes.
- Formatting unrelated files.
```

The Tester should not become “Performer 2.”

---

## 15. Tester Output Contract

Recommended structured output:

```json
{
  "role": "tester",
  "task_id": "NOY-L5-WINWIDGET-001",
  "decision": "fail",
  "summary": "Focused widget lifecycle tests fail after window minimize/restore.",
  "commands": [
    {
      "cmd": "pnpm test -- winwidget",
      "status": "failed",
      "duration_seconds": 41
    }
  ],
  "failures": [
    {
      "type": "regression",
      "file": "src/widgets/winwidget.test.ts",
      "message": "Expected widget state to persist after restore."
    }
  ],
  "tests_added": [
    "src/widgets/winwidget.lifecycle.test.ts"
  ],
  "recommended_action": "Return to performer for fix."
}
```

### 15.1 Tester decisions

Recommended values:

```text
pass
fail
blocked
```

### 15.2 Tester transition behavior

If Tester passes:

```text
test → review
```

or, when review is disabled:

```text
test → merge
```

If Tester fails:

```text
test → fix → test
```

If Tester is blocked:

```text
test → blocked
```

or operator intervention, depending on policy.

---

## 16. Testing Phase Configuration

Testing must be independently configurable, just like review.

### 16.1 Recommended configuration

```yaml
automation:
  coordinator:
    phases:
      testing:
        enabled: true
        mode: risk_based
        required_for:
          - feature
          - bugfix
          - auth
          - security
          - migration
        skip_for:
          - docs
          - chore
        max_attempts: 2
        can_write_tests: true
        can_modify_source: false

      review:
        enabled: true
        mode: required
        max_attempts: 2
```

### 16.2 Disable testing

```yaml
automation:
  coordinator:
    phases:
      testing:
        enabled: false
```

### 16.3 Disable both testing and review

Useful for fast local or solo mode:

```yaml
automation:
  coordinator:
    phases:
      testing:
        enabled: false
      review:
        enabled: false
```

Lifecycle:

```text
dev → merge
```

### 16.4 Testing modes

Recommended modes:

```text
disabled
required
risk_based
manual
```

| Mode | Meaning |
|---|---|
| `disabled` | Always skip Tester phase. |
| `required` | Always run Tester after Performer. |
| `risk_based` | Run Tester only for configured categories, changed files, or risk signals. |
| `manual` | Run Tester only when the operator triggers it. |

### 16.5 Review modes

Recommended modes:

```text
disabled
required
risk_based
manual
```

Testing and review must be independently controlled.

---

## 17. Coordinator Transition Logic

### 17.1 Phase plan

The coordinator should not hard-code one lifecycle. It should compute a phase plan from configuration.

```text
After dev succeeds:

if testing.enabled:
    next = test
else if review.enabled:
    next = review
else:
    next = merge
```

```text
After test succeeds:

if review.enabled:
    next = review
else:
    next = merge
```

```text
After test fails:

next = fix
```

```text
After fix succeeds:

if testing.enabled:
    next = test
else if review.enabled:
    next = review
else:
    next = merge
```

```text
After review succeeds:

next = merge
```

```text
After review requests changes:

next = fix
```

### 17.2 Skipped phase events

When a phase is disabled, MACC should emit an explicit event.

```json
{
  "type": "phase_skipped",
  "task_id": "NOY-L5-WINWIDGET-001",
  "phase": "test",
  "reason": "disabled_by_config",
  "severity": "info"
}
```

This keeps `macc explain` understandable.

---

## 18. CLI Overrides

One-off coordinator runs should support temporary overrides.

Recommended flags:

```bash
macc coordinator --disable-testing
macc coordinator --disable-review
macc coordinator --testing=required
macc coordinator --testing=risk-based
macc coordinator --review=required
macc coordinator --review=manual
```

Runtime CLI flags should override `.macc/macc.yaml`.

The TUI/Web UI should indicate when a runtime override is active.

---

## 19. TUI Settings

Automation settings should expose phase toggles clearly.

```text
Phases
[x] Development
[x] Testing
[x] Review
[x] Merge

Testing mode: risk_based
Review mode: required
```

When testing is disabled:

```text
Phases
[x] Development
[ ] Testing
[x] Review
[x] Merge

Testing mode: disabled
```

When both testing and review are disabled:

```text
Phases
[x] Development
[ ] Testing
[ ] Review
[x] Merge

Pipeline: dev → merge
```

### 19.1 Live Tasks behavior when testing is disabled

If testing is disabled, Live Tasks should never show:

```text
RUN test
```

unless the Performer itself is running internal validation during the dev phase. In that case, display:

```text
RUN dev  Running quick validation
```

not:

```text
RUN test
```

The `test` phase should be reserved for the dedicated Tester role.

---

## 20. Semantic Distinction: Dedicated Testing vs Local Validation

Disabling the Tester phase means:

```text
Do not launch a separate Tester role.
Do not require structured tester evidence.
Do not block merge on tester result.
```

It does not necessarily mean:

```text
Never run tests.
```

The Performer may still run quick validation inside the development phase if standards, prompts, or local conventions require it.

This distinction is important:

| Scenario | Dedicated Tester phase? | Local test command allowed? |
|---|---:|---:|
| `testing.enabled=true` | Yes | Yes |
| `testing.enabled=false` | No | Yes, inside performer/dev |
| `review.enabled=false` | Irrelevant | Yes |

---

## 21. Web API Additions

Recommended endpoints:

```text
GET  /api/v1/registry/tasks
GET  /api/v1/registry/tasks/{id}
GET  /api/v1/registry/tasks/{id}/events
GET  /api/v1/registry/tasks/{id}/logs
GET  /api/v1/registry/tasks/{id}/diff
GET  /api/v1/registry/tasks/{id}/explain
GET  /api/v1/tasks/{id}/stream
POST /api/v1/registry/tasks/{id}/retry
POST /api/v1/registry/tasks/{id}/stop
POST /api/v1/registry/tasks/{id}/run-testing
POST /api/v1/registry/tasks/{id}/run-review
```

### 21.1 SSE task stream

```text
GET /api/v1/tasks/{id}/stream
```

Should stream:

```text
heartbeat
phase_started
phase_progress
phase_completed
phase_skipped
command_started
command_completed
command_failed
test_summary
review_summary
task_failed
task_completed
```

### 21.2 Diff endpoint

```text
GET /api/v1/registry/tasks/{id}/diff?format=stat
GET /api/v1/registry/tasks/{id}/diff?format=patch
```

### 21.3 Explain endpoint

```text
GET /api/v1/registry/tasks/{id}/explain
```

Should return structured timeline data suitable for both TUI and Web UI.

---

## 22. Web UI Recommendations

The Web UI should mirror the TUI model but provide richer inspection.

### 22.1 Task detail tabs

Recommended tabs:

```text
Overview
Timeline
Logs
Diff
Test Report
Review Report
Artifacts
```

### 22.2 Live task cards

Example card:

```text
worker-03 · claude
NOY-L5-WINWIDGET-001

RUN dev · age 5m · hb 8s
Editing WinWidget client shell

[Open logs] [Diff] [Explain] [Stop]
```

### 22.3 Pipeline display

For a task:

```text
dev ✓ → test skipped → review running → merge pending
```

When testing is disabled:

```text
dev ✓ → test skipped by config → review running → merge pending
```

When both are disabled:

```text
dev ✓ → test skipped by config → review skipped by config → merge pending
```

---

## 23. Rust View Model

The TUI and Web API should use a normalized view model.

```rust
pub struct LiveTaskRow {
    pub health: TaskHealth,
    pub worker_id: String,
    pub task_id: String,
    pub workflow_state: TaskState,
    pub runtime_status: RuntimeStatus,
    pub phase: TaskPhase,
    pub tool: String,
    pub age: Duration,
    pub heartbeat_age: Option<Duration>,
    pub last_event_age: Option<Duration>,
    pub current_message: Option<String>,
    pub worktree: Option<PathBuf>,
    pub branch: Option<String>,
}
```

### 23.1 Display rule

Compact display uses:

```text
health + worker_id + task_id + runtime_status + phase + tool + age + heartbeat_age + current_message
```

Expanded display uses all fields.

---

## 24. Implementation Plan

### Phase 1 — Data model and display cleanup

Add or normalize:

```text
task_runtime.status
task_runtime.phase
task_runtime.message
task_runtime.worker_id
task_runtime.tool
task_runtime.worktree
task_runtime.branch
task_runtime.started_at
task_runtime.last_heartbeat
task_runtime.last_event_at
```

Update Live Tasks compact row:

```text
● worker-03  NOY-L5-WINWIDGET-001  RUN dev  claude  age 5m  hb 8s  Editing WinWidget client shell
```

### Phase 2 — Structured events

Add:

```text
.macc/log/events.jsonl
.macc/log/performer/<task-id>/<run-id>.events.jsonl
```

Implement event emission from coordinator and performer wrappers.

### Phase 3 — Explain and diff commands

Add:

```bash
macc explain <task-id>
macc diff <task-id>
macc task logs <task-id> --follow
```

### Phase 4 — Optional Tester role

Add config:

```yaml
automation:
  coordinator:
    phases:
      testing:
        enabled: true
        mode: risk_based
```

Add Tester prompt and structured output contract.

### Phase 5 — TUI/Web phase controls

Expose:

```text
Testing enabled/disabled
Testing mode
Review enabled/disabled
Review mode
```

Show skipped phases explicitly in timeline.

### Phase 6 — SQLite indexing

Keep JSONL logs, but index task events in SQLite for fast query and replay.

---

## 25. Migration Strategy

### 25.1 Existing tasks

For older tasks without runtime details:

```json
{
  "task_runtime": {
    "status": "idle",
    "phase": null,
    "message": null
  }
}
```

### 25.2 Existing logs

Existing coordinator and performer logs can remain valid.

`macc explain` should degrade gracefully:

```text
No structured timeline found.
Showing available registry state and log pointers.
```

### 25.3 Default config

Initial default should be conservative:

```yaml
automation:
  coordinator:
    phases:
      testing:
        enabled: false
        mode: disabled
      review:
        enabled: true
        mode: required
```

Alternative default for stricter projects:

```yaml
automation:
  coordinator:
    phases:
      testing:
        enabled: true
        mode: risk_based
      review:
        enabled: true
        mode: required
```

Recommended final default:

```yaml
automation:
  coordinator:
    phases:
      testing:
        enabled: true
        mode: risk_based
      review:
        enabled: true
        mode: required
```

This adds value without forcing every trivial task through a full Tester role.

---

## 26. Acceptance Criteria

### 26.1 Live Tasks

```text
- Live Tasks shows worker, task ID, runtime status, phase, tool, age, heartbeat age, and current message.
- Live Tasks uses relative time by default.
- Live Tasks does not show full ISO timestamps in compact rows.
- Live Tasks does not show raw `[claimed|running|dev]` in compact rows.
- Expanded task details show full workflow state, runtime status, phase, timestamps, worktree, branch, and log paths.
- Stale heartbeat is visually distinct.
- Failed phase is visually distinct.
- Waiting and blocked states are visually distinct.
```

### 26.2 Task events

```text
- Coordinator emits task_claimed, phase_started, phase_completed, phase_skipped, and task_completed events.
- Performer emits heartbeat and status_message events.
- Tester emits test_summary events.
- Reviewer emits review_summary events.
- Events include timestamp, severity, source, phase, message, and metadata.
- Events are written to JSONL.
```

### 26.3 Explain command

```text
- `macc explain <task-id>` prints a chronological task timeline.
- `macc explain <task-id> --json` returns machine-readable output.
- Timeline includes skipped phases and skip reasons.
- Timeline includes tester and reviewer decisions when available.
- Command works even if performer logs exist but structured events are incomplete.
```

### 26.4 Diff command

```text
- `macc diff <task-id>` resolves the task worktree automatically.
- `macc diff <task-id> --stat` shows a diff summary.
- `macc diff <task-id>` falls back to commit-based diff if the worktree was cleaned.
- User does not need to change directories to inspect task changes.
```

### 26.5 Tester role

```text
- Tester has a dedicated prompt.
- Tester produces structured PASS, FAIL, or BLOCKED output.
- Tester can be enabled or disabled independently of Review.
- Tester can run in disabled, required, risk_based, or manual mode.
- Tester can be configured to write tests or remain read-only.
- Tester failure transitions task back to fix.
- Tester success transitions task to review or merge depending on review config.
```

### 26.6 Phase configuration

```text
- `testing.enabled=false` skips the Tester phase entirely.
- `review.enabled=false` skips the Reviewer phase entirely.
- If both are disabled, successful dev transitions directly to merge.
- If testing is disabled but review is enabled, successful dev transitions to review.
- If testing is enabled but review is disabled, successful test transitions to merge.
- Live Tasks never displays phase=test when dedicated testing is disabled.
- `macc explain <task-id>` shows “testing skipped by config” as an event.
```

---

## 27. Risks and Mitigations

### 27.1 Risk: Tester increases cost and runtime

Mitigation:

```text
- Make Tester policy-driven.
- Support disabled, manual, and risk_based modes.
- Allow cheaper/faster tools for Tester.
- Use deterministic local test commands before invoking an AI Tester.
```

### 27.2 Risk: Tester becomes another Performer

Mitigation:

```text
- Restrict Tester permissions.
- Default to can_modify_source=false.
- Allow can_write_tests=true separately.
- Require structured test evidence.
```

### 27.3 Risk: Too much UI noise

Mitigation:

```text
- Compact rows show only operational essentials.
- Full timestamps move to expanded details.
- Logs are hidden unless toggled.
- Timeline and raw logs remain available on demand.
```

### 27.4 Risk: Event duplication or inconsistency

Mitigation:

```text
- Define a canonical event schema.
- Use run_id and event IDs.
- Treat registry runtime fields as the latest projection of the event stream.
```

---

## 28. Final Recommendation

MACC should adopt the following model:

```text
Task workflow state = durable lifecycle
Runtime status      = live process condition
Phase               = current work type
Events              = replayable explanation layer
Logs                = raw evidence
```

The user-facing task pipeline should be:

```text
Performer → [optional Tester] → [optional Reviewer] → Merge
```

The default Live Tasks row should be:

```text
● worker-03  NOY-L5-WINWIDGET-001  RUN dev  claude  age 5m  hb 8s  Editing WinWidget client shell
```

The most important rule:

> Live Tasks should show what the operator needs now. Full timestamps, registry internals, raw logs, and detailed state transitions belong in task details, `macc explain`, `macc diff`, and the Logs view.

This makes MACC more understandable, more debuggable, and closer to a real autonomous engineering control plane without turning it into a heavy CI/CD orchestrator.

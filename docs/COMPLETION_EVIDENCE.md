# Durable task completion

MACC records delivery on the coordinator's reference branch. Clearing transient
coordinator state must not erase evidence that a task was delivered.

## Runner contract

For `already_satisfied` and `success_without_changes`, the tool must verify the
acceptance criteria and emit `MACC_TASK_RESULT_EXP` with the criteria checked,
commands executed and their results. Missing evidence or contradictory source
changes fail validation (`E904`). Gate tasks also require
`MACC_TASK_GATE_VERDICT`.

The runner establishes Git evidence before publishing terminal success:

- If a matching `[macc:task ID]` commit is already on the reference branch and
  the current revision is published, attach the verified task to its SHA.
- Otherwise create a validation commit, possibly empty, with task ID, original
  result, evidence, tool, task specification hash and optional agent gate verdict.
- An unpublished validation commit follows the existing phase/review/merge
  workflow. The terminal event uses `success_with_changes` to select that lane
  and preserves the tool's original result in `reported_result_kind`.
- A retry reuses the current unpublished validation commit if task specification
  and gate verdict match. A new gate decision requires a new durable proof.
- A worktree PRD `passes` flag alone cannot complete a task. Its shortcut requires
  an existing published MACC task commit.

Commit failures remain Git errors (`E201`); success is never emitted first.
Validation commits use `--only` and do not stage unrelated files.

## Startup and queued PRDs

Before distributing tasks, the coordinator loads the active PRD and reconciles
MACC commits on the reference branch. This runs for **each** queued PRD, so a
manual sync on the previous PRD is no longer required to protect the next one.
Reconciled tasks retain `completion_commit_sha`. Normal published MACC delivery
commits and legacy MACC task subjects remain compatible with existing sync.

Startup treats a published MACC delivery commit as historical delivery evidence;
it does not rerun an AI agent or the acceptance suite for every historical task.
Fresh no-change runner results require the tool's current verification evidence.
Use distinct task IDs when a new PRD changes the scope of an already delivered task.

Agent gate decisions recorded in validation commits survive state recreation.
Human approval gates are excluded from automatic commit reconciliation: Git tags
cannot recreate a person's approval.

## Migration

The behavior applies after building and installing the updated MACC binary.
Reused worktrees receive the updated embedded performer and completion helper at
launch. Existing running processes continue using the code they started with.
Previously uncommitted no-change successes have no Git evidence to recover;
a future run verifies them and creates their validation commits.

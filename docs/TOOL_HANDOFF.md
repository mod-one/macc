# Preserve task work when changing tools

When a performer hits quota exhaustion (`E602`) or a rate/overload limit
(`E601`), its task keeps the **same worktree and branch** for continuation.
The replacement tool receives the existing task and implementation; MACC does
not reset the branch, clean files or move the task to another worker slot.

## What is retained

- Commits on the task branch.
- Staged changes in the Git index.
- Unstaged changes in tracked files.
- Untracked implementation files.
- The worktree assignment across coordinator cleanup and restart/recovery.

A limit can interrupt work before the first commit. Worktree retention therefore
does not depend on finding commits ahead of the reference branch. Parked retry
worktrees remain reserved and cannot be sanitized for another task.

## Availability and dispatch

The exhausted tool's cooldown remains separate from the task's implementation
retry budget. MACC can select another available enabled tool immediately and
resume in the attached worktree. If all eligible tools are throttled, MACC waits
using their existing availability timers and countdowns.

Configured automatic retries also retain a healthy worktree, with their existing
retry budgets. If a retained worktree or its expected branch is unavailable, or
Git has a lock/unfinished operation, dispatch stops with recovery context. MACC
refuses to acquire a fresh slot for that continuation. If an implementation retry
budget is exhausted with uncommitted work remaining, recovery preserves the
assignment and blocks for review. Such recovery slots remain reserved.

Conversation IDs belong to each tool. A tool switch selects the replacement's
own session or starts a new one; it does not reuse another provider's session ID.
The branch and files remain the shared source of implementation context.

## Review, correction and interrupted processes

Review and correction phases follow the same preservation rule. A provider
limit does not trigger a hard reset or file cleanup. The task keeps its phase,
branch and worktree; a task-specific fallback takes precedence over the configured
coordinator tool. The actual phase provider receives the cooldown. If every
enabled provider is unavailable, continuation waits for the earliest expiry.
This pending phase assignment survives coordinator startup recovery.

The replacement tool is recorded consistently in `.macc/tool.json` and
`.macc/worktree.json`, preserving the branch and base metadata. Old provider
conversation IDs are not reused by a new provider.

If a performer dies before sending a terminal result, recovery also checks for
uncommitted source work. It retains the assignment and blocks for review instead
of making that worktree available to another task.

## Replacement performer contract

The continuation prompt includes previous commits and `git status` for retained
staged, unstaged and untracked work. The performer first inspects the current
implementation and completes the remaining task, preserving correct prior work.
If the inherited uncommitted implementation already satisfies the task after
validation, it reports `success_with_changes` so the runner commits and delivers
those changes. It must not report a clean no-change result while source edits
still await delivery.

The behavior requires an updated MACC binary and embedded performer. Running
processes continue with their existing code until restarted. Clearing or deleting
project/worktree files explicitly is outside this handoff guarantee.

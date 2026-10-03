# Supervisor intervention lifecycle

The supervisor keeps a durable record per coordinator incident in
`.macc/state/supervisor-interventions.json`. A completed diagnosis is reused for
transient repair retries. An external condition, failed validation, rejected
verification or configuration problem waits for relevant evidence to change.
The watchdog does not repeatedly ask an AI to diagnose unchanged evidence.

Relevant changes include task error/precondition state, project HEAD/source
changes, configured tools or supervisor settings, and persisted tool availability.
A documented provider reset time permits one further diagnosis for unchanged
incident evidence. Reset expiry permits rechecking; it does not prove that quota
has been restored. Unknown deadlines and human decisions wait for new evidence
or an explicit `macc supervisor start --retry` after stopping the supervisor.
Provider classification uses the existing adapter normalizers.

Only classified transient tool/network/timeout failures consume automatic retry
attempts. Budgets belong to each incident and evidence context. Attempt log
numbers remain monotonic, including after an explicit retry, so existing logs
are never overwritten. Older conclusive external diagnoses can be imported from
the latest report without another AI invocation.

## Worktrees and evidence

Supervisor worktrees live in `.macc/worktree/supervisor-01`, `supervisor-02`, etc.
An ownership marker reserves a slot for its incident. Retries of that incident
reuse it with its index, working files and untracked changes intact. A clean
slot can be updated to a new project HEAD only when all its commits are already
integrated. A changed base with unintegrated work requires operator recovery.
Coordinator worker dispatch and bulk worker cleanup exclude these slots.

Each attempt keeps its prompt `.txt`, stdout/stderr `.log`, validation logs and
report under `.macc/log/supervisor/<incident>-attempt-N/`. Before closing a slot,
MACC copies its local `.macc` evidence (including diagnosis, verification and
repair notes), generated `.txt`/`.log` files, Git status and binary diff into that
attempt's archive. Git internals, dependency trees and symlink targets are
excluded. Archiving failure prevents removal.

A closed, clean slot is removed only if its HEAD is an ancestor of the current
project HEAD. Dirty files or unintegrated commits retain the slot for recovery.
A transient retry retains its slot until the next attempt or budget exhaustion.
Logs remain after worktree removal. Existing worktrees created by older binaries
under log directories are not automatically deleted or moved.

## Explicit stop

An attached supervisor watches coordinator control independently of AI execution.
A current operator force/graceful stop or terminal user stop cancels supervision
and any active agent invocation. Stale stop requests from an earlier run and
unexpected coordinator crashes do not authorize this shutdown. Drain requests
allow the coordinator to finish its active work; the attached supervisor exits
when the run reaches a user-stopped terminal state.

`macc supervisor stop` also writes a PID-specific cooperative stop request before
sending its signal. Shutdown marks an active intervention interrupted and archives
available evidence. Interrupted slots with unfinished work remain available for inspection; clean slots are closed after archiving. An
explicit operator stop never initiates another intervention or coordinator restart.

Changes require an updated MACC binary; already running supervisors retain the
code with which they started.

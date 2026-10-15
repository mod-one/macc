# P0-A: provider session isolation

P0-B now adds an explicit [SQLite ownership service](SESSION_STORE.md). The JSON
contract below remains the compatibility backend until migration is applied.

## Decision and scope

The first O2 delivery hardens the existing JSON session store before SQLite
cutover (P0-B). The common runner owns atomic claims; the native coordinator can
supply an eligible session hint but cannot bypass the runner's locked check.
This is a mandatory safety rule, not a configurable feature flag.

## Ownership contract

`session_identity` version 1 binds a provider transcript to canonical project
and execution roots, task, semantic role, tool, declared scope and resolved
performer/model-tier configuration. Execution routing tier and mode are included.
The configuration snapshot is a conservative compatibility check, not a provider
version/capability probe. Provider-specific command syntax stays in registry tool
specifications. New adapters use the same policy without core tool-name cases.

- `dev` and `fix` use the `performer` role; `review` uses `reviewer`.
- Default scope is `task_role_tool`. Legacy `project` and `worktree` names remain
  readable but never allow cross-task/role reuse. P0-A conservatively also
  requires the same execution root, including for project-scoped declarations.
- An explicit caller ID must match the same identity, age and availability checks
  as a pool candidate. Missing or legacy metadata starts a fresh conversation.
- A runner releases only its own PID/identity lease. Lease release preserves the
  immutable semantic identity; transient `owner_*` fields remain lease metadata.
- Expired heartbeat does not evict a live or ambiguous owner. A dead runner does
  not prove its provider child exited: unclean recovery marks `abandoned`, and
  never automatically resumes that transcript. Fenced, verified recovery belongs
  to P0-B. Finished-job cleanup requires every supplied task/worktree constraint
  to match, and cannot release a live runner's lease.
- Restoring an active snapshot quarantines it rather than advertising an
  available transcript; semantic identity is retained and checked against the
  destination project. Closed/abandoned entries stay ineligible.

## Fresh-session fallback and migration

No destructive migration is needed. Existing unscoped JSON entries remain
inspectable but are not automatic resume candidates. New invocations write
verified identity. All mutable pool access retains the shared Rust/shell flock;
when flock is unavailable, a runner starts fresh without reading/writing leases.
Lock acquisition errors refuse an unsafe resume.

Generated IDs require an explicit adapter `session.create` contract. Without it,
including on retries, invoke the normal new-session command. Never merge resume
arguments into a fresh create invocation. Discovery accepts only explicit session
metadata from the current invocation, never a global session list or arbitrary
UUID. Gemini's broad UUID/list fallback and Vibe's unverified generated-ID resume
were removed; tools without reliable ID metadata run fresh until their adapter
supports verified extraction. No new provider CLI flags are assumed.

These rules do not reset Git, switch branches, clear an index, stash changes or
remove untracked files. Existing same-task tool handoff preserves the worktree.
Deploy matching coordinator and shared runners together; do not mix old pool
consumers with the ownership-aware runner.

## Diagnostics and clients

Session reserve/reuse/release/rejection/recovery events are registered in the
canonical event schema. Runner events include task, tool, role, scope and reason,
without dumping the compatibility snapshot. Existing TUI/Web log views can
inspect them. P0-A adds no editable policy or bypass; dedicated session inspection,
configuration catalog and further client workflows remain part of subsequent O2
work packages and must meet the specification's client coverage gates.

## Verification

- `make test-tool-runner`: real shared runner with stub tools, including future
  adapters, cross-task/role rejection, explicit hints, legacy state, expiry,
  concurrent claims, missing flock and preservation of staged/unstaged/untracked
  implementation work; existing Codex effort/quota/create-resume regressions.
- `cargo test -p macc-core --lib`: coordinator eligibility and recovery tests,
  runtime refresh, restore normalization and event schema parity.
- `cargo test -p macc-core --test session_identity_contract --test tool_spec_session_flags --test tool_name_guardrail`:
  shared Rust/shell identity contract and adapter specification checks.

Real-provider version probes, SQLite leases/fencing, structured checkpoints and
normalized usage accounting are not delivered by P0-A.

P0-C adds [Task Checkpoints](TASK_CHECKPOINTS.md), reviewed recovery and session rotation for activated SQLite projects.

# Persistent project layout

Status: accepted, 2026-11-08.

## Context

Keeping control authority beneath Git metadata protected it from historical MACC
cleanup and shared it between worktrees, but hid important project data. Moving
it requires preserving both guarantees. Git visibility must not determine whether
clear retains a file, and runtime startup must not publish ignored configuration.

## Decision

Use the primary checkout's `.macc/persistent/` for one shared authority and its
locks; use `.macc/worktree/integration/` for the integration checkout. Resolve the
primary through actual Git checkout identity, with a validated locator for
separate Git directories. Retain only migration synchronization/locator metadata
and compatibility fences beneath Git metadata.

Migrate only idle legacy authority, after a verified SQLite backup. Publish by
atomic directory rename and move integration through Git. Refuse ambiguous state,
symlinks and unsupported cross-filesystem relocation without discarding work.

Share one clear-retention policy across execution and preview. Retain configuration,
catalogs, pinned skill dependencies, persistent authority, existing O2/session
history and Git exclusion rules. Preserve existing protections for unfinished work.

Explicit Init and reviewed Apply update an ordered visibility block. Runtime repair
and supervisor startup keep the existing visibility policy. Configuration/catalogs
may be tracked; active authority and worktrees remain local. Generic configuration
copies exclude persistent authority; governed save bundles continue to use consistent
control snapshots.

## Consequences

The primary checkout must remain available to linked workers. Existing processes
must stop before migration; old binaries fail against compatibility fences instead
of creating a second authority. Backups/session-bundle formats are unchanged.
Portable files become visible only after an explicit setup operation and still
require review before publication. No task/provider adapter acquires a private
copy of authority state.

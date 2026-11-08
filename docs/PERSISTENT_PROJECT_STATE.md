# Persistent project state

MACC stores its shared decision/graph authority in the primary checkout:

```text
.macc/
  macc.yaml                       portable project configuration
  catalog/                        portable project catalogs
  skills.lock.json                pinned skill dependencies
  persistent/
    layout.json                   storage-layout receipt
    control.sqlite                shared decision/graph authority
    controller.lock
    mutation.lock
    integration.lock
    broker/                       retained auxiliary authority state, when present
    migration-backups/            verified pre-migration SQLite snapshots
  worktree/integration/            detached Git integration checkout
```

Every linked worktree resolves the same primary checkout through Git's worktree
inventory. A worker does not create its own persistent authority. Separate Git
metadata directories are supported (prepare once from the primary checkout before
access from a linked checkout); a bare repository without a primary checkout
is rejected. Relocating the repository does not change the authority directory's
relative location.

## Migration from `.git/macc/`

Apply and coordinator launch prepare this layout automatically. Existing decision
store access and integration preparation use the same migration service. Migration
requires stopped coordinator/supervisor/task runners and idle authority locks.
No model or provider-specific setup is involved.

The migration service:

1. Acquires a common migration lock and the legacy controller/mutation/integration
   locks. It refuses competing state rather than choosing one database.
2. Checks SQLite integrity, completes the WAL checkpoint, takes a consistent
   `VACUUM INTO` snapshot, verifies its integrity, and records its SHA-256.
3. Moves a clean legacy integration checkout with `git worktree move`, preserving
   Git registration and its exact commit. Dirty or conflicting slots are retained
   and reported for explicit resolution.
4. Atomically renames the legacy state directory to `.macc/persistent`, retaining
   auxiliary files, SQLite sidecars, backups and lock identities.
5. Leaves compatibility fences under the old `.git/macc/` paths. These contain
   no decision history: they prevent an older MACC binary from silently opening
   a fresh authority or integration slot. The common migration lock and a validated primary-checkout locator also remain
   technical metadata under the Git directory.

Interrupted publication can finish idempotently on the next access. A second
legacy database, unknown nonempty destination, symlinked storage or an occupied
lock causes an actionable error tagged `MACC-PERSISTENT-LAYOUT`. No states are
merged implicitly. Cross-filesystem directory relocation is refused safely; the
legacy authority and its snapshot are retained. Stop old processes and resolve
such a storage relocation explicitly before restarting MACC. Do not downgrade to
an old binary after migration.

## Clear and preview

`macc clear --dry-run` builds the same read-only disposition plan used by CLI,
TUI and Web execution. Each entry has an action, reason and byte count:
remove, archive, preserve, retain user file, retain worktree or delete merged
branch. A retained worktree is not necessarily an active worker.

Clear resets generated runtime, including unregistered artifacts under `.macc`.
It preserves:

- `.macc/macc.yaml`, `.macc/catalog/`, and `.macc/skills.lock.json` (portable setup).
- `.macc/persistent/` (local authority, provenance, local overrides and archives).
- `.macc/memory.db` and existing WAL/SHM companions, together with
  `.macc/state/tool-sessions.json` and its lock. This pair remains necessary to
  retain O2 memory/checkpoints and identify its session authority; it is local,
  not portable Git setup.
- `.gitignore` and preexisting or modified user files outside `.macc`.
- Worktrees containing unintegrated commits, uncommitted/untracked work, unknown
  ignored files, parked supervisor repairs, locks or unverifiable ownership.

Before resetting the coordinator registry, Clear archives recovery state,
logs/reports, cache/drafts, backups and unclassified `.macc` data under
`.macc/persistent/clear-history/<id>/project/`. The archive contains a disposition
plan and execution result. Archived registry data is evidence for deliberate
recovery, not a live registry automatically loaded by the next run. Rebuildable
context/index/tmp/automation directories are removed. Symlinks are never followed
for recursive cleanup or evidence collection. Unknown worktree slots are retained.

Clean, MACC-owned worktrees are removed only when their commits are reachable
from the configured reference branch. Their `.macc` evidence and text/log files
are archived first. A generated tool config is disposable only with recorded
ownership and a matching content fingerprint; an unknown ignored credential or
user file keeps the slot. Only branches recorded as MACC-created (or verified
through an owned slot's metadata) and integrated into the reference are eligible
for deletion. User branches, checked-out branches and unmerged branches survive.

Generated project files have persistent provenance. Existing tool directories or
context files are never adopted. Clear removes matching generated files and empty
owned directories, preserving later user edits. Older manifests lack content
fingerprints: their unverifiable external files are conservatively retained and
listed. Clear releases managed ignore rules for removed generated files, while
retaining runtime exclusions and user-authored rules. Failed removals are reported with their cause, retain retry provenance,
and make the command fail instead of announcing complete success.

Client Clear requires an initialized project (configuration or a managed-project
receipt); it does not reset a standalone shared user cache. Active coordinators,
supervisors, task/session owners or occupied authority locks
block execution; `--force` bypasses confirmation only. Run Clear from the primary
checkout. Preview never creates locks, archives or authorities.

## Git visibility and backups

Retention is independent of Git tracking. Explicit Init and reviewed Apply maintain an ordered
visibility block: runtime files stay ignored, while configuration, catalogs and
the pinned skill lock become eligible for version control. Existing blanket
`.macc/` exclusions are superseded by the ordered block without deleting user
rules. Tool rules live in the managed block and are rooted at the project. A
preexisting user directory/file is never hidden wholesale by MACC: only newly
created children may receive precise rules. The generator records content hashes
and created directories in the ignored persistent provenance inventory.
Project-local Git exclusions additionally protect the persistent authority
and integration checkouts before the next Apply.

Runtime initialization and supervisor restarts do not rewrite visibility rules or
publish previously ignored configuration. MACC never automatically stages or
commits these portable files. Review
configuration/catalog contents for private values before publishing them. Doctor
and Apply diagnose typed native-session/lease/owner fields, literal credentials,
nonportable paths and runtime paths already in the Git index, without printing
private values. They never automatically untrack files or rewrite history.
Apply includes project-contained absolute catalog source conversions in its
reviewed plan; the persisted sources become project-relative and resolve from
the destination checkout/worktree. External sources are not silently remapped. Their
new visibility can make a previously clean checkout show untracked configuration;
commit the intended configuration or use the existing reviewed dirty-reference
workflow. Never commit active SQLite databases, WAL/SHM sidecars, broker secrets,
locks or worktrees.

Use the existing `macc save` session bundles to capture/restore governed authority
snapshots; the bundle format remains unchanged (`state/control.sqlite`). Existing
structured decision/graph exports remain available for deliberate publication.
Copying a live SQLite file or versioning runtime state is not a backup procedure.

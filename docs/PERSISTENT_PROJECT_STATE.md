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

`macc clear --dry-run` reports existing preserved paths and previews eligible
worktree removal using the same candidate selection as execution. Normal clear
preserves:

- `.macc/macc.yaml`, all of `.macc/catalog/`, and `skills.lock.json`.
- All of `.macc/persistent/`, including migration backups and evidence.
- Existing O2 memory/checkpoints/session authority and JSON session inventory.
- `.gitignore`, so runtime exclusions and user rules survive cleanup.

Clear removes managed generated files; it does not recursively erase `.macc`.
Unmanaged files and unfinished work retain their existing protections. Active
coordinators, supervisors, task/session owners or occupied authority locks block
execution; `--force` does not bypass those protections. Run clear from the primary
checkout, rather than from a worker. CLI, TUI and Web use this shared service.

## Git visibility and backups

Retention is independent of Git tracking. Explicit Init and reviewed Apply maintain an ordered
visibility block: runtime files stay ignored, while configuration, catalogs and
the pinned skill lock become eligible for version control. Existing blanket
`.macc/` exclusions are superseded by the ordered block without deleting user
rules. Project-local Git exclusions additionally protect the persistent authority
and integration checkouts before the next Apply.

Runtime initialization and supervisor restarts do not rewrite visibility rules or
publish previously ignored configuration. MACC never automatically stages or
commits these portable files. Review
configuration/catalog contents for private values before publishing them. Their
new visibility can make a previously clean checkout show untracked configuration;
commit the intended configuration or use the existing reviewed dirty-reference
workflow. Never commit active SQLite databases, WAL/SHM sidecars, broker secrets,
locks or worktrees.

Use the existing `macc save` session bundles to capture/restore governed authority
snapshots; the bundle format remains unchanged (`state/control.sqlite`). Existing
structured decision/graph exports remain available for deliberate publication.
Copying a live SQLite file or versioning runtime state is not a backup procedure.

# Clear runtime reset and project provenance

Status: accepted, 2026-11-08.

Clear previously removed only paths in a runtime manifest. Unregistered indexes,
logs and coordinator state survived without appearing in the preview, while the
manifest could disappear after a failed removal. Blanket adapter ignore rules
could hide files that predated MACC.

Use a single disposition plan for preview and execution. Enumerate the actual
`.macc` tree, preserving the explicit setup/durable authority contract, archiving
recoverable evidence and resetting disposable runtime. Preserve unfinished
worktrees; integration is checked against the publication target. Keep failure
causes and retry provenance in durable local storage.

A separate persistent inventory records MACC-created project files, directories,
branches and file fingerprints. Existing files are not adopted. A modified
created file is retained; an unknown private file keeps its worktree. This also
allows rooted, precise Git exclusions without hiding preexisting user folders.

Retention is distinct from Git sharing. Only configuration, catalogs and the skill
lock are shareable by default. Persistent authorities, native session IDs, O2
SQLite files, logs, archives and ownership records remain ignored. Content and
index diagnostics complement path exclusions; they do not modify Git's index.

Tradeoffs: archives consume local disk; old un-fingerprinted generated files are
retained conservatively; O2 storage and its current authority marker remain an
explicit local exception outside `persistent/`. Cleanup does not interpret a
human statement or a clean worktree as proof that a task was delivered.

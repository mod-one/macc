# Save and restore SQLite session state

`macc save <name>` includes the project's current session authority by default.
JSON projects produce the existing version 1 bundle. SQLite projects produce a
version 2 bundle containing a consistent snapshot of `.macc/memory.db`, its
identity metadata and SHA-256 checksums. This includes logical sessions,
checkpoints, usage receipts, schema upgrades and audit history. The existing
configuration payload also preserves `session_management`, including P0-D settings.

```bash
macc save before-update
macc restore before-update --dry-run
macc restore before-update
```

Use `macc save <name> --no-sessions` or `--only config` for configuration-only
bundles. On restore, `--config-only` and `--no-sessions` leave SQLite and its
session authority unchanged. `macc session-store backup` remains available for a
standalone database snapshot.

## Consistent capture

Capture uses the canonical session authority lock and SQLite `VACUUM INTO`.
Committed WAL data is included without copying a live database file or its
sidecars. Saving is allowed while runners are active; it does not alter their
leases, transcripts or worktrees. Configuration and database capture are separate
steps; the bundle is not an atomic snapshot of all application files at one instant.

The database and authority payload use private Unix permissions. The existing
credential scanner checks the snapshot's lossy text view so ASCII credentials
inside binary SQLite pages are not silently skipped. Detection is best effort.
A database containing detected secrets is never redacted as text: Abort rejects
it; Exclude removes the entire session payload; Redact reports that sessions must
be excluded. Configuration redaction retains its existing behavior. Failed or
dry-run bundle creation removes its temporary staging directory.

The optional `session_store` manifest section records snapshot version 1, relative
`database_path`, `database_sha256`, logical `state_sha256`, `project_id` and
`project_root`. SQLite bundles do not populate the legacy JSON session path:
older JSON-only readers must not publish a SQLite authority envelope on its own.
New readers accept version 1 JSON and version 2 SQLite bundles and reject unsupported
formats. Payload checksums, relative paths and symlink checks are validated before
restoration starts. Checksums detect corruption; they do not authenticate authors.

## Restore conditions and effects

- Stop the coordinator, supervisor and runners before restoring sessions. Existing
  SQLite leases, even expired ones, require explicit recovery first. Restore
  refuses active or ambiguous JSON session owners as well.
- SQLite state is restored only to its original canonical project root. Existing
  project UUIDs must match; `--yes` does not bypass session isolation. To reuse
  configuration elsewhere, use `--config-only`. A cleared store at the original
  root can reinstall the saved authority and project UUID.
- The snapshot is checked for integrity, supported schema versions, project
  identity and logical state. Destination preconditions run before configuration
  is overwritten. Dry-run performs validation without installing payloads.
- Before replacement, restore creates a consistent `pre-restore-<id>.sqlite`
  backup and a private `pre-restore-<id>.json` authority backup when present.
  Configuration retains its existing `macc.yaml.backup.<timestamp>` backup.
- A private staged database is normalized, synced and installed by rename under
  the authority lock. Existing WAL data is checkpointed before replacement;
  the SQLite authority envelope is then published atomically. This is not a
  transaction spanning the configuration file and database. A crash during initial
  JSON-to-SQLite installation may require explicit authority recovery; backups
  remain available. There is no automatic fallback that replays unverified leases.
- Restored lease rows are removed. Transcripts are quarantined as `abandoned`
  (closed, archived and legacy-untrusted states stay closed/quarantined).
  Generations advance beyond the saved and destination generations. A backup
  cannot roll back native provider history or establish that an orphan exited.
  The next invocation starts from checkpoint/Git reconciliation with a fresh
  transcript. Git branches, commits, staged/unstaged edits and untracked work
  are untouched.
- Historical usage receipts remain available. Cumulative watermarks are cleared
  so costs incurred outside the restored snapshot cannot be assigned to the next
  invocation. The restore itself emits `session_bundle_restored` in session events.
- A version 1 JSON session payload cannot overwrite SQLite authority. Restore
  configuration only, or perform an explicit session-store rollback first.

Saved-state detection compares a logical SQLite fingerprint with the saved
snapshot, rather than always treating SQLite as unsaved or depending on WAL/page
layout. Schema, row contents and audit/lease changes count as changes. The scan
covers the whole store; its work grows with the amount of retained history.

## Verification

```bash
cargo test -p macc-core --lib save::sqlite_tests --locked
cargo test --workspace --locked
```

Regression cases cover WAL-only writes, active-lease refusal before config writes,
checkpoint/usage/config round trips, fresh authority recovery, old JSON bundles,
config-only and dry-run modes, corruption, project isolation, secret handling,
path validation and logical saved-state comparison.

O2 P1 schema version 2 adds `native_setting_ownership` to the same snapshot.
Restoring older SQLite snapshots preserves their schema and evidence; use the
reviewed O2 upgrade before enabling version 2 services. A receipt authorizes only
the exact recorded setting value; changed native files remain user-owned.

P3 introduces no additional schema version. The same consistent SQLite snapshot
contains symbol-bound memory evidence and conflicts, native provider receipts,
semantic benchmark/approval history and reviewed policy trials/reports. Canonical
configuration travels with the bundle. A populated P3 round-trip regression
checks restored audit rows/checksums, active semantic retrieval, conflicts, trial
inspection and native symbol queries. Derived native cache artifacts remain
expendable; a receipt does not make a missing cache authoritative. Binary downgrade
still requires a configuration/evidence snapshot supported by that older binary.

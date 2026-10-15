# P0-B: canonical SQLite session ownership

## Decision

Provider-neutral ownership lives in `core::session_store`. Adapter declarations
still own new-session/resume arguments and native-ID extraction. Shell runners
use `macc session-store` as a local service transport; the coordinator uses the
same Rust services. There is no provider-name dispatch in this subsystem.

SQLite is an explicit rollout. Existing projects retain P0-A JSON behavior until
an operator reviews and applies migration. P0-C task checkpoints and rotation build on this service; see
[Task Checkpoints](TASK_CHECKPOINTS.md). P0-D adds normalized invocation usage through the same service; see
[Usage Telemetry](USAGE_TELEMETRY.md).

## Authority and persistence

- `.macc/memory.db`: O2 session schema, WAL, foreign keys, full synchronous commits
  and a five-second SQLite busy timeout. Coordinator task/runtime SQLite remains
  separate. Session schema version 1 is stored in `session_meta`.
- `.macc/state/tool-sessions.json`: the single atomic authority switch. In JSON
  mode it contains the legacy pool. In SQLite mode it is a frozen envelope with
  `session_store: {version, mode, project_id, project_root}` and `tools: []`.
  The incompatible tools shape makes P0-A JSON writers refuse updates.
- `.macc/state/tool-sessions.json.lock`: bounded kernel lock shared by authority
  transitions and session mutations. SQLite mutations additionally use
  `BEGIN IMMEDIATE` and transactions; no transaction spans an AI invocation.
- Immutable JSON evidence and SQLite-consistent snapshots are stored under
  `.macc/state/`; migration reports under `.macc/reports/`.

Authority replacement is atomic and synced. Migration commits imported SQLite
rows before replacing the JSON pool with the frozen envelope. A crash before
replacement leaves JSON authoritative. A crash after replacement leaves SQLite
publicly authoritative. Rollback exports SQLite into a JSON pool with the JSON
mode header and replaces the same file in one atomic write. A second backend
marker cannot disagree with the session file.

Missing/corrupt authority for an existing SQLite store is an error, not permission
to start a new JSON pool. Explicit migration can repair a missing marker after a
partial initial migration; ambiguous/corrupt evidence requires operator repair.
Do not run pre-P0-A binaries or force legacy snapshot restores against a migrated
project. Deploy the coordinator and embedded runners together.

## Identity, leases and recovery

A durable project UUID supplements the P0-A project/task/role/tool/configuration
identity. A logical session retains native provider ID, lineage, generation and
use count after release. Native IDs are unique within a tool. Each acquisition
has a new random lease token and a monotonically increasing generation.
Invocation request IDs have a durable ledger: retries of an active acquisition
return the original grant; a completed request cannot acquire another lease.

Bind, heartbeat and release validate token, generation, expiry and process birth
identity. Linux birth identity includes boot ID and process start ticks; other
supported Unix environments use process start time. A stale release replay is
harmless and cannot clear a later lease. Rebinding a logical session to another
provider transcript is rejected. Session views/events omit lease secrets.

- Clean completion releases a verified transcript for the same task/role/tool and
  compatible configuration; development and correction retain performer history.
- Independent review has a separate role and transcript.
- A live or ambiguous owner is never displaced by heartbeat expiry.
- Recovery quarantines expired dead-owner leases. It cannot prove an orphaned
  provider exited, so that transcript remains abandoned and is not resumed.
- Heartbeat failure stops the runner. Signal cleanup terminates its provider;
  Linux `setsid` also isolates and cleans up the provider's process group.
- Failed creation or a nonzero provider exit is kept ineligible, avoiding repeated
  resume attempts against a transcript that may not exist.
- All Git commits, staged/unstaged files and untracked work remain in place.

## Migration and rollback

Both transitions require the coordinator, supervisor and relevant runners to be
stopped. Review counts and source SHA-256 before Apply; stale plans are rejected.
Migration preserves the exact original JSON in a private backup. Verified P0-A
identities may become available sessions. Unknown legacy ownership is imported
as `legacy_untrusted`; task ownership is never inferred from an arbitrary
worktree or `last_task_id`. Unclean sessions are quarantined.

Rollback requires no outstanding SQLite leases, creates a consistent SQLite
snapshot using `VACUUM INTO`, then exports immutable ownership metadata to JSON.
Re-enabling SQLite preserves existing logical generations and audit history.
Snapshots, database files and JSON evidence use private Unix permissions.

Legacy JSON session save/restore is blocked while SQLite is authoritative. Use
**Sessions → Create consistent backup** or `macc session-store backup` for the SQL
store. Existing configuration bundles remain available with `--no-sessions`;
they must not silently claim to include SQLite sessions. Restoring a SQLite
backup as an active store is not an automatic legacy JSON restore operation.
Supervisor crash snapshots use the same consistent SQLite backup service.

## Settings and clients

TUI: press **J** (also listed in Help). Web: **Setup → Sessions**, at
`/config/sessions`; `/ops/sessions` is an equivalent route.

Both clients provide the shared field catalog, typed numeric editors, validation,
diff/activation preview, Apply, defaults, removal of project overrides, migration
and rollback plans, session inspection/events, closure, recovery and backups.
Session status refresh preserves drafts; revision conflicts prevent stale writes.
The Web client retains drafts in session storage across navigation.

The canonical project configuration is:

```yaml
session_management:
  lease:
    ttl_seconds: 1800
    heartbeat_seconds: 30
  lifecycle:
    max_idle_hours: 24
  checkpoints:
    max_content_bytes: 16384
    rotate_after_uses: 20
```

The checkpoint controls are documented in [Task Checkpoints](TASK_CHECKPOINTS.md).

TTL must be 10–86400 seconds; heartbeat 1–TTL/3; idle age 1–720 hours. Changes apply
to the next claim. Active heartbeats retain their original TTL. Removing the
project override restores defaults. Scope isolation, tokens, generations,
project/process identities and migration evidence are not arbitrary editable
settings. Backend activation is the reviewed authority operation, not an implicit
side effect of saving YAML.

The broader O2 configuration schema remains proposed beyond these delivered
fields. Legacy shell-only age/pool environment overrides apply to JSON mode;
SQLite uses the canonical policy above.

## CLI and Web contracts

```bash
macc session-store status
macc session-store migrate
macc session-store migrate --apply --expected-sha256 <reviewed-source-hash>
macc session-store backup
macc session-store recover
macc session-store close <logical-session-id>
macc session-store events <logical-session-id>
macc session-store rollback
macc session-store rollback --apply --expected-sha256 <reviewed-export-hash>
```

Internal `acquire`, `bind`, `heartbeat` and `release` commands use bounded JSON
requests on stdin. The coordinator supplies its own executable through
`MACC_SESSION_SERVICE_BIN`, so runners use the matching implementation.

Web endpoints under `/api/v1/sessions`: `GET store`, `PUT policy`,
`POST policy/preview`, `GET/POST authority/{migrate|rollback}`, `POST action`
(`close`, `recover`, `backup`), and `GET {id}/events`. Mutations honor project
ownership gates and the existing structured Web error envelope. Policy requests
include the canonical configuration revision; HTTP 409 preserves a stale draft.

## Verification

Core tests cover concurrent claims, task/role/tool/configuration isolation,
fencing, request/release replay, live-owner protection, migration evidence,
rollback/re-import, authority loss, private consistent backups and field coverage.
CLI tests drive the actual shared runner and service with a future fake adapter,
including same-task resume, review isolation, quota/failure and signal cleanup.
TUI and Web tests exercise field editing, stale revisions, override removal and
reviewed migrations. Existing JSON runners and Codex quota/effort tests remain
regression gates. No real AI calls are needed by these tests.

# Process Ownership

MACC treats long-running processes, such as the coordinator, as owned resources.

Policy:
- Only the current owner of a process may issue state-modifying commands against it.
- Viewers may inspect process state, logs, and runtime status without restriction.
- A viewer who attempts a modifying action receives `MaccError::NotProcessOwner`.

Coordinator commands currently gated by ownership:
- `stop`
- `resume`
- `unlock`
- `dispatch`
- `advance`
- `cleanup`

When a modifying action is rejected, request control with:

```bash
macc process takeover request --kind <kind> --pid <pid>
```

After the current owner accepts the takeover, the requesting client becomes the new owner and may retry the command.

## Project-level control lease

In addition to per-process records (which observable runtime inventory), MACC
supports a single project-wide control lease via `ProcessKind::Project`. When a
`Project` record exists, only its owner may perform mutating actions across all
processes in the project. The per-process record is only consulted as a
fallback when no project-wide lease exists.

To register and claim a project-wide lease:

```bash
macc process claim --kind project --pid 0
```

## Takeover-timeout policy

A pending takeover request is automatically resolved after
`automation.coordinator.takeover_timeout_seconds` (default `60`, `0` disables).
The policy is configured via `automation.coordinator.takeover_default_response`:

- `deny` (default) — request is dropped; ownership stays unchanged.
- `auto_accept` — ownership transfers to the requester.
- `admin_takeover` — ownership transfers to the requester and an
  `admin_takeover` event is emitted for audit logs.

A `takeover_timeout` event is emitted on every resolution via the coordinator
event broadcaster.

## Web client identity

Web clients must send their identity in the `X-Macc-Client-Id` header on every
ownership endpoint and on every mutating endpoint. A missing header on an
ownership endpoint returns `400`; a missing header on a mutating endpoint is
accepted only when no project lease is currently claimed (single-user mode).

When a mutating endpoint rejects a non-owner, the response is `HTTP 403` with
body `{ "error": { "code": "not_process_owner", ... } }`.

## Background operation

MACC long-running processes (coordinator, supervisor) are designed to run
independently of any client. The ownership layer does not add a lifecycle
dependency on any connected client.

### Processes run client-free

When `macc coordinator run --no-tui` starts without an interactive client, it
registers a `ProcessKind::Coordinator` inventory record with `owner: null` and
`viewers: []`. The record persists across repeated loads of the ownership store
with no owner until a client explicitly calls `macc process claim`. The
coordinator continues running and processing tasks regardless.

The same applies to the supervisor (`macc supervisor start --daemon`): its
`ProcessKind::Supervisor` record is created with `owner: null` and the process
survives independently.

### Surviving parent shell disconnect

To detach a coordinator from a terminal session so that closing the shell does
not send `SIGHUP`:

```bash
setsid macc coordinator run --no-tui </dev/null >macc-coord.log 2>&1 &
```

`setsid` places the process in a new session. With stdin closed and
stdout/stderr redirected, the coordinator is fully decoupled from the parent
shell. After detach, `macc process list` confirms the coordinator record is
still present with `owner: null`.

When starting with `--supervisor`, both records appear:

```bash
setsid macc coordinator run --no-tui --supervisor </dev/null >macc-coord.log 2>&1 &
# Verify both Coordinator and Supervisor records exist with owner=null:
macc process list
```

### Orphaned ownership entries are auto-evicted

If a client (TUI, web browser) is killed without cleanly releasing ownership,
its `last_heartbeat` field stops updating. On the next call to
`OwnershipStore::load()` (triggered by any ownership or coordinator command),
`evict_stale_records()` runs and:

1. Removes viewers whose `last_heartbeat` is older than the TTL (default 60 s).
2. Clears the `owner` field if the owner's `last_heartbeat` is older than the TTL.
3. Promotes the oldest fresh viewer to owner when the stale owner is removed.

The eviction is transparent to the background process: the coordinator and
supervisor continue running unaffected.

### First reconnecting client takes control

After the stale owner is evicted, the next client that calls
`macc process claim --kind <kind> --pid <pid>` becomes the new owner:

```bash
# Owner was killed; wait for TTL expiry, then reconnect:
macc process claim --kind coordinator --pid <pid>
# → Owner
```

If multiple clients reconnect simultaneously, the first writer wins (the store
uses a file lock to serialize concurrent modifications).

### Ownership store location

`.macc/state/process_ownership.json`

This file is managed by MACC and should not be edited manually. It is
automatically created on first process registration and remains until all
tracked processes unregister.

### Verification tests

- **Service-layer unit tests** (fast, no binary required): `core/tests/daemon_ownership_integration.rs`
  covers scenarios (1)–(3) using fake timestamps to avoid real waits.
- **E2E shell script** (requires built binary): `automat/tests/test_daemon_ownership.sh`
  covers scenario (3) (setsid shell disconnect) and optionally scenario (2)
  with `MACC_TEST_SLOW=1` (60-s TTL wait).

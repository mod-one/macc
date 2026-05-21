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

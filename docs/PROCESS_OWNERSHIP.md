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
macc process takeover request
```

After the current owner accepts the takeover, the requesting client becomes the new owner and may retry the command.

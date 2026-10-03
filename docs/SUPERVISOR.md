# Supervisor recovery

The supervisor watches the canonical coordinator run and task registry, including
SQLite-only projects. It diagnoses terminal failures, blocked root tasks, crashed
engines and engines whose tick exceeds the configured stall threshold.

## Start and observe

```bash
macc coordinator run --supervisor
macc coordinator run --no-client --supervisor
macc supervisor status
macc supervisor report
```

TUI, Web and no-client launch all ask `Also start the supervisor ... [Y/n]` when
neither supervision flag is supplied. Unattended launch enables supervision by
default; `--no-supervisor` disables it. The supervisor starts before the client
attaches. Both daemons detach from the launching terminal. Closing the terminal
or disconnecting SSH does not stop them. `macc coordinator stop` stops the
attached supervisor too. Standalone `macc supervisor start --daemon` is supported.

## Intervention contract

1. Wait for the failed runtime to exit; stop recorded orphan performer groups.
2. Lock intervention against concurrent coordinator launches.
3. Ask the selected AI tool to diagnose logs, task requirements, structured
   preconditions and source. Preserve its MACC dysfunction/improvement findings.
4. Repair in an isolated Git worktree based on the exact project HEAD.
5. Run project validation, then ask the AI tool for an independent verification.
   Verification must leave the tested source unchanged.
6. Commit and fast-forward the repair only if the project remains clean and its
   HEAD unchanged. PRD, runtime metadata and environment secrets are protected.
7. Requeue diagnosed roots through coordinator APIs, reconcile dependency blocks,
   restart the managed coordinator and wait for readiness.

Human approvals, operator gates and missing external authority remain external.
A dirty project, validation failure, rejected verification, timeout or unavailable
tool produces an escalation report. Monitoring failures are recorded and retried;
the daemon can resume after the underlying state problem is resolved.
Failed worktrees and logs remain available.
No reset or cleanup discards user changes. Existing task worktree attachments are
preserved for coordinator salvage/reconciliation on retry.

Tool selection uses `automation.supervisor.tool`, then the coordinator tool,
then the first enabled tool. ToolSpec command arguments, prompt transport,
model/effort settings and configured permission flags (including Codex `--yolo`)
are retained. Each tool invocation and validation command has a bounded timeout;
stopping the supervisor terminates their process groups.

## Configuration

```yaml
automation:
  supervisor:
    tool: codex
    # model and effort are optional overrides of the tool settings.
    watchdog_interval_seconds: 30
    max_restart_attempts: 3
    intervention_timeout_seconds: 3600
    log_analysis_window_seconds: 300
    validation_commands:
      - npm run build
      - npm test
    report_output_path: .macc/log/supervisor/report.json
```

When validation commands are omitted, the supervisor detects `make check`, Rust
workspace tests, or supported package scripts including a test command. Projects
without a detectable test workflow require explicit validation commands.
`log_analysis_window_seconds` also sets the stalled-tick threshold (minimum 30s).
Legacy `events_log_path` and `crash_debounce_checks` do not control the new
canonical incident loop; JSON-only registry compatibility remains supported.

Attempt counts persist in `.macc/state/supervisor-interventions.json`, preventing
restarting the supervisor from silently resetting the retry budget. A successful
coordinator run resets the consecutive attempt count. After inspecting and
resolving an escalation, stop the supervisor and explicitly reset the budget:

```bash
macc supervisor stop
macc supervisor start --daemon --retry
```

The previous ledger is archived. Successful incident IDs stay deduplicated within
a ledger; reports and isolated worktrees survive supervisor restarts.

## Evidence and architecture

- `.macc/state/supervisor-health.json`: health and latest intervention status.
- `.macc/log/supervisor/daemon.log`: daemon startup and runtime errors.
- `.macc/log/supervisor/latest.json`: latest intervention, diagnosis, MACC findings,
  validation commands, commit and restart identity.
- `.macc/log/supervisor/<incident>-attempt-N/`: phase prompts, stdout/stderr,
  validation logs, report and retained worktree.
- `.macc/log/coordinator/daemon-stderr.log`: coordinator startup errors.

Incident detection lives in core; CLI modules separate AI execution, isolated
repair, recovery and reporting. The TUI reads durable status and clears a stale
failure popup when the coordinator resumes. Legacy supervisor mode modules remain
available for compatibility; the CLI uses the complete incident recovery loop.

Regression tests use actual CLI processes, temporary Git projects and a
deterministic AI tool double. They do not call paid AI services. Run
`bash automat/tests/test_daemon_ownership.sh` or `make check`.

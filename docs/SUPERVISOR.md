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
   A task flagged with `out_of_scope_fix` names the paths, owned by another
   module, where its precondition must be fixed; the repair targets them.
4. Repair in an isolated Git worktree based on the tip of the coordinator
   reference branch (`automation.coordinator.reference_branch`), whichever
   branch the project root has checked out.
5. Run project validation, then ask the AI tool for an independent verification.
   Verification must leave the tested source unchanged. See
   [Baseline-aware validation](#baseline-aware-validation).
6. Commit the repair and publish it to the reference branch the same way the
   coordinator publishes task merges: a compare-and-swap of the ref when no
   worktree holds the branch, otherwise a fast-forward of the worktree that
   does. Integration is deferred if the branch moved or if local changes in
   that checkout overlap the repair; unrelated local changes do not block it.
   PRD, runtime metadata and environment files (`.env`, `.env.*`) are protected.
7. Requeue diagnosed roots through coordinator APIs, reconcile dependency blocks,
   restart the managed coordinator and wait for readiness.

Restart without repair: when the engine failed or crashed while every incident
task was between phases (`testing`, `reviewing`, `changes_requested`, `pr_open`,
`queued`), no task failed and nothing needs repairing. The supervisor then skips
diagnosis and repair, keeps those tasks in their state and restarts the
coordinator, which resumes their pending test, review, fix or merge (a
changes-requested review is handled by the fix phase it schedules). The AI
diagnosis can also choose this recovery with `restart_only: true` for another
engine-side stop. Each distinct stop (its reason and the tasks with their
states) is restarted at most once: if the same stop recurs after a restart, the
supervisor diagnoses and escalates instead of looping. The report records
`restart_only: true`; the ledger counts restarts under `restarts`.

Tasks left between phases by a stopped engine are part of the incident (repair
candidates), so a diagnosis naming them is accepted.

Human approvals, operator gates and missing external authority remain external.
A missing Git identity (checked before any AI call), a validation failure
introduced by the repair, rejected verification, deferred integration, timeout
or unavailable tool produces an escalation report. Monitoring failures are recorded and retried;
the daemon can resume after the underlying state problem is resolved.
Failed worktrees and logs remain available.
No reset or cleanup discards user changes. Existing task worktree attachments are
preserved for coordinator salvage/reconciliation on retry.

Tool selection uses `automation.supervisor.tool`, then the coordinator tool,
then the first enabled tool. ToolSpec command arguments, prompt transport,
model/effort settings and configured permission flags (including Codex `--yolo`)
are retained. Each tool invocation and validation command has a bounded timeout;
stopping the supervisor terminates their process groups.

## Baseline-aware validation

A validation command that fails with the repair is run again in the same slot on
the unmodified base, with the same installed dependencies and environment.

- Passes on the base, fails with the repair: the repair introduced the failure;
  the intervention escalates.
- Fails on the base too (for example a browser test suite missing a system
  library): it is not a gate. The command is recorded in
  `preexisting_failures`, both logs are kept (`validation-N.log`,
  `baseline-N.log`), and the independent verification must compare them and
  reject the repair if it adds a failure absent from the baseline.

A command-level comparison cannot see a new failure inside an already failing
command; that judgment rests on the independent verification. Configure
`validation_commands` with commands that pass on your machine to keep the gate
deterministic. While the base is checked out, the repair is held as a Git tree
named in `.macc/supervisor-restore-tree`; an interrupted baseline run is restored
on the next attempt and such a slot is never removed.

## Live visibility

The supervisor is not a coordinator task and is never counted in the task
registry. Clients show its intervention as a separate, labelled entry:

- **TUI, Coordinator Live:** a pinned `SUPERVISOR` row at the top of LIVE TASKS
  (`◆ SUP` while working, `! SUP` when it needs attention), in its own colour,
  with the repaired root task, phase, tool, total time and time in the phase.
  With no coordinator task selected, the detail pane becomes SUPERVISOR TASK
  DETAIL: current step, diagnosis, reason, next step, findings and log path.
- **Web dashboard:** a `Supervisor` entry above the workers, from the optional
  `supervisor` object of `GET /api/v1/snapshot` (also in `macc status --json`).

Phases are `detected`, `diagnosing`, `repairing`, `validating`, `verifying`,
`integrating`, `requeuing` and `restarting`; the report records the step in
progress (`activity`, for example `Validation 3/5: pnpm run test`) and the phase
start time. The entry disappears once the intervention recovers. An unrecovered
one stays visible until a coordinator runs again.

## Repair rounds

A repair the independent verification rejects is not abandoned at once. The
verifier's explanation is handed back to the repair agent, which continues in
the same worktree; validation and verification then run again. The same applies
to a validation command that passes on the unmodified base and fails with the
repair. `automation.supervisor.max_repair_rounds` (default 3, `1` disables the
loop) bounds the rounds of one attempt, and each AI call or validation command
stays bounded by `intervention_timeout_seconds`.

- Logs of later rounds are named `repair-round-N-*` and `verification-round-N-*`;
  the validation and baseline logs of a rejected round move to `round-N/`.
- The report records `repair_rounds`.
- The verifier can answer `requires_human: true` when accepting the repair needs
  a product, security or access-policy decision. The loop then stops, the
  intervention waits with the reason "Repair needs a human decision", and the
  work stays in the worktree.
- `macc supervisor start --retry` on a kept worktree hands the last rejection to
  the repair agent as well.

## When an intervention waits

`macc supervisor status`, `macc supervisor report` and the supervisor entry
above show the reason of an unrecovered intervention, the exact next step
(`next_action` in the report), and the MACC improvements recommended by the
diagnosis (`diagnosis.macc_findings`).

## Configuration

```yaml
automation:
  supervisor:
    tool: codex
    # model and effort are optional overrides of the tool settings.
    watchdog_interval_seconds: 30
    max_restart_attempts: 3
    intervention_timeout_seconds: 3600
    max_repair_rounds: 3
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

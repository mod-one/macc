# Apply, coordinator launch and task-source diagnostics

## Apply

The CLI keeps its interactive confirmation (and explicit unattended modes). The TUI and Web client show a separate confirmation window before writing project files. User-scope TUI operations additionally require typing `YES`; the Web endpoint retains its project-only scope.

In the TUI, Enter on Apply opens the review. Cancel is selected initially. Use Left/Right or Tab to choose a button, Enter to activate it, and Esc to cancel. Every planned operation is reachable with Up/Down; Page Up/Down scrolls the result pane. Refresh prepares a new plan. Preparing catalog sources runs in the background with terminal progress disabled, preventing output from corrupting the alternate screen. Opening a page or cancelling a confirmation does not apply project files.

Both graphical clients bind their confirmation to the reviewed plan and its observed file contents. If these change, refresh and confirm again. TUI draft configuration changes also invalidate the preview. A successful TUI apply records the same apply marker used by the CLI/Web context-generation workflow.

The Web command palette navigates to Apply instead of executing writes. Web dry runs render the returned plan, not an apply report.

### Apply API

- `POST /api/v1/plan` includes `planRevision` in its response.
- `POST /api/v1/apply` requires `confirmed: true` for writes; `dryRun: true` does not require confirmation.
- Graphical apply requests include `planRevision`; a changed plan is rejected before execution. This field remains optional for existing explicit API callers.
- The Web API client normalizes its legacy `yes` input to `confirmed`; explicit `confirmed: false` takes precedence.

## Coordinator launch

CLI startup retains its existing flags, client selection, scheduling and interactive reference-branch preflight. TUI/Web use the same configuration, reference preflight and PRD Queue services through the Engine facade. They display a launch review containing the active task source, configured reference, performer tools, concurrency, configuration warnings and blocking conditions.

Adaptive choices:

- Start the supervisor (selected by default, matching the CLI default question).
- Choose a source when the reference branch is missing. Branch creation happens only after final confirmation.
- Explicitly allow a dirty reference once when the configured policy would otherwise block it. An existing `allow`/`warn` policy is respected and displayed as a warning.
- Cancel or start. In the TUI, Page Up/Down scrolls long launch warnings. Monitoring stays in the current graphical client, so no redundant client-selection question is asked there.

TUI launch waits for startup/supervision on a worker thread; rendering stays responsive. Unsaved TUI configuration must be saved before launch. Cancelling never starts a run. Ownership gates remain in force at execution. If configuration, active PRD or reference conditions change during the review, a new confirmation is required. The supervisor is attached through the same bootstrap as CLI startup, after runtime readiness; failed attachment rolls back the newly started coordinator.

A dirty-reference approval is forwarded to the child runtime for this launch only; the saved project policy is unchanged.

### Launch API

1. `GET /api/v1/coordinator/launch-review` returns a read-only review with `revision`, `needsBranchCreation`, `branchSources`, `dirtyReference`, `warnings` and `blockers`.
2. `POST /api/v1/coordinator/run` requires a decision, for example:

```json
{"confirmed":true,"revision":"<review revision>","supervisor":true,"createFrom":null,"allowDirtyReference":false}
```

An empty/unconfirmed request is rejected. All Web Run entry points use the shared dialog, including Dashboard, Console and the command palette. This intentionally replaces the old empty-body Run API contract.

## Doctor and readiness

CLI, TUI and Web diagnostics resolve the configured `prd_files` queue before the legacy single-file fallback. The saved queue cursor determines the active PRD. Readiness never skips an incomplete entry merely because a later PRD has ready tasks. Changed queue definitions require an explicit progress reset; completed queues are reported as completed. New PRD tasks without a runtime `state` remain supported unless `passes` is true.

Onboarding uses this same resolver rather than always reading the first queue file or trusting a historical starter-task flag. Missing/invalid sources identify the actual configuration/progress problem. An idle coordinator is a warning, rather than a startup-blocking error; a stale running PID remains an error.

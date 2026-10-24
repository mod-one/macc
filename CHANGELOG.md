# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- Supervisor tests follow the validation command list: empty disables tests, adding commands enables them, and manual activation without commands reports an actionable message. Legacy empty-command configurations no longer prevent repair; AI verification and other repair safeguards retain their settings.
- Apply now uses a compact review/progress/result presentation, preserves complete logs, and offers verbose, quiet and clean JSON modes. Redirected input requires explicit initial `--yes` consent; preparation questions keep their own approval gates.
- Partial native indexes and parse failures are visible across clients. The assistant can propose a reviewed budget increase/rebuild or retain partial coverage with warnings. The default native build budget is 15 seconds; explicit existing budgets are preserved.

### Fixed
- Checkpoint resumes use finish headroom for retained evidence while keeping mandatory context within the initial limit. Compatible branch changes in the same idle worktree recapture Git evidence and rotate provider sessions; specification drift, divergent history and existing reconciliation blocks remain protected. Size/reconciliation errors identify their actual cause.
- Skill assets retain their exact source bytes across repeated Apply operations. Existing assets affected by MACC's structured-file reserialization can be repaired when they match that exact transformation of the locked source; local edits remain protected.
- Coordinator worktree Apply failures retain complete child output and report the task, worktree, underlying error and diagnostic log path.
- O2 Settings prioritizes Preview, Save and Discard shortcuts in the command footer, guides users through preview before saving, and clears stale preview evidence after edits or saving.
- AGY individual quotas now preserve multi-day reset durations and use adapter-owned model-pool cooldowns. Other configured model pools remain eligible; task/worktree preservation and existing Codex/Claude throttling are retained. See docs/AGY_QUOTAS.md.
- Tool preparation preserves timeout, launch, exit, response, network and authentication failures with evidence. Account setup runs only for explicit adapter authentication evidence.
- Coordinator startup waiting accounts for enabled checks and provider refresh budgets; launch failures include the current child's diagnostics and roll back failed CLI startup.
- Removed path-based “Missing placeholders” warnings for ordinary configuration and design-token files; real secret scanning remains enabled.
- Preparation output no longer carries a previous tool's output into the next phase. Migration/preparation markers no longer inflate changed-file counts.

### Added
- Confirmed Apply now prepares and verifies enabled O2 storage, provider runtimes/indexes, scoped MCP, native compaction and semantic evidence through the shared adapter layer. CLI, TUI and Web keep configuration, network permission and benchmark questions inside Apply, with progress and retained failure evidence.
- Coordinator startup verifies prerequisites and refreshes compatible local indexes without downloads or login. Doctor reports pending O2 preparation without repairing it.

## [0.3.0] - 2026-10-17

### Fixed — Coordinator restarts after an interrupted dispatch
- A new pool worktree takes the lowest free `worker-NN` slot (no directory, no registered worktree, no leftover `ai/<tool>/worker-NN` branch). It used to take `worker-<pool size + 1>`, so once a slot was missing (`worker-01`, `worker-02`, `worker-04`) it picked an occupied one and the run failed on `git worktree add … already exists`.

### Changed — Coordinator storage performance on long runs
- Readers no longer load the whole event log: the registry is read alone (`SqliteStorage::load_registry`), recent events through a window (`load_recent_events`), and "latest event of a kind" lookups stop at the first match (`scan_events_newest_first`). This covers the coordinator loop's registry loads and saves, `get_coordinator_status`, failure analysis, supervisor incident detection, the runtime snapshot and the TUI.
- New indexes `idx_events_seq` and `idx_events_type_seq`. The schema is stamped in `PRAGMA user_version`, so later connections skip the schema statements.
- Only the last heartbeat of each task is kept: a new heartbeat replaces the older ones of the same task (never the event a replay cursor points at). Existing databases are pruned and vacuumed once on upgrade; on a 40-task project the database went from 8.2 MB and 10,377 events to 1.0 MB and 242.
- `save_snapshot` rewrites only the tasks that changed and no longer copies the tasks into `registry_json`; a registry save no longer re-inserts every event.
- The PRD sync of each coordinator cycle saves and logs the registry only when it changed. Resource locks still held by the same claim keep their original `locked_at`.
- Performer heartbeats are no longer written to the coordinator run log, and the widened ghost grace is logged when it changes. Status and failure analysis read the last 256 KB of the log (`logs::read_log_tail`) instead of the whole file.
- The web event streams (`/api/v1/events`, task events) read new events by sequence from the database on a blocking thread, instead of loading the whole log every 250 ms per open tab. A new connection receives the last 500 events; a resumed one continues from its last event.
- The TUI polls the registry, recent events and (every 5 s) the run status on a background thread, renders from the last polled registry, and loads ToolSpecs once.
- Storage connections are reused per thread (`SqliteStorage::open` returns a pooled connection), and are reopened when the database file is replaced.
- Storage write transactions take the write lock when they begin (`BEGIN IMMEDIATE`). A transaction that read first could not upgrade once another process had committed, and the coordinator stopped with `E411 … database is locked`.

### Changed — Supervisor recovery
- Supervisor repairs are based on and published to the coordinator reference branch (shared `integration::publish_commit`), not the branch checked out in the project root. Unrelated local changes no longer block an intervention; overlapping ones defer integration and are preserved.
- Validation is baseline-aware: a command that also fails on the unmodified base no longer blocks the repair. It is recorded in `preexisting_failures` with `baseline-N.log`, and the independent verification compares both logs.
- The supervisor's intervention is visible live: a pinned, distinctly styled `SUPERVISOR` row in the TUI LIVE TASKS panel with a SUPERVISOR TASK DETAIL pane, and a `Supervisor` entry on the web dashboard fed by the new optional `supervisor` object of `GET /api/v1/snapshot`. The report records the phase (now including `validating` and `verifying`), the step in progress and phase timestamps.
- An unrecovered intervention reports its reason and an exact `next_action` in `macc supervisor status`, `macc supervisor report` and the supervisor row, together with the diagnosis' MACC findings.
- A missing Git identity escalates before any AI call.
- A rejected independent verification, or a validation regression introduced by the repair, is handed back to the repair agent for another round in the same worktree instead of waiting for an operator (`automation.supervisor.max_repair_rounds`, default 3). The verifier can refer the repair to a human decision with `requires_human`, which stops the loop. An explicit `--retry` on a kept worktree also passes the last rejection to the repair agent.

### Added — Interactive human approval
- `macc coordinator approvals` is interactive in a terminal: it walks every gate awaiting a decision, shows the full request and asks for the decision, the role, the evidence or reason, and a confirmation. `approve|reject|request-changes` prompt only for values missing from the command line. `--no-interactive` (or a non-terminal stdin/stdout) keeps the plain behaviour.
- The TUI opens an approval window by itself, on any screen, when a gate awaits a decision. It explains the request in full and records the decision (`a`/`r`/`c`, role picker, evidence or reason, confirmation). `Esc` puts the gate off until its subject revision changes; `A` re-opens it.
- The web UI opens an approval dialog by itself on every page, with an "N approvals waiting" pill in the top bar, backed by the new `GET /api/v1/coordinator/approvals` and `POST /api/v1/coordinator/approvals/{task_id}/decision` (owner-only, audit-logged).
- Approval listings (`macc coordinator approvals`, `macc status`, `GET /api/v1/status`, the snapshot) evaluate gates against the decision ledger and the current subject revisions when read, so a new subject revision is shown without waiting for a coordinator cycle.
- Pending approvals carry the full explanation shared by every client: `question`, `awaiting_decision`, the subject's title, objective, description, expected result and acceptance criteria, `required_roles`, the quorum rule in words, `evidence_required`, expiry and binding rules, the decisions already recorded, and the tasks the gate unblocks.

### Added — Current PRD in live clients
- The Coordinator Live summary line of the TUI names the PRD file being executed and its position in the PRD queue (`Coordinator: Idle | PRD: 03-web-bff-shell.prd.json (2/26) | Tasks: ...`). The web dashboard shows the same above the queue counters, from the new optional `prd` object of `GET /api/v1/snapshot`.

### Changed — PRD audit
- `macc prd audit --prd` accepts a directory as well as a file, and without `--prd` audits the coordinator PRD queue (`automation.coordinator.prd_files`), then `prd_file`, then `prd.json`. Several PRDs are audited in sequence; with `--json` the output is then `{"audits": [...], "failures": [...]}`.

### Added — Out-of-scope preconditions
- A `precondition_unmet` stop whose named paths lie outside the task's `change_scope` is flagged: the record says the fix belongs to another module, `task_runtime.out_of_scope_fix` keeps the paths, a `precondition_out_of_scope` event is emitted, the run summary gains `out_of_scope_paths` and a next action pointing to a supervisor repair or an owning task (TUI and web console show it), and the supervisor diagnosis receives the paths.
- `macc prd generate` and `macc prd audit` prompts include a cross-module contract check: each endpoint a task consumes from another module must declare an audience and contract that serve the consumer (for example a Web task calling an API route), else a contract-resolution task is planned in the owning module.

### Changed — Supervisor restarts the coordinator
- When the engine fails or crashes while every incident task is between phases (`testing`, `reviewing`, `changes_requested`, `pr_open`, `queued`), the supervisor restarts the coordinator without diagnosis or repair; the tasks keep their state and the restarted run resumes their pending phase. The AI diagnosis can request the same recovery with `restart_only`. The same stop is restarted at most once, then escalated.
- Tasks left between phases by a stopped engine are part of the supervisor incident, so a diagnosis naming them is no longer rejected as "a task outside this incident".

### Fixed
- A review verdict of "changes requested" recorded while the task is still in `testing` moves it to `changes_requested` instead of aborting the run with `Invalid coordinator FSM transition: from=testing event=ReviewChangesRequested`. Any other phase outcome the state machine cannot apply now blocks only that task (`E910`, worktree kept, `phase_outcome_unapplicable` event) and the run continues.
- A failed task's `last_error_message` is the performer's structured report (unmet preconditions, then `MACC_TASK_RESULT_EXP`) instead of an excerpt of the performer log. The log excerpt given to the error normalizer now starts at a full line (it started mid-word, e.g. "s does not serve without a page…"), a run that listed unmet preconditions is never reclassified from its log (a summary mentioning "usage limit" was read as a quota), and normalized messages are truncated on a UTF-8 character boundary.
- A rate-limit or quota `throttle_state` (and the delay it set) is removed from the task when a later attempt ends with any other outcome, and the tool-level throttle is cleared when a job that started after it was recorded completes with a structured report, so a stale cooldown of up to a day no longer blocks redispatch.
- `macc prd audit --tool`, `macc prd generate --tool` and AI context generation passed the ToolSpec argument template to the tool unrendered, so Claude Code was started with the literal model `{model}` and failed. The arguments are now rendered with the tool's configured model and effort, the `--model-tier` tier, or the `--model-routing manual --model` value.
- `macc supervisor start --retry` no longer fails with "Diagnosis modified source" when the slot still holds an unfinished repair.
- Blocked-run stop reasons and dependency-chain messages use `unmet_preconditions`/`result_explanation` instead of a truncated performer log header; the TUI precondition overlay is selected whenever preconditions are recorded, whatever the error code.
- A managed command observed after it exited reports how long it ran, not the time since it started.

### Added
- Added delayed one-shot coordinator execution with `macc coordinator run --in <duration>` and `macc coordinator run --at <datetime>`.

### Added — PRD Generation and Model Routing (spec §8)
- **`core/src/prd_generation/`** — new module implementing the shared PRD generation and audit infrastructure:
  - `request.rs`: `PrdGenerateRequest`, `ModelSelection`, `ModelRoutingMode` (auto/manual).
  - `metadata.rs`: `GenerationRunMetadata`, `PRD_GENERATION_DEFAULT_TARGET_DIR` (`.macc/generated/prd/macc-prd-planner`).
  - `target_dir.rs`: `resolve_target_dir()` — safe directory resolution with directory-traversal guard.
  - `validation.rs`: `validate_prd_file()`, `ValidationResult` — lightweight checks (JSON parse, required fields, unique IDs, provider-neutral routing hints).
  - `promotion.rs`: `promote_prd()`, `PromoteOptions`, `PromoteResult`, `list_generation_runs()` — backup-safe promotion.
  - `prompt_builder.rs`: `build_generate_prompt()`, `resolve_instructions()`, `resolve_tool()` — fixed `macc-prd-planner` prompt assembly.
  - `audit.rs`: `run_prd_audit()`, `PrdAuditRequest`, `PrdAuditResult` — wraps `core/src/coordinator/prd_auditor.rs` with `ModelSelection` support.
- **Config** — new `PrdGenerationConfig` and `ModelRoutingConfig` structs added to `CanonicalConfig` and `AutomationConfig` respectively (both optional, no breaking change).
- **`Engine` trait** — new default methods: `prd_audit()`, `prd_validate()`, `prd_promote()`, `prd_list_runs()`, `prd_build_generate_prompt()`.
- **CLI `macc prd` command group** with subcommands:
  - `generate --from <brief.md>` — builds fixed `macc-prd-planner` prompt and invokes selected tool. Options: `--tool`, `--model-routing auto|manual`, `--model`, `--instructions`, `--instructions-file`, `--target-dir`, `--update`, `--dry-run`, `--promote`, `--yes`, `--json`. No `--performer` or `--skill` options.
  - `audit --prd <prd.json>` — enriches existing PRD from commit history. Same option set as `generate` plus `--reference-branch` and `--diff-stat`.
  - `promote <source>` — promotes a generated PRD with backup before overwrite.
  - `validate <prd>` — lightweight validation.
  - `runs` — lists all generation runs under `PRD_GENERATION_DEFAULT_TARGET_DIR`.
  - `show-run <run-id>` — shows files and metadata for a specific run.
- **Web API** — new PRD generation endpoints: `POST /api/v1/prd/generate`, `POST /api/v1/prd/audit`, `POST /api/v1/prd/promote`, `POST /api/v1/prd/validate`, `GET /api/v1/prd/generation-runs`, `GET /api/v1/prd/generation-runs/{run_id}`.

### Changed — PRD audit migration (spec §22)
- `macc prd audit` replaces `macc coordinator audit-prd`. The underlying business logic in `core/src/coordinator/prd_auditor.rs` is preserved unchanged.
- `POST /api/v1/prd/audit` replaces `POST /api/v1/coordinator/audit-prd`.

### Removed — Legacy coordinator audit-prd
- **`CoordinatorCommand::AuditPrd`** variant removed from `core/src/service/coordinator_workflow.rs`.
- **`coordinator_audit_prd()`**, `invoke_audit_tool()`, `parse_audit_prd_command()`, `build_sync_summary_for_prompt()` removed from `coordinator_workflow.rs`.
- **`audit_prd_report`** field removed from `CoordinatorCommandResponse`.
- **`POST /api/v1/coordinator/audit-prd`** web route removed.
- **`audit-prd`** entry removed from the CLI coordinator command description.

### Added — Skills & Catalog Lifecycle (spec §7)
- **`core/src/skills_catalog.rs`** — new lifecycle layer implementing the four-state model (available → selected → installed → locked): `SkillsLockFile`, `SkillLockEntry`, `LockedSource`, `LockedPackage`, `InstalledTargets`, `InstalledTarget`, `SkillStatusKind`, `SkillStatus`, `PackageManifest`, `InstallConflict`, `ConflictKind`, `OwnershipMarker`, `SkillsPolicy`, `VerifyFinding`, `SkillDiffEntry`. Functions: `compute_skills_status()`, `verify_skills()`, `diff_skill()`, `detect_conflicts()`, `git_cache_key()`, `http_cache_key()`, `sha256_digest()`, `file_digest()`, `write_ownership_marker()`. Error code constants `MACC-SKILL-1001` through `MACC-SKILL-4003`.
- **`core/src/catalog.rs` `SkillEntry`** — extended with lifecycle fields: `tools`, `recommended_ref`, `risk`, `requires_mcp`, `writes_user_level_config`, `targets`, `category`, `compatibility` (all optional for backward compatibility).
- **`ProjectPaths`** — new path methods: `skills_lock_path()` (`.macc/skills.lock.json`), `skills_cache_dir()` (`.macc/cache/`).
- **`Engine` trait** — new default methods: `catalog_skills_available()`, `skills_lockfile()`, `skills_status()`, `skills_verify()`.
- **CLI `macc skills` subcommands** — new lifecycle subcommands: `available`, `status`, `install`, `update`, `verify`, `prune`, `diff`, `uninstall`. All support `--tool`, `--json`, `--dry-run` as appropriate.
- **Web API** — new catalog endpoints: `GET /api/v1/catalog/skills/available`, `GET /api/v1/catalog/skills/status`, `GET /api/v1/catalog/skills/installed`, `POST /api/v1/catalog/skills/verify`, `GET /api/v1/catalog/skills/lockfile`.
- **Catalog** — 7 hook-bundle entries added: `test-output-failures-only`, `lint-errors-only`, `stacktrace-collapse`, `git-diff-stat-before-full-diff`, `log-grep-error-first`, `coordinator-event-summarizer`, `performer-log-summary`. All tagged `hook-bundle` with `category: "hook-bundle"` and per-tool targets.
- **`SkillsPolicy`** config type — `require_pin`, `allow_mutable_refs`, `conflict_policy`, `offline_uses_lockfile_only`, `write_ownership_markers` settings.
- **Tests** — 12 unit tests covering cache key generation, SHA digest, manifest path-escape validation, lockfile round-trip, conflict detection, policy defaults, and hook bundle presence.

### Added — Reference Branch Preflight Gate (spec §6)
- **`core/src/coordinator/preflight.rs`** — new module with all preflight logic: `inspect_reference_branch_preflight()`, `create_reference_branch()`, structured `ReferenceBranchPreflightReport`, `ReferencePreflightStatus`, `ReferencePreflightAction`, `ReferenceBranchPreflightConfig`, `MissingBranchPolicy`, `DirtyReferencePolicy`, `BranchCreateSourcePolicy`, `build_preflight_log_event()`, `format_report_cli()`, `is_blocking()`.
- **Git helpers** added to `core/src/git.rs`: `check_ref_format_branch()`, `local_branch_exists()`, `remote_tracking_refs_for_branch()`, `worktrees_for_branch()`, `status_porcelain_v1()` (with `GitPorcelainEntry`), `create_branch_at()`, `create_tracking_branch()`, `is_bare_repository()`.
- **Config** — `ReferenceBranchPreflightConfigRaw` deserialized from `automation.coordinator.reference_branch_preflight`; `require_clean_reference_branch: bool` MVP field on `CoordinatorConfig`; both resolve into `CoordinatorConfigResolved.reference_branch_preflight: ReferenceBranchPreflightConfig`.
- **CLI flags** on `macc coordinator`: `--preflight-only` (check and exit), `--allow-dirty-reference` (override dirty block), `--create-reference-branch` (non-interactive creation), `--reference-branch-base <branch>` (base for creation).
- **Coordinator integration** — `run_reference_branch_preflight()` called before `CoordinatorCommand::RunControlPlane` / `DispatchReadyTasks`: resolves config, handles interactive/non-interactive missing-branch and dirty-branch flows, logs result to `.macc/log/coordinator/preflight-latest.json`.
- **TUI Automation screen** — fields 38–39 added: `[Preflight] Require Clean Reference Branch` and `[Preflight] Preflight Enabled` with help text and display values.
- **Web API** — `POST /api/v1/coordinator/preflight` runs inspection and returns `ReferenceBranchPreflightReport` as JSON; `POST /api/v1/coordinator/preflight/create-reference-branch` creates the local branch (caution-level, audit-gated).
- **Error codes** `E701`–`E707` (Git/Preflight range) documented in README and implemented in `preflight.rs`.
- **Unit tests** (15 cases): dirty policy block/warn/allow, missing policy fail/create-non-interactive, `is_blocking`, `format_report_cli`, `build_preflight_log_event`.
- **Integration tests** (6 cases using real Git repos): clean branch, missing branch, invalid name, dirty branch blocked, dirty branch warn, create branch from HEAD.

### Added — Usability, Onboarding, and README Improvement (spec §5)
- **`macc quickstart` extended**: new flags `--tool`, `--starter-task`, `--start-coordinator`, `--check-only`, `--demo`; interactive tool selection from detected adapters; starter task creation (`QS-001`) when no PRD exists; teaching mode prints equivalent manual commands at the end.
- **`macc doctor` extended**: `--json` flag emits structured `DiagnosticFinding` list with stable error codes (`MACC-GIT-IDENTITY-MISSING`, `MACC-COORDINATOR-IPC-MISSING`, etc.); `--fix --git-name "…" --git-email "…"` applies git identity locally; `--coordinator` filters to coordinator group; readiness summary printed at the end of human output.
- **New diagnostic checks** (spec §5.3): disk-space check against formula `max(repo_size × max_parallel × 1.25, 2 GB)`; coordinator IPC check reads `coordinator.sqlite` and verifies PID liveness; task-readiness check inspects `prd.json` for dispatchable tasks.
- **`DiagnosticFinding` / `DiagnosticSeverity`** shared types (spec §14.2) in `core/src/doctor.rs` — stable IDs, category, message, recommended action, fix availability.
- **`ReadinessLadder`** (spec §9): `core::onboarding` module with `compute_readiness(paths)` assembling an 8-step ladder from live project state; persisted as `.macc/state/onboarding.json`.
- **`macc status` enhanced** (spec §8): coordinator PID/uptime/mode section; worktree summary; health block with throttled-tool countdowns; stale-worker `▲` flag; `--events N` (default 5) and `--verbose` flags; degraded output when coordinator is absent.
- **TUI Home screen readiness checklist** (spec §13.1): right panel now shows the 8-step readiness ladder computed from `core::onboarding::compute_readiness()`, replacing the static "Next Steps" text.
- **Web Welcome page readiness cards** (spec §13.2): `Welcome.tsx` now fetches `GET /api/v1/snapshot`, computes a 5-item readiness list (project, tool, config, PRD/task, coordinator), and renders it with per-step action links; `blockingCount` summary links to `/ops/diagnostics`.
- **New stable error codes**: `MACC-GIT-IDENTITY-MISSING`, `MACC-COORDINATOR-IPC-MISSING`, `MACC-COORDINATOR-IPC-STALE`, `MACC-TASK-NONE-READY`, `MACC-TOOL-NOT-RUNNABLE`, `MACC-WORKTREE-DISK-LOW`, `MACC-CONFIG-NOT-APPLIED`.
- **README redesign** (spec §10-11): onboarding-first structure — 30-second quickstart at the top, visual overview Mermaid diagram, "Why MACC?" comparison table, core-workflows table, first-run readiness ladder, structured troubleshooting, error-code table, collapsible advanced sections.

## [0.2.0] - 2026-05-31

### Added
- Web UI coverage for `Welcome`, `Init`, `Dashboard`, config subpages, PRD/Plan/Apply, ops views, and the supporting `Help`/`About` pages.
- Web UI coverage for notifications, global search, worktree terminal drawers, and terminal sessions for project and worktree PTYs.
- Web UI contributor workflows in the top-level README for development, linting, testing, and production-like serving.
- Web API contract documentation for the full `/api/v1` surface, including config, PRD, plan, apply, worktrees, registry, logs, doctor, backups, terminal, SSE, and coordinator actions.
- Web error catalog entries for confirmation, not-found, conflict, dependency, and terminal handling.
- Notifications center in the app shell for coordinator alerts and task status.
- Real-time notification system using SSE event stream.
- New notification store and hook for global notification management.
- Notifications drawer for viewing and dismissing notifications.
- Coordinator full-cycle command and stop flow improvements.
- Worktree/performer logging improvements and `macc logs tail`.
- Embedded automation/tool catalog defaults for clean-machine bootstrap.
- TUI improvements (status/footer, search filtering, undo/redo).
- GitHub `curl -sSL` install path via `scripts/install.sh`, including source fetch (`--repo`/`--ref`) when not running from a local clone.
- Installed `macc-uninstall` helper alongside `macc`.
- **Task Lifecycle & Visibility Layer** (spec §3–26):
  - Extended `WorkflowState` enum with `Testing` and `Reviewing` states for the full `dev → test → review → merge` pipeline.
  - Added valid FSM transitions to/from `Testing` and `Reviewing` (spec §17).
  - Expanded `TaskRuntime` model with `worker_id`, `message`, `started_at`, `stdout_log`, `stderr_log`, `events_log`, and `branch` fields for live observability.
  - Redesigned Live Tasks TUI compact row: health symbol (`●/◐/▲/!/✓/·`) + worker + task ID + `RUN dev`-style label + tool + relative age + relative heartbeat + current message (spec §6.1).
  - `Testing` and `Reviewing` states included in active task detection in TUI.
  - Added `macc explain <task-id>` CLI command — prints chronological task timeline with runtime info, log pointers, and structured events (spec §11).
  - Added `macc diff <task-id>` CLI command — resolves active worktree and runs `git diff` without requiring `cd` (spec §12, with fallback to commit-based diff).
  - Added `PhaseConfig` and `PhasesConfig` to `CoordinatorConfig` for independent testing/review phase control (spec §16).
  - Default phases: `testing.enabled=false, testing.mode=disabled`; `review.enabled=true, review.mode=required` (spec §25.3 conservative default).
  - TUI Automation screen exposes Testing and Review phase toggles as settings fields 34–37 (spec §19).
- **UX, Observability, Skill Runner, and Web Client** (spec §4):
  - **U1 — TUI Observer Mode**: `macc status [--json|--watch|--control]` and `macc watch` alias open a live Ratatui cockpit sourced from the shared `RuntimeSnapshot`. The Watch screen shows active workers (with timestamp-based stale detection > 180 s highlighted ▲), queue counts, recent events, throttled tools, a paused-coordinator banner, and a coordinator log tail. Supports `--logs-only` and `--events-only` filter flags. Polling at 2 Hz, independent of the Coordinator Live screen.
  - **U2 — Unified Skill Runner**: `macc run <skill>` executes canonical MACC skills defined in `.macc/skills/*.yaml`. Flags: `--tool`, `--dry-run`, `--watch` (streaming output), `--json`, `--yes`. Tool is resolved via an 8-step algorithm (flag → worktree.json → tool.json → `skills.run_policy.default_tool` → `tool_priority` → first enabled). Worktree-aware: `--task <id>` finds the active worktree and runs there. Risk gate: caution → confirm, dangerous → type `YES`. Logs written to `.macc/log/run/<ts>-<skill>.jsonl` and `.macc/log/run/<ts>-<skill>.log` (spec §3.10).
  - `macc skills list|show|explain|doctor` — catalog inspection commands.
  - `ToolAdapter` trait extended with `supports_skill_run()`, `supports_prompt_stdin()`, `supports_skill_install()`, `supports_session_resume()`, and `build_skill_invocation()` (spec §3.6). Adapters declare their execution strategy; the runner uses the result as a subprocess spec.
  - **U3 — Web Client**: new endpoints `GET /api/v1/snapshot`, `GET /api/v1/search?q=`, `GET /api/v1/skills[/{id}[/dry-run|/run]]`, `GET /api/v1/runs[/{id}[/logs]]`, `GET /api/v1/failures/recent`, `GET /api/v1/workers/{id}/snapshot`. Skill run web handler moved to `tokio::task::spawn_blocking` to avoid blocking the Axum thread pool. Dashboard wired to `GET /api/v1/snapshot` for `QueueSummary` KPI cards. Skill Runner page at `/ops/skill-runner`. CommandPalette wired to live skills list. `ApiRuntimeSnapshot` and `ApiSkillItem` types added to web client models.
  - **U4 — Token/Context Budget**: `core::context` module with five summarisation bundles (`test-output-failures-only`, `lint-errors-only`, `stacktrace-collapse`, `git-diff-stat-before-full-diff`, `log-grep-error-first`) and `enforce_budget()`. `SkillsConfig` and `ContextConfig` now surfaced in `ResolvedConfig`. `engine.run_skill()` applies the summarisation pipeline and budget enforcement after collecting output; results include `SummaryMetadata` (raw/summary size, bundles applied, truncation flag) shown in CLI and Skill Runner web page (spec §5.6).
  - **Shared runtime model**: `core::runtime` module with `RuntimeSnapshot`, `WorkerRuntime`, `QueueSummary`, `ToolThrottleStatus`, `SkillRunSummary`, `GitRuntimeSummary`. `RuntimeSnapshotProvider` trait introduced with a `ProjectPaths` blanket implementation. `CoordinatorStatus` populated from the pause file and active `coordinator_runs` SQLite row. Event parser normalises both v1 schema (`"version"`/`"timestamp"`/`"type"`) and legacy format. `Engine` trait gains `runtime_snapshot()`, `list_skills()`, `resolve_skill()`, `dry_run_skill()`, `run_skill()`, `resolve_skill_tool()`, and `find_task_worktree()` — all accessed through the facade, never directly from clients.

### Changed
- Documentation rationalization (`docs/README.md` as docs index, historical docs marked).
- `scripts/uninstall.sh` now supports installed-helper usage and removes both `macc` and `macc-uninstall` by default.

### Fixed
- Preview/TUI display stability by silencing fetch logs in quiet mode and improving redraw behavior.

## [0.1.0] - 2026-02-13

### Added
- Initial public baseline of MACC:
  - canonical config + `plan`/`apply`,
  - tool registry/adapters,
  - TUI flows,
  - worktree and coordinator automation,
  - backup/restore/doctor.

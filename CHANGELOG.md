# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

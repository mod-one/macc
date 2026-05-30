# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

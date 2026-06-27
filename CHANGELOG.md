# Changelog

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

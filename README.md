# MACC

MACC (Multi-Agentic Coding Config) is a agentic coding tool configuration manager. It generates tool-specific files (Claude code, Codex, Gemini Cli, etc.) via adapters.

It also integrates an autonomous AI agent loop that runs Installed agentic coding tool.
They can run on the same project in parallel (using worktrees) repeatedly until all assigned tasks are completed. All of this is managed by a coordinator and can be done autonomously or semi-autonomously.

## What MACC provides

- Canonical config and deterministic generation (`plan` then `apply`).
- Tool-agnostic TUI for tool selection, tool settings, skills, MCP, automation coordinator settings, and global preferences.
- Embedded defaults for ToolSpecs and catalogs so clean machines are usable immediately.
- Project automation with embedded `coordinator.sh` + `performer.sh` + per-tool runners.
- Worktree orchestration for parallel task execution.
- Safe cleanup (`macc clear`) with confirmation: removes worktrees first, then MACC-managed project artifacts.

## Installation

### Recommended: install from GitHub (`curl -sSL`)

```bash
curl -sSL https://raw.githubusercontent.com/Brand201/macc/master/scripts/install.sh | bash -s -- --release
```

Pinned release/tag example:

```bash
curl -sSL https://raw.githubusercontent.com/Brand201/macc/v0.1.0/scripts/install.sh | bash -s -- --release --ref v0.1.0
```

### Alternative: install from a local clone

```bash
git clone https://github.com/Brand201/macc.git
cd macc
./scripts/install.sh --release
```

Options:

- `--release`: build optimized binary.
- `--prefix <dir>`: install `macc` into a custom directory.
- `--system`: install to `/usr/local/bin` (uses `sudo`).
- `--no-path`: do not update shell profile `PATH`.
- `--repo <url>`: install from another git repository URL.
- `--ref <ref>`: install from branch/tag/commit (default: `master`).

### Uninstall

Recommended:

```bash
macc-uninstall
```

Alternative from a local clone:

```bash
./scripts/uninstall.sh
```

Uninstall directly from GitHub script:

```bash
curl -sSL https://raw.githubusercontent.com/Brand201/macc/master/scripts/uninstall.sh | bash
```

Options:

- `--system`: remove `/usr/local/bin/macc`.
- `--prefix <dir>`: remove `<dir>/macc` and `<dir>/macc-uninstall`.
- `--clean-profile`: remove installer-added `PATH` lines from shell profiles.
- `--keep-helper`: keep `macc-uninstall` while removing `macc`.

## Quick start

1. Initialize a project:

```bash
macc init
```

2. Open the TUI:

```bash
macc tui
```

3. Preview and apply changes:

```bash
macc plan
macc apply
```

4. If user-scope writes are needed (for example in `~/.claude`), explicitly allow them:

```bash
macc apply --allow-user-scope
```

### Default startup behavior

- `macc` (no subcommand) runs `init` if needed, then opens the TUI.
- `macc tui` also ensures initialization first.

## Web UI

The web UI lives in [`web/`](web) and talks to the MACC backend served by `macc web`.
These commands assume `macc` is already available in `PATH` (for example via `./scripts/install.sh --release`).

The web app now covers these areas:

- `Welcome` and `Init`: first-run onboarding, environment checks, and project bootstrap.
- `Dashboard`: coordinator health, worktree status, and live orchestration summary.
- `Config / Tools`, `Config / Standards`, `Config / Skills`, and `Config / Settings`: tool selection, standards preview, skill selection, and global/web settings.
- `PRD`, `Plan`, and `Apply`: PRD editing, plan previews, and apply execution.
- `Ops / Console`: coordinator live controls and runtime state.
- `Ops / Registry`: task registry inspection and task actions.
- `Ops / Worktrees` and `Ops / Live`: worktree management, concurrent log tiles, and per-worktree terminal access.
- `Ops / Worker Detail`: focused task/worktree drill-down.
- `Ops / Locks`: task/worktree lock visualization.
- `Ops / Logs`: coordinator event stream, log browser, and log tailing.
- `Ops / Diagnostics`: doctor report and safe-fix workflow.
- `Ops / Backups`: backup browsing and restore.
- `Ops / Git`: repository graph view.
- Global search and the notifications drawer: quick navigation, task discovery, and coordinator alerts.
- Terminal drawer sessions: project-root and worktree PTY access over WebSocket.
- `Help` and `About`: supporting information pages.

Development workflow:

```bash
cd web
npm install
npm run dev
```

In a second terminal, start the backend from the repo root:

```bash
macc web
```

Open `http://localhost:5173`. The Vite dev server proxies `/api` requests to `http://localhost:3450`, so the UI can hit the local MACC backend without any extra configuration.

For frontend changes, run `npm run lint` and `npm test` before building.

Production-like workflow (`web/dist` served by the backend):

```bash
cd web
npm install
npm run build
cd ..
macc web --assets dist
```

Open `http://localhost:3450`.

Embedded mode note: `macc web --assets embedded` serves assets baked into the binary. Build `web/dist` first, then rebuild the `macc` binary so the latest frontend is embedded.

## Operational runbook (blank machine -> full automation cycle)

### 1) Prepare a blank machine

1. Install base dependencies:

```bash
sudo apt-get update
sudo apt-get install -y git curl jq build-essential pkg-config libssl-dev
```

2. Install MACC (recommended: GitHub installer):

```bash
curl -sSL https://raw.githubusercontent.com/Brand201/macc/master/scripts/install.sh | bash -s -- --release
```

Alternative (local clone):

```bash
git clone https://github.com/Brand201/macc.git
cd macc
./scripts/install.sh --release
```

3. Verify:

```bash
macc --version
```

### 2) Install AI tools (Codex / Claude / Gemini)

1. Open TUI:

```bash
macc tui
```

2. Go to `Tools`:
- missing tools are shown as not installed,
- press `i` to run install action for the selected tool.

3. Confirm account/API-key prerequisite when prompted.
4. Complete tool login/API setup when the installer opens the tool command.
5. Run health checks in Tools view (`d`) or:

```bash
macc doctor
```

### 3) Initialize a target project

In your project directory:

```bash
macc init
macc tui
```

In TUI:
- enable tools,
- set tool defaults (models, approvals, etc.),
- configure `Automation / Coordinator` (base/reference branch, max dispatch, max parallel, staleness policy),
- save.

Then apply:

```bash
macc apply
```

### 4) Run coordinator full cycle

Prepare task source (for example PRD JSON), then run:

```bash
macc coordinator
```

`macc coordinator` runs the full loop (`sync -> dispatch -> advance -> reconcile -> cleanup`) until convergence.
If a merge conflict stays unresolved (even after AI merge-fix hook), coordinator enters a paused state and waits for operator signal:
```bash
macc coordinator resume
```

Useful commands during execution:

```bash
macc coordinator status
macc coordinator sync
macc coordinator sync-prd       # reconcile tasks from commit history
macc coordinator reconcile
```

After a run completes, optionally enrich the PRD with AI-generated notes:

```bash
macc coordinator audit-prd -- --tool claude --dry-run  # preview the prompt
macc coordinator audit-prd -- --tool claude             # invoke the tool
```

## Error codes and auto-retry

MACC emits structured error codes when a performer or coordinator step fails. These codes are written into coordinator events and task runtime metadata.

### Error code schema (v1)

- `E100` Runner/Tool
  - `E101` Runner exited non-zero
  - `E102` Tool runner not found / not executable
  - `E103` Tool output malformed / parsing failed
  - `E104` Performer produced partial changes before failure. Conditional retry (prefer same-worktree salvage).
  - `E105` Performer completed output but exited non-zero. Conditional retry (verify completion state first).
- `E200` Capability/Contract
  - `E201` Requested unavailable tool
  - `E202` Capability guard triggered
- `E300` Worktree/FS
  - `E301` Worktree missing
  - `E302` PRD missing
  - `E303` tool.json missing
  - `E304` Worktree branch conflict. Conditional retry (after conflict resolution or branch refresh).
  - `E305` Worktree checkout failure. Conditional retry (transient git failures may clear).
  - `E306` Worktree reset failure. Conditional retry (depends on index/worktree state).
- `E400` Coordinator/Registry
  - `E401` Task registry read/write failure
  - `E402` Task state transition invalid
  - `E403` Task state conflict. Retryable (reconcile + retry usually converges).
- `E500` Merge
  - `E501` Merge conflict
  - `E502` Merge worker failed
  - `E503` Merge blocked by policy. **Not retryable** - requires operator or policy change.
  - `E504` Post-merge validation failed. Conditional retry (depends on validation failure root cause).
- `E600` Rate-limit / Provider throttle
  - `E601` Rate-limited / transient 429 or 529. Retryable with exponential backoff.
  - `E602` Quota exhausted / hard limit. **Not retryable** - requires operator action.
  - `E603` Session conflict. Retryable with a new session.
- `E900` Unknown/Unexpected
  - `E901` Unknown fatal error

### Auto-retry policy (coordinator)

Coordinator can auto-retry failed tasks based on error code. This is configured in `.macc/macc.yaml` under `automation.coordinator`:

- `error_code_retry_list` default: `E101,E102,E103,E301,E302,E303,E601,E603`
- `error_code_retry_max` default: `2`
- `max_dispatch_retries` default: `5` (caps dispatch preparation retries before task is blocked with `dispatch_retry_limit_exceeded`)

When a failed task has an error code in the allow-list and retries are below the max, the task is requeued to `todo` with an `auto_retry:<code>` reason.

E602 (quota exhausted) is intentionally excluded - it requires operator action, not automatic retry.

Retry strategy details:

- Fetch failures during dispatch sanitize are non-blocking warnings: if `git fetch origin` fails during worktree preparation, coordinator logs a warning and continues. Dispatch still aborts on required sanitize failures (for example `git reset --hard` failure).
- `salvage_before_retry` (default `true`): before retrying a failed task with existing local work, coordinator attempts salvage/merge of the last worktree state.
- `retry_on_same_worktree` (default `true`): retries prefer reusing the same worktree slot when the slot is healthy, so local context and session continuity are preserved.
- `merge_gate_on_dispatch` (default `true`): for retry attempts, coordinator runs a pre-dispatch merge-gate; if prior branch can already merge cleanly, task is marked merged and dispatch is skipped.
- `sync_unmerged_branches` (default `true`): `sync` also scans recoverable unmerged branches and can merge recovered work before another retry cycle.
- `max_salvage_attempts_per_task` (default `1`) and `salvage_merge_timeout_seconds` (default `120`) bound salvage cost during recovery.
- `tag_abandoned_branches` (default `true`): when a retry path abandons a branch with commits, a tag is created before reset to avoid silent loss.

All knobs live under `.macc/macc.yaml` -> `automation.coordinator` (see `docs/CONFIG.md`).

### Rate-limit handling

MACC automatically handles provider rate-limits (HTTP 429/529) via:

- **Canonical error normalization**: per-adapter normalizers (Claude, Codex, Gemini) classify errors into `CanonicalClass::RateLimit` (E601) or `CanonicalClass::QuotaExhausted` (E602).
- **Exponential backoff**: E601 tasks get a `delayed_until` timestamp; the coordinator skips them until the delay expires.
- **Tool fallback**: when the primary tool is throttled, the coordinator dispatches to the next tool in `tool_priority` (if `rate_limit_fallback_enabled=true`).
- **Concurrency reduction**: `effective_max_parallel` is reduced on each E601 and restored on recovery (if `rate_limit_throttle_parallel=true`).
- **Quota hard stop**: E602 pauses the coordinator with a specific "quota exhausted" banner; resume with `macc coordinator resume` after fixing quota.

Rate-limit controls in `.macc/macc.yaml` -> `automation.coordinator`:
```yaml
rate_limit_backoff_base_seconds: 60
rate_limit_backoff_max_seconds: 3600
rate_limit_fallback_enabled: true
rate_limit_throttle_parallel: true
```

Logs:
- coordinator: `.macc/log/coordinator/`
- performer: `.macc/log/performer/`

### 5) Failure recovery playbook

When tasks fail/block:

1. Inspect status and logs:

```bash
macc coordinator status
ls -la .macc/log/coordinator
ls -la .macc/log/performer
```

2. Attempt deterministic recovery:

```bash
macc coordinator sync-prd
macc coordinator reconcile
macc coordinator unlock
macc coordinator cleanup
```

3. Resume cycle:

```bash
macc coordinator
```

4. Stop safely if needed:

```bash
macc coordinator stop --graceful
```

5. Hard stop and cleanup worktrees/branches if required:

```bash
macc coordinator stop --remove-worktrees --remove-branches
```

6. If project state must be reset to pre-MACC managed artifacts:

```bash
macc clear
```

`macc clear` asks confirmation, runs forced worktree cleanup first, then removes MACC-managed paths only.

## Core commands

All commands support these global flags:
- `-q, --quiet`: suppress non-essential output.
- `--offline`: disable all network operations.
- `--web-port <PORT>`: set the port for the web interface.

### Project lifecycle

- `macc init [--force] [--wizard] [--profile <name>]`: create/update `.macc/` layout and default config (`--wizard` asks 3 setup questions, `--profile` restores a saved profile after init).
- `macc quickstart [-y|--yes] [--apply] [--no-tui]`: zero-friction happy path (checks prerequisites, initializes, seeds defaults, opens TUI or runs plan+apply).
- `macc plan [--tools tool1,tool2] [--json] [--explain]`: build preview only (no writes), with machine-readable JSON/explanations when needed.
- `macc apply [--tools ...] [--dry-run] [--allow-user-scope] [--json] [--explain]`: apply planned writes (`--dry-run` behaves as plan with same preview modes).
- `macc backups list [--user]`: list available backup sets (project or user-level).
- `macc backups open <id>|--latest [--user] [--editor <cmd>]`: print/open a backup set location.
- `macc restore --latest [--user] [--dry-run] [-y]` (or `--backup <id>`): restore files from a backup set.
- `macc clear`: asks confirmation, removes all non-root worktrees with force, then removes MACC-managed files/directories in the current project.
- `macc start [--intent <configure-tools|run-one-task|run-batch|inspect-project>] [--preset <conservative|balanced|throughput>] [--web] [--tui] [--dry-run]`: guided cockpit startup entry point composing detect -> resolve -> preview -> confirm -> apply -> launch.
- `macc trust`: audit repository safety state (local-only, terminal permissions, backups, catalog pinning, secrets).
- `macc lock <generate|check|diff|explain>`: manage build reproducibility lock manifest (`.macc/macc.lock.yaml`).
- `macc failure <list|show <id>|retry <id>|salvage <id>|restore <id>|inspect-diff <id>`: guided recovery for task failures.
- `macc settings <show [--advanced] [--admin]|preset <name>>`: manage progressive settings and presets (conservative, balanced, throughput).
- `macc migrate [--apply]`: migrate legacy config to current format.
- `macc doctor [--fix]`: actionable diagnostics (tools, paths/permissions, worktrees/sessions, cache health). `--fix` applies safe fixes only (create missing dirs, add `.macc/cache/` to `.gitignore`, repair session state file when corrupt).

### Configuration profiles

Save and reuse project configurations across repositories. Profiles are stored
under `~/.macc/profiles/` and serialize the full `CanonicalConfig`, so new config
fields added in the future are automatically included without code changes.

- `macc config save <name> [--description "..."] [--only sections]`: save current config as a profile.
- `macc config restore <name> [--only sections]`: restore a profile into the current project.
- `macc config list`: list saved profiles.
- `macc config delete <name>`: delete a profile.

The `--only` flag accepts a comma-separated list of sections:
`tools`, `standards`, `selections`, `automation`, `settings`, `mcp_templates`.

Absolute paths are automatically stripped to filenames on save for portability.

### TUI and tools

- `macc tui`: open interactive UI.
- `macc tool install <tool_id> [-y|--yes]`: install local tool via ToolSpec-defined install workflow.
- `macc tool update <tool_id> [--check] [-y|--yes] [--force] [--rollback-on-fail]`: update one installed tool.
- `macc tool update --all [--only enabled|installed] [--check] [-y|--yes] [--force] [--rollback-on-fail]`: batch update tools.
- `macc tool outdated [--only enabled|installed]`: show installed/current/latest status and outdated tools.
- `macc context [--tool <tool_id>] [--from <file> ...] [--dry-run] [--print-prompt]`: sends a context prompt to the selected AI tool; the tool must edit its target context file in-place (for example `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`).
- In TUI `Tools` screen, press `f` to generate context for the selected tool.
- To prevent `macc apply` from overwriting existing context files, set per-tool protection in `.macc/macc.yaml`:
  - `tools.config.<tool_id>.context.protect: true`

### Catalog and installs

- `macc catalog skills list|search|add|remove`
- `macc catalog mcp list|search|add|remove`
- `macc catalog import-url --kind <skill|mcp> ...`
- `macc catalog search-remote --kind <skill|mcp> --q <query> [--add|--add-ids ...]`
- `macc install skill --tool <tool_id> --id <skill_id>`
- `macc install mcp --id <mcp_id>`

`macc catalog import-url` now prints:
- parsed source understanding (kind/url/ref/subpath),
- immediate validation status (subpath/manifest when source can be materialized),
- trust hints (pinned ref/checksum presence).
Hints are informational only and do not guarantee security.

### Worktrees

Worktrees let MACC run multiple isolated task branches and tool sessions in parallel without contaminating each other, while keeping the main repo clean and reviewable.

- `macc worktree create <slug> --tool <tool_id> [--count N] [--base BRANCH] [--scope CSV] [--feature LABEL] [--skip-apply] [--allow-user-scope]`
- `macc worktree list`
- `macc worktree status`
- `macc worktree open <id|path> [--editor <cmd>] [--terminal]`
- `macc worktree apply <id|path> [--allow-user-scope]`
- `macc worktree apply --all [--allow-user-scope]`
- `macc worktree doctor <id|path>`
- `macc worktree run <id|path>`
- `macc worktree exec <id|path> -- <cmd...>`
- `macc worktree remove <id|path> [--force] [--remove-branch]`
- `macc worktree remove --all [--force] [--remove-branch]`
- `macc worktree prune`

### Coordinator

Coordinator orchestrates the end-to-end automation cycle: it reads the task registry, dispatches work to tools, tracks state transitions, supervises performers, and reconciles/cleans up until convergence.

- `macc coordinator` (default full cycle: sync -> dispatch -> advance -> reconcile -> cleanup in loop until convergence)
- `macc coordinator` opens the TUI `Coordinator Live` screen and starts coordinator run.
- `macc coordinator [run|dispatch|advance|sync|sync-prd|audit-prd|status|reconcile|unlock|cleanup|stop|sessions]`
- `macc coordinator run --no-tui` keeps the previous headless CLI behavior.
- `macc coordinator stop [--graceful] [--remove-worktrees] [--remove-branches]`
- `macc coordinator sessions [save|restore|list|delete]`: manage tool session snapshots at user level (`~/.macc/sessions/<project>/`). `save` captures active + archived sessions; `restore` merges saved sessions back into `tool-sessions.json` without overwriting existing entries; `list` shows all snapshots; `delete` removes one. Graceful stop (`--graceful`) auto-saves a snapshot.
- `macc supervisor start [--daemon]`
- `macc supervisor stop`
- `macc supervisor status`
- `macc supervisor report`
- Coordinator options can override config at runtime:
  - `--prd`, `--coordinator-tool`
  - `--tool-priority`, `--max-parallel-per-tool-json`, `--tool-specializations-json`
  - `--max-dispatch`, `--max-parallel`, `--timeout-seconds`
  - `--phase-runner-max-attempts`
  - `--stale-claimed-seconds`, `--stale-changes-requested-seconds`, `--stale-action`: auto-stale thresholds for `claimed`/`changes_requested` states (`0` disables).
  - `--stale-in-progress-seconds`: hard kill timeout for the performer process in seconds (`0` disables). When exceeded, the coordinator sends SIGTERM then SIGKILL after `force_kill_grace_seconds`. Default `0` — tasks run until they exit naturally. Set to e.g. `3600` to cap task runtime at one hour.
  - Heartbeat events update `task_runtime.last_heartbeat` from `events.jsonl`.
  - Runtime stale heartbeat policy via env: `STALE_HEARTBEAT_SECONDS`, `STALE_HEARTBEAT_ACTION=retry|block|requeue` (retry/requeue resets task to `todo`; retry also increments runtime retries).
- Task registry path is fixed to `.macc/automation/task/task_registry.json`.
- Coordinator emits event bus lines to `.macc/log/coordinator/events.jsonl` (used by TUI live screen).
- `run`, `dispatch`, `advance`, `reconcile`, and `cleanup` are executed by native Rust handlers (async supervision + retries/timeouts per phase).
- Legacy shell coordinator removed; all coordinator actions run natively in Rust.
- Worktrees are managed as a reusable worker pool (not task-named): a merged/clean slot is reset to the reference branch, switched to a fresh branch, updated (`worktree.prd.json` + apply), then reused for the next task.
- If no reusable slot is available, coordinator creates a new worker worktree; total pool size is bounded by `--max-parallel` / `automation.coordinator.max_parallel`.
- Realtime orchestrator target design (state model + event contract + rollout): `docs/COORDINATOR_REALTIME.md`.
- Use `--` only for coordinator subcommands that require raw passthrough args.

## TUI overview

Main screens:

- Home
- Tools
- Tool Settings
- Automation / Coordinator (settings only)
- Coordinator Live (runtime monitoring)
- Skills
- MCP
- Global Settings
- Logs
- Preview
- Apply

Common keys:

- Navigation: `h` Home, `t` Tools, `o` Automation, `e` Settings, `v` Coordinator Live, `m` MCP, `g` Logs, `p` Preview
- Save/apply: `s` Save config, `x` Apply
- Help: `?`
- Back: `Backspace`
- Quit: `q` / `Esc`

Tools screen includes:

- Toggle enabled tools.
- Open tool-specific settings.
- Install missing tools (`i`) using ToolSpec-defined install workflow.
- Refresh doctor checks (`d`).

## Configuration model

Primary file:

- `.macc/macc.yaml`

Important paths:

- `.macc/backups/` for project backups.
- `.macc/tmp/` for temporary files.
- `.macc/cache/` for fetched packages.
- `.macc/skills/` for local skills.
- `.macc/catalog/skills.catalog.json` and `.macc/catalog/mcp.catalog.json`.
- `.macc/automation/` for embedded coordinator/performer scripts and runners.
- `.macc/log/coordinator/` and `.macc/log/performer/` for centralized runtime logs.
- `.macc/state/managed_paths.json` for safe cleanup tracking.
- `.macc/state/tool-sessions.json` for performer session leasing/reuse.

## ToolSpec and catalog layering

### ToolSpecs (effective precedence: low -> high)

1. Built-in ToolSpecs embedded in the binary.
2. User overrides in `~/.config/macc/tools.d`.
3. Project overrides in `.macc/tools.d`.

### Catalogs

- Built-in skills/MCP catalogs are embedded in the binary.
- Project catalog files in `.macc/catalog/*.catalog.json` are local overrides and editable.
- Local skills in `.macc/skills/<id>/` are auto-discovered and shown in TUI/selection.

## Automation: coordinator + performer

MACC installs embedded automation assets into `.macc/automation/`:

- Native Rust coordinator control-plane (primary runtime path for `run` + core actions).
- `coordinator.sh`: thin wrapper for native Rust coordinator actions.
- `performer.sh`: worktree executor.
- `runners/<tool>.performer.sh`: tool-specific execution scripts.
- All automation logs are written under `.macc/log/` (coordinator + performer).

Coordinator defaults and advanced settings live in:

- `.macc/macc.yaml` under `automation.coordinator`

You can edit these settings visually in the TUI Automation screen or override them with `macc coordinator` flags. Legacy environment variables are no longer used for coordinator configuration.

## Commit convention and PRD reconciliation

MACC enforces a unified commit message format across all performers and merge workers:

```
<type>: <TASK-ID> - <title>

[macc:task <TASK-ID>]
[macc:phase <phase>]
[macc:tool <tool>]
```

This convention enables two post-run features:

### `sync-prd` - deterministic reconciliation

`macc coordinator sync-prd` scans the reference branch for committed task IDs and transitions matching tasks to `merged`. No AI involved - pure pattern matching. Also runs automatically as part of `macc coordinator sync`.

### `audit-prd` - AI-powered PRD enrichment

`macc coordinator audit-prd -- --tool <tool_id>` gathers commit context for completed tasks and builds a structured prompt for an LLM to:
- Update `notes` of completed tasks with what was actually delivered.
- Rewrite `description`/`steps` of todo tasks if integrated code changed the architecture.
- Task IDs are never deleted or renamed.

Use `--dry-run` to preview the prompt without invoking any tool.

## Session strategy

Performer session management is project-level, tool-aware, and lease-based:

- Session state file: `.macc/state/tool-sessions.json`
- Default isolation scope: per worktree (prevents cross-worktree context contamination).
- Sessions are reused in serial execution when available and not leased by active work.
- If all known sessions are occupied (or none exist), a new session is created.
- Lease release happens on performer exit, so closed worktrees can donate reusable sessions.
- Session snapshots can be saved to user-level storage (`~/.macc/sessions/`) and restored across coordinator runs via `macc coordinator sessions save|restore`. Graceful stop auto-saves a snapshot.
- Coordinator retries preserve session continuity: on `error_with_changes` / `error_without_changes`, the last active session ID is stored and reused on the next attempt for the same tool when available.

### Session optimization

- Dispatch is session-aware: coordinator prefers reuse paths that keep warm tool/worktree context when possible instead of forcing cold starts.
- Scheduling is TTL-aware: recent worktree/session activity is favored so cached context is reused before it expires.
- `session_cache_ttl_seconds` (default `300`) controls how long a session is considered warm for dispatch preference; higher values bias reuse, lower values bias fresh distribution.

## Safety guarantees

- Writes are atomic and idempotent.
- Backups are created for changed project files.
- User-scope writes require explicit `--allow-user-scope` plus an interactive confirmation showing touched paths, backup location, and restore commands.
- Secret checks block unsafe generated output.
- `macc clear` is a two-step cleanup: confirm, then run forced worktree cleanup before deleting MACC-managed paths.
- Pre-existing project files/directories are preserved; only MACC-managed artifacts are removed.

## Documentation map

- `docs/README.md`: documentation index (active vs historical docs).
- `MACC.md`: full architecture/specification.
- `docs/COORDINATOR_REALTIME.md`: short design doc for event-driven coordinator evolution.
- `CHANGELOG.md`: release notes by version (Keep a Changelog format).
- `SECURITY.md`: vulnerability disclosure and supported version policy.
- `docs/CONFIG.md`: canonical config schema and semantics.
- `docs/TOOLSPEC.md`: ToolSpec format and field kinds.
- `docs/CATALOGS.md`: catalog schemas and workflows.
- `docs/TOOL_ONBOARDING.md`: add a tool end-to-end.
- `docs/COMPATIBILITY.md`: OS/MSRV compatibility policy.
- `docs/RELEASE.md`: SemVer/tag/release process.
- `docs/ralph.md`: Ralph automation flow.
- `docs/ADDING_TOOLS.md`: adding new tools/adapters.
- `CONTRIBUTING.md`: contribution workflow and PR quality baseline.

## Quality and release model

- CI runs on GitHub Actions (`.github/workflows/ci.yml`) with:
  - quality checks (format, lint, tests, tool-agnostic guardrails),
  - cross-platform build matrix (Linux/macOS/Windows).
- Releases are tag-driven (`vX.Y.Z`) with SemVer policy.
- Release workflow: `docs/RELEASE.md`
- Compatibility policy (OS + MSRV): `docs/COMPATIBILITY.md`

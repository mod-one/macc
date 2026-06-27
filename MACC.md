# MACC — Multi-Agentic Coding Config

> **MACC** (*Multi-Agentic Coding Config*) orchestrates AI coding tools across a project: canonical configuration generation, parallel worktree execution, coordinator-driven task dispatch, and live observability via TUI and web UI.
> **Last updated**: 2026-06-04
> **Status**: Active — reflects the current production codebase.

---

## Table of contents

1. [Overview](#1-overview)
2. [Architecture](#2-architecture)
3. [Installation](#3-installation)
4. [Configuration — `macc.yaml`](#4-configuration--maccyaml)
5. [CLI reference](#5-cli-reference)
   - [Project setup](#51-project-setup)
   - [Configuration & profiles](#52-configuration--profiles)
   - [Plan & apply](#53-plan--apply)
   - [Tools](#54-tools)
   - [Catalog](#55-catalog)
   - [Skills](#56-skills)
   - [PRD generation & audit](#57-prd-generation--audit)
   - [Coordinator](#58-coordinator)
   - [Supervisor](#59-supervisor)
   - [Status & observability](#510-status--observability)
   - [Worktrees](#511-worktrees)
   - [Logs](#512-logs)
   - [Save, restore & backups](#513-save-restore--backups)
   - [Clear](#514-clear)
   - [Failure diagnostics](#515-failure-diagnostics)
   - [Process ownership](#516-process-ownership)
   - [Lock](#517-lock)
   - [Settings](#518-settings)
   - [Miscellaneous](#519-miscellaneous)
   - [TUI & web UI](#520-tui--web-ui)
6. [Coordinator runtime](#6-coordinator-runtime)
7. [Model routing](#7-model-routing)
8. [PRD system](#8-prd-system)
9. [Skills runner](#9-skills-runner)
10. [Process ownership](#10-process-ownership)
11. [Adapters — supported tools](#11-adapters--supported-tools)
12. [Web API](#12-web-api)
13. [Debug mode](#13-debug-mode)
14. [File layout](#14-file-layout)
15. [Security](#15-security)
16. [Technical stack](#16-technical-stack)

---

## 1. Overview

MACC manages configuration for multiple AI coding assistants from one canonical source (`.macc/macc.yaml`) and runs tasks across parallel git worktrees. Its three main surfaces are:

| Surface | Launch | Purpose |
|---|---|---|
| CLI | `macc <command>` | Scripting, CI, one-shot operations |
| TUI | `macc` or `macc tui` | Interactive operator console |
| Web UI | `macc web` | Browser-based dashboard, config editor, coordinator console |

### Core concepts

- **Canonical config** (`macc.yaml`) — single source of truth; MACC generates tool-specific files from it.
- **Coordinator** — autonomous dispatch engine that reads a PRD, claims worktrees, and runs performers.
- **Performer** — a shell adapter that wraps a tool (Claude, Codex, Agy, etc.) and drives it to complete a task.
- **Worktree** — a git worktree checked out for one parallel task; isolated from `main`.
- **PRD** — a JSON task registry (`prd.json`) listing tasks with IDs, titles, dependencies, and routing hints.
- **Skill** — a YAML-defined workflow (`.macc/skills/<id>.yaml`) that a performer can execute.

---

## 2. Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  Clients  (CLI · TUI · Web)                                     │
│  Never call core service functions directly.                    │
│  All access goes through the Engine trait (facade).            │
└───────────────────────────────┬─────────────────────────────────┘
                                │  Engine trait
┌───────────────────────────────▼─────────────────────────────────┐
│  macc-core                                                      │
│  Business logic: config, plan, apply, coordinator, PRD,        │
│  skills, adapters, storage, doctor, process ownership, etc.    │
└─────────────────────────────────────────────────────────────────┘
```

**Architecture rules:**
- `core/` — pure business logic, tool-agnostic.
- `cli/`, `tui/`, `web/` — clients; they call `Engine` methods only.
- Adapters (`adapters/<tool>/`) translate between MACC's abstract API and each tool's CLI.

### Crate structure

| Crate | Path | Role |
|---|---|---|
| `macc-core` | `core/` | Business logic, coordinator, config, PRD, skills |
| `macc-cli` | `cli/` | All `macc <cmd>` commands |
| `macc-tui` | `tui/` | Ratatui terminal UI |
| `macc-registry` | `registry/` | Built-in tool specs and default registry |
| Adapter crates | `adapters/<tool>/` | Per-tool performer logic |

---

## 3. Installation

```bash
# Linux / macOS
./scripts/install.sh [--tools] [--no-tui] [--prefix ~/.local]

# Windows
./install.ps1 [-Tools] [-NoTui]
```

After installation, `macc` is available in `PATH`. Run `macc --version` and `macc --help` to verify.

### Flags (common)

| Flag | Description |
|---|---|
| `--verbose` / `-v` | Enable verbose logging; also sets `MACC_DEBUG=1` for performers |
| `--cwd <path>` | Override the working directory |
| `--version` | Print version |

---

## 4. Configuration — `macc.yaml`

Project config lives at `.macc/macc.yaml`. Initialize it with `macc init`.

### Top-level sections

| Section | Description |
|---|---|
| `tools` | Enabled adapters and per-tool config |
| `standards` | Inline or path-based coding standards |
| `skills` | Selected skill IDs and catalog sources |
| `automation` | Coordinator settings, phases, model routing |
| `process_ownership` | Heartbeat TTL and takeover policy |
| `prd_generation` | PRD generation defaults |
| `context` | Token budget and summarization policy |

### Key automation fields

```yaml
automation:
  coordinator:
    max_parallel: 4            # global parallel worker cap
    max_dispatch: null         # optional dispatch limit
    timeout_seconds: 3600
    tool_priority: [claude, codex, agy]
    max_parallel_per_tool:
      claude: 2
      codex: 2
    tool_specializations:
      frontend: [vibe, codex]
    phases:
      testing:
        mode: disabled         # disabled | required | risk_based | manual
      review:
        mode: required
        coordinator_tool: claude
    model_routing:
      mode: auto               # auto | manual
```

### Model tier config per tool

Each tool can define model tiers that the coordinator selects from:

```yaml
tools:
  config:
    claude:
      model_tiers:
        mini:     { model: claude-haiku-4-5, effort: low }
        standard: { model: claude-sonnet-4-6, effort: medium }
        heavy:    { model: claude-opus-4-8, effort: high }
```

---

## 5. CLI reference

Run `macc <command> --help` for full flag documentation.

### 5.1 Project setup

| Command | Description |
|---|---|
| `macc init` | Initialize `.macc/` in the current repo |
| `macc init --wizard` | Interactive 4-step setup wizard (root, tools, standards, review) |
| `macc init --profile <name>` | Initialize and immediately restore a saved profile |
| `macc init --restore [name]` | Restore the best matching save (or a named one) after init |
| `macc init --fresh` | Ignore matching saves; create a baseline from scratch |
| `macc init --apply` | Run `macc apply` after initialization completes |
| `macc quickstart` | Zero-friction setup: preflight, init, TUI or plan+apply |
| `macc quickstart --check-only --json` | CI-safe environment validation; exits with JSON result |
| `macc quickstart --start-coordinator` | Also start the coordinator after setup |
| `macc start [--intent <intent>]` | Guided onboarding / task-startup entry point |
| `macc migrate [--apply]` | Migrate legacy config to the current format |

### 5.2 Configuration & profiles

```
macc config save <name> [--only tools,standards] [--description "..."]
macc config restore <name> [--only tools,automation]
macc config list
macc config delete <name>
```

Profiles are stored at `~/.macc/profiles/<name>.yaml`.

**Sections** accepted by `--only`: `tools`, `standards`, `selections`, `automation`, `settings`, `mcp_templates`.

### 5.3 Plan & apply

| Command | Description |
|---|---|
| `macc plan [-t tools] [--json] [--explain]` | Preview config changes without writing |
| `macc apply [-t tools] [--dry-run] [--allow-user-scope] [--locked]` | Generate and write tool-specific files |
| `macc context [--tool <id>] [--dry-run] [--print-prompt]` | Ask AI tools to refresh their context files |
| `macc lock generate` | Write `macc.lock.yaml` pinning the current environment |
| `macc lock check` | Verify current environment against the lock |
| `macc lock diff` | Show drift between environment and lock |
| `macc lock explain` | Human-readable explanation of what is pinned |

### 5.4 Tools

```
macc tool install <tool-id> [-y]
macc tool update [tool-id] [--all] [--check] [-y]
macc tool outdated [--only enabled|installed]
```

| Command | Description |
|---|---|
| `macc tool install <id>` | Install a tool adapter using its ToolSpec install steps |
| `macc tool update [id]` | Update one or all tools; `--check` previews without running |
| `macc tool outdated` | Show which tools have a newer version available |

### 5.5 Catalog

```
macc catalog skills list
macc catalog skills search <query>
macc catalog skills add --id <id> --name <name> --url <url> --kind git
macc catalog skills remove --id <id>
macc catalog import-url --kind skill --id <id> --url <url>
macc catalog search-remote --kind skill --q <query> [--add]

macc install skill --tool <tool> --id <id>
macc install mcp --id <id>
```

### 5.6 Skills

Skills are YAML workflows stored in `.macc/skills/`. They can also be installed from a catalog.

#### Run-skill commands

| Command | Description |
|---|---|
| `macc skills list [--tool <id>]` | List available run-skills |
| `macc skills show <skill>` | Show skill definition |
| `macc skills explain <skill>` | Human-readable explanation of a skill |
| `macc skills doctor` | Health check for skill configuration |

#### Catalog-skill lifecycle commands

| Command | Description |
|---|---|
| `macc skills available [--tool] [--source] [--tag] [--json]` | List skills from configured catalogs |
| `macc skills status [--tool] [--verbose] [--json]` | Show installed skills with provenance |
| `macc skills install <id> --tool <tool> [--pin] [--dry-run]` | Install a catalog skill |
| `macc skills update [id] [--tool] [--dry-run] [--latest]` | Update installed skills |
| `macc skills verify [--tool] [--json]` | Check lockfile/cache/filesystem integrity |
| `macc skills prune [--tool] [--dry-run]` | Remove skills no longer selected |
| `macc skills diff [id] [--tool]` | Show local modifications to installed skill files |
| `macc skills uninstall <id> [--tool] [--all-tools]` | Remove an installed skill |

#### Execute a skill

```bash
macc run <skill-id> [--tool <id>] [--task-id <id>] [--scope <glob>] [--dry-run] [--yes] [--json]
```

`--dry-run` previews the skill execution plan without running it. Skills with `risk: dangerous` require explicit `--yes`.

### 5.7 PRD generation & audit

The `macc prd` group manages the full PRD lifecycle. The internal generation tool is always `macc-prd-planner`; generated files land in `.macc/generated/prd/macc-prd-planner/`.

| Command | Description |
|---|---|
| `macc prd generate --from <brief.md>` | Generate a new PRD from a brief file |
| `macc prd generate --from <brief.md> --update <prd.json>` | Update an existing PRD from a brief |
| `macc prd generate --from <brief.md> --dry-run` | Preview the generation prompt without invoking the tool |
| `macc prd generate --from <brief.md> --promote` | Generate and immediately promote to `prd.json` |
| `macc prd audit [--prd prd.json] [--tool <id>]` | Enrich PRD from commit history; prints prompt when `--tool` is omitted |
| `macc prd audit --dry-run` | Preview the audit prompt without invoking a tool |
| `macc prd promote <source.json> [--dest prd.json] [-y]` | Promote a generated PRD to the active `prd.json` |
| `macc prd validate <prd.json>` | Validate PRD structure |
| `macc prd runs [--json]` | List all generation runs |
| `macc prd show-run <run-id>` | Show details for a specific generation run |

**Model flags** (available on `generate` and `audit`):

| Flag | Description |
|---|---|
| `--model-routing auto\|manual` | Routing mode (default: auto) |
| `--model-tier mini\|standard\|heavy` | Override tier (auto mode only) |
| `--model <id>` | Explicit model (manual mode only) |
| `--instructions <text>` | Inline instructions appended to the prompt |
| `--instructions-file <path>` | File whose content is appended to the prompt |

### 5.8 Coordinator

The coordinator orchestrates autonomous task execution. It reads `prd.json`, claims worktrees, spawns performers, and manages the full `todo → claimed → in_progress → testing → reviewing → merged` lifecycle.

```bash
macc coordinator [command] [flags]
```

Default command is `run`. The coordinator starts, runs until the queue is exhausted or a limit is reached, then exits. It also opens a client (TUI by default, web if `--client web`, or none with `--no-client`).

#### Coordinator commands

| Command | Description |
|---|---|
| `run` | Start orchestration (default) |
| `stop` | Request coordinator stop |
| `dispatch` | Manually dispatch one task |
| `advance` | Advance coordinator state machine one step |
| `resume` | Resume from a paused state |
| `sync` | Sync worktree state with git |
| `sync-prd` | Reconcile `prd.json` against commit history (marks merged tasks) |
| `status` | Print coordinator status |
| `reconcile` | Run coordinator reconciliation loop |
| `unlock` | Release locked resources |
| `cleanup` | Clean up dead workers and stale state |
| `retry-phase` | Retry a failed phase for a task |
| `cutover-gate` | Evaluate the cutover gate condition |
| `sessions` | Manage session resumption state |
| `validate-transition` | Validate a workflow state transition |
| `storage-import` | Import storage snapshot |
| `storage-export` | Export storage snapshot |
| `events-export` | Export coordinator events |
| `storage-verify` | Verify storage consistency |
| `storage-sync` | Sync SQLite ↔ JSON storage |
| `select-ready-task` | Select the next ready task to dispatch |
| `state-apply-transition` | Apply a workflow state transition |
| `state-set-runtime` | Set runtime metadata for a task |

#### Key coordinator flags

| Flag | Description |
|---|---|
| `--no-client` / `--client none` | Run headless (no TUI or web) |
| `--client web` | Open web client after starting |
| `--client tui` | Open TUI client (default) |
| `--supervisor` | Start supervisor watchdog before coordinator |
| `--max-parallel <n>` | Override global parallel cap |
| `--max-dispatch <n>` | Stop after dispatching N tasks |
| `--timeout-seconds <n>` | Per-task timeout |
| `--tool-priority <csv>` | Comma-separated tool priority order |
| `--preset conservative\|balanced\|throughput` | Apply a named concurrency preset |
| `--model-tier mini\|standard\|heavy` | Force a global tier for all tasks |
| `--testing <mode>` | Override testing phase mode |
| `--review <mode>` | Override review phase mode |
| `--disable-testing` | Disable the testing phase entirely |
| `--disable-review` | Disable the review phase entirely |
| `--storage-mode json\|dual-write\|sqlite` | Override storage backend |
| `--prd <path>` | Override PRD file path |
| `--reference-branch <branch>` | Default base branch |
| `--drain` | Disable new dispatch; let active tasks finish |
| `--preflight-only` | Run preflight checks and exit |
| `--allow-dirty-reference` | Start even if reference branch is dirty |
| `--merge-ai-fix` | Enable AI-assisted merge conflict resolution |
| `--in <duration>` | Start the coordinator after a relative delay (e.g., 30m, 2h) |
| `--at <datetime>` | Start the coordinator at an absolute target date-time |

#### Task workflow states

```
todo → queued → claimed → in_progress → testing → reviewing → pr_open → changes_requested → merged
                                                              ↘ failed / blocked
```

States `claimed`, `in_progress`, `testing`, `reviewing`, `pr_open`, `changes_requested`, and `queued` are counted as "active" for parallelism.

#### Delayed one-shot coordinator run

`macc coordinator run` supports an optional process-bound delayed start:

```bash
macc coordinator run --in 30m
macc coordinator run --at "2026-06-28T02:00"
```

- `--in` accepts a human-readable relative duration.
- `--at` accepts RFC 3339 or local `YYYY-MM-DDTHH:MM[:SS]` date-time.
- A local date-time without an offset uses the machine's local timezone.
- The flags are mutually exclusive.
- Past timestamps and zero durations are rejected.
- The process must remain alive until execution.
- `Ctrl+C` cancels the pending run.
- No scheduler state is persisted.

### 5.9 Supervisor

The supervisor is a watchdog process that monitors the coordinator and restarts it if it crashes.

```bash
macc supervisor start [--daemon] [--attach] [--coordinator-pid <pid>]
macc supervisor stop
macc supervisor status
macc supervisor report
```

The coordinator can automatically start the supervisor with `macc coordinator --supervisor`.

### 5.10 Status & observability

```bash
macc status                       # human-readable snapshot
macc status --json                # full RuntimeSnapshot as JSON
macc status --watch               # open TUI observer (read-only)
macc status --watch --control     # observer with operator actions
macc status --verbose             # per-worker details
macc status --failed              # show only failed tasks
macc status --task <id>           # focus on a specific task
macc watch                        # alias for --watch
```

`macc status --json` emits the same `RuntimeSnapshot` model that `GET /api/v1/snapshot` serves. Fields:

- `project`: name, root, config_version
- `coordinator`: running, paused, pause_reason, run_id, epoch
- `queue`: todo, ready, claimed, in_progress, testing, reviewing, changes_requested, blocked, merged, failed, total
- `workers`: id, worktree_path, tool, task_id, branch, phase, runtime_status, last_heartbeat, retry_count
- `throttled_tools`: tool, reason, error_code, retryable, backoff_seconds
- `recent_events`: ts, event_type, task_id, phase, status, message
- `git`: current_branch, clean, worktrees_count
- `diagnostics`: issues_count, warnings_count, critical_count

#### Task timeline and diff

```bash
macc explain <task-id>            # chronological event timeline
macc explain <task-id> --logs     # include raw performer logs
macc explain <task-id> --compact  # condensed, hide verbose ticks
macc explain <task-id> --since 1h # filter to last hour
macc explain <task-id> --json

macc diff <task-id>               # git diff in the task's active worktree
macc diff <task-id> --stat        # diff --stat summary
macc diff <task-id> --name-only   # changed file names only
macc diff <task-id> --base <branch> # override base branch
```

### 5.11 Worktrees

```bash
macc worktree create <slug> --tool <id> [--count 2] [--base main] [--scope "..."] [--feature "..."]
macc worktree list
macc worktree status
macc worktree open <id> [--editor code] [--terminal]
macc worktree apply <id> | --all
macc worktree doctor <id>
macc worktree run <id>            # run performer.sh inside the worktree
macc worktree exec <id> -- <cmd>  # execute a command inside the worktree
macc worktree remove <id> | --all [--force] [--remove-branch]
macc worktree prune               # git worktree prune
```

Worktree metadata is written to `.macc/worktree.json` in each worktree.

### 5.12 Logs

```bash
macc logs tail [--component all|coordinator|performer] [--worktree <id>] [--task <id>] [-n 120] [--follow]
macc logs list [--component all|coordinator|performer] [--since 24h]
```

Coordinator logs land in `.macc/log/coordinator/`. Performer logs land in `.macc/log/performers/`.

### 5.13 Save, restore & backups

#### Save bundles

```bash
macc save [name] [--overwrite] [--description "..."] [--only <sections>]
macc save [name] --include-logs [--log-max-size 50MB] [--log-since 7d] [--redact-logs]
macc save --dry-run               # preview without writing
macc save list [--matching <pattern>]
macc save show <name>
macc save delete <name> [-y]
```

Saves are stored in `.macc/saves/<name>/`.

#### Restore

```bash
macc restore [name]               # restore a named save bundle
macc restore --latest             # restore the most recent backup or save
macc restore --backup <timestamp> # restore a specific backup set
macc restore --dry-run            # preview without writing
macc restore --config-only        # restore config only
macc restore --apply              # run macc apply after restore
macc restore -y                   # skip confirmation
```

#### Backups (safety backups created by apply/clear)

```bash
macc backups list [--user]
macc backups open [id] [--latest] [--user] [--editor <cmd>]
```

Backups are stored in `.macc/backups/<timestamp>/`.

### 5.14 Clear

```bash
macc clear [--save <name>] [--force] [--dry-run] [--no-save-prompt]
```

Removes MACC-managed project files. Prompts to save unsaved state unless `--no-save-prompt` is set. Non-root worktrees are removed first.

### 5.15 Failure diagnostics

```bash
macc failure list
macc failure show <task-id>
macc failure retry <task-id> [--tool <id>]
macc failure salvage <task-id>
macc failure restore <task-id>
macc failure inspect-diff <task-id>
macc failure abandon <task-id>
```

### 5.16 Process ownership

MACC uses a project-wide control lease to prevent multiple clients from issuing conflicting mutations simultaneously.

```bash
macc process list
macc process ownership --kind project --pid 0
macc process claim --kind project --pid 0
macc process release --kind project --pid 0 --client-id <id>
macc process release-stale           # force-clear dead owner without ownership
macc process takeover request --kind project --pid 0
macc process takeover accept --kind project --pid 0 --owner-client-id <id> --request-id <id>
macc process takeover reject --kind project --pid 0 --owner-client-id <id> --request-id <id>
```

`release-stale` is the recovery command when a client (TUI, browser tab, etc.) died without releasing ownership. It bypasses the ownership check and prints the cleared client ID.

### 5.17 Lock

```bash
macc lock generate     # write / update macc.lock.yaml
macc lock check        # verify against lock file (exits non-zero on drift)
macc lock diff         # show drift
macc lock explain      # human-readable pin explanations
```

The lock file records tool versions, resolved skill references, and config hashes for reproducible environments.

### 5.18 Settings

```bash
macc settings show [--advanced] [--admin]
macc settings preset <conservative|balanced|throughput>
```

Presets adjust `max_parallel`, `max_parallel_per_tool`, `timeout_seconds`, and phase modes for common use cases.

### 5.19 Miscellaneous

| Command | Description |
|---|---|
| `macc doctor [--fix] [--json] [--coordinator]` | Run diagnostics; `--fix` applies safe auto-fixes |
| `macc doctor --git-name <name> --git-email <email> --fix` | Fix git identity |
| `macc trust` | Display project trust and safety parameters |
| `macc migrate [--apply]` | Migrate legacy config to the current format |
| `macc context [--tool <id>] [--from <files>] [--dry-run]` | Refresh tool context files |

### 5.20 TUI & web UI

```bash
macc                              # open TUI (default when no command given)
macc tui                          # explicit TUI command

macc web                          # serve web UI on http://127.0.0.1:3450
macc web --port 3451              # custom port
macc web --host 0.0.0.0           # expose on all interfaces (LAN access)
macc web --daemon                 # run as background daemon (survives SSH close)
```

The web server binds to `127.0.0.1` by default. Use `macc web --daemon` to run it as a background daemon independent of the terminal session.

---

## 6. Coordinator runtime

### Storage backends

The coordinator uses SQLite as its primary storage (`.macc/automation/task/coordinator.sqlite`) with optional JSON mirror:

| Mode | Description |
|---|---|
| `json` | Legacy JSON-only storage |
| `dual-write` | Write to both SQLite and JSON simultaneously |
| `sqlite` | SQLite primary; JSON mirror on a debounced schedule |

Override with `--storage-mode <mode>`.

### Pause file

When the coordinator pauses (on error, merge conflict, or `stop` command), it writes `.macc/automation/task/coordinator.pause.json`. This file is automatically cleared when:
- A new coordinator run starts.
- `macc coordinator resume` is called.
- The coordinator is running (the status endpoint ignores the pause file when a live PID is active).

### Phase configuration

Phases run after the main execution phase:

| Phase | Mode options | Description |
|---|---|---|
| `testing` | `disabled`, `required`, `risk_based`, `manual` | Run tests against the task's output |
| `review` | `disabled`, `required`, `risk_based`, `manual` | AI-driven code review |

`coordinator_tool` selects which adapter handles review/merge-fix phases. `max_review_cycles` limits the review-fix loop.

### Coordinator events

Events are appended to `.macc/log/coordinator/events.jsonl`. The web SSE stream (`GET /api/v1/events`) streams them in real time.

### Background operation

When launched with `--no-client`, the coordinator uses `setsid()` to detach from the terminal. All subprocesses (performers, merge workers) run in their own process groups, surviving SSH session close. Use `macc status` or `macc web` to observe a running headless coordinator.

---

## 7. Model routing

The coordinator automatically selects a model tier and reasoning depth for each task based on:

1. **Phase defaults** — exploration/summarization → mini/light; architecture/deep_refactor → heavy/deep; default → standard/standard.
2. **`routing_hints`** in the PRD task's `extra` field — override phase defaults.
3. **Global `--model-tier`** flag — overrides everything in auto mode.
4. **Manual mode** — no automatic selection; uses the tool's configured default model.

### Tiers and depths

| Tier | `ModelTier` | Typical use |
|---|---|---|
| `mini` | Fast, cheap | Summarization, exploration, simple transforms |
| `standard` | Balanced | Most development tasks (default) |
| `heavy` | Full power | Architecture, refactors, high-risk tasks |

| Depth | `ReasoningDepth` | Description |
|---|---|---|
| `light` | Low reasoning budget | Fast, routine operations |
| `standard` | Normal reasoning | Default |
| `deep` | Extended reasoning | Complex multi-file changes |

### `routing_hints` in PRD tasks

```json
{
  "id": "TASK-001",
  "extra": {
    "routing_hints": {
      "execution_mode": "structural",
      "risk_level": "high",
      "context_scope": "cross-cutting"
    }
  }
}
```

- `execution_mode: structural` → `heavy` tier
- `risk_level: high` → `heavy` tier
- `context_scope: cross-cutting` → `heavy` tier

### Environment variables injected into performers

| Variable | Description |
|---|---|
| `MACC_MODEL_TIER` | Resolved tier: `mini`, `standard`, or `heavy` |
| `MACC_REASONING_DEPTH` | Resolved depth: `light`, `standard`, or `deep` |
| `MACC_MODEL_ROUTING_MODE` | `auto` or `manual` |

### Tool-level model tier mapping

Each tool's `model_tiers` config maps symbolic tiers to concrete models and effort settings:

```yaml
# in macc.yaml or registry/tools.d/<tool>.tool.yaml
model_tiers:
  mini:
    model: claude-haiku-4-5
    effort: low
  standard:
    model: claude-sonnet-4-6
    effort: medium
  heavy:
    model: claude-opus-4-8
    effort: high
```

For tools that use a config file for effort (e.g. Codex with `.codex/config.toml`), the adapter writes the effort value before invoking the tool.

---

## 8. PRD system

### PRD file format

`prd.json` contains a flat list of tasks:

```json
{
  "tasks": [
    {
      "id": "NOY-L5-TASK-001",
      "title": "Implement feature X",
      "description": "...",
      "state": "todo",
      "tool": "claude",
      "base_branch": "main",
      "dependencies": [],
      "extra": {
        "routing_hints": { "risk_level": "high" }
      }
    }
  ]
}
```

### PRD generation workflow

1. Write a brief in Markdown (scope, requirements, constraints).
2. Run `macc prd generate --from brief.md` — the `macc-prd-planner` skill produces `prd.json` in `.macc/generated/prd/macc-prd-planner/<timestamp>/`.
3. Review and edit the generated file.
4. Run `macc prd promote <generated-file.json>` to copy it to `prd.json`.
5. Run `macc coordinator run` to start dispatch.

### PRD audit

`macc prd audit` builds a structured LLM prompt from commit history and PRD state, enabling an AI tool to update task completion status and add delivery notes. Use `--dry-run` to preview the prompt.

### PRD reconciliation

`macc coordinator sync-prd` deterministically transitions tasks to `merged` based on commit history without AI. It runs automatically as part of `coordinator sync`. Task IDs are extracted from commit messages using:
1. `[macc:TASK-ID]` structured trailer
2. Conventional commit subject matching
3. Legacy heuristic fallback

---

## 9. Skills runner

Skills are YAML-defined workflows that a performer executes within a worktree. They live in `.macc/skills/<id>.yaml` or are installed from a catalog.

### Skill schema

```yaml
id: validate
title: Validate implementation
kind: prompt             # local_command | prompt | hybrid | agent | coordinator
risk: safe               # safe | caution | dangerous
steps:
  - run: "npm test"
  - prompt: "Review the test output and report any failures."
targets:
  claude:
    model: claude-sonnet-4-6
```

### Risk levels

| Risk | Behavior |
|---|---|
| `safe` | Runs without confirmation |
| `caution` | Prompts for confirmation |
| `dangerous` | Requires explicit `--yes` |

### Built-in skills

| Skill ID | Description |
|---|---|
| `macc-performer` | Core task performer — used by the coordinator to drive a tool through a task |
| `macc-prd-planner` | PRD generation — creates `prd.json` from a brief |

---

## 10. Process ownership

MACC maintains a project-wide control lease that prevents multiple clients from issuing conflicting mutations. The lease is stored in `.macc/state/process_ownership.json`.

### How it works

1. When a client (TUI, browser, CLI) starts a mutation (apply, coordinator action, clear, etc.), it calls `gate_owner_action`.
2. If no owner exists, the first client takes ownership.
3. Subsequent clients become viewers or request a takeover.
4. Clients send heartbeats every few seconds to renew their lease.
5. If a client dies without releasing ownership, the lease expires after **15 seconds** of missed heartbeats.
6. The gate evicts stale owners on every ownership read (passive eviction).

### Recovery

If a client died and the lease is stuck:

```bash
macc process release-stale
# Cleared stale owner: 1748xxx-16f4fd0104f468
```

### Takeover flow

When a second client wants control:

```bash
macc process takeover request --kind project --pid 0
# Takeover requested. request_id: abc-123

# On the owning client:
macc process takeover accept --kind project --pid 0 --owner-client-id <id> --request-id abc-123
```

---

## 11. Adapters — supported tools

| Tool ID | Binary | Notes |
|---|---|---|
| `claude` | `claude` | Claude Code CLI; supports `--effort` flag |
| `codex` | `codex` | OpenAI Codex CLI; effort via `.codex/config.toml` |
| `gemini` | `gemini` | Gemini CLI |
| `agy` | `agy` | Agy CLI; config via `.agents/settings.json` |
| `vibe` | `vibe` | Vibe Coding CLI |

Each tool has a spec in `registry/tools.d/<tool>.tool.yaml` defining:
- `performer`: command, args, prompt mode, session config, retry config
- `model_tiers`: symbolic tier → concrete model + effort mapping
- `effort_config` / `effort_flag`: how to inject effort level
- `fields`: user-configurable settings (model, reasoning effort, sandbox mode, etc.)
- `install` / `update` / `version_check`: lifecycle commands
- `doctor`: health checks

### Tool configuration in TUI / Web

All tool settings (model, effort, tier models, approval policy, etc.) are editable from the TUI's **Settings** screen (tabbed: General, Coordinator, Tools, Phases, Reliability, Admin) and from the web **Config → Tools** page.

---

## 12. Web API

The web server exposes a REST/SSE API on `http://127.0.0.1:3450/api/v1/`.

### Core endpoints

| Method | Path | Description |
|---|---|---|
| GET | `/api/v1/health` | Health check |
| GET | `/api/v1/trust` | Project trust parameters |
| GET | `/api/v1/config` | Read canonical config |
| PUT | `/api/v1/config` | Update canonical config |
| GET | `/api/v1/status` | Coordinator status |
| GET | `/api/v1/snapshot` | Full RuntimeSnapshot (same as `macc status --json`) |
| POST | `/api/v1/coordinator` | Run a coordinator action |
| GET | `/api/v1/events` | SSE stream of coordinator events |

### Worktrees

| Method | Path | Description |
|---|---|---|
| GET | `/api/v1/worktrees` | List worktrees with status |
| POST | `/api/v1/worktrees` | Create worktrees |
| GET | `/api/v1/worktrees/:id` | Worktree detail |
| DELETE | `/api/v1/worktrees/:id` | Remove a worktree |
| GET | `/api/v1/worktrees/:id/logs` | SSE log stream for a worktree |

Worktree status reflects the coordinator registry: `in_progress` is surfaced when the coordinator has an active task in that worktree.

### PRD

| Method | Path | Description |
|---|---|---|
| GET | `/api/v1/prd` | Read active PRD |
| PUT | `/api/v1/prd` | Update active PRD |
| POST | `/api/v1/prd/generate` | Generate a new PRD |
| POST | `/api/v1/prd/audit` | Audit PRD from commit history |
| POST | `/api/v1/prd/promote` | Promote generated PRD |
| POST | `/api/v1/prd/validate` | Validate a PRD file |
| GET | `/api/v1/prd/generation-runs` | List generation runs |
| GET | `/api/v1/prd/generation-runs/:id` | Generation run detail |

### Skills & runs

| Method | Path | Description |
|---|---|---|
| GET | `/api/v1/skills` | List available skills |
| GET | `/api/v1/skills/:id` | Skill definition + dry-run preview |
| GET | `/api/v1/runs` | List skill run logs |
| GET | `/api/v1/runs/:id` | Single run result |

### Observability

| Method | Path | Description |
|---|---|---|
| GET | `/api/v1/doctor` | Run diagnostics |
| POST | `/api/v1/doctor/fix` | Apply safe auto-fixes |
| GET | `/api/v1/logs` | List log files |
| GET | `/api/v1/logs/tail` | Tail latest log (SSE) |
| GET | `/api/v1/logs/*path` | Read a log file |
| GET | `/api/v1/git/graph` | Git graph data |
| GET | `/api/v1/search?q=` | Search tasks, skills, worktrees, error codes |

### Process ownership

| Method | Path | Description |
|---|---|---|
| GET | `/api/v1/processes` | List process records |
| GET | `/api/v1/processes/:kind` | Ownership record for a process kind |
| POST | `/api/v1/processes/:kind/claim` | Claim ownership |
| POST | `/api/v1/processes/:kind/release` | Release ownership |
| POST | `/api/v1/processes/:kind/heartbeat` | Renew heartbeat |
| POST | `/api/v1/processes/:kind/takeover` | Request takeover |
| POST | `/api/v1/processes/:kind/respond-takeover` | Accept or reject takeover |

### Plan, apply, backups

| Method | Path | Description |
|---|---|---|
| POST | `/api/v1/plan` | Run plan (dry-run) |
| POST | `/api/v1/apply` | Apply configuration |
| GET | `/api/v1/backups` | List backup sets |
| POST | `/api/v1/terminal` | Create WebSocket PTY session |

### Catalog skills

| Method | Path | Description |
|---|---|---|
| GET | `/api/v1/catalog/skills/available` | Skills from configured catalogs |
| GET | `/api/v1/catalog/skills/status` | Installed skills with provenance |
| GET | `/api/v1/catalog/skills/installed` | Installed skill list |
| POST | `/api/v1/catalog/skills/verify` | Verify skill integrity |
| GET | `/api/v1/catalog/skills/lockfile` | Skill lockfile content |

### Security

- Binds to `127.0.0.1` by default; requires `--host 0.0.0.0` for LAN access.
- No authentication (single local operator).
- All mutating requests are audit-logged to `.macc/log/ops.jsonl`.
- Path parameters are sanitized against directory traversal.
- Web viewer heartbeats are refreshed automatically by the server every 30 seconds.

---

## 13. Debug mode

Set `MACC_DEBUG=1` or pass `--verbose` to enable debug output across all MACC components:

| Effect | Description |
|---|---|
| Performer prompt dump | The full prompt sent to the tool is printed |
| `[MACC] invoke` lines | Tool invocation commands are logged |
| Coordinator verbose | Tick and dispatch decisions are logged |

`--verbose` automatically sets `MACC_DEBUG=1` for all child processes (coordinator, performers, workers).

In `macc.yaml`, set `debug: true` to enable persistently.

---

## 14. File layout

```
<project-root>/
  .macc/
    macc.yaml                     # canonical config
    macc.lock.yaml                # environment lock (optional)
    prd.json                      # active PRD task registry
    generated/
      prd/macc-prd-planner/       # generated PRDs (one folder per run)
    automation/
      task/
        coordinator.sqlite        # primary coordinator storage
        task_registry.json        # JSON mirror (dual-write / fallback)
        coordinator.pause.json    # pause state (auto-cleared on restart)
    backups/
      <timestamp>/                # safety backups created before apply/clear
    saves/
      <name>/                     # named save bundles
    skills/
      <skill-id>.yaml             # project-level run-skills
    state/
      process_ownership.json      # client lease state
      supervisor.json             # supervisor state
    log/
      coordinator/
        events.jsonl              # coordinator event stream
        <timestamp>.log           # coordinator log
      performers/
        <worktree>/<task>-<ts>.log  # per-task performer logs
      ops.jsonl                   # web API mutation audit log
    cache/                        # fetched remote packages (gitignored)
    worktree.json                 # (in each worktree) worktree metadata
    scope.md                      # (optional) per-worktree scope

  registry/
    tools.d/
      claude.tool.yaml            # built-in Claude adapter spec
      codex.tool.yaml             # built-in Codex adapter spec
      gemini.tool.yaml            # built-in Gemini adapter spec
      agy.tool.yaml               # built-in Agy adapter spec
      vibe.tool.yaml              # built-in Vibe adapter spec

  adapters/
    shared/
      performer_lib.sh            # shared performer shell library
    claude/                       # Claude-specific adapter
    codex/                        # Codex-specific adapter
    gemini/                       # Gemini-specific adapter
    agy/                          # Agy-specific adapter
    vibe/                         # Vibe-specific adapter

  automat/
    coordinator.sh                # coordinator orchestration entry
    performer.sh                  # performer entry point
    merge_worker.sh               # merge worker entry point
    hooks/                        # lifecycle hooks

  core/src/
    config/                       # canonical config parsing and validation
    coordinator/                  # coordinator runtime, FSM, control plane
    coordinator_storage.rs        # storage abstraction (SQLite + JSON)
    prd_generation/               # PRD generation workflows
    skills_runner/                # skill resolver and executor
    runtime/                      # RuntimeSnapshot builder
    process_ownership/            # client lease, heartbeat, eviction
    tool/                         # ToolSpec parser, model tier config
    doctor/                       # diagnostics framework
    engine.rs                     # Engine trait (the façade)

  cli/src/
    commands/
      web/                        # Axum web server + handlers

  tui/src/
    lib.rs                        # Ratatui main loop
    state.rs                      # TUI application state

  web/src/
    pages/                        # React route pages
    components/                   # Shared UI components
    api/                          # API client + TypeScript models
    stores/                       # Zustand state stores
    hooks/                        # Custom React hooks
```

---

## 15. Security

- **No secrets in repo.** Tool API keys, tokens, and credentials must never be written to disk by MACC. MCP env values must use placeholder syntax (`${ENV_VAR}`).
- **Remote packages are data-only.** No post-install scripts. Web install review shows permissions and risks before installing.
- **Web server binds to localhost.** Explicit `--host 0.0.0.0` required for external access.
- **Process ownership gate.** All mutating CLI and web API calls are gated behind the project control lease. The gate evicts stale owners using a 15-second heartbeat TTL.
- **API audit log.** All mutating web API requests are logged to `.macc/log/ops.jsonl`.
- **Backup before overwrite.** `macc apply` and `macc clear` create safety backups in `.macc/backups/<timestamp>/` before modifying any file.
- **User-scope operations require consent.** `--allow-user-scope` must be passed explicitly; disabled by default.

---

## 16. Technical stack

| Layer | Technology |
|---|---|
| Primary language | Rust (stable) |
| TUI | Ratatui + Crossterm |
| Web backend | Axum + Tokio + RustEmbed |
| Web frontend | React 19, TypeScript, Vite, Tailwind CSS 4, Radix UI, Zustand |
| Storage | SQLite (rusqlite) + JSON mirror |
| Git | CLI-based invocations (no native git library) |
| Performer shell | bash (`adapters/shared/performer_lib.sh`) |

### Performance targets

- `macc apply` < 1 second (excluding downloads)
- Downloads: cached, incremental
- Web UI initial load < 2 seconds
- 20+ concurrent SSE streams supported
- Virtualized tables for PRD (500+ tasks) and logs (10 000+ lines)

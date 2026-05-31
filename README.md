# MACC

**Multi-agent coding config and orchestration.** MACC manages tool configuration across Claude Code, Codex, Gemini CLI, and others from a single canonical file, then coordinates them as autonomous agents running parallel tasks across git worktrees.

If you run more than one AI coding tool on the same codebase, or want to leave tasks running unattended across multiple branches, this is the layer that keeps it coherent.

---

## What MACC does

**Configuration management**
- One canonical config (`macc.yaml`) generates tool-specific files for any enabled tool via adapters. Change a setting once; all tools see it.
- `macc plan` shows what will change before any file is written. `macc apply` writes it.
- Profiles save and restore full configurations across repositories.

**Autonomous coordination**
- The coordinator dispatches tasks from a PRD to available tool/worktree slots, tracks state transitions, handles merge conflicts, and retries failures automatically.
- Multiple agents run in parallel across git worktrees without contaminating each other.
- Stale heartbeats, rate limits, and quota exhaustion are handled automatically with structured error codes and configurable backoff.

**Observability**
- Live TUI observer (`macc watch`) shows active workers, queue counts, throttled tools, and recent events.
- `macc explain <task-id>` prints a chronological execution timeline. `macc diff <task-id>` shows the worktree diff without changing directory.
- `macc status --json` emits a full `RuntimeSnapshot` for scripting or dashboards.

**Skill runner**
- `macc run <skill>` executes canonical skills (local commands or AI prompts) with risk classification, dry-run preview, and worktree-aware execution.

---

## Quick start

```bash
# Install
curl -sSL https://raw.githubusercontent.com/Brand201/macc/master/scripts/install.sh | bash -s -- --release

# Initialize a project
cd your-project
macc init

# Open the TUI to configure tools and start a coordinator run
macc tui
```

`macc` with no subcommand runs `init` if needed, then opens the TUI. `macc tui` does the same.

---

## Installation

**From GitHub (recommended):**

```bash
curl -sSL https://raw.githubusercontent.com/Brand201/macc/master/scripts/install.sh | bash -s -- --release
```

Pinned to a release:

```bash
curl -sSL https://raw.githubusercontent.com/Brand201/macc/v0.1.0/scripts/install.sh | bash -s -- --release --ref v0.1.0
```

**From a local clone:**

```bash
git clone https://github.com/Brand201/macc.git
cd macc
./scripts/install.sh --release
```

**Installer flags:**

| Flag | Effect |
|---|---|
| `--release` | Build optimized binary |
| `--prefix <dir>` | Install into a custom directory |
| `--system` | Install to `/usr/local/bin` (uses `sudo`) |
| `--no-path` | Skip updating shell profile `PATH` |
| `--repo <url>` | Install from a different git repository |
| `--ref <ref>` | Branch, tag, or commit (default: `master`) |

**Uninstall:**

```bash
macc-uninstall
# or
./scripts/uninstall.sh
# or
curl -sSL https://raw.githubusercontent.com/Brand201/macc/master/scripts/uninstall.sh | bash
```

Uninstall flags: `--system`, `--prefix <dir>`, `--clean-profile`, `--keep-helper`.

---

## Interfaces

MACC has three surfaces that share the same state model.

### CLI

All commands support `--quiet`, `--offline`, and `--web-port <PORT>`.

### TUI

```bash
macc tui
```

Screens: Home, Tools, Tool Settings, Automation/Coordinator, Coordinator Live, Skills, MCP, Global Settings, Logs, Preview, Apply, Observer.

Navigation keys: `h` Home · `t` Tools · `o` Automation · `e` Settings · `v` Coordinator Live · `m` MCP · `g` Logs · `p` Preview · `s` Save · `x` Apply · `?` Help · `q`/`Esc` Quit/Back.

### Web UI

```bash
macc web          # serves at localhost:3450
macc web --assets dist  # serve pre-built frontend
```

Development workflow:

```bash
# Terminal 1: frontend dev server
cd web && npm install && npm run dev

# Terminal 2: backend
macc web
```

Open `http://localhost:5173`. The Vite dev server proxies `/api` to `http://localhost:3450`.

The web UI covers: Dashboard, Config, PRD/Plan/Apply, Ops (Console, Registry, Worktrees, Live, Logs, Diagnostics, Backups, Git), and the Skill Runner at `/ops/skill-runner`.

---

## Core commands

### Project lifecycle

```bash
macc init [--force] [--wizard] [--profile <name>] [--fresh] [--restore [<name>]]
macc quickstart [-y] [--apply] [--no-tui]
macc plan [--tools t1,t2] [--json] [--explain]
macc apply [--tools ...] [--dry-run] [--allow-user-scope] [--json]
macc clear [--save <name>] [--force] [--dry-run]
macc doctor [--fix]
macc trust
```

`macc clear` prompts to save unsaved state, asks confirmation, removes worktrees first, then removes MACC-managed artifacts only.

### Save / restore bundles

```bash
macc save <name> [--overwrite] [--description "..."] [--only <sections>]
macc save list [--matching]
macc save show <name>
macc save delete <name>
macc restore [<name>] [--latest] [--dry-run] [--apply]
```

### Configuration profiles

Profiles store the full `CanonicalConfig` under `~/.macc/profiles/` and can be scoped to specific sections.

```bash
macc config save <name> [--description "..."] [--only tools,standards,selections,automation,settings,mcp_templates]
macc config restore <name> [--only <sections>]
macc config list
macc config delete <name>
```

### Tools and context

```bash
macc tool install <tool_id> [-y]
macc tool update <tool_id> [--check] [-y] [--force] [--rollback-on-fail]
macc tool update --all [--only enabled|installed] [--check]
macc tool outdated [--only enabled|installed]
macc context [--tool <id>] [--from <file> ...] [--dry-run] [--print-prompt]
```

To prevent `macc apply` from overwriting a tool's context file, set in `.macc/macc.yaml`:
```yaml
tools:
  config:
    <tool_id>:
      context:
        protect: true
```

### Catalogs and skills

```bash
macc catalog skills list|search|add|remove
macc catalog mcp list|search|add|remove
macc catalog import-url --kind <skill|mcp> ...
macc catalog search-remote --kind <skill|mcp> --q <query> [--add]
macc install skill --tool <tool_id> --id <skill_id>
macc install mcp --id <mcp_id>
```

### Worktrees

Worktrees run isolated task branches in parallel without contaminating the main repo.

```bash
macc worktree create <slug> --tool <tool_id> [--count N] [--base BRANCH] [--scope CSV] [--feature LABEL]
macc worktree list
macc worktree status
macc worktree open <id|path> [--editor <cmd>] [--terminal]
macc worktree apply <id|path> [--allow-user-scope]
macc worktree apply --all
macc worktree doctor <id|path>
macc worktree run <id|path>
macc worktree exec <id|path> -- <cmd...>
macc worktree remove <id|path> [--force] [--remove-branch]
macc worktree remove --all
macc worktree prune
```

---

## Coordinator

The coordinator reads a task registry, dispatches work to tool/worktree slots, tracks state transitions, supervises performers, and reconciles until convergence.

```bash
macc coordinator           # full cycle: sync → dispatch → advance → reconcile → cleanup
macc coordinator run --no-tui   # headless
macc coordinator status
macc coordinator sync
macc coordinator sync-prd       # reconcile tasks from commit history
macc coordinator reconcile
macc coordinator unlock
macc coordinator cleanup
macc coordinator stop [--graceful] [--remove-worktrees] [--remove-branches]
macc coordinator sessions save|restore|list|delete
macc supervisor start [--daemon] | stop | status | report
```

Runtime overrides (pass to `macc coordinator`):

```
--prd, --coordinator-tool
--tool-priority, --max-parallel-per-tool-json
--max-dispatch, --max-parallel, --timeout-seconds
--disable-testing, --disable-review
--testing <mode>, --review <mode>        # disabled | required | risk_based | manual
--stale-claimed-seconds, --stale-in-progress-seconds, --stale-action
```

When a merge conflict stays unresolved, the coordinator pauses. Resume with:
```bash
macc coordinator resume
```

After a run, enrich the PRD with AI-generated notes from commit context:
```bash
macc coordinator audit-prd -- --tool claude --dry-run  # preview prompt
macc coordinator audit-prd -- --tool claude             # run
```

Coordinator and performer logs: `.macc/log/coordinator/` and `.macc/log/performer/`.

### Auto-retry policy

Failed tasks are retried automatically when their error code is in the allow-list and retries are below the max.

```yaml
automation:
  coordinator:
    error_code_retry_list: "E101,E102,E103,E301,E302,E303,E601,E603"
    error_code_retry_max: 2
    max_dispatch_retries: 5
```

Rate-limit controls:

```yaml
automation:
  coordinator:
    rate_limit_backoff_base_seconds: 60
    rate_limit_backoff_max_seconds: 3600
    rate_limit_fallback_enabled: true
    rate_limit_throttle_parallel: true
```

E602 (quota exhausted) is never auto-retried — it requires operator action.

### Error codes

| Code | Meaning | Retryable |
|---|---|---|
| E101 | Runner exited non-zero | Yes |
| E102 | Tool runner not found / not executable | Yes |
| E103 | Tool output malformed | Yes |
| E104 | Performer produced partial changes | Conditional |
| E105 | Performer completed but exited non-zero | Conditional |
| E201 | Unavailable tool requested | No |
| E202 | Capability guard triggered | No |
| E301–303 | Worktree / PRD / tool.json missing | Yes |
| E304–306 | Worktree checkout / reset conflict | Conditional |
| E401 | Task registry read/write failure | No |
| E410–418 | Coordinator ledger / lease / recovery errors | No |
| E501 | Merge conflict | No |
| E503 | Merge blocked by policy | No |
| E601 | Rate-limited (429/529) | Yes — exponential backoff |
| E602 | Quota exhausted | No — operator action required |
| E603 | Session conflict | Yes |
| E901 | Unknown fatal error | No |

### Failure recovery

```bash
macc coordinator status
macc coordinator sync-prd
macc coordinator reconcile
macc coordinator unlock
macc coordinator cleanup
macc coordinator               # resume cycle
macc coordinator stop --graceful
macc coordinator stop --remove-worktrees --remove-branches
macc clear [--save <name>]     # reset to pre-MACC state
```

### Commit convention

All performers write commits in this format:

```
<type>: <TASK-ID> - <title>

[macc:task <TASK-ID>]
[macc:phase <phase>]
[macc:tool <tool>]
```

`macc coordinator sync-prd` scans committed task IDs and transitions matching tasks to `merged` (no AI involved). `audit-prd` uses an LLM to update task notes and descriptions from commit context.

---

## Observability

```bash
macc status [--json] [--watch] [--control] [--logs-only] [--events-only]
macc watch                        # alias for macc status --watch
macc explain <task-id> [--json] [--since <duration>] [--severity <level>] [--logs] [--compact]
macc diff <task-id> [--stat] [--name-only] [--base <branch>] [--cached] [--open]
```

`macc status --watch` opens a live TUI observer: worker grid, queue counts, event timeline, throttled tools, and a coordinator log tail. Stale workers (heartbeat age > 180 s) are flagged ▲. `--control` enables operator actions.

`macc status --json` emits the full `RuntimeSnapshot` — the same model consumed by the TUI and `GET /api/v1/snapshot`.

---

## Skill Runner

Skills are canonical workflows (local commands or AI prompts) defined in `.macc/skills/*.yaml`.

```bash
macc run <skill> [--tool <tool>] [--dry-run] [--watch] [--json] [--yes]
macc skills list [--tool <tool>]
macc skills show <skill>
macc skills explain <skill>
macc skills doctor
```

**Built-in skills:**

| Skill | Kind | Risk | What it does |
|---|---|---|---|
| `validate` | local_command | safe | Runs lint, build, and tests |
| `implement` | prompt | caution | Implements the next pending task |
| `security-check` | prompt | safe | Reviews changed files for security issues |

Caution-risk skills prompt for confirmation. Dangerous-risk skills require typing `YES`.

`--dry-run` shows commands, risk level, adapter strategy, and the log path without executing.

**Custom skills** — add a YAML file to `.macc/skills/`:

```yaml
id: my-skill
title: My custom skill
kind: local_command
risk: safe
description: Runs my custom validation.
steps:
  - run: pnpm lint
  - run: pnpm test
```

Tool is resolved automatically: `--tool` flag → `worktree.json` → `tool.json` → `skills.run_policy.default_tool` → `tool_priority` → first enabled tool.

### Token/context budget

MACC can summarise noisy output (test logs, lint errors, stack traces, diffs) before it reaches model context.

```yaml
context:
  token_budget:
    default: 12000        # rough token proxy: 1 token ≈ 4 chars
    tool_output: 4000
    diff: 6000
    logs: 3000

  summarization:
    enabled: true
    default_bundles:
      - test-output-failures-only
      - lint-errors-only
      - stacktrace-collapse
      - git-diff-stat-before-full-diff
      - log-grep-error-first

    per_skill:
      validate:
        bundles: [test-output-failures-only, lint-errors-only]
      security-check:
        bundles: [git-diff-stat-before-full-diff]
```

Available bundles:

| Bundle | Keeps |
|---|---|
| `test-output-failures-only` | Failed test names, assertions, file/line refs, exit code |
| `lint-errors-only` | Error-level lint lines only |
| `stacktrace-collapse` | Exception type/message, first 3 app frames, first external frame, omitted count |
| `git-diff-stat-before-full-diff` | Diff stat + file list; full diff only when it fits in budget |
| `log-grep-error-first` | Error/warning lines with surrounding context |

When summarisation runs, output includes:
```
Output summarized
  Raw size:     187k chars
  Summary size: 9k chars
  Policy:       test-output-failures-only + stacktrace-collapse
```

---

## Configuration

Primary file: `.macc/macc.yaml`

All settings are configurable in the TUI Automation screen or via `macc coordinator` flags. Legacy environment variables are no longer used.

### Key paths

| Path | Purpose |
|---|---|
| `.macc/macc.yaml` | Canonical config |
| `.macc/automation/` | Embedded coordinator/performer scripts |
| `.macc/log/coordinator/` | Coordinator event stream and logs |
| `.macc/log/performer/` | Per-worktree performer logs |
| `.macc/log/run/` | Skill run logs (`.log` + `.jsonl`) |
| `.macc/state/tool-sessions.json` | Performer session leases |
| `.macc/state/coordinator.sqlite` | Coordinator runtime ledger |
| `.macc/skills/` | Local skill definitions |
| `.macc/catalog/*.catalog.json` | Skill and MCP catalogs |
| `.macc/backups/` | Project backup sets |
| `~/.macc/profiles/` | Saved configuration profiles |
| `~/.macc/sessions/` | Coordinator session snapshots |

### ToolSpec precedence (low → high)

1. Built-in ToolSpecs (embedded in binary)
2. User overrides: `~/.config/macc/tools.d`
3. Project overrides: `.macc/tools.d`

### Session strategy

Sessions are per-worktree by default. The coordinator prefers reusing warm sessions over cold starts (`session_cache_ttl_seconds`, default `300`). Session snapshots are saved automatically on graceful stop and can be restored with `macc coordinator sessions restore`.

### Safety guarantees

- Writes are atomic and idempotent.
- Backups are created for changed project files before overwriting.
- User-scope writes require explicit `--allow-user-scope` plus an interactive confirmation listing touched paths, backup location, and restore commands.
- Secret scanning blocks unsafe generated output.
- `macc clear` requires confirmation and only removes MACC-managed artifacts.

---

## Automation runbook (blank machine)

Minimal sequence from a clean machine to a running coordinator:

```bash
# 1. Install dependencies (Linux)
sudo apt-get install -y git curl jq build-essential pkg-config libssl-dev

# 2. Install MACC
curl -sSL https://raw.githubusercontent.com/Brand201/macc/master/scripts/install.sh | bash -s -- --release
macc --version

# 3. Install AI tools via TUI
macc tui   # go to Tools → press i on any missing tool

# 4. Initialize your project
cd your-project
macc init && macc tui  # configure tools, set coordinator options, save
macc apply

# 5. Run the coordinator
macc coordinator
```

If a merge conflict pauses the run: `macc coordinator resume`

---

## Documentation

| Document | Contents |
|---|---|
| `MACC.md` | Full architecture and specification |
| `CHANGELOG.md` | Release notes |
| `docs/CONFIG.md` | Canonical config schema |
| `docs/TOOLSPEC.md` | ToolSpec format and field kinds |
| `docs/CATALOGS.md` | Catalog schemas and workflows |
| `docs/COORDINATOR_RESILIENCE.md` | Stop semantics, recovery, runtime snapshot |
| `docs/TOOL_ONBOARDING.md` | Adding a new tool end-to-end |
| `docs/ADDING_TOOLS.md` | Adding new adapters |
| `docs/WEB_API_CONTRACT.md` | Full `/api/v1` surface |
| `CONTRIBUTING.md` | Contribution workflow |
| `SECURITY.md` | Vulnerability disclosure policy |

---

## Quality and releases

- CI: format, lint, tests, tool-agnostic guardrails, cross-platform build matrix (Linux / macOS / Windows).
- Releases are tag-driven (`vX.Y.Z`) with SemVer policy. See `docs/RELEASE.md`.
- Compatibility policy (OS + MSRV): `docs/COMPATIBILITY.md`.

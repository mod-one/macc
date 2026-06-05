# Antigravity CLI : Complete Reference, Usage Guide, and Integration Specification

**Generated:** 2026-05-23  
**Language:** English  
**Primary binary:** `agy`  
**Scope:** installation, startup, authentication, command usage, features, configuration, permissions, plugins, MCP, skills, hooks, sidecars, migration from Gemini CLI, troubleshooting, likely error cases, and MACC adapter notes.

> This document was generated from the public Antigravity CLI documentation pages supplied by the user, the Google Developers transition announcement, and the linked hands-on guide. Some official Antigravity documentation pages are rendered dynamically; when exact command syntax was not fully exposed in the indexed page text, this document marks entries as **verify with `agy --help`** or **observed/reported** rather than treating them as guaranteed stable API.

---

## 1. Executive Summary

Antigravity CLI is Google Antigravity's terminal-first agent client. It is invoked with the `agy` binary and is positioned as the replacement path for most individual-tier Gemini CLI users. Google describes the move as a unification around the Antigravity platform and a shared server-side agent harness. The transition keeps core Gemini CLI workflow concepts such as Agent Skills, Hooks, Subagents, and Extensions, with Extensions renamed or migrated into Antigravity Plugins.

Key points:

- `agy` is the command to start the CLI.
- It is a Terminal User Interface (TUI), optimized for keyboard-driven and remote/SSH workflows.
- It uses the same Antigravity agent harness as Antigravity 2.0, but exposes it through a terminal surface.
- It supports asynchronous/background subagents for parallel work.
- It supports plugins, skills, hooks, MCP servers, permissions, keybindings, model selection, usage/quota inspection, and session management.
- It supports a one-command Gemini CLI plugin/extension import workflow: `agy plugin import gemini`.
- It does **not** have perfect 1:1 parity with Gemini CLI at launch. Migration should be tested per workflow.

---

## 2. Product Positioning

### 2.1 What Antigravity CLI is

Antigravity CLI is a terminal interface for working with Antigravity agents. It can:

- inspect a repository,
- reason over project files,
- edit multiple files,
- run tools and terminal commands subject to permissions,
- manage conversation history,
- spawn subagents,
- connect to MCP servers,
- use skills and plugins,
- work in interactive and prompt/headless modes.

### 2.2 Antigravity CLI vs Antigravity 2.0

| Surface | Primary use | Strength |
|---|---|---|
| Antigravity CLI | Terminal-first workflows, SSH, scripts, fast keyboard-driven work | Low overhead, scriptable, quick to start |
| Antigravity 2.0 | Visual orchestration, artifact review, multiple local agents, project management | Rich UI, review panes, visual task management |
| Antigravity SDK | Programmatic embedding | Custom apps and workflows |
| Antigravity IDE | IDE-oriented development | Editor-centric work |

### 2.3 Migration implication

Treat the move from Gemini CLI to Antigravity CLI as a migration, not a drop-in binary rename. The binary changes from `gemini` to `agy`, several file locations change, MCP config moves to a dedicated file, and Gemini CLI Extensions become Antigravity Plugins.

---

## 3. Prerequisites

### 3.1 Operating systems

Supported installation targets reported by the supplied documentation:

- macOS
- Linux
- Windows, including native PowerShell usage

### 3.2 Required local tools

- A terminal/shell.
- `curl` on macOS/Linux or Windows CMD installation path.
- PowerShell 5+ on Windows if using the PowerShell installer.
- A Google account, Google AI subscription, or enterprise/Google Cloud credentials depending on your access mode.
- A project folder/repository if you want repository-aware behavior.

### 3.3 Recommended safety baseline

Before using agentic tools that can edit files or run commands:

- Commit or stash current work.
- Work inside a Git repository.
- Prefer a disposable branch or worktree for first tests.
- Use conservative terminal execution policy first.
- Keep non-workspace file access disabled unless there is a clear need.
- Review generated changes before accepting them.

---

## 4. Installation

### 4.1 macOS / Linux

```bash
curl -fsSL https://antigravity.google/cli/install.sh | bash
```

Then restart the shell or source the updated profile and verify:

```bash
agy --version
```

### 4.2 Windows PowerShell

```powershell
irm https://antigravity.google/cli/install.ps1 | iex
```

Then open a new terminal and verify:

```powershell
agy --version
```

### 4.3 Windows CMD

```cmd
curl -fsSL https://antigravity.google/cli/install.cmd -o install.cmd && install.cmd && del install.cmd
```

Then open a new terminal and verify:

```cmd
agy --version
```

### 4.4 Installation paths

Observed/reported paths:

| Platform | Typical binary destination |
|---|---|
| macOS/Linux | `~/.local/bin/agy` |
| Windows | `%LOCALAPPDATA%\Antigravity\agy.exe` or equivalent installer-controlled path |

Always confirm with:

```bash
which agy
agy --version
agy --help
```

On Windows PowerShell:

```powershell
Get-Command agy
agy --version
agy --help
```

### 4.5 PATH troubleshooting

If `agy --version` fails with `command not found`, `agy is not recognized`, or similar:

1. Find where the installer placed the binary.
2. Add that directory to your PATH.
3. Restart the terminal.
4. Re-run `agy --version`.

---

## 5. Authentication

### 5.1 Default authentication flow

On first launch, `agy` attempts to authenticate through the platform's secure credential storage and browser-based Google sign-in flow. On desktop machines, it may open the browser automatically. Credentials are cached in the operating system keyring or credential manager.

### 5.2 SSH and headless environments

In remote/SSH contexts, the CLI is reported to detect the remote session and print an authorization URL or one-time code. Open the URL locally, complete the sign-in, and paste the result back if requested.

### 5.3 API key / automation mode

Some guides report API-key-oriented usage for CI or scripting:

```bash
export ANTIGRAVITY_API_KEY="your_api_key_here"
agy -p "Summarize this repository"
```

Verify the exact environment variable and enterprise authentication behavior with current official docs and:

```bash
agy --help
agy help auth
```

### 5.4 Logout

Inside the TUI, use:

```text
/logout
```

Expected behavior: sign out and clear cached CLI credentials for the current account/session. Verify exact scope with current `agy` help output.

---

## 6. Startup Modes

### 6.1 Interactive TUI mode

From a project directory:

```bash
agy
```

```bash
$ gy --help
Usage of agy:
  --add-dir                       Add a directory to the workspace (repeatable) (default [])
  -c                              Short alias for --continue
  --continue                      Continue the most recent conversation
  --conversation                  Resume a previous conversation by ID
  --dangerously-skip-permissions  Auto-approve all tool permission requests without prompting
  -i                              Short alias for --prompt-interactive
  --log-file                      Override CLI log file path
  -p                              Short alias for --print
  --print                         Run a single prompt non-interactively and print the response
  --print-timeout                 Timeout for print mode wait (default 5m0s)
  --prompt                        Alias for --print
  --prompt-interactive            Run an initial prompt interactively and continue the session
  --sandbox                       Run in a sandbox with terminal restrictions enabled

Available subcommands:
  changelog       Show changelog and release notes
  help            Show help for subcommands
  install         Configure environment paths and shell settings
  plugin          Manage plugins (install, uninstall, list, enable, disable)
  plugins         Alias for plugin
  update          Update CLI
```

Expected behavior:

- Opens the interactive terminal UI.
- Shows a conversation pane and prompt.
- Allows natural-language requests.
- Can reference files, directories, and project context.
- Can request permission for file edits, terminal commands, browser actions, or MCP actions depending on configuration.

Example prompts:

```text
Explain this repository.
What does @src/main.ts do and where is it called from?
Refactor @src/api/ to remove duplicated validation logic.
Generate tests for the authentication module.
```

### 6.2 Prompt/headless mode

Use `-p` for a one-shot prompt:

```bash
agy -p "List all TODO comments in this repository"
```

Useful for:

- shell scripts,
- Git hooks,
- CI-style checks,
- automation wrappers,
- MACC performer integration.

### 6.3 Structured output

Reported syntax:

```bash
agy -p "List all TODOs in this codebase" --output-format json
```

Some integrations report stream-oriented output formats such as `stream-json`. Confirm with:

```bash
agy -p "hello" --help
agy --help | grep -i output
```

### 6.4 Model override

Reported syntax:

```bash
agy -m gemini-3.1-pro -p "Summarize the changes in the last 5 commits"
```

Inside the TUI:

```text
/model gemini-3.1-pro
```

Model availability depends on account, plan, region, enterprise policy, and current Google model rollout.

---

## 7. Project Context and File Referencing

### 7.1 Context files

Antigravity CLI reads project instructions from context/rules files. The migration docs and community material repeatedly reference:

| File | Scope | Notes |
|---|---|---|
| `AGENTS.md` | Project/workspace | Cross-tool instruction format; useful for Claude, Codex, Gemini/Antigravity-style agents |
| `GEMINI.md` | Project/workspace or global | Gemini/Antigravity-oriented instructions; verify exact precedence in your version |
| `~/.gemini/GEMINI.md` | Global | Legacy/global context reportedly still read in some flows |

Recommended project root baseline:

```text
AGENTS.md
GEMINI.md        # optional Antigravity-specific override if needed
.agents/
  skills/
  mcp_config.json
```

### 7.2 Inline context references

Reported reference syntax:

```text
@src/main.go
@src/
@**/*.ts
```

Use cases:

```text
Explain @src/main.go and identify its callers.
Review @src/auth/ for security issues.
Generate tests for @packages/core/**/*.ts.
```

### 7.3 Inspecting loaded context

```bash
agy inspect
```

Expected output categories:

- loaded context files,
- `.agents/` configuration,
- global and workspace skills,
- plugins,
- hooks,
- MCP servers from `mcp_config.json`.

Use `agy inspect` as the first debugging step when the agent ignores expected rules, skills, hooks, or MCP servers.

---

## 8. Slash Command Reference

> Commands evolve quickly. Treat this table as a practical reference and verify on your installed version with `/help`.

### 8.1 Core help and account commands

| Command | Purpose | Notes |
|---|---|---|
| `/help` or `?` | Show commands and keybindings | First command to run in TUI |
| `/usage` | Show quota/rate-limit/model usage information | Useful before long subagent work |
| `/logout` | Sign out and clear cached credentials | Scope may vary by version |
| `/feedback` | Submit feedback | Report issues or product suggestions |

### 8.2 Conversation/session commands

| Command | Purpose | Notes |
|---|---|---|
| `/resume` | Resume or switch conversations | Some docs list alias `/switch` |
| `/rewind` | Rewind conversation to previous checkpoint | Some docs list alias `/undo`; may support reverting chat, code changes, or both |
| `/rename <name>` or `/title` | Rename conversation or window title | Availability/version-specific |
| `/fork` | Branch current conversation | Verify exact syntax |
| `Ctrl+C` | Interrupt current operation | Common terminal behavior; verify current TUI semantics |
| `Ctrl+D` | Exit/end input or session | Common terminal behavior; verify current TUI semantics |

### 8.3 Configuration commands

| Command | Purpose | Notes |
|---|---|---|
| `/config` | Open interactive configuration/settings panel | Includes safety, editor, visual, performance settings |
| `/settings` | Open settings | May overlap with `/config` |
| `/permissions` | Manage agent permission rules | Allow/Deny/Ask rules |
| `/model` | Select reasoning model | Persists depending on config/session |
| `/keybindings` | Open keyboard shortcut editor | Writes/updates keybinding config |
| `/statusline` | Configure TUI status bar | Usage indicators, model, context, custom status |

### 8.4 Tools and monitoring commands

| Command | Purpose | Notes |
|---|---|---|
| `/agents` | View/manage subagents | Used for asynchronous background subagents |
| `/tasks` | View/monitor background tasks | Logs, progress, termination depending on version |
| `/skills` | Browse available local/global skills | Some versions use `/skills list`; verify |
| `/mcp` | Manage/check MCP servers | Use after editing `mcp_config.json` |
| `/hooks` | Manage hook configurations | Verify exact hook schema/version |
| `/open <path>` | Open file in configured editor | Version-specific |
| `/export` | Export session to Antigravity 2.0 | Used to continue in GUI with richer artifact review |

### 8.5 Planning and execution commands

| Command | Purpose | Notes |
|---|---|---|
| `/goal <goal>` | Run until a specified goal is finished | Reported in recent command lists; verify |
| `/agent <task>` | Spawn subagent/background task | Some guides use `/agent refactor "..."`; newer docs may prefer `/agents` panel |
| `/plan` or Planning mode toggle | Plan before editing | Exact trigger varies; GUI uses Fast/Planning modes |

---

## 9. Configuration Files and Locations

### 9.1 Global CLI configuration

Reported official CLI settings path:

```text
~/.gemini/antigravity-cli/settings.json
```

Reported keybindings path:

```text
~/.gemini/antigravity-cli/keybindings.json
```

Reported plugin staging path:

```text
~/.gemini/antigravity-cli/plugins/<plugin_name>/
```

### 9.2 Workspace configuration

Recommended workspace configuration layout:

```text
.agents/
  skills/
    <skill-name>/
      SKILL.md
  mcp_config.json
  hooks.json              # verify exact file name/schema in current docs
AGENTS.md
GEMINI.md                 # optional
```

### 9.3 Model endpoint configuration

The DEV guide reports a custom model configuration file at:

```text
~/.config/antigravity/config.toml
```

Example reported structure:

```toml
[[models]]
name = "my-custom-model"
model = "gemini-3.1-pro"
base_url = "https://example-model-gateway.local/v1"
env_key = "MY_CUSTOM_MODEL_API_KEY"
```

Because official CLI settings are also reported under `~/.gemini/antigravity-cli/settings.json`, validate model configuration with your installed version before standardizing this in automation.

### 9.4 Configuration precedence

Expected precedence pattern for agent CLIs:

1. Launch flags.
2. Workspace/project settings.
3. Global user settings.
4. Built-in defaults.

Verify with:

```bash
agy inspect
agy --help
```

---

## 10. Models

Official indexed docs mention Antigravity access to Gemini Enterprise Agent Platform models such as:

- Gemini 3.5 Flash
- Gemini 3.1 Pro (high)

Other guides report model selection involving Claude-family and open-source backends, subject to plan and provider configuration. Treat non-Gemini model availability as account/version-dependent.

Commands:

```text
/model
/model gemini-3.1-pro
```

Headless:

```bash
agy -m gemini-3.1-pro -p "Review this diff for regressions"
```

Best practices:

- Use faster models for small/localized edits.
- Use stronger reasoning models for planning, architecture, migrations, and security reviews.
- Check `/usage` before running parallel subagents.
- Prefer Planning mode for high-risk changes.

---

## 11. Agent Modes

### 11.1 Fast mode

Fast mode executes directly and is best for:

- simple renames,
- small localized edits,
- quick commands,
- short explanations,
- low-risk code modifications.

### 11.2 Planning mode

Planning mode asks the agent to research and plan before implementation. Use it for:

- architecture changes,
- multi-file refactors,
- migrations,
- security-sensitive changes,
- tasks requiring explicit review or artifacts.

Expected outputs can include:

- implementation plan,
- task list,
- proposed steps,
- artifacts,
- diffs and walkthroughs.

---

## 12. Permissions and Safety Model

### 12.1 Permission lists

Antigravity uses Allow, Deny, and Ask lists to control what the agent can do.

| List | Behavior |
|---|---|
| Allow | Auto-approved without prompting |
| Deny | Blocked immediately |
| Ask | Agent pauses and asks for approval |

### 12.2 Permission rule format

```text
action(target)
```

Reported action types:

| Action | Target format | Meaning |
|---|---|---|
| `command` | `command(prefix)` or `command(*)` | Match shell commands by prefix |
| `read_file` | `read_file(/absolute/path)` | Allow/deny reading a file or directory |
| `write_file` | `write_file(/absolute/path)` | Allow/deny writing; also implies read access to same target |
| `read_url` | `read_url(domain)` or `read_url(*)` | Control URL/domain reads |
| `mcp` | `mcp(server/tool)`, `mcp(server/*)`, or `mcp(*)` | Control MCP server/tool usage |

### 12.3 Conservative starting policy

Recommended first-run policy:

```text
Terminal command auto execution: Request Review
Artifact review: Request Review / Asks for Review
Browser JavaScript execution: Request Review or Disabled
Terminal sandbox: Enabled
Agent Non-Workspace File Access: Disabled
Deny commands: rm, rmdir, del, format, sudo rm, powershell Remove-Item -Recurse, curl | sh, wget | sh
Allow commands only after observing safe repeated usage
```

### 12.4 Strict mode

Strict mode restricts the agent's filesystem access to authorized areas and, according to indexed official documentation, respects `.gitignore`. Use it for untrusted repositories or first tests.

### 12.5 Non-workspace file access

Keep `Agent Non-Workspace File Access` disabled by default. Enable only when:

- the task truly requires reading or editing files outside the project,
- you understand the risk,
- secrets and private files are protected,
- the workspace is trusted.

---

## 13. Skills

### 13.1 Concept

A skill is a folder containing a `SKILL.md` file. It provides reusable instructions or behavior that the agent can load when relevant or when invoked explicitly. Skills reduce repeated prompting and keep specialized guidance out of the main context until needed.

### 13.2 Global and workspace skill locations

| Scope | Antigravity CLI path |
|---|---|
| Global | `~/.gemini/antigravity-cli/skills/` |
| Workspace | `.agents/skills/` |

### 13.3 Minimal skill structure

```text
.agents/skills/code-review/
  SKILL.md
```

Example `SKILL.md`:

```markdown
---
name: code-review
description: Review code changes for correctness, maintainability, security, and project conventions.
---

When invoked, review the current diff and report:

1. Correctness risks.
2. Security issues.
3. Test gaps.
4. Maintainability problems.
5. Suggested fixes.

Do not edit files unless explicitly asked.
```

### 13.4 Using skills

Inside the TUI:

```text
/skills
@code-review Review the current diff.
```

Some versions may support direct slash invocation of skill names. Verify with `/skills` and `/help`.

### 13.5 Migration from Gemini CLI skills

| Gemini CLI location | Antigravity CLI target |
|---|---|
| `~/.gemini/skills/` | `~/.gemini/antigravity-cli/skills/` or auto-import/auto-load depending on version |
| `.gemini/skills/` | `.agents/skills/` |

Recommended migration for workspace skills:

```bash
mkdir -p .agents
mv .gemini/skills .agents/skills
agy inspect
```

---

## 14. Plugins

### 14.1 Concept

Plugins are namespaced bundles that group capabilities such as:

- skills,
- rules,
- MCP servers,
- hooks,
- subagents or agent definitions,
- project/workspace customizations.

### 14.2 Plugin location

Reported user-level staging path:

```text
~/.gemini/antigravity-cli/plugins/<plugin_name>/
```

### 14.3 Migration command

```bash
agy plugin import gemini
```

Expected behavior:

- Search legacy Gemini CLI extension locations.
- Convert locally installed extensions into Antigravity plugins where supported.
- Preserve or prompt before modifying original files.
- Convert commands into skills where applicable.
- Migrate many MCP and hook definitions, subject to changed config paths and fields.

### 14.4 Known migration limitations

- Custom themes may not migrate cleanly.
- Workspace skills may need manual movement into `.agents/skills/`.
- MCP config may need manual conversion to `mcp_config.json`.
- Remote MCP field names must be updated from `url` or `httpUrl` to `serverUrl`.
- Always run `agy inspect`, `/skills`, `/mcp`, and a representative workflow after import.

---

## 15. MCP Servers

### 15.1 Concept

MCP servers expose tools and external context to the agent. Antigravity CLI supports local and remote MCP servers.

### 15.2 Config files

| Scope | Antigravity CLI MCP config |
|---|---|
| Global | `~/.gemini/antigravity-cli/mcp_config.json` |
| Workspace | `.agents/mcp_config.json` |

### 15.3 Remote MCP field name

Use:

```json
{
  "servers": {
    "example": {
      "serverUrl": "https://mcp.example.com",
      "auth": "oauth"
    }
  }
}
```

Do **not** use old Gemini CLI remote fields unless your version explicitly supports them:

```json
{
  "url": "https://mcp.example.com",
  "httpUrl": "https://mcp.example.com"
}
```

### 15.4 Local stdio MCP example

```json
{
  "servers": {
    "local-docs": {
      "command": "node",
      "args": ["./tools/mcp/local-docs-server.js"],
      "env": {
        "DOCS_ROOT": "${PROJECT_ROOT}/docs"
      }
    }
  }
}
```

### 15.5 Validation

After editing MCP config:

```text
/mcp
```

And from shell:

```bash
agy inspect
```

Check for:

- config syntax errors,
- unavailable commands,
- authentication failures,
- old field names,
- wrong global vs workspace path,
- missing environment variables.

---

## 16. Rules and Workflows

### 16.1 Rules

Rules are manually defined constraints that guide agent behavior globally or within a workspace.

Examples:

```markdown
# AGENTS.md

- Use TypeScript strict mode.
- Prefer functional/declarative code.
- Never use `any`; use `unknown` or generics.
- Run `pnpm lint` and `pnpm build` before finalizing changes.
- Ask before modifying database migrations.
- Do not edit files outside the repository.
```

### 16.2 Workflows

Workflows are reusable prompts or processes that can be invoked by slash command or selection UI.

Example workflow intent:

```markdown
# generate-unit-tests

Generate unit tests for each changed file.
Name tests using the existing project convention.
Cover happy paths, edge cases, and error cases.
Do not modify production code unless explicitly requested.
```

Use case:

```text
/generate-unit-tests
```

Verify exact workflow storage location and slash invocation syntax with `/help`, `/config`, or the official rules/workflows page for your installed version.

---

## 17. Hooks

### 17.1 Concept

Hooks allow scripts or shell commands to run at specific points in the agent execution loop. Use them to enforce formatting, block risky changes, collect telemetry, or validate generated output.

Reported lifecycle examples:

- before a tool call,
- after a file edit,
- on session start,
- stop/finalization events.

### 17.2 Safety modes

Indexed official hook docs mention decision behaviors such as:

- `ask`: prompt the user but respect cached Always Allow settings,
- `force_ask`: always prompt, ignoring cached permissions.

Verify the current hook schema before standardizing.

### 17.3 Example hook ideas

- Run `gofmt` after Go file edits.
- Run `pnpm lint --fix` only on changed frontend files.
- Block writes to `vendor/`, `.env`, `.ssh/`, `.npmrc`, or credential files.
- Require confirmation for `rm`, `rmdir`, `del`, `format`, `sudo`, or shell-pipe installers.
- Emit JSON logs for MACC or another orchestrator.

### 17.4 Troubleshooting hooks

If a hook does not fire:

1. Run `agy inspect`.
2. Check hook file path and JSON validity.
3. Check permissions for the hook executable.
4. Confirm the hook event name exists in your CLI version.
5. Run the command manually outside Antigravity.
6. Reduce the hook to a minimal script that prints a timestamp.

---

## 18. Subagents, Background Tasks, and Async Work

### 18.1 Concept

Subagents allow the main agent to delegate work in parallel. This is useful for long-running tasks such as:

- large refactors,
- codebase research,
- generating tests across modules,
- investigating multiple bug hypotheses,
- documentation generation,
- dependency audits.

### 18.2 Managing subagents

Inside TUI:

```text
/agents
/tasks
```

Reported direct invocation:

```text
/agent refactor "Convert all callback-based handlers in @internal/api to use context.Context"
```

Verify direct syntax with `/help`; newer versions may route through `/agents` or `/tasks` panels.

### 18.3 Quota warning

Parallel subagents consume quota in parallel. Before running large background work:

```text
/usage
```

Recommended pattern:

1. Ask the main agent to produce a plan.
2. Split the plan into independent scopes.
3. Launch subagents only for independent scopes.
4. Review all diffs before accepting.
5. Run tests once all subagents finish.

---

## 19. Sidecars

Sidecars are auxiliary processes or environments that can interact programmatically with Antigravity. Official indexed sidecar docs mention that sidecars can use the `agentapi` CLI and that the executable is automatically added to the sidecar path.

Potential uses:

- monitoring agent state,
- automating reviews,
- collecting artifacts,
- bridging an external orchestrator to the agent,
- running long-lived helper processes.

Verify current sidecar configuration and `agentapi` command syntax in the official sidecars page before implementation.

---

## 20. Artifacts and Review

Antigravity agents produce artifacts to make work reviewable. In the broader Antigravity platform, artifacts include:

- task lists,
- implementation plans,
- walkthroughs,
- code diffs,
- screenshots,
- browser recordings,
- logs.

In CLI workflows, artifact visibility may be more compact than in Antigravity 2.0. Use `/export` when a terminal session needs richer visual review in the desktop UI.

---

## 21. Migration from Gemini CLI

### 21.1 High-level migration checklist

1. Install Antigravity CLI.
2. Verify `agy --version`.
3. Run one small interactive task.
4. Import Gemini CLI extensions/plugins:

   ```bash
   agy plugin import gemini
   ```

5. Move workspace skills:

   ```bash
   mkdir -p .agents
   mv .gemini/skills .agents/skills
   ```

6. Move MCP configuration from inline `settings.json` to `mcp_config.json`.
7. Rename remote MCP fields from `url` / `httpUrl` to `serverUrl`.
8. Keep or consolidate `AGENTS.md` / `GEMINI.md` context files.
9. Run:

   ```bash
   agy inspect
   ```

10. Inside TUI, validate:

   ```text
   /skills
   /mcp
   /permissions
   /usage
   ```

11. Run a representative real workflow.
12. Keep Gemini CLI as rollback only while your account tier still supports it.

### 21.2 Mapping table

| Concept | Gemini CLI | Antigravity CLI |
|---|---|---|
| Binary | `gemini` | `agy` |
| Runtime language | Node.js | Go |
| Extensions | Extensions | Plugins |
| Plugin import | N/A | `agy plugin import gemini` |
| Global skills | `~/.gemini/skills/` | `~/.gemini/antigravity-cli/skills/` |
| Workspace skills | `.gemini/skills/` | `.agents/skills/` |
| MCP config | Inline in `settings.json` | Dedicated `mcp_config.json` |
| Remote MCP URL field | `url` / `httpUrl` | `serverUrl` |
| Context files | `GEMINI.md`, `AGENTS.md` | `GEMINI.md`, `AGENTS.md` |
| Hooks | JSON hooks | JSON hooks, verify schema |
| SSH auth | Workarounds/common friction | First-class browser URL/code flow |
| Subagents | Limited or earlier implementation | Built-in background orchestration |

---

## 22. Practical Usage Recipes

### 22.1 Explore a new repository

```bash
cd my-project
agy
```

In TUI:

```text
Explain this repository architecture.
Identify the main entry points and test strategy.
```

Then:

```bash
agy inspect
```

### 22.2 One-shot codebase query

```bash
agy -p "Find all TODO comments and group them by package" --output-format json
```

### 22.3 Generate tests safely

```text
Use Planning mode. Inspect @src/auth/ and propose a test plan first. Do not edit files yet.
```

After approving the plan:

```text
Proceed with tests only. Do not modify production code.
```

### 22.4 Run a background refactor

```text
Create a plan to refactor @src/api/ validation. Split independent work into subagents. Ask before editing files.
```

Then use:

```text
/agents
/tasks
```

### 22.5 Export to GUI for review

```text
/export
```

Use this when diffs, artifacts, or walkthroughs are easier to review visually in Antigravity 2.0.

### 22.6 Confirm environment after migration

```bash
agy inspect
```

Inside TUI:

```text
/skills
/mcp
/hooks
/permissions
/usage
```

---

## 23. Possible Error Cases and Troubleshooting Catalog

> No complete public Antigravity CLI numeric error-code catalog was found in the supplied docs. The table below provides practical, observable error classes, likely symptoms, and recommended remediation. The `AGY-*` codes are documentation/local-operations codes for this guide, not official Google codes.

### 23.1 Install and binary errors

| Code | Symptom | Likely cause | Remediation | MACC mapping |
|---|---|---|---|---|
| `AGY-INSTALL-001` | `agy: command not found` / not recognized | PATH not updated or binary not installed | Add install directory to PATH, restart terminal, run `agy --version` | `E102 ToolNotFound` |
| `AGY-INSTALL-002` | Permission denied executing `agy` | Missing executable bit on Unix | `chmod +x ~/.local/bin/agy`; reinstall if needed | `E102` |
| `AGY-INSTALL-003` | Installer download fails | DNS/TLS/proxy/firewall | Retry on trusted network, use enterprise proxy settings, validate TLS interception | `E101 Network` |
| `AGY-INSTALL-004` | PowerShell script blocked | Execution policy or corporate controls | Use approved installer path or signed enterprise distribution | `E201 Auth/Policy` |

### 23.2 Authentication errors

| Code | Symptom | Likely cause | Remediation | MACC mapping |
|---|---|---|---|---|
| `AGY-AUTH-001` | Browser sign-in fails | Invalid account, blocked OAuth, browser issue | Retry login, check account eligibility, use enterprise auth path | `E201 Auth` |
| `AGY-AUTH-002` | SSH login code does not complete | Remote callback/code flow expired | Re-run `agy`, open new URL locally, paste fresh code | `E101 Timeout` / `E201 Auth` |
| `AGY-AUTH-003` | Keyring/credential-store error | Missing libsecret/keychain access/Windows Credential Manager issue | Install keyring dependencies, unlock keychain, retry login | `E201 Auth` |
| `AGY-AUTH-004` | API key ignored | Wrong env var or unsupported auth mode | Confirm current env var with official docs and `agy --help` | `E201 Auth` |

### 23.3 Provider, quota, and rate-limit errors

| Code | Symptom | Likely cause | Remediation | MACC mapping |
|---|---|---|---|---|
| `AGY-PROVIDER-001` | Rate limit / transient quota message | Temporary throttling | Back off, reduce subagents, retry later | `E601 RateLimit` |
| `AGY-PROVIDER-002` | Weekly/monthly quota exhausted | Hard account cap | Wait for reset, switch model/account, use enterprise/API billing | `E602 QuotaExhausted` |
| `AGY-PROVIDER-003` | Model unavailable | Plan, rollout, region, enterprise policy | Use `/model`, choose available model, check account policy | `E201` / `E202` |
| `AGY-PROVIDER-004` | Internal server error | Provider backend issue | Retry with backoff; avoid repeated immediate retries | `E901 Internal` |

### 23.4 Permission and filesystem errors

| Code | Symptom | Likely cause | Remediation | MACC mapping |
|---|---|---|---|---|
| `AGY-PERM-001` | Command blocked | Deny list or Ask policy | Review `/permissions`; allow only safe command prefixes | `E202 CapabilityGuard` |
| `AGY-PERM-002` | File read/write denied | Workspace boundary, Strict Mode, file permission | Keep restriction or add explicit safe permission | `E202` |
| `AGY-PERM-003` | Agent asks too often | Conservative Request Review policy | Add narrow Allow entries such as `command(git status)` | Not an error |
| `AGY-PERM-004` | Agent edits outside expected scope | Non-workspace file access enabled or broad write rule | Disable non-workspace access; tighten `write_file` paths | `E202` |

### 23.5 MCP errors

| Code | Symptom | Likely cause | Remediation | MACC mapping |
|---|---|---|---|---|
| `AGY-MCP-001` | MCP server not listed | Wrong config file path | Use global `~/.gemini/antigravity-cli/mcp_config.json` or workspace `.agents/mcp_config.json` | `E303 MissingConfig` |
| `AGY-MCP-002` | Remote server silently fails | Used `url`/`httpUrl` instead of `serverUrl` | Rename field to `serverUrl` | `E103 OutputMalformed` / config validation |
| `AGY-MCP-003` | Local MCP command fails | Command missing or env var absent | Run command manually; set env placeholders/secrets locally | `E102` / `E201` |
| `AGY-MCP-004` | MCP auth fails | OAuth/API credential missing | Authenticate server separately; verify `/mcp` status | `E201 Auth` |

### 23.6 Skills and plugin errors

| Code | Symptom | Likely cause | Remediation | MACC mapping |
|---|---|---|---|---|
| `AGY-SKILL-001` | Skill not shown in `/skills` | Wrong folder or missing `SKILL.md` | Place under `.agents/skills/<name>/SKILL.md` or global skills path | `E303` |
| `AGY-SKILL-002` | Skill loads but behaves poorly | Weak description/instructions | Add clear description, trigger cases, allowed behavior, constraints | Not an error |
| `AGY-PLUGIN-001` | `agy plugin import gemini` fails | Legacy extension schema unsupported | Inspect plugin output; migrate manually | `E103` |
| `AGY-PLUGIN-002` | Imported plugin missing theme/feature | Unsupported migration component | Re-author as plugin/skill or accept limitation | `E202` |

### 23.7 Hook errors

| Code | Symptom | Likely cause | Remediation | MACC mapping |
|---|---|---|---|---|
| `AGY-HOOK-001` | Hook does not run | Wrong event/schema/path | Validate with `agy inspect`, reduce to minimal hook | `E103` |
| `AGY-HOOK-002` | Hook blocks too much | Over-broad deny/exit status | Narrow match rules; log hook decisions | `E202` |
| `AGY-HOOK-003` | Hook command not found | Missing executable dependency | Install dependency or use absolute path | `E102` |

### 23.8 TUI and terminal errors

| Code | Symptom | Likely cause | Remediation | MACC mapping |
|---|---|---|---|---|
| `AGY-TUI-001` | UI rendering glitches | Terminal compatibility, resize bug, unsupported TERM | Try modern terminal, update CLI, set `TERM=xterm-256color` | `E101` |
| `AGY-TUI-002` | Non-interactive shell hangs | TUI launched in CI/no TTY | Use `agy -p` and structured output | `E101 Timeout` |
| `AGY-TUI-003` | Keybindings not working | Terminal intercepts keys | Use `/keybindings`; check terminal shortcuts | Not an error |

---

## 24. Troubleshooting Runbook

### 24.1 First five checks

```bash
agy --version
agy --help
which agy
agy inspect
```

Inside TUI:

```text
/help
/usage
/permissions
/mcp
/skills
```

### 24.2 When the agent ignores instructions

1. Confirm `AGENTS.md` or `GEMINI.md` is in the workspace root.
2. Run `agy inspect` and verify loaded context files.
3. Remove conflicting global instructions temporarily.
4. Make instructions explicit and test with a small prompt.
5. Prefer short, enforceable rules over long prose.

### 24.3 When skills do not appear

1. Confirm folder layout:

   ```text
   .agents/skills/<skill-name>/SKILL.md
   ```

2. Run `agy inspect`.
3. Open `/skills`.
4. Restart the CLI if needed.
5. Avoid symlink/junction setups until your version confirms support.

### 24.4 When MCP does not connect

1. Validate JSON syntax.
2. Confirm correct path:

   ```text
   .agents/mcp_config.json
   ~/.gemini/antigravity-cli/mcp_config.json
   ```

3. For remote servers, use `serverUrl`.
4. Check `/mcp`.
5. Run local server commands manually.
6. Verify credentials and environment variables.

### 24.5 When quota disappears quickly

1. Run `/usage` before and after a task.
2. Avoid parallel subagents for exploratory prompts.
3. Use Fast mode and smaller scopes for simple tasks.
4. Use Planning mode to prevent wasted edits.
5. Switch to a cheaper/faster model if available.
6. Avoid repeated automatic retries on hard quota errors.

### 24.6 When the agent wants to run dangerous commands

1. Use Request Review policy.
2. Add Deny rules for destructive commands.
3. Keep terminal sandbox enabled.
4. Work inside a Git worktree.
5. Reject commands that touch absolute paths outside the repository.
6. Ask the agent to explain command purpose before approval.

---

## 25. Recommended Default Project Setup

### 25.1 `AGENTS.md`

```markdown
# Agent Instructions

- Use English for code, commits, and documentation.
- Prefer small, reviewable changes.
- Before editing, explain the plan for non-trivial tasks.
- Use the package manager already configured in the repository.
- Do not modify secrets, credentials, or files outside this repository.
- Ask before running destructive commands or database migrations.
- After code changes, run the smallest relevant validation command first.
```

### 25.2 `.agents/skills/validate/SKILL.md`

```markdown
---
name: validate
description: Run the project's standard validation sequence and summarize failures.
---

Run the smallest relevant validation commands first. Prefer existing package scripts.
If a command fails, summarize the error, likely cause, and next fix.
Do not change code unless explicitly asked.
```

### 25.3 `.agents/mcp_config.json`

```json
{
  "servers": {}
}
```

Start empty. Add MCP servers one at a time and verify with `/mcp`.

---

## 26. MACC Adapter Notes

MACC is a multi-assistant configuration manager whose source of truth is `.macc/macc.yaml`. It aims to generate tool-specific config files, skills, agents, MCP definitions, permissions, and safe backups from a canonical project configuration. Antigravity CLI fits naturally as another adapter because it has its own context files, skill locations, plugin layer, MCP config, permissions, and runner command.

### 26.1 Proposed MACC tool ID

```yaml
tools:
  antigravity_cli:
    enabled: true
    binary: agy
    display_name: Antigravity CLI
```

### 26.2 Proposed runner commands

```yaml
runner:
  interactive: "agy"
  version: "agy --version"
  inspect: "agy inspect"
  prompt_json: "agy -p {prompt} --output-format json"
  prompt_text: "agy -p {prompt}"
```

### 26.3 Proposed generated files

| MACC source | Antigravity target |
|---|---|
| Canonical standards | `AGENTS.md` and optional `GEMINI.md` |
| Selected workspace skills | `.agents/skills/<skill>/SKILL.md` |
| Selected MCP servers | `.agents/mcp_config.json` |
| Permission policy | Antigravity settings/permissions file, if stable schema is confirmed |
| Hooks | Workspace hook config, if stable schema is confirmed |

### 26.4 Proposed global/user files

| Artifact | Target |
|---|---|
| Global skills | `~/.gemini/antigravity-cli/skills/<skill>/SKILL.md` |
| Global MCP | `~/.gemini/antigravity-cli/mcp_config.json` |
| Plugins | `~/.gemini/antigravity-cli/plugins/<plugin>/` |
| User settings | `~/.gemini/antigravity-cli/settings.json` |
| Keybindings | `~/.gemini/antigravity-cli/keybindings.json` |

User-level writes should require backup and explicit consent.

### 26.5 Proposed MACC safety policy

- Never write real secrets into MCP config.
- Use `${ENV_VAR}` placeholders.
- Require consent before modifying `~/.gemini/antigravity-cli/`.
- Generate a diff preview before `macc apply` writes Antigravity files.
- Add `.macc/cache/` to `.gitignore` for fetched remote packages.
- Default Antigravity CLI runner to conservative permissions and non-workspace isolation.

### 26.6 Proposed MACC error mapping

| Antigravity local category | MACC canonical code |
|---|---|
| Binary missing | `E102` |
| Non-zero CLI exit | `E101` |
| Invalid JSON/stream output | `E103` |
| Permission/capability denied | `E202` |
| Auth/billing/policy | `E201` |
| Network/timeout | `E101` |
| Rate limit/transient throttle | `E601` |
| Hard quota exhaustion | `E602` |
| Session conflict | `E603` |
| Unknown fatal | `E901` |

### 26.7 Proposed MACC smoke test

```bash
agy --version
agy inspect
agy -p "Reply with a one-line readiness check." --output-format json
```

Expected:

- version command exits 0,
- inspect reports context paths without fatal errors,
- prompt command returns parseable output or a documented non-zero error.

---

## 27. Security Recommendations

1. Use Git worktrees or disposable branches for agent work.
2. Keep terminal auto-execution on Request Review initially.
3. Enable terminal sandbox where available.
4. Keep non-workspace file access disabled.
5. Use Deny rules for destructive commands.
6. Keep MCP environment variables as placeholders in committed files.
7. Use `serverUrl` for remote MCP servers and verify their trust boundary.
8. Review plugin contents before import.
9. Avoid installing unknown remote plugins or skills without inspection.
10. Run `agy inspect` before trusting a workspace.
11. Check `/usage` before launching parallel tasks.
12. Record generated changes through Git commits or MACC logs.

---

## 28. Quick Reference

### Install

```bash
curl -fsSL https://antigravity.google/cli/install.sh | bash
agy --version
```

```powershell
irm https://antigravity.google/cli/install.ps1 | iex
agy --version
```

### Start

```bash
cd my-project
agy
```

### One-shot

```bash
agy -p "Explain this repository"
agy -p "List TODOs" --output-format json
```

### Inspect

```bash
agy inspect
```

### Migrate Gemini CLI plugins/extensions

```bash
agy plugin import gemini
```

### Validate in TUI

```text
/help
/usage
/permissions
/model
/skills
/mcp
/agents
/tasks
/keybindings
/statusline
```

### Workspace paths

```text
AGENTS.md
GEMINI.md
.agents/skills/<skill>/SKILL.md
.agents/mcp_config.json
```

### Global paths

```text
~/.gemini/antigravity-cli/settings.json
~/.gemini/antigravity-cli/keybindings.json
~/.gemini/antigravity-cli/skills/
~/.gemini/antigravity-cli/mcp_config.json
~/.gemini/antigravity-cli/plugins/
```

---

## 29. Source URLs Used

Official/user-supplied URLs:

- https://antigravity.google/docs/cli-overview
- https://antigravity.google/docs/cli-getting-started
- https://antigravity.google/docs/cli-using
- https://antigravity.google/docs/cli-features
- https://antigravity.google/docs/gcli-migration
- https://antigravity.google/docs/projects
- https://antigravity.google/docs/models
- https://antigravity.google/docs/agent-settings
- https://antigravity.google/docs/permissions
- https://antigravity.google/docs/subagents
- https://antigravity.google/docs/strict-mode
- https://antigravity.google/docs/plugins
- https://antigravity.google/docs/mcp
- https://antigravity.google/docs/skills
- https://antigravity.google/docs/rules-workflows
- https://antigravity.google/docs/hooks
- https://antigravity.google/docs/sidecars
- https://dev.to/arindam_1729/antigravity-cli-a-hands-on-guide-to-googles-terminal-coding-agent-5bc7

Additional public sources used for cross-checking:

- https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/
- https://codelabs.developers.google.com/getting-started-google-antigravity

---

## 30. Validation To-Do for a Live Installation

Because Antigravity CLI is new and changing quickly, run this validation against the installed version before hardcoding automation:

```bash
agy --version
agy --help
agy -p "Return JSON with key status and value ok" --output-format json
agy inspect
```

Inside TUI:

```text
/help
/config
/permissions
/model
/usage
/skills
/mcp
/agents
/tasks
/keybindings
/statusline
```

Record:

- exact version,
- supported flags,
- output formats,
- settings file schema,
- hook schema,
- permission schema,
- MCP schema,
- skill discovery behavior,
- plugin import output,
- exit codes and stderr patterns.

Use that data to finalize a production MACC adapter.

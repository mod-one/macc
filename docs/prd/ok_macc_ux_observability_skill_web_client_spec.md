# MACC UX, Observability, Skill Runner, and Web Client Improvement Specification

**Project:** MACC — Multi-Assistant Code Config  
**Document type:** Product + technical specification  
**Language:** English  
**Status:** Proposal / implementation-ready design  
**Date:** 2026-05-27  
**Scope:** Developer Experience, TUI Observer Mode, Unified Skill Runner, Mission-Control Web Client, token/context ergonomics, shared runtime architecture, roadmap, and acceptance criteria.

---

## 0. Executive summary

MACC already has the core foundations of a multi-assistant development orchestration platform:

- a canonical project configuration source of truth;
- adapters that render tool-specific files;
- worktree-based parallelism;
- coordinator and performer automation;
- task registry and PRD reconciliation;
- centralized logs;
- planned Rust/Ratatui TUI;
- planned local Web UI with Axum, React, SSE, WebSocket terminals, logs, diagnostics, git graph, and worktree operations.

The main product gap is not capability. The main gap is **operability**.

When MACC runs multiple AI coding tools across multiple worktrees, the developer needs a calm, trustworthy, high-signal interface that answers four questions at all times:

1. **What is MACC doing right now?**
2. **Why is it doing that?**
3. **What is blocked, risky, stale, or rate-limited?**
4. **What is the safest next action I can take?**

This document proposes three primary UX motifs and one cross-cutting token/context motif:

- **U1. Real-Time TUI Observer Mode**  
  A live Ratatui cockpit exposed through `macc status --watch` and `macc watch`.

- **U2. Unified Skill Runner**  
  A canonical skill execution facade exposed through `macc run <skill-name>`, independent of Gemini, Codex, Claude, Vibe, Agy, or future tool syntax.

- **U3. Mission-Control Web Client**  
  A local browser-based control surface for configuration, planning, observability, logs, worktrees, skill runs, diagnostics, PRD editing, and recovery.

- **U4. Context and Token Budget UX**  
  Default hooks and summarization pipelines that prevent raw logs, test output, stack traces, and full diffs from flooding model context.

The unifying recommendation is to build a **shared runtime snapshot and event model** in `macc-core`, then consume it from CLI, TUI, Web UI, logs, diagnostics, and automation.

---

## 1. Design principles

### 1.1 One source of truth

The same canonical state should power:

```bash
macc status --json
macc status --watch
macc web
macc coordinator status
```

The Web UI, TUI, and CLI must not invent separate state models.

Recommended shared model:

```rust
pub struct RuntimeSnapshot {
    pub coordinator: CoordinatorStatus,
    pub workers: Vec<WorkerRuntime>,
    pub tasks: Vec<TaskRuntimeSummary>,
    pub throttled_tools: Vec<ToolThrottleStatus>,
    pub recent_events: Vec<CoordinatorEvent>,
    pub git: GitRuntimeSummary,
    pub diagnostics: RuntimeDiagnostics,
    pub active_runs: Vec<SkillRunSummary>,
}
```

### 1.2 Read-first, control-second

Observability should be safe by default.

- `macc status --watch` is read-only.
- `macc status --watch --control` enables pause, resume, retry, stop, and worker-level actions.
- Web UI pages should visually distinguish read-only inspection from mutating operations.
- Dangerous operations require typed confirmation or double-confirmation.

### 1.3 High signal, low noise

MACC should collapse noisy output into meaningful summaries:

- failed tests only;
- lint errors only;
- collapsed stack traces;
- diff stats before full diffs;
- error-first log views;
- structured event timelines before raw logs.

The user should always be able to open the raw source, but raw output should not be the default.

### 1.4 Spatial continuity

The interface should keep core project state in stable locations:

- left navigation;
- top project/search/command area;
- right context/git panel;
- bottom runtime status strip;
- center content;
- drawers for details.

This applies especially to the Web Client.

### 1.5 Adapter isolation

Tool-specific differences must remain inside adapters.

The user should not need to know whether a skill is installed as:

```text
.gemini/commands/*.toml
.gemini/skills/*
.codex/skills/*
.vibe/skills/*
.agents/skills/*
a generated prompt
a tool-native command
```

The user-facing operation should be:

```bash
macc run validate
macc run implement
macc run security-check
```

### 1.6 Trust before automation

Every generated change should be explainable.

Before MACC writes files, modifies user-level configuration, creates worktrees, resumes tasks, or launches agents, the user should be able to inspect:

- what will happen;
- why it will happen;
- what files will change;
- what backup will be created;
- what risk level applies;
- how to revert.

---

## 2. U1 — Real-Time TUI Observer Mode

### 2.1 Problem

MACC can schedule multiple worktrees and run tool-specific performers concurrently. However, without a real-time cockpit, this execution feels opaque.

Developers currently need to inspect multiple sources manually:

```bash
macc coordinator status
tail -f .macc/log/coordinator/*
tail -f .macc/log/performer/*
git worktree list
cat .macc/automation/task/task_registry.json
```

This is inefficient, mentally expensive, and risky during long autonomous runs.

### 2.2 Recommendation

Add a live Ratatui observer mode:

```bash
macc status --watch
```

Optional alias:

```bash
macc watch
```

This opens a terminal dashboard that shows:

- coordinator state;
- worktree grid;
- active workers;
- task progress;
- runtime phases;
- logs;
- event timeline;
- stale workers;
- failed tasks;
- rate-limited tools;
- merge blockers;
- recent commits;
- effective parallelism.

### 2.3 CLI surface

```bash
macc status
macc status --json
macc status --watch
macc status --watch --control
macc status --watch --task AUTH-012
macc status --watch --tool codex
macc status --watch --failed
macc status --watch --rate-limited
macc status --watch --logs-only
macc status --watch --events-only
macc watch
```

### 2.4 Default mode

`macc status --watch` should be read-only.

It allows:

- navigating workers;
- opening log panes;
- filtering events;
- searching within logs;
- viewing task details;
- inspecting git state;
- copying diagnostics.

It does not allow:

- stopping the coordinator;
- killing workers;
- deleting worktrees;
- retrying tasks;
- editing the registry;
- mutating PRD files;
- applying configuration.

### 2.5 Control mode

`macc status --watch --control` enables operator actions.

Suggested keys:

```text
p   pause coordinator
r   resume coordinator
s   graceful stop
S   emergency stop
d   dispatch next task
R   retry selected failed task
b   block selected task
u   unblock selected task
k   kill selected performer
l   open selected log
t   open selected worktree terminal
g   open git diff for selected worktree
q   quit dashboard
```

Dangerous keys must trigger confirmation.

### 2.6 TUI layout

Recommended default layout:

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│ MACC Observer | repo: /project | branch: main | state: running | workers 4/6 │
├─────────────────────┬──────────────────────────────────────┬────────────────┤
│ Worktrees           │ Task / PRD progress                  │ Event timeline │
│                     │                                      │                │
│ wt-01 codex running │ AUTH                                 │ 12:41 dispatch │
│ wt-02 gemini review │ ├─ AUTH-001 merged                  │ 12:42 heartbeat│
│ wt-03 claude stale  │ ├─ AUTH-012 running                 │ 12:43 E601     │
│ wt-04 idle          │ └─ AUTH-018 blocked                 │ 12:44 fallback │
├─────────────────────┴──────────────────────────────────────┴────────────────┤
│ Logs: selected worker / task / coordinator                                  │
│ ...                                                                          │
├──────────────────────────────────────────────────────────────────────────────┤
│ codex throttled 02:31 | stale: wt-03 | queue: 12 todo, 3 running, 1 blocked  │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 2.7 Worktree grid

Fields:

```text
Worktree ID
Tool
Task ID
Task title
Phase
Runtime status
Last heartbeat
Git cleanliness
Branch
Retry count
Delayed-until timestamp
```

Example:

```text
┌──────────┬────────┬──────────┬──────────────┬────────────┬─────────────┐
│ Worktree │ Tool   │ Task     │ Phase        │ Runtime    │ Git         │
├──────────┼────────┼──────────┼──────────────┼────────────┼─────────────┤
│ wt-01    │ codex  │ AUTH-012 │ dev          │ running    │ modified    │
│ wt-02    │ gemini │ UI-008   │ review       │ active     │ clean       │
│ wt-03    │ claude │ API-021  │ fix          │ stale      │ conflict    │
│ wt-04    │ codex  │ DB-004   │ rate-limit   │ delayed    │ clean       │
└──────────┴────────┴──────────┴──────────────┴────────────┴─────────────┘
```

Runtime statuses:

```text
idle
dispatched
running
phase_done
failed
stale
rate_limited
blocked
merge_conflict
cleanup_pending
released
```

### 2.8 Task checklist

The TUI should show semantic PRD progress rather than only raw runtime logs.

Example:

```text
PRD
├─ AUTH
│  ├─ [merged] AUTH-001 Add login form
│  ├─ [running] AUTH-012 Implement refresh token rotation
│  └─ [blocked] AUTH-018 Add OAuth provider
├─ UI
│  ├─ [review] UI-008 Refactor dashboard shell
│  └─ [todo] UI-011 Add empty states
└─ DB
   └─ [rate-limited] DB-004 Generate migration review
```

Task states should reflect the canonical registry and runtime overlay.

### 2.9 Unified log viewer

The bottom pane should support:

- coordinator log tail;
- performer log tail;
- selected worktree logs;
- selected task logs;
- selected skill run logs;
- event JSONL view;
- failure-first mode.

Useful controls:

```text
f   follow / unfollow
/   search
e   errors only
w   warnings only
a   all logs
c   collapse stack traces
j/k scroll
n/N next/previous match
```

### 2.10 Event timeline

The TUI should present structured events.

Example:

```text
12:41:03 dispatch AUTH-012 to wt-01/codex
12:41:08 heartbeat AUTH-012 phase=dev
12:42:19 commit_created AUTH-012 sha=abc123
12:42:30 review_started AUTH-012
12:43:01 E601 codex rate-limited, backoff=120s
12:43:02 fallback dispatch UI-008 to gemini
```

The timeline should show:

- dispatch;
- heartbeat;
- phase start;
- phase result;
- commit created;
- review result;
- retry scheduled;
- throttle applied;
- fallback selected;
- merge conflict;
- cleanup deferred;
- coordinator pause/resume;
- user action from TUI or Web UI.

### 2.11 Implementation approach

Introduce a core snapshot builder:

```rust
pub trait RuntimeSnapshotProvider {
    fn current_snapshot(&self) -> Result<RuntimeSnapshot>;
    fn watch(&self) -> Result<RuntimeSnapshotStream>;
}
```

Sources:

- task registry JSON or SQLite;
- `events.jsonl`;
- `.macc/state/tool-sessions.json`;
- git worktree state;
- git status per worktree;
- log metadata;
- throttle registry;
- coordinator status file or IPC channel;
- recent skill run logs.

### 2.12 MVP scope

Include:

- read-only observer mode;
- worktree grid;
- task checklist;
- selected log tail;
- event timeline;
- throttled tools display;
- stale worker display;
- paused coordinator banner;
- keyboard navigation;
- `--json` snapshot output.

Defer:

- embedded terminal;
- interactive registry editing;
- custom layouts;
- advanced log analytics;
- direct branch operations;
- destructive controls.

### 2.13 Acceptance criteria

- `macc status --watch` opens a Ratatui dashboard.
- Dashboard shows coordinator state, worktrees, workers, tasks, throttled tools, and recent events.
- Dashboard tails selected logs without leaving the TUI.
- Dashboard detects stale workers and rate-limited tools.
- Dashboard works while `macc coordinator` is running.
- Dashboard is read-only unless `--control` is passed.
- Dangerous actions require confirmation.
- The same runtime snapshot is available through `macc status --json` and Web API.

---

## 3. U2 — Unified Skill Runner

### 3.1 Problem

MACC aims to provide a consistent developer experience across AI coding tools, but skill execution remains fragmented.

Each tool may have its own convention:

```text
Gemini: .gemini/commands/*.toml or .gemini/skills/
Codex: .codex/skills/
Vibe: .vibe/skills/
Agy: .agents/skills/
Other tools: custom prompt files, command runners, or agent definitions
```

This forces developers to remember tool-specific syntax and maintain duplicate workflows.

### 3.2 Recommendation

Add a canonical execution facade:

```bash
macc run <skill-name>
```

Examples:

```bash
macc run validate
macc run validate --tool codex
macc run implement --task AUTH-012
macc run security-check --scope "src/auth/**"
macc run refresh-context
macc run update-progress --feature onboarding
macc run validate --watch
```

The user should think in terms of MACC skills, not tool-specific command syntax.

### 3.3 Skill execution contract

A MACC skill should be canonical.

Conceptual YAML shape:

```yaml
id: validate
title: Validate project
kind: command_workflow
description: Run lint, build, and tests.
risk: safe
inputs:
  - name: scope
    type: string
    optional: true
steps:
  - run: pnpm lint
  - run: pnpm build
  - run: pnpm test:e2e
targets:
  codex:
    strategy: prompt
  gemini:
    strategy: command
  claude:
    strategy: prompt
```

### 3.4 Skill kinds

#### 3.4.1 Prompt skill

A prompt-only skill sent to an AI tool.

Best for:

- security review;
- SEO review;
- architecture review;
- PRD audit;
- context refresh;
- code review.

Example:

```bash
macc run security-check --tool codex
```

#### 3.4.2 Command workflow skill

A deterministic local command sequence.

Best for:

- validation;
- formatting;
- linting;
- test execution;
- doctor checks;
- database checks.

Example:

```bash
macc run validate
```

Executes:

```bash
pnpm lint
pnpm build
pnpm test:e2e
```

No LLM is required unless a later step asks for diagnosis.

#### 3.4.3 Hybrid skill

A deterministic command sequence followed by AI interpretation.

Example:

```bash
macc run validate-explain
```

Flow:

```text
run lint/build/test
capture failures only
collapse stack traces
summarize output within token budget
send summary to selected tool
ask for diagnosis and fix plan
```

#### 3.4.4 Agent skill

A skill routed to a specific agent persona.

Examples:

```bash
macc run tech-stack --agent architect
macc run seo-check --agent seo-specialist
macc run review --agent code-reviewer
```

#### 3.4.5 Coordinator skill

A skill that acts on PRD, task registry, commit history, or coordinator state.

Examples:

```bash
macc run next-task
macc run audit-prd
macc run update-progress
macc run sync-prd
```

### 3.5 Tool selection algorithm

When the user runs:

```bash
macc run validate
```

MACC resolves the target tool in this order:

1. `--tool` flag.
2. `.macc/worktree.json`, if running inside a MACC worktree.
3. `.macc/tool.json`, if present.
4. `skills.run_policy.default_tool`.
5. `automation.coordinator.default_tool`.
6. First available tool in `tool_priority`.
7. First enabled tool in `.macc/macc.yaml`.
8. Error with suggested available tools.

For local-only command skills, MACC may not need a tool.

### 3.6 Adapter interface

Extend `ToolAdapter` with skill execution capabilities.

```rust
pub trait ToolAdapter {
    fn id(&self) -> ToolId;

    fn supports_skill_install(&self) -> bool;
    fn supports_skill_run(&self) -> bool;
    fn supports_prompt_stdin(&self) -> bool;
    fn supports_session_resume(&self) -> bool;

    fn render_skill(&self, skill: &ResolvedSkill) -> Result<Vec<GeneratedFile>>;
    fn build_skill_invocation(&self, request: SkillRunRequest) -> Result<ToolInvocation>;
}
```

Skill run request:

```rust
pub struct SkillRunRequest {
    pub skill_id: String,
    pub tool_id: Option<String>,
    pub cwd: PathBuf,
    pub task_id: Option<String>,
    pub feature: Option<String>,
    pub scope: Option<Vec<String>>,
    pub inputs: BTreeMap<String, String>,
    pub session_policy: SessionPolicy,
    pub dry_run: bool,
    pub watch: bool,
}
```

### 3.7 Execution strategies

Adapters should declare supported strategies.

#### Strategy A — native skill invocation

The tool can directly run a named skill.

```bash
tool run validate
```

#### Strategy B — generated skill plus trigger

MACC installs a generated skill file, then invokes the tool with a trigger phrase.

#### Strategy C — prompt fallback

MACC compiles the canonical skill into a complete prompt and sends it to the tool.

#### Strategy D — local-only execution

MACC runs commands directly without invoking an AI tool.

#### Strategy E — hybrid execution

MACC runs deterministic commands, summarizes outputs, then invokes the selected AI tool.

### 3.8 CLI surface

```bash
macc run <skill>
macc run <skill> --tool <tool>
macc run <skill> --agent <agent>
macc run <skill> --task <task-id>
macc run <skill> --scope <glob>
macc run <skill> --feature <name>
macc run <skill> --dry-run
macc run <skill> --watch
macc run <skill> --json
macc run <skill> --yes
```

Skill inspection commands:

```bash
macc skills list
macc skills list --enabled
macc skills list --tool codex
macc skills show <skill>
macc skills explain <skill>
macc skills doctor
```

### 3.9 Dry-run UX

Dry-run is essential for trust.

Example:

```bash
macc run validate --dry-run
```

Output:

```text
Skill: validate
Kind: command_workflow
Tool: none
Risk: safe

Commands:
  pnpm lint
  pnpm build
  pnpm test:e2e

Writes:
  none

Logs:
  .macc/log/run/<timestamp>-validate.log
```

For an LLM-backed skill:

```bash
macc run security-check --tool codex --dry-run
```

Output:

```text
Skill: security-check
Kind: prompt
Tool: codex
Adapter strategy: prompt_stdin
Risk: safe
Estimated context: 4.2k tokens

Context:
  config/standards.md
  .macc/scope.md
  git diff --stat
  changed files

Writes:
  none unless explicitly confirmed
```

### 3.10 Skill run logging

Every skill run should emit structured logs.

Paths:

```text
.macc/log/run/<timestamp>-<skill>.log
.macc/log/run/<timestamp>-<skill>.jsonl
```

Events:

```json
{
  "type": "skill_started",
  "skill_id": "validate",
  "tool": "codex",
  "cwd": ".macc/worktree/wt-01"
}
```

```json
{
  "type": "skill_finished",
  "skill_id": "validate",
  "status": "success",
  "duration_ms": 42133
}
```

These events should appear in:

- TUI Observer Mode;
- Web Mission Control;
- logs explorer;
- run history.

### 3.11 Worktree integration

Inside a MACC worktree:

```bash
macc run implement
```

MACC should automatically use:

```text
.macc/worktree.json
.macc/scope.md
.macc/selections.lock.json
.macc/tool.json
worktree-scoped session
```

From project root:

```bash
macc run implement --task AUTH-012
```

MVP behavior:

- find an existing worktree assigned to `AUTH-012`;
- run there if found;
- otherwise fail with a clear message and suggested command.

Future behavior:

- create or reuse a compatible worktree automatically.

### 3.12 Skill run policy

Add canonical configuration:

```yaml
skills:
  selected:
    - validate
    - implement
    - security-check

  run_policy:
    default_tool: codex
    allow_local_commands: true
    require_confirmation_for_writes: true
    summarize_tool_output: true
    token_budget: 12000

  skill_defaults:
    validate:
      mode: local
    implement:
      tool: codex
      session: worktree
    security-check:
      tool: gemini
      write_policy: read_only
```

### 3.13 Safety model

Classify each skill:

```text
safe       read-only, analysis, validation, log viewing
caution    local project writes, generated files, formatting
dangerous  deletes, branch operations, resets, force pushes
```

Examples:

```text
validate              safe
security-check        safe
seo-check             safe
implement             caution
git-add-commit-push   caution or dangerous depending on push behavior
permissions-allow     dangerous
clear                 dangerous
```

### 3.14 MVP scope

Include:

- `macc run <skill>`;
- `--tool`;
- `--dry-run`;
- `--watch`;
- local command workflow support;
- prompt skill support for one or two adapters;
- structured run logs;
- basic risk classification;
- worktree-aware execution;
- `macc skills list/show/doctor`.

Defer:

- automatic worktree creation;
- multi-agent orchestration from a single skill;
- marketplace skill execution;
- full native execution for every tool;
- interactive forms in CLI;
- advanced input schemas.

### 3.15 Acceptance criteria

- `macc run validate` executes the canonical validate skill.
- `macc run <skill> --tool <tool>` routes through the selected adapter.
- `macc run <skill> --dry-run` shows commands, prompts, files, risk, and context estimate.
- Skills can be local-only, prompt-only, command-based, or hybrid.
- Run results are logged under `.macc/log/run/`.
- Skill execution respects worktree context.
- Caution and dangerous skills require confirmation.
- Adapters can declare native skill execution, prompt fallback, or install-only rendering support.

---

## 4. U3 — Mission-Control Web Client

### 4.1 Problem

The planned Web Client already includes many strong features: local Axum server, React SPA, REST API, SSE, WebSockets, config editing, logs, diagnostics, git graph, PRD editor, worktree management, and terminals.

The UX opportunity is to make it feel like a coherent **mission-control interface**, not a set of admin pages.

### 4.2 Recommendation

MACC Web should become the primary observability and recovery interface.

It should help the developer:

- understand system state;
- configure safely;
- inspect generated changes;
- run skills;
- monitor autonomous work;
- debug failures;
- edit PRD tasks;
- inspect logs;
- open terminals;
- recover from blocked states;
- trust automation.

### 4.3 Information architecture

Use three top-level groups.

```text
Setup
  Welcome
  Init Wizard
  Tools & Adapters
  Skills / Agents / MCP
  Standards
  Settings
  Plan
  Apply

Build
  PRD Editor
  Task Graph
  Worktrees
  Skill Runner

Ops
  Mission Control
  Live Wall
  Logs
  Diagnostics
  Backups
  Git Graph
  Terminals
```

This separates:

- configuration;
- planning/execution;
- operations/recovery.

### 4.4 Persistent app shell

Recommended layout:

```text
┌─────────────────────────────────────────────────────────────┐
│ Top bar: project path, search, command palette, connection  │
├───────────────┬─────────────────────────────┬───────────────┤
│ Sidebar       │ Current page                │ Context panel │
│               │                             │ Git/task/run  │
├───────────────┴─────────────────────────────┴───────────────┤
│ Bottom status: coordinator, workers, queue, throttles, git   │
└─────────────────────────────────────────────────────────────┘
```

Persistent regions:

- **Left sidebar:** navigation.
- **Top bar:** project, connection status, global search, command palette.
- **Right panel:** git graph or selected task/worker/run context.
- **Bottom strip:** live runtime status.
- **Drawers:** details, forms, confirmations.

### 4.5 Mission Control dashboard

The dashboard should answer:

```text
What is MACC doing right now?
What is blocked?
What needs attention?
What changed recently?
What safe action can I take?
```

Recommended sections:

- coordinator state;
- active workers;
- task queue;
- blocked tasks;
- failed tasks;
- throttled tools;
- recent events;
- recent commits;
- active skill runs;
- diagnostics summary.

Example KPI cards:

```text
Active workers
Ready tasks
Blocked tasks
Merged today
Failed retryable tasks
Throttled tools
Effective parallelism
Last successful merge
```

Use actionable status cards, not decorative charts:

```text
1 blocked merge needs attention
2 tools are rate-limited
3 workers stale for more than 5 minutes
12 tasks ready
```

Each card should have a primary action:

```text
View blocker
Open logs
Retry
Switch tool
Resume coordinator
Run doctor
```

### 4.6 Worktree grid

The Web UI should treat worktrees as live worker slots.

Card view:

```text
┌──────────────────────────────┐
│ wt-01 / codex                │
│ Task: AUTH-012               │
│ Phase: dev                   │
│ Status: running              │
│ Last heartbeat: 12s ago      │
│ Git: 4 files modified        │
│                              │
│ [Logs] [Terminal] [Diff]     │
└──────────────────────────────┘
```

Table view for many workers:

```text
Worktree | Tool | Task | Phase | Runtime | Heartbeat | Git | Actions
```

Interactions:

```text
Click worker       open detail drawer
Double click       open full worker page
j/k                move selection
Enter              open selected worker
l                  logs
t                  terminal
d                  diff
r                  retry, if failed
```

### 4.7 Worker detail drawer

Fields:

- worktree path;
- tool;
- task ID;
- task title;
- branch;
- base branch;
- phase;
- runtime status;
- last heartbeat;
- retry count;
- delayed until;
- git status;
- changed files;
- recent events;
- logs;
- actions.

Actions:

```text
Open logs
Open terminal
View diff
Retry task
Block task
Graceful stop
Kill performer
Cleanup worktree
```

Dangerous actions must use confirmation.

### 4.8 PRD Editor

The PRD editor should not be only a JSON editor. It should be a planning and task-management surface.

Views:

```text
Table view      bulk edit tasks
Board view      todo / in_progress / review / merged / blocked
Graph view      dependency DAG
Timeline view   task execution history
Diff view       compare before/after audit
JSON view       raw fallback
```

Task detail drawer:

```text
Task ID
Title
Description
Steps
Dependencies
Exclusive resources
Assigned tool
Preferred agent
State
Runtime status
Related commits
Related logs
Related PR
Notes
```

### 4.9 AI-assisted PRD audit preview

Expose PRD audit visually.

Flow:

```bash
macc coordinator audit-prd --dry-run
```

Web representation:

```text
Task AUTH-012

Before:
  Implement refresh token rotation.

After:
  Implement refresh token rotation using the new session lease model and updated auth middleware.

Reason:
  Commit abc123 introduced session lease storage and changed middleware boundaries.

[Accept] [Reject] [Edit]
```

Acceptance should be per-task or batch-level.

### 4.10 Skill Runner page

The Web Client should expose U2 visually.

Page sections:

```text
Skill catalog
Selected skill details
Input form
Target tool selector
Risk level
Dry-run preview
Context estimate
Run controls
Run history
```

Actions:

```text
Dry Run
Run
Run & Watch
Copy prompt
Open logs
Open artifacts
```

Skill run detail:

```text
Command steps
Prompt preview
Tool adapter strategy
Logs
Events
Output summary
Artifacts
Exit status
Duration
```

### 4.11 Plan and Apply UX

`macc plan` and `macc apply` are trust-critical.

Plan page should group changes by risk:

```text
Safe
  create .codex/skills/validate.md
  create .gemini/commands/validate.toml

Caution
  modify .mcp.json
  update .gitignore

Dangerous
  user-level config merge
```

Each change should show:

- path;
- adapter;
- reason;
- diff;
- backup behavior;
- risk;
- consent requirement.

Apply page should use a confirmation ladder:

```text
1. Review generated files
2. Review backups
3. Review user-level changes
4. Confirm apply
```

For dangerous changes:

```text
Type APPLY USER CONFIG to confirm.
```

### 4.12 Logs Explorer

The logs page should not be only a file browser.

Recommended features:

- global search across logs;
- filter by tool;
- filter by task;
- filter by error code;
- jump to first failure;
- collapse stack traces;
- show only warnings/errors;
- copy summarized failure report;
- open related task;
- open related worktree;
- open related skill run;
- raw JSONL view.

Add a primary action:

```text
Show me what failed
```

This should surface:

- last non-zero runner exit;
- error code;
- stderr excerpt;
- affected task;
- retry status;
- recommended action;
- related logs.

### 4.13 Diagnostics page

Turn `macc doctor` into a visual repair center.

Sections:

```text
Health score
Critical issues
Warnings
Safe fixes available
Manual actions required
Recently resolved issues
```

Example cards:

```text
Codex binary not found
Severity: critical
Impact: selected adapter cannot run
Action: [Open install guide] [Disable adapter]

.macc/cache is not gitignored
Severity: warning
Impact: remote packages may be committed accidentally
Action: [Apply safe fix]

Gemini selected but not authenticated
Severity: critical
Impact: performer runs will fail
Action: [Open terminal]
```

Classify each fix:

```text
auto-fixable
requires confirmation
manual only
```

### 4.14 Command palette

Add `Ctrl+K`.

Commands:

```text
Run skill
Open task
Open worker
Open logs
Open terminal
Run doctor
Create worktree
Apply config
Pause coordinator
Resume coordinator
Search settings
Search skills
Search MCP servers
Open git graph
Open backups
```

### 4.15 Global search

Add `Ctrl+/`.

Search across:

```text
tasks
skills
tools
logs
settings
MCP servers
agents
worktrees
commits
backups
error codes
```

Result example:

```text
AUTH-012
Task · running · wt-01 · codex

validate
Skill · local command workflow

E601
Error code · rate limit · 3 recent events
```

### 4.16 Right-side context panel

Default: Git Graph.

Contextual modes:

```text
Git Graph
Selected Task
Selected Worker
Selected Skill Run
Selected Log Event
Selected File Diff
```

This avoids excessive page transitions.

### 4.17 Visual design direction

MACC Web should use a console-grade dark UI.

Conceptual references:

- Linear density;
- GitHub Actions clarity;
- Raycast command palette;
- Datadog-style observability;
- VS Code panel ergonomics;
- Stripe-like settings forms.

Visual language:

```text
dark background
high-contrast text
subtle borders
monospace logs
compact cards
clear status badges
low-noise icons
minimal animation
```

Status colors:

```text
green    success / merged
blue     running / active
yellow   warning / stale / retrying
red      failed / blocked / dangerous
purple   AI/tool activity
gray     idle / disabled
```

Color must never be the only signal.

### 4.18 Motion and interaction ergonomics

Good animation uses:

- connection pulse;
- new event slide-in;
- worker state transition;
- drawer open/close;
- diff expansion.

Avoid:

- constant blinking logs;
- large animated backgrounds;
- excessive glassmorphism;
- layout shifts during streaming;
- decorative motion during high-alert states.

### 4.19 Accessibility

Minimum requirements:

- full keyboard navigation;
- visible focus rings;
- ARIA labels;
- screen-reader friendly status updates;
- reduced motion mode;
- high contrast mode;
- resizable panes;
- log font-size control;
- no color-only state encoding.

### 4.20 Component system

Recommended shared components:

```text
StatusBadge
RiskBadge
ToolBadge
TaskStateBadge
RuntimeStatusBadge
KpiCard
WorkerCard
TaskDrawer
WorkerDrawer
SkillRunDrawer
LogStream
EventTimeline
DiffViewer
ConsentGate
CommandPalette
SearchDialog
EmptyState
ErrorBoundary
ConnectionIndicator
ThrottleBadge
GitGraphPanel
TerminalDrawer
DoctorIssueCard
```

### 4.21 Web API additions

Add UX-focused endpoints:

```text
GET  /api/v1/snapshot
GET  /api/v1/search?q=
GET  /api/v1/skills
GET  /api/v1/skills/{id}
POST /api/v1/skills/{id}/dry-run
POST /api/v1/skills/{id}/run
GET  /api/v1/runs
GET  /api/v1/runs/{id}
GET  /api/v1/tasks/{id}/context
GET  /api/v1/workers/{id}/snapshot
GET  /api/v1/failures/recent
```

Most important:

```text
GET /api/v1/snapshot
```

This should return the same shared runtime snapshot used by CLI and TUI.

### 4.22 MVP scope

Include:

- Mission Control dashboard;
- worktree grid;
- task detail drawer;
- global status bar;
- right-side git graph panel;
- logs explorer with filtering;
- plan/apply diff viewer;
- command palette;
- connection state indicator;
- basic Skill Runner page.

Defer:

- drag/drop PRD board;
- full task graph editing;
- multi-terminal layout;
- advanced log analytics;
- custom dashboard layouts;
- collaborative mode;
- remote hosted UI.

### 4.23 Acceptance criteria

- Web UI clearly answers “what is MACC doing right now?”
- Dashboard shows coordinator state, workers, tasks, throttles, blockers, and recent events.
- Users can inspect a worker without leaving the dashboard.
- Users can open logs, diffs, and terminals from selected workers.
- Plan/apply pages show file diffs, risk levels, and backup behavior before writing.
- PRD editor supports table view, detail drawer, and dependency graph.
- Skill Runner can dry-run and run selected skills.
- Dangerous actions use explicit confirmation gates.
- UI remains usable with 20+ concurrent stream tiles.
- Keyboard navigation and command palette work across the app.
- Web UI consumes the same runtime snapshot as CLI and TUI.

---

## 5. U4 — Context and Token Budget UX

### 5.1 Problem

Autonomous coding tools produce huge outputs:

- test logs;
- build logs;
- lint output;
- stack traces;
- full diffs;
- dependency warnings;
- repeated errors;
- raw JSON;
- verbose tool traces.

If MACC forwards this raw output into model context, it wastes tokens and degrades reasoning quality.

### 5.2 Recommendation

Add default tool-output summarization hooks generated by `macc apply`.

Default bundles:

```text
test-output-failures-only
lint-errors-only
stacktrace-collapse
git-diff-stat-before-full-diff
log-grep-error-first
```

These hooks should be configurable per tool and per skill.

### 5.3 Hook pipeline

Recommended flow:

```text
tool output
  ↓
adapter-specific capture
  ↓
MACC hook pipeline
  ↓
structured summary
  ↓
token budget enforcement
  ↓
model context
```

### 5.4 Hook examples

#### test-output-failures-only

Input:

```text
full test output
```

Output:

```text
failed test names
assertion messages
file paths
line numbers
short stderr excerpts
command exit code
```

#### lint-errors-only

Input:

```text
full linter output
```

Output:

```text
error code
file
line
rule
message
fixability
```

#### stacktrace-collapse

Input:

```text
full stack trace
```

Output:

```text
exception type
message
application frames
first external dependency frame
omitted frame count
```

#### git-diff-stat-before-full-diff

Input:

```text
full git diff
```

Output:

```text
diff stat
changed file list
risk hints
only include full diff when requested or under token budget
```

#### log-grep-error-first

Input:

```text
large log file
```

Output:

```text
error lines
warning lines
surrounding context
last non-zero exit
first fatal error
```

### 5.5 Configuration

Add to `.macc/macc.yaml`:

```yaml
context:
  token_budget:
    default: 12000
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

    per_tool:
      codex:
        enabled: true
        token_budget: 12000
      gemini:
        enabled: true
        token_budget: 16000
      claude:
        enabled: true
        token_budget: 12000

    per_skill:
      validate:
        bundles:
          - test-output-failures-only
          - lint-errors-only
      security-check:
        bundles:
          - git-diff-stat-before-full-diff
```

### 5.6 UX exposure

TUI and Web UI should show when output was summarized.

Example:

```text
Output summarized
Raw size: 187k chars
Summary size: 9k chars
Policy: test-output-failures-only + stacktrace-collapse
[Open raw] [Copy summary] [Send full output]
```

### 5.7 Acceptance criteria

- MACC can summarize test, lint, stack trace, diff, and log outputs.
- Summaries preserve failure details.
- Raw output remains accessible.
- Skills can declare which summarization bundles they need.
- Web UI and TUI show summary metadata.
- The summary pipeline can enforce a token budget.
- The system defaults to high-signal summaries before model invocation.

---

## 6. Shared runtime architecture

### 6.1 Target architecture

```text
Coordinator / Skill Runner / Worktree Runner
        ↓
Structured events
        ↓
Event store: .macc/log/events.jsonl
        ↓
Runtime snapshot builder
        ↓
CLI JSON / TUI Observer / Web Mission Control / Diagnostics
```

### 6.2 Event contract

Every meaningful runtime transition should emit a structured event.

Example:

```json
{
  "version": 1,
  "timestamp": "2026-05-27T12:41:03Z",
  "type": "task_dispatched",
  "task_id": "AUTH-012",
  "worktree_id": "wt-01",
  "tool": "codex",
  "phase": "dev"
}
```

Event types:

```text
coordinator_started
coordinator_paused
coordinator_resumed
coordinator_stopped
task_selected
task_dispatched
task_started
heartbeat
phase_started
phase_result
commit_created
review_started
review_completed
fix_started
merge_started
merge_conflict
merge_completed
task_merged
task_failed
task_retry_scheduled
tool_rate_limited
tool_quota_exhausted
tool_throttle_cleared
fallback_tool_selected
worktree_created
worktree_reused
worktree_cleaned
skill_started
skill_step_started
skill_step_finished
skill_finished
diagnostic_detected
user_action
```

### 6.3 Runtime snapshot

Snapshot fields:

```rust
pub struct RuntimeSnapshot {
    pub generated_at: DateTime<Utc>,
    pub project: ProjectSummary,
    pub coordinator: CoordinatorStatus,
    pub queue: QueueSummary,
    pub workers: Vec<WorkerRuntime>,
    pub tasks: Vec<TaskRuntimeSummary>,
    pub active_runs: Vec<SkillRunSummary>,
    pub throttled_tools: Vec<ToolThrottleStatus>,
    pub recent_events: Vec<CoordinatorEvent>,
    pub git: GitRuntimeSummary,
    pub diagnostics: RuntimeDiagnostics,
}
```

### 6.4 Worker runtime

```rust
pub struct WorkerRuntime {
    pub id: String,
    pub worktree_path: PathBuf,
    pub tool: String,
    pub task_id: Option<String>,
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub phase: Option<String>,
    pub runtime_status: RuntimeStatus,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub git_status: GitStatusSummary,
    pub retry_count: u32,
    pub delayed_until: Option<DateTime<Utc>>,
}
```

### 6.5 Queue summary

```rust
pub struct QueueSummary {
    pub todo: usize,
    pub ready: usize,
    pub claimed: usize,
    pub in_progress: usize,
    pub review: usize,
    pub changes_requested: usize,
    pub blocked: usize,
    pub merged: usize,
    pub failed: usize,
}
```

### 6.6 Tool throttle status

```rust
pub struct ToolThrottleStatus {
    pub tool: String,
    pub reason: String,
    pub error_code: String,
    pub retryable: bool,
    pub delayed_until: Option<DateTime<Utc>>,
    pub effective_parallelism_delta: i32,
}
```

### 6.7 Snapshot consumers

```text
macc status --json
macc status --watch
macc web /api/v1/snapshot
macc doctor
macc logs
macc run --watch
```

---

## 7. CLI command matrix

### 7.1 Observability

```bash
macc status
macc status --json
macc status --watch
macc status --watch --control
macc watch
```

### 7.2 Skills

```bash
macc run <skill>
macc run <skill> --tool <tool>
macc run <skill> --dry-run
macc run <skill> --watch
macc skills list
macc skills show <skill>
macc skills doctor
```

### 7.3 Web

```bash
macc web
macc web --port 3450
macc web --host 127.0.0.1
macc web --assets embedded
macc web --assets dist
```

### 7.4 Logs

```bash
macc logs
macc logs tail
macc logs tail --task AUTH-012
macc logs tail --tool codex
macc logs failures
macc logs summarize
```

### 7.5 Coordinator

Existing coordinator commands should remain compatible:

```bash
macc coordinator
macc coordinator run
macc coordinator status
macc coordinator dispatch
macc coordinator advance
macc coordinator reconcile
macc coordinator cleanup
macc coordinator resume
macc coordinator stop --graceful
```

---

## 8. Web API contract additions

### 8.1 Snapshot

```http
GET /api/v1/snapshot
```

Returns:

```json
{
  "generated_at": "...",
  "project": {},
  "coordinator": {},
  "queue": {},
  "workers": [],
  "tasks": [],
  "active_runs": [],
  "throttled_tools": [],
  "recent_events": [],
  "git": {},
  "diagnostics": {}
}
```

### 8.2 Search

```http
GET /api/v1/search?q=AUTH-012
```

Searches:

- tasks;
- tools;
- skills;
- logs;
- commits;
- worktrees;
- settings;
- MCP servers;
- error codes.

### 8.3 Skills

```http
GET  /api/v1/skills
GET  /api/v1/skills/{id}
POST /api/v1/skills/{id}/dry-run
POST /api/v1/skills/{id}/run
```

### 8.4 Runs

```http
GET /api/v1/runs
GET /api/v1/runs/{id}
GET /api/v1/runs/{id}/logs
```

### 8.5 Failures

```http
GET /api/v1/failures/recent
```

Returns last significant failures with:

- task ID;
- tool;
- worktree;
- error code;
- retryable;
- recommended action;
- log excerpt.

### 8.6 Worker context

```http
GET /api/v1/workers/{id}/snapshot
```

Returns:

- worker metadata;
- task metadata;
- recent events;
- changed files;
- git status;
- logs;
- available actions.

---

## 9. Implementation roadmap

### 9.1 Phase 1 — Shared snapshot foundation

Deliverables:

- define `RuntimeSnapshot`;
- define event schema version 1;
- emit skill run events;
- emit coordinator events consistently;
- implement snapshot builder;
- add `macc status --json`.

Acceptance:

- Web/TUI/CLI can consume identical snapshot data.
- Snapshot works with both idle and active coordinator state.
- Snapshot includes throttled tools and stale workers.

### 9.2 Phase 2 — TUI Observer MVP

Deliverables:

- `macc status --watch`;
- worktree grid;
- task checklist;
- event timeline;
- log tail pane;
- filters;
- read-only mode.

Acceptance:

- usable during a running coordinator session;
- handles missing logs gracefully;
- handles no-worktree state gracefully;
- does not mutate project state.

### 9.3 Phase 3 — Unified Skill Runner MVP

Deliverables:

- `macc run <skill>`;
- local command skills;
- prompt skill fallback;
- dry-run preview;
- run logs;
- risk classification;
- `macc skills list/show/doctor`.

Acceptance:

- `macc run validate` works;
- `macc run security-check --dry-run` shows prompt/context/risk;
- run events appear in snapshot.

### 9.4 Phase 4 — Web Mission Control MVP

Deliverables:

- `/api/v1/snapshot`;
- Mission Control page;
- worker grid;
- task drawer;
- log viewer;
- global status strip;
- right git/context panel;
- command palette shell.

Acceptance:

- Web UI answers “what is MACC doing right now?”
- selected worker can show logs and task details;
- status updates via SSE or polling fallback.

### 9.5 Phase 5 — Token/context hooks

Deliverables:

- summarization hook registry;
- default bundles;
- per-skill configuration;
- per-tool configuration;
- summary metadata;
- raw output access.

Acceptance:

- validation failures are summarized before model context;
- raw logs remain accessible;
- summary size and source size are visible.

### 9.6 Phase 6 — Web Client completion

Deliverables:

- Skill Runner page;
- PRD visual editor;
- PRD audit preview;
- diagnostics repair center;
- Plan/Apply diff viewer;
- advanced logs explorer;
- terminal drawer;
- command palette actions.

Acceptance:

- user can run, watch, inspect, and recover from most MACC workflows from Web UI.

---

## 10. Suggested repository structure changes

```text
macc/
  core/
    src/
      runtime/
        mod.rs
        snapshot.rs
        events.rs
        watch.rs
      skills/
        mod.rs
        resolver.rs
        runner.rs
        dry_run.rs
        risk.rs
      context/
        mod.rs
        summarizers.rs
        token_budget.rs
      logs/
        mod.rs
        failure_detector.rs
  cli/
    src/
      commands/
        status.rs
        run.rs
        skills.rs
        logs.rs
        web/
          snapshot.rs
          skills.rs
          runs.rs
          search.rs
          failures.rs
  tui/
    src/
      observer/
        mod.rs
        layout.rs
        worktree_grid.rs
        task_tree.rs
        log_pane.rs
        event_timeline.rs
  web/
    src/
      pages/
        MissionControl.tsx
        SkillRunner.tsx
        LogsExplorer.tsx
        PrdEditor.tsx
        Diagnostics.tsx
      components/
        runtime/
        workers/
        tasks/
        logs/
        skills/
        command-palette/
      stores/
        runtimeStore.ts
        skillRunStore.ts
```

---

## 11. Risk and consent model

### 11.1 Risk levels

```text
safe
caution
dangerous
```

### 11.2 Safe

Examples:

- view status;
- read logs;
- run validation;
- dry-run skill;
- inspect diffs;
- search tasks.

No confirmation required.

### 11.3 Caution

Examples:

- create worktree;
- run implement skill;
- edit PRD;
- apply project-level config;
- format files.

Single confirmation required.

### 11.4 Dangerous

Examples:

- delete worktree;
- remove branches;
- restore backup;
- emergency stop;
- user-level config merge;
- force push;
- clear project MACC artifacts.

Double confirmation or typed phrase required.

---

## 12. Observability UX patterns

### 12.1 Failure-first diagnostics

Every failure should produce:

```text
What failed
Where it failed
Why MACC thinks it failed
Whether it is retryable
What MACC will do automatically
What the user can do manually
Links to raw evidence
```

### 12.2 Error card example

```text
E601 — Codex rate-limited

Task: AUTH-012
Worktree: wt-01
Phase: dev
Retryable: yes
Backoff: 120s
Fallback: gemini available
Next action: automatic retry scheduled

[Open logs] [Switch tool] [Retry now] [Block task]
```

### 12.3 Merge blocker card

```text
Merge conflict

Task: API-021
Branch: ai/claude/api-021
Files:
  src/api/routes.ts
  src/db/schema.ts

AI merge-fix: attempted once, failed
Coordinator: paused

[Open diff] [Open terminal] [Retry AI merge-fix] [Mark blocked] [Resume]
```

### 12.4 Stale worker card

```text
Worker stale

Worktree: wt-03
Tool: claude
Last heartbeat: 11m ago
Phase: fix
Policy: stale_action=requeue

[Open logs] [Requeue task] [Kill performer] [Ignore]
```

---

## 13. UX quality checklist

Use this checklist before merging U1/U2/U3 implementations.

### 13.1 CLI

- Clear command names.
- `--dry-run` available for risky operations.
- `--json` available for automation.
- Errors include recommended next action.
- Commands respect worktree context.
- Output is readable in small terminals.

### 13.2 TUI

- Read-only by default.
- Works without mouse.
- No flickering or unstable layout.
- Handles long logs.
- Handles many worktrees.
- Handles idle/no-data states.
- Clear paused/throttled/stale banners.
- Search and filtering available.

### 13.3 Web

- Keyboard navigation works.
- Focus states are visible.
- Streaming updates do not cause layout shifts.
- Logs are virtualized.
- Large PRDs remain performant.
- Color is not the only state indicator.
- Dangerous actions are clearly separated.
- Connection loss is obvious and recoverable.

### 13.4 Accessibility

- Reduced motion support.
- High contrast mode.
- ARIA labels for interactive controls.
- Screen-reader friendly status changes.
- Resizable panes.
- Log font-size control.

---

## 14. Final integrated product narrative

After these changes, MACC should feel like this:

```bash
macc init
macc apply
macc run validate
macc coordinator
macc watch
```

Or visually:

```bash
macc web
```

The developer configures tools once, runs canonical skills without remembering tool-specific syntax, launches autonomous work across worktrees, and monitors everything through a shared runtime cockpit.

The CLI remains fast and scriptable.

The TUI becomes the best terminal-native observer.

The Web Client becomes the rich mission-control interface.

All three surfaces share the same runtime snapshot, events, logs, risk model, and consent gates.

---

## 15. Consolidated acceptance criteria

### 15.1 U1 — TUI Observer

- `macc status --watch` opens a live Ratatui dashboard.
- Dashboard shows coordinator state, worktrees, workers, tasks, throttles, blockers, and recent events.
- Dashboard can tail selected logs.
- Dashboard detects stale workers and rate-limited tools.
- Dashboard is read-only unless `--control` is passed.
- Dangerous actions require confirmation.
- Dashboard consumes shared runtime snapshot.

### 15.2 U2 — Unified Skill Runner

- `macc run <skill>` executes a canonical MACC skill.
- `macc run <skill> --tool <tool>` routes through selected adapter.
- `macc run <skill> --dry-run` shows commands, prompts, files, risk, and context estimate.
- Skills support local-only, prompt-only, command workflow, hybrid, agent, and coordinator modes.
- Runs are logged under `.macc/log/run/`.
- Skill runs emit structured events.
- Worktree context is respected.
- Risk classification and confirmations are enforced.

### 15.3 U3 — Web Client

- Web UI answers “what is MACC doing right now?”
- Mission Control shows workers, tasks, coordinator state, throttles, blockers, and recent events.
- Users can inspect workers, logs, diffs, and terminals.
- Plan/Apply pages show diffs, risk, and backups before writing.
- PRD editor supports table, drawer, graph, and diff views.
- Skill Runner supports dry-run and run.
- Diagnostics page separates auto-fixable, confirmation-required, and manual issues.
- UI supports keyboard navigation and command palette.
- Web Client consumes shared runtime snapshot.

### 15.4 U4 — Token/context UX

- MACC summarizes noisy outputs before model context.
- Default hooks exist for tests, lint, stack traces, diffs, and logs.
- Token budgets are configurable.
- Raw output remains accessible.
- Summary metadata is visible in TUI and Web UI.
- Skills can choose summarization bundles.

---

## 16. Recommended priority order

1. Shared runtime snapshot and event schema.
2. `macc status --json`.
3. `macc status --watch` read-only TUI.
4. `macc run validate` and `macc run --dry-run`.
5. Skill run events and logs.
6. Web `/api/v1/snapshot`.
7. Mission Control Web page.
8. Logs failure-first view.
9. Token/context summarization hooks.
10. Web Skill Runner and PRD audit preview.

This order minimizes duplicated work and creates useful value early.

---

## 17. Final motif set

### U1. Real-Time TUI Observer Mode

A read-first Ratatui dashboard exposed through `macc status --watch`, giving developers live visibility into worktrees, tasks, logs, events, throttles, stale workers, and blocked states.

### U2. Unified Skill Runner

A canonical skill execution facade exposed through `macc run <skill-name>`, decoupling developer workflows from tool-specific skill formats and execution syntax.

### U3. Mission-Control Web Client

A local browser-based control surface for configuration, planning, skill execution, worktree operations, logs, diagnostics, git graph, PRD editing, and autonomous run recovery.

### U4. Context and Token Budget UX

A default output summarization and token-budget layer that turns noisy tool output into high-signal model context while preserving access to raw evidence.

Together, these motifs make MACC more than a config generator. They make it an operable, observable, trustworthy control plane for multi-agent software development.

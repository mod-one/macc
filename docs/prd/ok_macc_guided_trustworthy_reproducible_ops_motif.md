# MACC Motif Proposal: Guided, Trustworthy, Reproducible Operations

## 1. Executive Summary

This motif turns MACC from a powerful configuration/orchestration system into a product that feels safe, guided, and repeatable for both solo developers and teams.

The core direction is:

- Shorten the first successful path with `macc start`.
- Hide complex coordinator/runtime controls behind progressive disclosure.
- Make trust, scope, locality, and backups visible before every meaningful write or execution.
- Treat reproducibility as a first-class artifact through `macc.lock.yaml`.
- Convert failures into guided recovery workflows instead of raw log spelunking.
- Reduce Rust architectural hot spots before they slow feature work.
- Generate CLI, TUI, Web, TypeScript, docs, and examples from shared contracts.
- Keep the Web Client fast by enforcing bundle budgets and lazy-loading operationally heavy surfaces.

The motif can be summarized as:

> MACC should feel like a cockpit: one obvious route to start, visible safety state, reproducible controls, and guided emergency procedures.

---

## 2. Design Principles

### 2.1 One obvious path, many expert exits

New users should not need to understand `init`, `plan`, `apply`, worktrees, PRDs, catalogs, adapters, and coordinator settings before reaching value. Expert commands remain available, but the default flow should optimize for a successful first run.

### 2.2 Every write should be previewable, reversible, and attributable

MACC already has strong safety mechanics such as previews, atomic writes, backups, consent gates, user-level write confirmation, localhost default binding, and audit logs. The improvement is to surface these mechanics as product UX, not only implementation behavior.

### 2.3 Reproducibility is a product feature, not a by-product

If two developers run MACC on the same repository, they should be able to answer:

- Which tools were resolved?
- Which catalog versions were used?
- Which remote artifacts were fetched?
- Which checksums were verified?
- Which generated files were produced?
- Which runtime-relevant versions affected execution?

### 2.4 Failures should become decisions

A failure screen should not begin with raw stderr. It should begin with: what happened, whether the task is safe, where the last safe state is, and what guarded actions are available.

---

## 3. Recommendation 3 — Create a Shorter Happy Path with `macc start`

### 3.1 Problem

MACC currently has several strong entry points: `macc init`, `macc init --wizard`, `macc quickstart`, `macc apply`, TUI launch, and Web launch. This is powerful, but it creates choice overhead for first-time users and teams onboarding a repository.

### 3.2 Proposed command

```bash
macc start
```

`macc start` should be the default guided entry point. It should orchestrate the existing lifecycle rather than replace existing commands.

### 3.3 Behavior

`macc start` should:

1. Detect repository state.
2. Detect installed AI tools and known config files.
3. Detect whether `.macc/macc.yaml`, PRD files, catalogs, and backups already exist.
4. Present a compact intent selector.
5. Generate a write preview.
6. Let the user accept an existing PRD, create a minimal PRD, import one, or skip PRD creation for config-only setup.
7. Run plan/apply with consent gates.
8. Launch the relevant surface:
   - TUI for terminal users.
   - Web dashboard when `--web` is passed or when terminal UX is limited.
   - Coordinator dashboard when the intent includes task execution.

### 3.4 Intent presets

```text
What do you want to do?

[1] Configure tools
    Detect assistants, generate config, install selected skills/MCP templates.

[2] Run one task
    Create or select a PRD task, prepare one worktree, run with supervision.

[3] Run a batch
    Prepare coordinator settings, validate PRD, run multiple tasks with dashboard.

[4] Inspect existing MACC project
    Open status, backups, diagnostics, config, and logs without writing.
```

### 3.5 CLI shape

```bash
macc start
macc start --intent configure-tools
macc start --intent run-one-task
macc start --intent run-batch
macc start --dry-run
macc start --web
macc start --tui
macc start --profile team-defaults
macc start --preset conservative
```

### 3.6 Internal pipeline

`macc start` should compose existing MACC subsystems:

```text
Detect -> Diagnose -> Resolve -> Preview -> Confirm -> Apply -> Launch
```

Potential Rust module:

```text
cli/src/commands/start.rs
core/src/startup/
  detect.rs
  intent.rs
  preview.rs
  launch.rs
```

### 3.7 Acceptance criteria

- A new user can configure a project with no prior MACC knowledge using `macc start`.
- The command never writes without preview and confirmation.
- Existing commands remain stable and scriptable.
- `macc start --dry-run` outputs the same planned writes as the guided flow.
- The Web and TUI welcome screens mirror the same intent presets.

---

## 4. Recommendation 4 — Move Expert Knobs Behind Progressive Disclosure

### 4.1 Problem

The coordinator now exposes many runtime controls: storage modes, stale policies, cutover gates, retry lists, rate-limit controls, merge hooks, heartbeat grace windows, dispatch cooldowns, JSON compatibility, fallback behavior, and more. These controls are valuable, but they should not dominate first-run UX.

### 4.2 Proposed information architecture

Use a three-level settings model across CLI, TUI, and Web.

#### Basic

Visible by default.

- Preferred tools
- Concurrency
- Timeout
- Safety policy
- Offline mode
- Quiet mode
- Web port
- Preset selector

#### Advanced

Collapsed by default, with clear descriptions.

- Stale handling
- Retry policy
- Rate-limit fallback
- Merge AI fix behavior
- Tool specialization
- Worktree reuse policy
- Log verbosity

#### Admin

Hidden behind explicit “Show Admin Settings”.

- Coordinator storage backend
- Cutover controls
- Migration toggles
- Legacy compatibility flags
- Raw JSON compatibility modes
- Flush behavior
- Internal registry repair controls
- Experimental runtime supervisor switches

### 4.3 Presets

```yaml
presets:
  conservative:
    max_parallel: 1
    rate_limit_fallback_enabled: false
    merge_ai_fix: false
    safety_policy: strict
    destructive_actions: double_confirm

  balanced:
    max_parallel: 3
    rate_limit_fallback_enabled: true
    merge_ai_fix: true
    safety_policy: standard
    destructive_actions: double_confirm

  throughput:
    max_parallel: 6
    rate_limit_fallback_enabled: true
    rate_limit_throttle_parallel: true
    merge_ai_fix: true
    safety_policy: standard
    destructive_actions: double_confirm
```

### 4.4 CLI pattern

```bash
macc settings show
macc settings show --advanced
macc settings show --admin
macc settings preset balanced
macc coordinator run --preset throughput
```

### 4.5 Web/TUI pattern

- Default Settings page shows Basic only.
- Advanced and Admin groups have explanatory warnings.
- Each setting shows:
  - current value,
  - source of value: CLI override, project config, profile, default,
  - impact summary,
  - whether restart/resume is required.

### 4.6 Acceptance criteria

- First-run users see fewer than 10 settings.
- All existing coordinator parameters remain available.
- Every setting has a stable schema, description, default, examples, and UI grouping.
- The same grouping is used in CLI help, TUI, Web, and generated docs.

---

## 5. Recommendation 5 — Make Trust Visible

### 5.1 Problem

MACC already includes safety features: localhost Web binding, no default authentication, consent gates, user-level backups, audit logs, path restrictions, terminal directory restrictions, remote package restrictions, no secret writing, and generated-output scanning. The next step is to make these properties visible at the moment users make decisions.

### 5.2 Add a Trust Strip

Display a compact trust strip in the TUI and Web top/bottom bar:

```text
Local only: yes | Terminal: disabled | User writes: 0 | Backups: ready | Catalog: pinned | Secrets: redacted
```

### 5.3 Add a Trust Center screen

Route:

```text
/web: /ops/trust
/tui: Trust & Safety
/cli: macc trust
```

The screen should show:

| Area | What to show |
|---|---|
| Server exposure | Bound host, port, whether exposed beyond localhost |
| Terminal access | Enabled/disabled, active PTY sessions, allowed roots |
| User-scope writes | Files that may change outside the project |
| Project writes | Generated paths, changed paths, deleted paths |
| Backups | Backup location, latest backup, restore command |
| Catalog integrity | Source URL, pinned rev/checksum, verification status |
| Remote packages | Data-only manifest, target files, no-script validation |
| Secrets | Redaction/scanning status and warnings |
| Audit | Last mutating operations and log file location |

### 5.4 Trust states

Use simple states:

```text
Trusted      Everything local, pinned, backed up, and scanned.
Caution      Some inputs are unpinned or terminal/user-scope writes are enabled.
Risky        Exposed host, missing checksums, destructive action pending, or secret warning.
Blocked      Policy violation or unsafe write path detected.
```

### 5.5 Pre-write trust card

Before `apply`, backup restore, worktree removal, catalog install, terminal enablement, or user-level write, show:

```text
Trust Review

Scope: project-level write
Files to change: 8
User-level files: 0
Backups: .macc/backups/2026-05-27T...
Remote inputs: 2 pinned, 0 unpinned
Secrets detected: none
Rollback: macc restore --backup <id>

Proceed? [y/N]
```

### 5.6 Acceptance criteria

- Users can always see whether MACC is local-only.
- Any user-level write is visible before confirmation.
- Any unpinned remote catalog or package is visible before apply.
- Terminal capability is never ambiguous.
- Every backup has a discoverable restore path.

---

## 6. Recommendation 6 — Add Reproducibility with `macc.lock.yaml`

### 6.1 Problem

MACC supports canonical config, remote artifact fetching, adapters, generated outputs, profiles, worktrees, and coordinator execution. Without a lock manifest, teams may not know exactly which resolved inputs produced a given environment.

### 6.2 Proposed lock file

```text
.macc/macc.lock.yaml
```

This file should be committed for team reproducibility unless it contains machine-local paths. Machine-local fields should be stored separately under `.macc/state/`.

### 6.3 Example lock manifest

```yaml
lock_version: 1
created_at: "2026-05-27T00:00:00Z"
macc:
  version: "0.5.0"
  binary_sha256: "sha256:..."
  schema_version: "2026-05-27"

project:
  root_fingerprint: "git:<repo-url>#<commit>"
  reference_branch: "main"

config:
  source: ".macc/macc.yaml"
  sha256: "sha256:..."
  active_profile: "team-defaults"
  preset: "balanced"

tools:
  - id: "codex"
    detected_version: "..."
    adapter_version: "..."
    model: "..."
    generated_files:
      - path: "AGENTS.md"
        sha256: "sha256:..."
  - id: "claude"
    detected_version: "..."
    adapter_version: "..."
    model: "..."

catalogs:
  - id: "default-skills"
    kind: "git"
    url: "https://example.invalid/macc-skills.git"
    rev: "abc123"
    checksum: null
  - id: "team-mcp"
    kind: "http"
    url: "https://example.invalid/team-mcp.tar.gz"
    checksum: "sha256:..."

packages:
  - id: "security-check"
    type: "skill"
    source_id: "default-skills"
    subpath: "skills/security-check"
    manifest_sha256: "sha256:..."
    installed_targets:
      - tool: "codex"
        path: ".codex/skills/security-check/SKILL.md"
        sha256: "sha256:..."

runtime:
  os: "linux"
  git_version: "..."
  node_version: "..."
  pnpm_version: "..."
  rust_version: "..."

coordinator:
  storage_mode: "sqlite"
  max_parallel: 3
  retry_policy_hash: "sha256:..."
  rate_limit_policy_hash: "sha256:..."
```

### 6.4 Commands

```bash
macc lock generate
macc lock check
macc lock diff
macc lock explain
macc apply --locked
macc start --locked
```

### 6.5 Modes

| Mode | Behavior |
|---|---|
| `macc lock generate` | Resolves and writes/updates the lock file |
| `macc lock check` | Verifies config, catalogs, packages, generated files, and tool versions |
| `macc apply --locked` | Fails if resolution would differ from lock |
| `macc lock diff` | Shows drift between current environment and lock |
| `macc lock explain` | Human-readable explanation of what is pinned and why |

### 6.6 Acceptance criteria

- `macc apply --locked` is deterministic or fails with actionable drift information.
- Remote Git sources must resolve to pinned commits in the lock.
- HTTP sources without checksums are marked `Caution` or blocked under strict safety policy.
- Generated file hashes can be verified.
- Machine-local volatile state does not pollute the committed lock file.

---

## 7. Recommendation 7 — Turn Failures into Guided Recovery

### 7.1 Problem

MACC has structured error codes, canonical error classes, retry policies, task state, runtime status, worktrees, logs, backups, and recovery commands. The UX should combine these into a guided failed-task view.

### 7.2 Failed-task view

Route/screen:

```text
TUI: Coordinator > Failed Tasks > Task Detail
Web: /ops/failures/:taskId
CLI: macc failure show <task-id>
```

### 7.3 Failure card

```text
Task: AUTH-014 — Add OAuth callback validation
Status: failed
Cause: E601 RateLimit / Provider overloaded
Retryable: yes
Tool: claude
Worktree: .macc/worktree/auth-014
Last safe state: committed dev changes, not merged
Affected files: 6
Recommended action: retry with backoff or switch tool
```

### 7.4 Guarded actions

| Action | Purpose | Guard |
|---|---|---|
| Retry | Re-run same phase using same tool/session if safe | Single confirm |
| Retry with different tool | Use fallback tool based on capability match | Single confirm |
| Salvage | Preserve branch, commits, artifacts, logs, and mark task for manual review | Single confirm |
| Restore | Restore from backup or last known clean worktree state | Double confirm |
| Inspect diff | Open generated diff and changed files | Read-only |
| Open terminal | Open restricted terminal in affected worktree | Caution confirm if terminal disabled |
| Mark blocked | Preserve state and stop redispatch | Single confirm |
| Abandon | Mark task abandoned and preserve audit trail | Double confirm |

### 7.5 Normalized cause model

Add or formalize a `FailureSummary` contract:

```rust
pub struct FailureSummary {
    pub task_id: String,
    pub normalized_cause: CanonicalClass,
    pub error_code: String,
    pub retryable: bool,
    pub user_action_required: bool,
    pub last_safe_state: LastSafeState,
    pub affected_worktree: Option<PathBuf>,
    pub affected_files: Vec<PathBuf>,
    pub recommended_action: RecommendedAction,
    pub guarded_actions: Vec<GuardedAction>,
    pub evidence_refs: Vec<EvidenceRef>,
}
```

### 7.6 CLI examples

```bash
macc failure list
macc failure show AUTH-014
macc failure retry AUTH-014
macc failure retry AUTH-014 --tool codex
macc failure salvage AUTH-014
macc failure restore AUTH-014 --to-last-safe-state
macc failure inspect-diff AUTH-014
```

### 7.7 Acceptance criteria

- Every coordinator failure can produce a `FailureSummary`.
- The first visible failure message is human-readable, not raw stderr.
- Raw logs remain one click/keypress away.
- Destructive recovery actions require confirmation matching the existing risk model.
- Recovery actions are recorded in the ops audit log.

---

## 8. Recommendation 9 — Reduce Architectural Hot Spots

### 8.1 Problem

Large files such as `state.rs`, `coordinator_workflow.rs`, `fsm.rs`, and `lib.rs` can become coordination bottlenecks. As MACC adds TUI screens, Web routes, runtime supervision, contracts, and recovery flows, these files will attract unrelated changes and increase merge conflicts.

### 8.2 Refactoring principle

Split by domain responsibility and user-facing use case, not only by technical layer.

### 8.3 Proposed module split

#### Coordinator state

```text
core/src/coordinator/state/
  mod.rs
  task.rs
  runtime.rs
  registry.rs
  storage.rs
  snapshots.rs
  transitions.rs
```

#### Workflow orchestration

```text
core/src/coordinator/workflow/
  mod.rs
  scheduler.rs
  dispatcher.rs
  phase_runner.rs
  event_monitor.rs
  runtime_monitor.rs
  reconciler.rs
  cleanup.rs
  pause_gate.rs
```

#### FSM / transitions

```text
core/src/coordinator/fsm/
  mod.rs
  states.rs
  events.rs
  transition_table.rs
  guards.rs
  effects.rs
  tests.rs
```

#### Web composition

```text
cli/src/commands/web/
  routes/
    config.rs
    coordinator.rs
    failures.rs
    trust.rs
    lock.rs
  contracts/
    dto.rs
    errors.rs
  middleware/
    audit.rs
    path_guard.rs
```

#### TUI screens

```text
tui/src/screens/
  start.rs
  settings/
    basic.rs
    advanced.rs
    admin.rs
  trust.rs
  lock.rs
  failures.rs
  coordinator.rs
```

### 8.4 Boundary rules

- Transition rules live in core only.
- CLI/TUI/Web may request transitions, but must not implement transition logic.
- DTOs are generated from shared schemas or contract fixtures.
- UI modules consume view models, not storage structs directly.
- Runtime supervision is separate from task workflow state.
- Recovery logic uses canonical failure summaries, not ad-hoc log parsing.

### 8.5 Acceptance criteria

- No coordinator source file exceeds an agreed threshold, for example 500–700 lines, except generated code.
- Transition tests live next to transition definitions.
- UI routes/screens do not depend on storage internals.
- Adding a Web page should not require editing coordinator internals unless a new domain capability is needed.

---

## 9. Recommendation 10 — Unify Contracts Across CLI, TUI, and Web

### 9.1 Problem

MACC has multiple user surfaces: CLI, TUI, Web API, SPA, generated config files, docs, and example YAML. If each surface hand-rolls models and help text, inconsistencies will accumulate.

### 9.2 Contract-first approach

Define shared Rust schemas for:

- `CanonicalConfig`
- `ResolvedConfig`
- `ActionPlan`
- `TrustSummary`
- `LockManifest`
- `FailureSummary`
- `CoordinatorStatus`
- `TaskRuntime`
- `ToolSpec`
- `CatalogSource`
- `PackageManifest`
- `ApiError`
- `SettingDescriptor`

Generate from these contracts:

| Output | Source |
|---|---|
| TypeScript API models | Rust schema / OpenAPI |
| CLI help tables | `SettingDescriptor` and command metadata |
| TUI forms | schema + setting groups |
| Web forms | schema + setting groups |
| Config reference docs | schema descriptions |
| Example YAML | schema fixtures |
| Validation errors | same validator used by CLI/TUI/Web |

### 9.3 Tooling options

Use one of these patterns:

1. Rust structs + `serde` + `schemars` → JSON Schema → TypeScript.
2. Rust OpenAPI generation with `utoipa` for Web API DTOs.
3. Contract fixtures checked into `contracts/fixtures/*.json` and used by Rust + frontend tests.

### 9.4 Repository layout

```text
contracts/
  schemas/
    canonical-config.schema.json
    action-plan.schema.json
    trust-summary.schema.json
    lock-manifest.schema.json
    failure-summary.schema.json
  fixtures/
    config.minimal.yaml
    config.balanced.yaml
    failure.rate-limit.json
    failure.merge-conflict.json
    trust.local-pinned.json
  generated/
    typescript/
    docs/
```

### 9.5 Commands

```bash
macc dev contracts generate
macc dev contracts check
macc dev contracts fixtures
```

### 9.6 CI gate

CI should fail if:

- Rust schema changed but generated TypeScript was not updated.
- Example YAML no longer validates.
- CLI help references a removed setting.
- Web form contains a setting not present in the schema.
- Error codes are used but not documented.

### 9.7 Acceptance criteria

- One source of truth for configuration field names and descriptions.
- TypeScript models are generated, not manually duplicated.
- CLI help, docs, TUI labels, and Web labels agree.
- Example YAML is always validated in CI.

---

## 10. Recommendation 11 — Tighten Frontend Performance

### 10.1 Problem

The Web Client has operationally heavy surfaces: terminals, logs, help content, syntax highlighting, git graph, live wall, PRD graph, and large tables. The production build may succeed while still shipping too much JavaScript or unnecessary syntax-highlighting languages in the initial bundle.

### 10.2 Performance strategy

Keep the initial shell small, then lazy-load heavy surfaces.

### 10.3 Route-level lazy loading

Lazy-load these routes:

- `/ops/live`
- `/ops/worker/:id`
- `/ops/logs`
- `/ops/git`
- `/ops/diagnostics`
- `/ops/backups`
- `/help`
- `/prd` graph/diff panels
- terminal drawer

### 10.4 Syntax highlighting policy

Avoid importing all languages from highlighters.

Allowed default languages:

- Markdown
- JSON
- YAML
- TOML
- Bash/Shell
- Rust
- TypeScript
- TSX
- Diff

All other languages should be loaded on demand.

### 10.5 Bundle budgets

Set budgets in CI:

```text
Initial JS gzip:        <= 250 KB target, <= 350 KB hard limit
Initial CSS gzip:       <= 80 KB target, <= 120 KB hard limit
Single route chunk:     <= 300 KB hard limit
Total app gzip:         tracked, warning on +10% regression
Assets over 200 KB:     require justification
```

### 10.6 Vite tools

Add:

```bash
npm run analyze
npm run size-check
```

Potential package choices:

- `rollup-plugin-visualizer`
- `vite-bundle-visualizer`
- `size-limit`
- `source-map-explorer`

### 10.7 Frontend architecture changes

```text
web/src/routes/lazy.tsx
web/src/components/terminal/LazyTerminal.tsx
web/src/components/code/CodeBlock.tsx
web/src/components/help/LazyHelpViewer.tsx
web/src/components/logs/VirtualLogViewer.tsx
web/src/components/prd/LazyPrdGraph.tsx
```

### 10.8 Operational performance

- Use virtualized lists for PRDs, logs, event timelines, and task tables.
- Keep SSE tile buffers bounded.
- Pause offscreen live streams or reduce update frequency when not visible.
- Defer git graph refreshes when the panel is collapsed.
- Persist user-selected panels but do not prefetch all heavy panels on first load.
- Avoid rendering full terminal instances until opened.

### 10.9 Acceptance criteria

- Initial load remains under the existing target.
- Terminals and help content do not appear in the initial bundle.
- Syntax highlighter only includes approved default languages initially.
- CI fails or warns on bundle regression.
- Live Wall can support 20+ tiles without unbounded memory growth.

---

## 11. Integrated UX Flow

### 11.1 First run

```text
macc start
  -> detect tools and project state
  -> choose intent
  -> choose preset
  -> trust review
  -> preview writes
  -> create/update PRD if needed
  -> apply
  -> generate lock
  -> open dashboard
```

### 11.2 Daily use

```text
macc start --intent run-one-task
  -> choose task
  -> preview worktree/action plan
  -> run with dashboard
  -> show success or guided failure recovery
```

### 11.3 Team reproducibility

```text
git pull
macc lock check
macc apply --locked
macc doctor
```

### 11.4 Failure recovery

```text
macc failure list
macc failure show <task-id>
macc failure retry <task-id> --tool <fallback-tool>
```

---

## 12. Implementation Roadmap

### Phase 1 — UX shell and contracts

- Add `macc start` command.
- Add `SettingDescriptor` schema and Basic/Advanced/Admin grouping.
- Add TrustSummary contract.
- Add FailureSummary contract.
- Add initial Web/TUI trust strip.
- Start contract generation for TypeScript models.

### Phase 2 — Reproducibility and recovery

- Add `macc.lock.yaml` generation.
- Add `macc lock check/diff/explain`.
- Add `macc apply --locked`.
- Add failed-task view in CLI and Web.
- Add guarded recovery actions.

### Phase 3 — Architecture refactor

- Split coordinator state/workflow/FSM modules.
- Move runtime supervision into dedicated modules.
- Add transition table tests.
- Ensure UI layers consume view models/contracts only.

### Phase 4 — Frontend performance hardening

- Add route-level lazy loading.
- Restrict syntax highlighter languages.
- Add bundle analysis and CI size gates.
- Virtualize heavy tables/logs.
- Lazy-load terminal/help/git graph panels.

---

## 13. Updated Acceptance Criteria

### Happy path

- `macc start` can configure tools, run one task, run a batch, or inspect an existing project.
- The same intent presets appear in CLI, TUI, and Web.
- No writes happen without preview and consent.

### Progressive disclosure

- Basic settings remain understandable to a non-expert.
- Advanced/Admin controls remain accessible and documented.
- Presets map to explicit config values.

### Trust

- Local-only/exposed status is always visible.
- Terminal access status is always visible.
- User-scope writes are shown before confirmation.
- Backups and restore commands are shown after writes.
- Catalog and package pinning/checksum status is visible.

### Reproducibility

- `macc.lock.yaml` captures resolved environment inputs.
- `macc apply --locked` fails on drift.
- `macc lock diff` explains mismatches.

### Recovery

- Failed tasks show normalized cause, last safe state, worktree, affected files, recommended action, and guarded actions.
- Raw logs are available but not the primary failure UX.

### Architecture

- Coordinator hot spots are split by domain responsibility.
- Transition logic is centralized and tested.
- UI layers do not implement coordinator state transitions.

### Contracts

- TypeScript API models, CLI help, config reference, and example YAML are generated from shared schemas or contract fixtures.
- CI detects schema drift.

### Frontend performance

- Heavy operational panels are lazy-loaded.
- Syntax highlighting imports are restricted.
- Bundle budgets are enforced.
- Large logs, task tables, and PRD views are virtualized.

---

## 14. Final Recommendation

Implement this motif as a cross-cutting UX and architecture initiative rather than as isolated feature tickets.

Suggested epic name:

```text
Guided Operations, Trust, and Reproducibility
```

Suggested sub-epics:

1. `macc start` guided happy path.
2. Settings progressive disclosure and presets.
3. Trust Center and Trust Strip.
4. Lock manifest and locked apply.
5. Guided failure recovery.
6. Coordinator module decomposition.
7. Shared schema/contract generation.
8. Web performance budget and lazy-loading.

This will make MACC easier to adopt, safer to operate, more reproducible for teams, and easier to evolve internally.

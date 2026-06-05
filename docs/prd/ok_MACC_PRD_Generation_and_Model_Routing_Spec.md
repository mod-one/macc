# MACC Specification Addendum: Client-Accessible PRD Generation and Token-Aware Model Routing

**Project:** MACC — Multi-Assistant Code Config  
**Document type:** Design specification / implementation addendum  
**Language:** English  
**Date:** 2026-05-26  
**Status:** Proposed design — aligned with existing code  

---

## 1. Purpose

This document consolidates the proposed improvements discussed for MACC around two closely related capabilities:

1. **Client-toggleable automatic model selection**: MACC should be able to choose the lightest sufficient model and reasoning depth automatically, while allowing clients to disable this behavior and manually select a model.
2. **Simple, direct PRD generation**: MACC should expose a direct `macc prd generate --from brief.md` workflow that builds a complete prompt, invokes a selected tool, uses the fixed internal `macc-prd-planner` skill, writes outputs to a safe target directory, validates them, and optionally promotes the generated PRD.
3. **PRD audit command migration**: The existing `macc coordinator audit-prd` command is removed and replaced by `macc prd audit`. The audit uses the same option set as `macc prd generate` (tool, model routing, instructions, dry-run, json) but operates on an existing PRD file and uses a dedicated audit prompt to enrich it from commit history and delivered code. The underlying business logic in `core/src/coordinator/prd_auditor.rs` is preserved unchanged.

The design intentionally avoids turning MACC into a complex internal planning engine. MACC should orchestrate prompt construction, tool selection, target paths, validation, promotion, and client access. The actual planning is delegated to a selected AI tool through the built-in PRD planner flow.

---

## 2. Source alignment

This proposal follows the existing MACC direction:

- MACC is built around a canonical source of truth, tool-specific adapters, TUI/Web/CLI clients, worktrees, coordinator execution, PRD reconciliation, and local observability.
- MACC already defines agents, phases, worktree execution, `.macc/tool.json`, `.macc/worktree.json`, `worktree.prd.json`, `sync-prd`, and `audit-prd` concepts. The `audit-prd` concept is migrated to `macc prd audit` (see §22).
- The `macc-prd-planner` skill is already scoped specifically to generating or updating `prd.json` files, preserving task identity, improving parallel safety, using `exclusive_resources`, adding routing hints, and avoiding concrete provider model names in PRD tasks.
- `core/src/coordinator/prd_auditor.rs` already implements the core audit business logic: `gather_task_commit_context`, `build_audit_context`, `build_audit_prompt`, `prepare_audit`. This module is not replaced — it is wrapped by `core/src/prd_generation/audit.rs` and called from `macc prd audit` instead of from `macc coordinator audit-prd`.

The resulting design therefore keeps the PRD planner as a fixed internal capability while exposing a clean, client-friendly command and API.

---

## 3. Design principles

### 3.1 Keep client UX simple

The user should not need to know about internal performers or skill IDs.

The default command should be:

```bash
macc prd generate --from brief.md
```

The following options should **not** be exposed:

```text
--performer
--skill
```

Those are implementation details.

### 3.2 Keep PRD generation direct

MACC should not build a large internal PRD factory that duplicates AI planning behavior. Instead, MACC should:

1. collect the brief and project context;
2. assemble a complete planner prompt;
3. invoke the selected tool;
4. constrain output to a target directory;
5. validate generated files;
6. optionally promote the generated `prd.json`.

### 3.3 Make automatic model routing explicit and client-controllable

Automatic model selection must be toggleable by all clients.

```text
model_selection.mode = auto | manual
```

In `manual` mode, MACC must not silently route to a different model. In `auto` mode, MACC may select the lightest sufficient model and reasoning depth based on phase, task hints, adapter profiles, budget, availability, and failure history.

### 3.4 Use one shared core operation across CLI, TUI, and Web

All clients must call the same core PRD generation service.

```text
CLI command   -> same core operation
TUI action    -> same core operation
Web API call  -> same core operation
Web UI button -> same core operation
```

This prevents drift between clients and guarantees that prompt generation, validation, output placement, and promotion are consistent everywhere.

### 3.5 Keep PRD metadata provider-neutral

Generated PRD files may include routing hints, but they must not contain concrete provider model names.

Allowed:

```json
{
  "routing_hints_mapping": {
    "execution_mode": "micro | standard | structural",
    "reasoning_depth": "light | standard | deep",
    "context_scope": "local | module | cross-cutting",
    "risk_level": "low | medium | high",
    "validation_profile": "light | standard | heavy"
  },
}
```

Not allowed:

```json
{
  "model": "provider-specific-model-name"
}
```

Concrete model selection belongs to the coordinator, runner, and tool adapters.

---

# Part I — Client-Toggleable Automatic Model Selection

## 4. Feature summary

MACC should support an explicit model selection mode available to every client:

```text
manual | auto
```

When the user or client selects `manual`, MACC uses the selected model and reasoning depth exactly, unless the model is unavailable or incompatible. When the user or client selects `auto`, MACC activates the routing engine and chooses the cheapest sufficient model and reasoning depth for the current operation.

---

## 5. Model selection precedence

MACC should apply a clear precedence order:

```text
1. Per-request client override
2. CLI flag, TUI selection, or Web API request
3. Project config in .macc/macc.yaml
4. Tool adapter default
5. MACC built-in fallback
```

This precedence should apply consistently to:

- `macc prd generate`;
- coordinator runs;
- worktree performer launches;
- review/fix/merge phases;
- PRD audit flows.

---

## 6. Configuration model

Recommended project-level configuration:

```yaml
automation:
  model_routing:
    mode: auto # auto | manual
    client_override_allowed: true

    manual:
      default_tool: null
      default_model: null
      default_reasoning_depth: null

    auto:
      policy: efficiency_first # efficiency_first | balanced | quality_first
      allow_escalation: true
      allow_tool_fallback: true
      allow_model_fallback: true
      prefer_mini_under_budget_pressure: true

      phase_defaults:
        exploration:
          tier: mini
          reasoning_depth: light
        triage:
          tier: mini
          reasoning_depth: light
        summarization:
          tier: mini
          reasoning_depth: light
        prd_generation:
          tier: standard
          reasoning_depth: standard
        implementation:
          tier: standard
          reasoning_depth: standard
        review:
          tier: standard
          reasoning_depth: standard
        light_review:
          tier: mini
          reasoning_depth: light
        architecture:
          tier: heavy
          reasoning_depth: deep
        deep_refactor:
          tier: heavy
          reasoning_depth: deep
        merge_fix:
          tier: standard
          reasoning_depth: standard

      escalation_rules:
        repeated_failure_count: 2
        cross_cutting_scope: true
        high_risk_task: true
        large_context: true
        merge_conflict: true
```

---

## 7. Separation of routing controls

MACC should separate three independent controls:

```text
model_routing.mode
  -> manual or auto

auto.allow_escalation
  -> may MACC move from Mini to Standard or Heavy?

auto.allow_tool_fallback
  -> may MACC switch providers/tools if throttled or unavailable?

auto.allow_model_fallback
  -> may MACC switch models inside the same provider/tool?
```

Example:

```yaml
model_routing:
  mode: auto
  auto:
    allow_escalation: true
    allow_tool_fallback: false
    allow_model_fallback: true
```

Meaning:

> MACC may choose and escalate models automatically inside the selected tool, but may not switch to another provider.

---

## 8. Phase and agent defaults

Recommended default routing table:

| Phase or role | Default tier | Reasoning depth |
|---|---:|---:|
| Exploration | Mini | Light |
| Triage | Mini | Light |
| Log Reader | Mini | Light |
| Summarization | Mini | Light |
| PRD generation, small lot | Standard | Standard |
| PRD generation, structural lot | Heavy | Deep |
| Standard implementation | Standard | Standard |
| Light code review | Mini | Light |
| Normal code review | Standard | Standard |
| Architecture design | Heavy | Deep |
| Deep refactoring | Heavy | Deep |
| Recovery after repeated failures | Heavy | Deep |
| Merge conflict analysis | Standard or Heavy | Standard or Deep |

Agent mapping:

| Agent | Default routing |
|---|---|
| Architect | Heavy / deep |
| Code Reviewer | Standard / standard, Mini / light for shallow review |
| Analyst | Mini / light |
| Triage | Mini / light |
| Log Reader | Mini / light |
| Standard Developer | Standard / standard |
| PRD Planner | Standard / standard by default, Heavy / deep for structural lots |

---

## 9. Adapter-level model profiles

Each tool adapter should expose provider-specific models through a provider-neutral profile.

Example shape:

```json
{
  "id": "provider-model-id",
  "tier": "standard",
  "cost_rank": 2,
  "latency_rank": 2,
  "capabilities": [
    "implementation",
    "review",
    "planning"
  ],
  "max_context_rank": 3,
  "supports_reasoning_level": true,
  "recommended_for": [
    "implementation",
    "review",
    "prd_generation"
  ],
  "avoid_for": [
    "deep_refactor"
  ]
}
```

The router should choose the lowest-cost model that satisfies the current operation.

---

## 10. Routing decision inputs

The routing engine should consider:

```text
tool_id
selected_agent
workflow_phase
task.routing_hints
task.priority
task.category
exclusive_resources
context_size_estimate
changed_files_count
hotspot/shared-contract risk
previous_attempt_count
previous_error_code
provider throttle/quota state
manual override state
client request policy
```

---

## 11. Escalation and de-escalation

### 11.1 Escalate when

```text
execution_mode = structural
context_scope = cross-cutting
risk_level = high
validation_profile = heavy
task touches shared contracts
task touches coordinator/runtime/error handling
merge conflict occurs
same task failed more than N times
mini model produces invalid output
review flags architectural uncertainty
```

### 11.2 De-escalate when

```text
task is local and low-risk
phase is summarization or log reading
context is small
previous phase produced a clear implementation plan
task is docs-only
task is validation-only
```

---

## 12. Runtime logging and observability

Every routed operation should log a routing decision event.

Example:

```json
{
  "task_id": "ROUTING-001",
  "phase": "review",
  "agent": "code-reviewer",
  "tool": "claude",
  "model_selection_mode": "auto",
  "selected_model_tier": "standard",
  "selected_reasoning_depth": "standard",
  "selection_reason": [
    "phase=review",
    "risk_level=medium",
    "context_scope=module"
  ],
  "estimated_input_tokens": 18400,
  "estimated_output_budget": 4000,
  "fallback_used": false,
  "escalated": false
}
```

The TUI and Web UI should display a compact human-readable form:

```text
Routing: Auto -> Standard / standard reasoning
Reason: review phase, medium risk, module scope
```

Ops dashboards should show aggregated statistics:

```text
Token efficiency: 63% Mini, 29% Standard, 8% Heavy
Escalations: 4
Fallbacks due to throttling: 2
Estimated token savings: available when usage data exists
```

---

# Part II — Simple Direct PRD Generation

## 13. Feature summary

`macc prd generate` should generate a complete prompt and launch the selected tool using MACC's fixed internal PRD planner flow.

Default command:

```bash
macc prd generate --from brief.md
```

The command should:

1. read the brief file;
2. collect relevant MACC context;
3. assemble a complete prompt that explicitly instructs the tool to use the built-in PRD planner flow;
4. invoke the selected tool;
5. write generated files to a safe target directory;
6. run minimal validation;
7. optionally promote the generated PRD.

---

## 14. Removed options

The following options must not be exposed:

```text
--performer
--skill
```

Rationale:

- The command should be opinionated.
- The user should not choose the internal performer.
- The user should not choose the PRD skill.
- The fixed internal skill is `macc-prd-planner`.
- The command exposes intent, not implementation details.

---

## 15. User-facing CLI options

Recommended options:

```text
--from <path>                  Required input brief
--tool <tool_id>               Optional selected tool
--model-routing <auto|manual>  Optional model routing mode
--model <model_id>             Optional, only valid with manual routing
--instructions <text>          Optional inline client instructions
--instructions-file <path>     Optional client instruction file
--target-dir <path>            Optional output directory override
--update <path>                Optional existing PRD to update
--dry-run                      Build and display the prompt without invoking the tool
--promote                      Promote generated prd.json after validation
--yes                          Accept non-dangerous confirmations in non-interactive mode
--json                         Emit machine-readable result
```

Minimal:

```bash
macc prd generate --from brief.md
```

With tool and routing:

```bash
macc prd generate \
  --from brief.md \
  --tool claude \
  --model-routing auto
```

With client instructions:

```bash
macc prd generate \
  --from brief.md \
  --instructions-file planning-notes.md
```

Manual model selection:

```bash
macc prd generate \
  --from brief.md \
  --tool codex \
  --model-routing manual \
  --model selected-model-id
```

Update an existing PRD:

```bash
macc prd generate \
  --from brief.md \
  --update prd.json
```

Generate and promote:

```bash
macc prd generate \
  --from brief.md \
  --promote
```

---

## 16. Internal fixed behavior

Internally, MACC resolves:

```text
internal performer role: prd-planner
internal skill:          macc-prd-planner
default target dir:      .macc/generated/prd/macc-prd-planner/<run-id>/
```

The target directory can be overridden, but the default should remain stable and explicit.

Recommended run ID format:

```text
YYYY-MM-DD-HHMMSS
```

Example:

```text
.macc/generated/prd/macc-prd-planner/2026-05-26-143012/
```

---

## 17. Tool selection resolution

Tool selection should resolve in this order:

```text
1. --tool argument
2. .macc/macc.yaml prd_generation.default_tool
3. automation.coordinator.coordinator_tool
4. first enabled planning-capable tool
5. error with actionable message
```

If `--tool` is omitted and no default is configured, MACC may choose the first enabled planning-capable tool only if this is consistent with project settings.

---

## 18. PRD generation configuration

Recommended config addition:

```yaml
prd_generation:
  default_tool: null
  default_target_dir: .macc/generated/prd/macc-prd-planner
  model_selection:
    mode: auto
  outputs:
    prd_json: true
    summary: true
    validation_notes: true
  client_instructions:
    allow_inline: true
    allow_file: true
  promotion:
    require_confirmation_when_overwriting: true
    default_output_path: prd.json
```

The fixed skill should not be configurable in normal user-facing config. It can remain an internal constant.

---

## 19. Prompt assembly

MACC should assemble the full prompt before invoking the tool.

The prompt should include:

```text
1. MACC PRD generation role
2. Fixed planner skill instruction
3. Input brief content
4. Target output directory
5. Required output files
6. Repository/MACC context
7. Existing PRD content when --update is used
8. Client-provided instructions
9. Execution constraints
10. Output contract
```

Recommended prompt template:

```md
You are running MACC's built-in PRD generation flow.

Use the MACC PRD Planner skill:
- `macc-prd-planner`

Goal:
Generate or update a MACC-compatible PRD file(s).

Input brief:
<contents of brief.md>

Target output directory:
<target-dir>

Client instructions:
<contents from --instructions or --instructions-file>
```

---

## 20. Context collection

MACC should collect only the context needed for PRD generation.

MACC should not read the whole repository by default. If richer context is needed, the brief or client instructions should request it explicitly.

---

## 21. Minimal validation

After generation, MACC should run lightweight mandatory validation.

Checks:

```text
new prd file(s) exists in <target-dir> or default target directory
all new prd file(s) parses as JSON
required top-level fields exist according to schema or reference shape
task IDs are unique
dependencies reference existing task IDs
routing_hints values are valid when present
routing_hints do not contain provider-specific model names
output files stayed inside target-dir
```

Validation should not become a complex planner. Its job is to catch unsafe or unusable output before promotion.

---

## 22. PRD audit (`macc prd audit`)

### 22.1 Purpose

`macc prd audit` enriches an existing `prd.json` file based on what was actually delivered: completed task notes are updated from commit history, and todo task descriptions are rewritten when integrated code has changed the intended architecture.

This command replaces `macc coordinator audit-prd`. The underlying business logic module `core/src/coordinator/prd_auditor.rs` is preserved unchanged. The CLI dispatch is moved from the coordinator subcommand group to the `macc prd` subcommand group.

### 22.2 Command

```bash
macc prd audit --prd prd.json
```

### 22.3 Options

The audit command shares the same option set as `macc prd generate`, substituting `--prd` for `--from`:

```text
--prd <path>                   Required: PRD file to audit (default: prd.json)
--tool <tool_id>               Optional: tool to invoke
--model-routing <auto|manual>  Optional: model routing mode
--model <model_id>             Optional: only valid with --model-routing manual
--instructions <text>          Optional: inline client instructions appended to prompt
--instructions-file <path>     Optional: client instruction file
--reference-branch <branch>    Optional: override reference branch (default: from config)
--diff-stat                    Include git diff --stat summaries per commit
--dry-run                      Build and display the audit prompt without invoking the tool
--yes                          Accept non-dangerous confirmations non-interactively
--json                         Emit machine-readable result
```

Options **not** exposed (same as `macc prd generate`):

```text
--performer
--skill
```

### 22.4 How it works

1. Loads the PRD file (`--prd` or `automation.coordinator.prd_file` or `prd.json`).
2. Loads the task registry from the PRD (tasks array or full TaskRegistry shape).
3. Reads all commits on `reference_branch` and maps them to completed tasks.
4. Gathers `git diff --stat` for each commit when `--diff-stat` is set.
5. Builds a structured LLM prompt via `prd_auditor::prepare_audit` containing:
   - The current `prd.json` content (truncated at 80 000 chars if needed).
   - Per-completed-task commit context (SHA, subject, diff stats).
   - List of todo task IDs for architectural impact review.
6. When `--dry-run` or no `--tool` is provided: prints the prompt and exits.
7. When `--tool <id>` is provided: the prompt is returned to the CLI/Web layer which routes it to the tool performer via the existing MACC performer infrastructure.

### 22.5 LLM instructions in the prompt

- Update `notes` of completed tasks to reflect what was actually delivered.
- Rewrite `description`/`steps` of todo tasks if integrated code changed the architecture.
- Move completed task objects to the `tasks_done` array.
- Never delete or rename task IDs.

### 22.6 Modes

| Invocation | Behaviour |
|---|---|
| `--dry-run` | Generate and print the audit prompt; do not invoke any tool. |
| No `--tool` | Same as `--dry-run`. |
| `--tool <id>` | Deliver the prompt to the specified tool via its performer spec. |

### 22.7 Underlying module

`core/src/coordinator/prd_auditor.rs` — pure business logic (prompt building, context gathering, no tool invocation). Called via `core/src/prd_generation/audit.rs`, which wraps the auditor and applies the shared `ModelSelection` option set.

### 22.8 Migration from `macc coordinator audit-prd`

| Old invocation | New invocation |
|---|---|
| `macc coordinator audit-prd` | `macc prd audit` |
| `macc coordinator audit-prd -- --tool claude` | `macc prd audit --tool claude` |
| `macc coordinator audit-prd -- --dry-run` | `macc prd audit --dry-run` |

The `CoordinatorCommand::AuditPrd` variant and its `parse_audit_prd_command` dispatcher are removed from the coordinator service layer. The `audit_prd_report` response field on `CoordinatorCommandResponse` is removed.

---

## 23. Promotion

Generation and activation should be separate.

Default:

```bash
macc prd generate --from brief.md
```

This writes to:

```text
.macc/generated/prd/macc-prd-planner/<run-id>/prd_X.json
```

Promotion:

```bash
macc prd promote .macc/generated/prd/macc-prd-planner/<run-id>/prd_X.json
```

Convenience:

```bash
macc prd generate --from brief.md --promote
```

If `prd.json` already exists, promotion should require confirmation unless `--yes` is provided in a safe non-interactive context.

Promotion should create a backup when overwriting an existing PRD.

---

# Part III — Access from All Clients

## 24. Shared core operation

Define one shared core request object used by CLI, TUI, and Web.

Example Rust-like shape:

```rust
struct PrdGenerateRequest {
    from_path: PathBuf,
    tool: Option<String>,
    model_selection: Option<ModelSelection>,
    instructions: Option<String>,
    instructions_file: Option<PathBuf>,
    target_dir: Option<PathBuf>,
    update_path: Option<PathBuf>,
    dry_run: bool,
    promote: bool,
}
```

All clients call the same pipeline:

```text
PrdGenerateRequest
  -> collect brief + MACC context
  -> build fixed macc-prd-planner prompt
  -> invoke selected tool/performer
  -> write output to target directory
  -> validate generated PRD
  -> optionally promote to project prd.json
```

---

## 25. CLI UX

Primary command:

```bash
macc prd generate --from brief.md
```

Other commands:

```bash
macc prd audit --prd prd.json
macc prd promote <generated-prd-path>
macc prd validate <prd-path>
macc prd runs
macc prd show-run <run-id>
```

`macc coordinator audit-prd` is **removed**. Its functionality is fully replaced by `macc prd audit` (see §22).

Recommended JSON output:

```bash
macc prd generate --from brief.md --json
```

Example JSON response:

```json
{
  "status": "generated",
  "run_id": "2026-05-26-143012",
  "target_dir": ".macc/generated/prd/macc-prd-planner/2026-05-26-143012",
  "files": [
    "prd.json",
    "prd.summary.md"
  ],
  "validation": {
    "status": "ok",
    "warnings": []
  },
  "promoted": false
}
```

---

## 26. TUI UX

Add a PRD Generation screen under Setup & Config or the PRD Editor.

Suggested screen:

```text
PRD Generation

Brief file:        [ brief.md                    ]
Tool:              [ Auto / claude / codex / ... ]
Model routing:     [ Auto / Manual               ]
Manual model:      [ model picker, if manual     ]
Instructions:      [ multiline text area         ]
Instructions file: [ optional path               ]
Target directory:  [ default generated path      ]

[Preview prompt] [Generate] [Generate + Promote]
```

After generation:

```text
Generated:
- .macc/generated/prd/macc-prd-planner/<run-id>/prd_x.json
- .macc/generated/prd/macc-prd-planner/<run-id>/prd.summary.md

Validation:
- OK

Actions:
[Open PRD] [Open Summary] [Promote] [Back]
```

---

## 27. Web API

Add the following endpoints:

```http
POST /api/v1/prd/generate
POST /api/v1/prd/audit
POST /api/v1/prd/promote
POST /api/v1/prd/validate
GET  /api/v1/prd/generation-runs
GET  /api/v1/prd/generation-runs/{run_id}
```

`POST /api/v1/prd/audit` replaces the former `coordinator audit-prd` functionality and uses the same core operation as `macc prd audit`.

### 27.1 Generate request

```json
{
  "from_path": "brief.md",
  "tool": "claude",
  "model_selection": {
    "mode": "auto"
  },
  "instructions": "Split coordinator, adapter, TUI, and Web work into separate tasks.",
  "instructions_file": null,
  "target_dir": ".macc/generated/prd/macc-prd-planner",
  "update_path": null,
  "dry_run": false,
  "promote": false
}
```

### 27.2 Generate response

```json
{
  "status": "generated",
  "run_id": "2026-05-26-143012",
  "target_dir": ".macc/generated/prd/macc-prd-planner/2026-05-26-143012",
  "files": [
    "prd.json",
    "prd.summary.md"
  ],
  "validation": {
    "status": "ok",
    "warnings": []
  },
  "promoted": false
}
```

### 27.3 Dry-run response

When `dry_run = true`, the response should include the generated prompt and should not invoke the tool.

```json
{
  "status": "dry_run",
  "prompt": "...",
  "target_dir": ".macc/generated/prd/macc-prd-planner/2026-05-26-143012",
  "tool": "claude",
  "model_selection": {
    "mode": "auto"
  }
}
```

---

## 28. Web UI

Add PRD generation to the existing PRD area:

```text
/prd
  - Current PRD
  - Generate PRD
  - Audit PRD
  - Generation Runs
  - Validate
  - Promote
```

The Web UI should support:

```text
select or upload brief file
choose tool or Auto
select model routing mode
select manual model when manual mode is active
enter client instructions
select optional instructions file
preview generated prompt
run generation with live logs/SSE
inspect generated files
validate result
promote to active prd.json
```

The Web UI should use the same API and same core operation as CLI/TUI.

---

# Part IV — Implementation Architecture

## 29. Suggested core modules

```text
core/src/prd_generation/
  mod.rs
  request.rs              # PrdGenerateRequest, ModelSelection, ModelRoutingMode
  prompt_builder.rs       # fixed macc-prd-planner prompt assembly
  context.rs              # minimal context collection
  target_dir.rs           # safe output directory resolution
  runner.rs               # selected tool invocation
  validation.rs           # lightweight generated PRD checks
  promotion.rs            # promote generated PRD with backup/confirmation
  metadata.rs             # generation run metadata (GenerationRunMetadata, run ID)
  audit.rs                # wraps prd_auditor.rs; PrdAuditRequest, PrdAuditResult

core/src/coordinator/prd_auditor.rs   # EXISTING — not replaced; called from audit.rs
core/src/coordinator/model_routing.rs
core/src/tool_api/model_profile.rs
core/src/tool_api/model_ranking.rs
```

CLI:

```text
cli/src/commands/prd.rs     # macc prd generate | audit | promote | validate | runs | show-run
```

TUI:

```text
tui/src/screens/prd_generation.rs
```

Web backend:

```text
cli/src/commands/web/prd_generation.rs
```

Web frontend:

```text
web/src/pages/prd/GeneratePrdPage.tsx
web/src/pages/prd/PrdGenerationRunsPage.tsx
web/src/api/prdGeneration.ts
```

**Removed** (migrated to `macc prd audit`):

```text
CoordinatorCommand::AuditPrd variant in core/src/service/coordinator_workflow.rs
parse_audit_prd_command() in core/src/service/coordinator_workflow.rs
coordinator_audit_prd() in core/src/service/coordinator_workflow.rs
audit-prd entry in cli/src/main.rs coordinator command list
audit_prd_report field on CoordinatorCommandResponse
```

---

## 30. State and storage

Generated outputs:

```text
.macc/generated/prd/macc-prd-planner/<run-id>/prd.json
.macc/generated/prd/macc-prd-planner/<run-id>/prd.summary.md
.macc/generated/prd/macc-prd-planner/<run-id>/prd.validation-notes.md
.macc/generated/prd/macc-prd-planner/<run-id>/generation-metadata.json
```

Optional prompt storage:

```text
.macc/generated/prd/macc-prd-planner/<run-id>/prompt.md
```

Prompt storage should be configurable because prompts may contain large context or sensitive project details.

---

## 31. Security and safety constraints

PRD generation must respect these constraints:

```text
Do not write outside the target directory during generation.
Do not edit source files.
Do not run implementation commands.
Do not execute remote code.
Do not write secrets.
Do not promote over an existing PRD without confirmation or explicit --yes.
Sanitize Web API paths against directory traversal.
Restrict Web API operations to project root and MACC-managed directories.
Log mutating Web API requests through the existing ops audit mechanism.
```

---

## 32. Error behavior

Recommended errors:

| Error | Condition | Suggested action |
|---|---|---|
| `PRD-GEN-INPUT-MISSING` | `--from` file does not exist | Provide a valid brief path |
| `PRD-GEN-TOOL-UNAVAILABLE` | Selected tool is not enabled or unavailable | Choose another tool or configure it |
| `PRD-GEN-MODEL-INVALID` | Manual model is missing or incompatible | Select a compatible model |
| `PRD-GEN-PROMPT-FAILED` | Prompt construction failed | Check config and input files |
| `PRD-GEN-RUNNER-FAILED` | Tool invocation failed | Inspect performer logs |
| `PRD-GEN-OUTPUT-MISSING` | `prd.json` was not generated | Retry or inspect generated files |
| `PRD-GEN-VALIDATION-FAILED` | Generated PRD failed validation | Review validation notes |
| `PRD-GEN-PROMOTE-CONFLICT` | Existing PRD would be overwritten | Confirm, backup, or choose another output |

Web API errors should be mapped into the existing structured Web API error envelope.

---

# Part V — Rollout Plan

## 33. Milestone 1 — Shared core service

Implement:

```text
PrdGenerateRequest
fixed prompt builder
target directory resolution
basic context collection
dry-run support
```

Acceptance criteria:

```text
CLI can build and print a prompt with --dry-run.
The prompt includes the fixed macc-prd-planner instruction.
No --performer or --skill options exist.
Target directory is deterministic and safe.
```

---

## 34. Milestone 2 — Tool invocation and output validation

Implement:

```text
selected tool resolution
model selection mode handling
performer/tool invocation
mandatory output checks
validation result reporting
```

Acceptance criteria:

```text
macc prd generate --from brief.md invokes the selected/default tool.
Generated files are written under .macc/generated/prd/macc-prd-planner/<run-id>/.
prd.json is parsed and validated minimally.
Failures keep output files for inspection.
```

---

## 35. Milestone 3 — Promotion flow

Implement:

```text
macc prd promote <path>
--promote convenience flag
backup before overwrite
confirmation gates
```

Acceptance criteria:

```text
Generated PRD can be promoted to root prd.json.
Existing PRD is backed up before overwrite.
Non-interactive promotion requires --yes.
Validation failure blocks promotion by default.
```

---

## 36. Milestone 4 — TUI integration

Implement:

```text
PRD Generation screen
prompt preview
generate action
generate + promote action
run result summary
validation warnings display
```

Acceptance criteria:

```text
TUI uses the same core request and response types as CLI.
TUI exposes no performer or skill selector.
TUI can choose tool, model routing mode, instructions, and target dir.
```

---

## 37. Milestone 5 — Web API and Web UI integration

Implement:

```text
POST /api/v1/prd/generate
POST /api/v1/prd/promote
POST /api/v1/prd/validate
GET /api/v1/prd/generation-runs
GET /api/v1/prd/generation-runs/{run_id}
Generate PRD page
Generation Runs page
SSE progress/log integration if available
```

Acceptance criteria:

```text
Web UI can generate, inspect, validate, and promote PRDs.
Web API uses the same core operation as CLI/TUI.
Path handling is restricted and sanitized.
Mutating operations are audit-logged.
```

---

## 38. Milestone 6 — Model routing integration

Implement:

```text
model_routing.mode support
manual model validation
auto mode routing decision for PRD generation
routing decision metadata
adapter model profiles
basic tier selection
```

Acceptance criteria:

```text
All clients can select auto or manual model routing.
Manual mode does not silently switch models.
Auto mode chooses a tier based on prd_generation phase and policy.
Routing decisions are logged and visible in metadata.
```

---

# Part VI — Acceptance Criteria

## 39. Functional acceptance criteria

```text
macc prd generate --from brief.md works with default configuration.
The command exposes no --performer or --skill options.
The generated prompt always references the fixed internal macc-prd-planner flow.
The generated files are written to a safe target directory.
Generated prd.json is validated before promotion.
Generation and promotion are separate by default.
--promote is available as a convenience flag.
All clients can access the same PRD generation capability.
CLI, TUI, Web API, and Web UI use the same core operation.
Automatic model selection is toggleable by all clients.
Manual model selection is respected and not silently overridden.
PRD routing hints remain provider-neutral.
macc prd audit accepts the same options as macc prd generate (--tool, --model-routing, --model, --instructions, --instructions-file, --dry-run, --yes, --json).
macc coordinator audit-prd no longer exists; macc prd audit is its replacement.
core/src/coordinator/prd_auditor.rs is not replaced or duplicated; it is called from prd_generation/audit.rs.
```

---

## 40. Non-functional acceptance criteria

```text
The command remains simple for common use.
The system is deterministic around prompt assembly and output placement.
Generated output is reproducible enough to inspect via stored metadata.
The implementation does not duplicate the planner's responsibilities inside MACC core.
The implementation preserves MACC's adapter/performer architecture.
The design remains compatible with coordinator execution, worktrees, sync-prd, and macc prd audit.
The audit migration introduces no new business logic — prd_auditor.rs is reused as-is.
```

---

# Part VII — Final Recommended Specification

## 41. Final CLI contract

```bash
macc prd generate --from <brief.md> [options]
macc prd audit   --prd <prd.json>   [options]
macc prd promote <generated-prd-path>
macc prd validate <prd-path>
macc prd runs
macc prd show-run <run-id>
```

### `macc prd generate` options

```text
--from <path>                  Required input brief
--tool <tool_id>
--model-routing <auto|manual>
--model <model_id>
--instructions <text>
--instructions-file <path>
--target-dir <path>
--update <path>
--dry-run
--promote
--yes
--json
```

### `macc prd audit` options

```text
--prd <path>                   Required: PRD file to audit (default: prd.json)
--tool <tool_id>
--model-routing <auto|manual>
--model <model_id>
--instructions <text>
--instructions-file <path>
--reference-branch <branch>
--diff-stat
--dry-run
--yes
--json
```

Forbidden user-facing options (both commands):

```text
--performer
--skill
```

---

## 42. Final internal constants

```text
PRD_GENERATION_INTERNAL_ROLE = "prd-planner"
PRD_GENERATION_INTERNAL_SKILL = "macc-prd-planner"
PRD_GENERATION_DEFAULT_TARGET_DIR = ".macc/generated/prd/macc-prd-planner"
```

---

## 43. Final design statement

`macc prd generate` should be an opinionated, client-accessible MACC operation that generates a complete planner prompt and invokes a selected tool through the fixed internal `macc-prd-planner` flow. The user should only provide the brief, optional tool/model preferences, optional client instructions, and optional output/promotion choices. The command must be available consistently through CLI, TUI, Web API, and Web UI.

`macc prd audit` is the successor to `macc coordinator audit-prd`. It shares the same option set as `macc prd generate` (tool, model routing, instructions, dry-run, json) and uses the existing `core/src/coordinator/prd_auditor.rs` business logic unchanged. The only difference is the prompt: audit enriches an existing PRD from delivered commits, whereas generate creates a new PRD from a brief.

Automatic model selection must be explicit and toggleable. In manual mode, MACC respects the selected model. In auto mode, MACC chooses the lightest sufficient model and reasoning depth based on the operation phase, adapter capabilities, token efficiency policy, risk, context size, availability, and fallback rules.

The resulting system remains simple, predictable, and aligned with MACC's core architecture: MACC coordinates; adapters execute; the PRD planner plans; the coordinator consumes validated PRDs.

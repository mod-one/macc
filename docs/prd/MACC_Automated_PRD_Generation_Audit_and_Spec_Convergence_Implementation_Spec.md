# MACC Automated PRD Generation, Implementation Audit, and Specification Convergence

**Project:** MACC — Multi-Assistant Code Config  
**Document type:** Implementation specification  
**Language:** English  
**Date:** 2026-08-11  
**Status:** Proposed implementation — aligned with the current MACC architecture  

---

## 1. Executive summary

This specification defines a configurable extension of the MACC coordinator workflow so that `macc coordinator run` can optionally manage the complete development loop:

```text
source brief/specification
        ↓
PRD generation
        ↓
PRD validation + activation
        ↓
Coordinator execution
        ↓
implementation / testing / review
        ↓
final implementation audit
        ↓
.macc/reports/implementation-audit.md
        ↓
PASS ───────────────────────────────→ workflow complete
        │
        └─ ACTION_REQUIRED
                 ↓
          remediation PRD generation
                 ↓
          Coordinator execution
                 ↓
          final audit again
                 ↓
          convergence or bounded stop
```

The feature must reuse MACC's existing components rather than create a parallel orchestration stack:

- the existing PRD generation flow;
- the fixed internal `macc-prd-planner` skill;
- the existing tool/model selection and prompt invocation infrastructure;
- the existing generated-PRD run directories under `.macc/generated/prd/`;
- the existing PRD validation and promotion logic;
- the existing Coordinator task registry, worktree pool, phases, retries, rate-limit handling, merge logic, and observability;
- the existing `macc-auditor` skill, already created for MACC, which generates Markdown reports under `.macc/reports/`, including `.macc/reports/implementation-audit.md`.

The feature is deliberately configurable and backward-compatible. A project that does not enable it must preserve today's Coordinator behavior.

The recommended defaults are:

```yaml
automation:
  coordinator:
    prd_generation:
      mode: existing_only

    spec_convergence:
      mode: disabled
      max_remediation_cycles: 2
      on_blocked: pause
```

With these defaults, `macc coordinator run` continues to require and execute an existing PRD exactly as before.

---

## 2. Problem statement

MACC already contains two strong but currently separate capabilities:

1. **PRD generation** through `macc prd generate`, which builds a prompt around the internal `macc-prd-planner` skill, invokes an AI tool, writes a generated `prd.json` into a safe `.macc/generated/prd/.../<run-id>/` directory, validates it, and can promote it.
2. **PRD execution** through `macc coordinator run`, which consumes an existing PRD and orchestrates implementation across worktrees, performers, testing/review phases, merge handling, retries, recovery, and cleanup.

The missing link is an optional workflow layer connecting them.

A second gap exists after a PRD is completed. Tests and code review answer important questions, but they do not necessarily answer the final product question:

> Does the integrated implementation actually conform to the original specification, expected behavior, architecture constraints, UX/UI requirements, design references, and acceptance intent?

The new `macc-auditor` skill addresses that verification problem by producing an implementation audit report. MACC should then be able to consume that report deterministically and, when configured for automation, ask `macc-prd-planner` to generate a remediation PRD from unresolved findings.

The result is a bounded **Specification Convergence Loop**.

---

## 3. Current-code alignment

The implementation should be aligned with the following existing MACC architecture.

### 3.1 Existing PRD generation module

Current PRD-generation code lives under:

```text
core/src/prd_generation/
  mod.rs
  audit.rs
  metadata.rs
  promotion.rs
  prompt_builder.rs
  request.rs
  target_dir.rs
  validation.rs
```

Important current characteristics:

- `PRD_GENERATION_INTERNAL_SKILL` is fixed to `macc-prd-planner`.
- `PrdGenerateRequest` already exists as the intended unified request model for PRD generation.
- generated PRDs use run directories under:

```text
.macc/generated/prd/macc-prd-planner/<run-id>/
```

- PRD generation already supports tool selection, model routing information, extra instructions, target directories, update mode, dry run, validation, and promotion.
- `Engine::prd_invoke_tool(...)` is the authoritative tool invocation path for PRD generation/audit flows.

### 3.2 Existing PRD generation logic is not yet fully centralized

At present, significant orchestration still exists in the CLI implementation of `macc prd generate`, including:

```text
read input
→ resolve instructions/model/tool
→ create generation run directory
→ build prompt
→ invoke tool
→ validate generated prd.json
→ optional promotion
```

Before the Coordinator starts using PRD generation internally, this flow should be centralized into one shared core/Engine operation.

This is important because Coordinator, CLI, TUI, and Web must not develop separate implementations of PRD generation.

### 3.3 Existing Coordinator workflow

The current Coordinator service already provides:

- `CoordinatorCommand::Run`;
- managed coordinator process startup/polling;
- PRD registry synchronization;
- dispatch/advance/reconcile/cleanup;
- task runtime state;
- worktree reuse;
- phase handling;
- rate-limit/backoff logic;
- pause/recovery semantics;
- process ownership;
- delayed one-shot runs through `--in` and `--at` in the CLI launch flow.

The new workflow must wrap or extend this existing run behavior; it must not replace the control plane.

### 3.4 Extensible task model

The current `Task` model contains typed fields such as:

```text
id
state
title
priority
category
scope
tool
dependencies
exclusive_resources
task_runtime
```

and also preserves unknown task fields through a flattened `extra` map.

This allows audit-specific task metadata to be introduced with minimal schema disruption in the first implementation.

### 3.5 Existing `macc-auditor` skill

This specification assumes the existing `macc-auditor` skill is the canonical implementation auditor.

The skill already writes reports under:

```text
.macc/reports/
```

including, for the final implementation-conformance audit:

```text
.macc/reports/implementation-audit.md
```

**Do not create another `macc-implementation-auditor` skill.**

If needed, only align `macc-auditor` with the machine-readable report contract defined in this specification.

---

## 4. Goals

The implementation must provide all of the following.

### G1 — Optional initial PRD generation

`macc coordinator run` can optionally generate a PRD from a source brief/specification before executing tasks.

### G2 — Preserve existing behavior

Existing projects must remain valid and keep the current behavior unless the new workflow options are enabled.

### G3 — One PRD-generation implementation

CLI, TUI, Web, and Coordinator must share the same core PRD-generation service.

### G4 — Planner-aware final audit

When specification convergence is enabled, `macc-prd-planner` must include a final implementation-audit task in generated PRDs.

### G5 — Reuse `macc-auditor`

The audit task must use the existing `macc-auditor` skill.

### G6 — Canonical report path

The current audit report used by MACC must always be:

```text
.macc/reports/implementation-audit.md
```

The path is a MACC contract and is not configurable in V1.

### G7 — Safe cross-worktree publication

Because `.macc` is gitignored and worktree-local, an audit report produced inside a worker worktree must be validated and published by the Coordinator into the primary project `.macc/reports/` directory.

### G8 — Deterministic report gate

The Coordinator must not ask an LLM merely to decide whether remediation is needed. It must read structured metadata from the report and act deterministically.

### G9 — Automatic remediation PRD

When configured in automatic mode and the report contains actionable findings, the Coordinator must generate a new PRD using the existing `macc-prd-planner` flow.

### G10 — Bounded convergence

The audit/remediation loop must have a configurable maximum cycle count and must never run indefinitely.

### G11 — Traceability

Remediation tasks must trace back to stable audit finding IDs.

### G12 — Token efficiency

Audit findings should be converted into focused remediation tasks so normal implementation workers do not need the full audit report unless required.

---

## 5. Non-goals

The first implementation must not introduce:

- a second PRD generation engine;
- a second audit skill;
- a new external scheduler or daemon;
- CI/CD replacement behavior;
- automatic unbounded self-repair;
- configurable arbitrary report paths;
- multiple concurrent writers to the canonical implementation audit report;
- an LLM-based decision step just to interpret `PASS` versus `ACTION_REQUIRED`;
- automatic modification of a completed source PRD in place;
- a new task terminal-state model solely for audit tasks;
- a complex workflow DSL.

---

## 6. Terminology

### Initial PRD

The first PRD executed by a Coordinator workflow.

### Remediation PRD

A new PRD generated from actionable findings in an implementation audit report.

### PRD generation run

One invocation of the existing MACC PRD generation flow, producing a run directory under `.macc/generated/prd/...`.

### Specification convergence

The process of auditing the delivered implementation against expectations and generating bounded remediation work until the audit passes or the configured cycle limit is reached.

### Audit cycle

One final implementation audit for one PRD execution.

### Remediation cycle

One automatic remediation PRD generated because a preceding audit returned `action_required`.

### Canonical audit report

The primary-project report:

```text
<project-root>/.macc/reports/implementation-audit.md
```

### Worker audit report

A report first produced inside an audit task's worktree:

```text
<worker-worktree>/.macc/reports/implementation-audit.md
```

It is not canonical until the Coordinator validates and publishes it.

---

# Part I — Configuration

## 7. Coordinator configuration model

Add two nested configuration blocks to `CoordinatorConfig`.

Recommended YAML:

```yaml
automation:
  coordinator:
    prd_generation:
      # existing_only | generate_if_missing | always_generate
      mode: existing_only

      # Source brief/specification used when generation is required.
      # Relative paths are resolved from the primary project root.
      source: null

      # pause | fail
      on_failure: pause

    spec_convergence:
      # disabled | manual | auto
      mode: disabled

      # Maximum automatically generated remediation PRDs
      # for one initial PRD workflow.
      max_remediation_cycles: 2

      # V1: pause is the recommended and default behavior.
      on_blocked: pause
```

### 7.1 Backward-compatible defaults

The defaults must be:

```text
prd_generation.mode = existing_only
spec_convergence.mode = disabled
max_remediation_cycles = 2
on_blocked = pause
```

Therefore an existing `macc.yaml` with none of these fields keeps the existing Coordinator behavior.

---

## 8. `prd_generation.mode`

### 8.1 `existing_only`

Behavior:

```text
existing PRD required
      ↓
Coordinator executes it
```

No automatic initial PRD generation is performed.

This is the compatibility default.

### 8.2 `generate_if_missing`

Behavior:

```text
active PRD exists?
     │
 ┌───┴───┐
 │       │
yes      no
 │       │
 ▼       ▼
use it   generate from configured source
 │       │
 └───┬───┘
     ▼
Coordinator
```

If no active PRD exists and no source is available, the workflow must not guess. It must stop with an actionable error/pause.

### 8.3 `always_generate`

A fresh initial PRD is generated from the configured source for every new Coordinator workflow run.

This mode is useful when the source specification is the authoritative input and a stale `prd.json` must never be reused.

It must not be the default.

---

## 9. `spec_convergence.mode`

### 9.1 `disabled`

No final audit is required solely for specification convergence.

The Coordinator completes when the PRD execution converges according to the existing task/runtime rules.

### 9.2 `manual`

Generated PRDs include a final audit task.

The Coordinator executes the audit and publishes the report, but it does not automatically generate a remediation PRD.

Possible terminal result:

```text
completed
```

or:

```text
completed_with_findings
```

`completed_with_findings` is not a task failure. It means the implementation completed but the final audit reported actionable work and automatic remediation is disabled.

### 9.3 `auto`

Generated PRDs include the final audit task.

After the report is published:

```text
pass
  → workflow complete

action_required
  → generate remediation PRD
  → execute it
  → audit again

blocked
  → pause
```

Automatic remediation stops when:

- the audit passes;
- the configured maximum remediation cycle count is reached;
- the audit is blocked;
- PRD generation fails;
- a Coordinator blocking error occurs;
- the operator stops the workflow.

---

## 10. Configuration precedence

Use the existing MACC precedence principle:

```text
per-run CLI override
        ↓
.macc/macc.yaml
        ↓
built-in default
```

Do not duplicate PRD tool/model configuration inside `automation.coordinator.prd_generation`.

Tool and model selection for automatic PRD generation must reuse the same configuration resolution used by `macc prd generate`, including:

- explicit request override where available;
- PRD-generation default tool;
- Coordinator tool fallback where already supported;
- enabled tool list;
- model routing configuration.

---

# Part II — CLI contract

## 11. Coordinator CLI additions

Recommended flags:

```text
macc coordinator run
  [--prd-from <PATH>]
  [--prd-generation <existing_only|generate_if_missing|always_generate>]
  [--spec-convergence <disabled|manual|auto>]
  [--max-remediation-cycles <N>]
```

### 11.1 `--prd-from`

Example:

```bash
macc coordinator run --prd-from feature-brief.md
```

This is the user-friendly shortcut for a generated initial PRD.

Recommended behavior:

- if `--prd-generation` is not explicitly provided, `--prd-from` implies `generate_if_missing` for that run;
- if `--prd-generation always_generate` is explicitly provided, the source is always used;
- if `--prd-generation existing_only` is explicitly combined with `--prd-from`, return a validation error rather than silently ignore one option.

### 11.2 Interaction with existing `--prd`

`--prd` means "execute this existing PRD".

`--prd-from` means "generate a PRD from this source".

They should be mutually exclusive to avoid ambiguous behavior.

### 11.3 Full automatic example

```bash
macc coordinator run \
  --prd-from specs/feature.md \
  --spec-convergence auto
```

Conceptually:

```text
specs/feature.md
  → generate PRD
  → validate/promote
  → execute
  → audit
  → remediation PRD if needed
  → execute
  → audit
  → pass or bounded pause
```

### 11.4 Delayed one-shot integration

The existing delayed run syntax remains compatible:

```bash
macc coordinator run \
  --in 2h \
  --prd-from specs/feature.md \
  --spec-convergence auto
```

or:

```bash
macc coordinator run \
  --at "2026-08-12T02:00:00+02:00" \
  --prd-from specs/feature.md \
  --spec-convergence auto
```

PRD generation must occur at actual workflow start, not hours before a delayed start, so that the latest source/project state is used.

---

# Part III — Shared PRD generation service

## 12. Centralize PRD generation before Coordinator integration

This is a prerequisite.

The current `PrdGenerateRequest` should become the real input contract for one shared operation.

Add:

```rust
pub enum PrdGenerationPurpose {
    Standard,
    AuditRemediation,
}
```

Extend the request conceptually:

```rust
pub struct PrdGenerateRequest {
    pub from_path: PathBuf,
    pub purpose: PrdGenerationPurpose,
    pub tool: Option<String>,
    pub model_selection: Option<ModelSelection>,
    pub instructions: Option<String>,
    pub instructions_file: Option<PathBuf>,
    pub target_dir: Option<PathBuf>,
    pub update_path: Option<PathBuf>,
    pub dry_run: bool,
    pub promote: bool,
    pub yes: bool,
}
```

Add a shared result:

```rust
pub struct PrdGenerateResult {
    pub run_id: String,
    pub purpose: PrdGenerationPurpose,
    pub generated_prd: PathBuf,
    pub target_dir: PathBuf,
    pub tool: String,
    pub validation: ValidationResult,
    pub promoted_to: Option<PathBuf>,
}
```

Add the Engine facade method:

```rust
fn prd_generate(
    &self,
    paths: &ProjectPaths,
    request: &PrdGenerateRequest,
) -> Result<PrdGenerateResult>;
```

### 12.1 Required ownership

The shared service owns:

```text
input reading
→ instruction resolution
→ model selection metadata
→ run metadata
→ target directory
→ prompt building
→ tool resolution
→ tool invocation
→ output validation
→ optional promotion
→ generation result
```

### 12.2 Clients become thin adapters

After refactoring:

```text
CLI macc prd generate ───────┐
TUI PRD action ──────────────┤
Web PRD action ──────────────┤
Coordinator initial planning ┤
Coordinator remediation ─────┤
                             ▼
                    Engine::prd_generate()
```

No client should duplicate generation orchestration.

---

## 13. PRD activation in the Coordinator workflow

For V1, reuse existing promotion rather than invent a new active-PRD store.

Recommended flow:

```text
.macc/generated/prd/.../<run-id>/prd.json
                  ↓
              validate
                  ↓
              promote
                  ↓
     configured coordinator PRD path
                  ↓
            existing Coordinator
```

### 13.1 Destination resolution

The Coordinator workflow must resolve one authoritative active PRD destination.

Recommended precedence:

```text
explicit --prd destination (existing-only mode)
        ↓
automation.coordinator.prd_file
        ↓
prd_generation.promotion.default_output_path
        ↓
prd.json
```

For generated PRDs, promotion must target the same file the Coordinator will consume.

### 13.2 History remains preserved

Promoting does not lose generated PRD history because each generation run remains under:

```text
.macc/generated/prd/macc-prd-planner/<run-id>/
```

Existing backup behavior should also remain active for replaced promoted files.

---

# Part IV — `macc-prd-planner` changes

## 14. Inject workflow policy into the PRD generation prompt

`macc-prd-planner` should not independently discover Coordinator configuration.

MACC should inject resolved workflow policy into the generation prompt.

Example:

```text
## MACC workflow policy

PRD generation purpose: standard
Specification convergence mode: auto
Final implementation audit: required
Audit skill: macc-auditor
Canonical audit report: .macc/reports/implementation-audit.md
Automatic remediation: enabled
Maximum remediation cycles: 2
```

For disabled mode:

```text
Specification convergence mode: disabled
Do not add a final implementation-conformance audit task solely for MACC convergence.
```

---

## 15. Planner rule: one final canonical implementation audit task

For V1, when `spec_convergence.mode` is `manual` or `auto`, every generated PRD must contain **exactly one final task responsible for the canonical implementation audit report**.

This is intentionally simpler and safer than allowing many audit tasks to race on the same file.

Recommended task example:

```json
{
  "id": "FEATURE-AUDIT-001",
  "title": "Audit integrated implementation against the specification",
  "category": "implementation_audit",
  "dependencies": [
    "FEATURE-TEST-001",
    "FEATURE-INTEGRATION-001"
  ],
  "required_skill": "macc-auditor",
  "execution_mode": "artifact_only",
  "artifact_outputs": [
    ".macc/reports/implementation-audit.md"
  ],
  "objective": "Verify the final integrated implementation against authoritative specifications, requirements, design constraints, acceptance criteria, and relevant project conventions.",
  "result": "A canonical implementation audit report is generated.",
  "routing_hints": {
    "execution_mode": "structural",
    "reasoning_depth": "deep",
    "context_scope": "cross-cutting",
    "risk_level": "medium",
    "validation_profile": "heavy"
  }
}
```

The extra fields can initially be carried through the task model's extensible metadata map.

### 15.1 Dependency rule

The final audit task must depend on all tasks whose merged output is necessary to evaluate the complete feature.

The audit must never run against a partially integrated feature.

### 15.2 Audit task does not modify source code

The planner must state that the audit task:

- uses `macc-auditor`;
- analyzes the final integrated state;
- writes only its report artifacts;
- does not implement fixes;
- does not modify production source code;
- does not resolve its own findings.

---

## 16. Planner behavior for remediation PRDs

When `purpose = AuditRemediation`, the planner must follow a stricter contract.

It must:

1. read the implementation audit report as the primary remediation brief;
2. create work only for actionable unresolved findings;
3. not recreate already compliant work;
4. group related findings into coherent tasks rather than one task per line item;
5. split complex findings when necessary;
6. preserve stable finding IDs in generated task metadata;
7. preserve normal MACC dependency and exclusive-resource rules;
8. include appropriate validation work;
9. include one final `implementation_audit` task again when convergence remains enabled;
10. never modify the completed previous PRD in place.

Recommended remediation task metadata:

```json
{
  "id": "AUDIT-FIX-001",
  "title": "Restore responsive sidebar behavior",
  "category": "frontend",
  "source_findings": [
    "AUD-003",
    "AUD-007"
  ]
}
```

---

# Part V — Audit execution and artifact publication

## 17. Reuse `macc-auditor` as an artifact-only worker task

The recommended V1 execution model is:

- keep the audit represented as a PRD task;
- dispatch it only after all required dependencies are merged;
- create/reuse a worktree from the latest reference branch using existing Coordinator mechanisms;
- explicitly instruct the performer to use `macc-auditor`;
- allow the task to complete without a source-code commit;
- validate and publish the generated report artifact;
- mark the task terminal using existing task state compatibility.

This avoids creating a separate audit daemon or entirely separate worker framework.

---

## 18. `artifact_only` task semantics

A normal implementation task is generally expected to produce source changes and follow dev/review/merge semantics.

An implementation audit is different because `.macc/reports/implementation-audit.md` is gitignored and should not be committed.

Therefore add a narrow execution specialization:

```text
execution_mode = artifact_only
```

For an `artifact_only` audit task:

1. run the performer/tool using the required skill;
2. do not require a source commit;
3. do not enter normal code-review/merge phases unless the audit task unexpectedly modifies tracked source files;
4. require all declared artifacts to exist;
5. validate the artifacts;
6. publish them to the primary project;
7. set:

```text
task_runtime.completion_kind = artifact_only
```

8. transition to the existing terminal-compatible task state used by the Coordinator for completed work (to avoid introducing a broad new state migration in V1);
9. release the worktree normally.

### 18.1 Safety rule

If an artifact-only audit task modifies tracked source files, treat that as a contract violation.

Recommended behavior:

```text
pause/block audit task
reason = AUDIT_TASK_MODIFIED_SOURCE
```

Do not silently merge those changes.

---

## 19. Why the Coordinator must publish the report

`.macc` is gitignored and each worktree has a separate local filesystem context.

Therefore:

```text
worker/.macc/reports/implementation-audit.md
```

is not automatically visible as:

```text
project/.macc/reports/implementation-audit.md
```

Git cannot be used as the transport mechanism.

The publication flow must be:

```text
Audit worker
    ↓
<worktree>/.macc/reports/implementation-audit.md
    ↓
Coordinator validates artifact
    ↓
Coordinator copies atomically
    ↓
<primary-project>/.macc/reports/implementation-audit.md
```

### 19.1 Ownership rule

The canonical primary-project `.macc/reports/` directory is Coordinator-owned runtime state.

Workers produce candidate artifacts; the Coordinator publishes canonical artifacts.

---

## 20. Canonical report path

Define a constant in core code:

```rust
pub const IMPLEMENTATION_AUDIT_REPORT_REL_PATH: &str =
    ".macc/reports/implementation-audit.md";
```

Do not make this configurable in V1.

This gives MACC a stable contract for:

- publication;
- status display;
- remediation generation;
- troubleshooting;
- Web/TUI links;
- future report history tooling.

---

# Part VI — Machine-readable audit report contract

## 21. Do not use file existence alone

The Coordinator must **not** behave as follows:

```text
if implementation-audit.md exists:
    generate a PRD
```

That is unsafe because the file may be:

- stale;
- produced by a prior PRD;
- partially written;
- left over after an audit failure;
- unrelated to the current workflow.

The Coordinator must validate report metadata.

---

## 22. Required Markdown frontmatter

The existing `macc-auditor` skill should emit machine-readable YAML frontmatter for the canonical implementation audit report.

Recommended V1 contract:

```md
---
schema_version: 1
report_type: implementation_audit
status: action_required
follow_up_required: true
source_prd_sha256: "sha256:..."
audit_task_id: "FEATURE-AUDIT-001"
remediation_cycle: 0
generated_at: "2026-08-11T23:10:00+02:00"
critical_findings: 0
major_findings: 2
minor_findings: 1
---

# Implementation Audit

...
```

### 22.1 Required status values

Only:

```text
pass
action_required
blocked
```

### 22.2 Status semantics

#### `pass`

```yaml
status: pass
follow_up_required: false
```

The implementation meets the audit's required acceptance threshold.

#### `action_required`

```yaml
status: action_required
follow_up_required: true
```

One or more findings require implementation work.

#### `blocked`

```yaml
status: blocked
follow_up_required: false
```

The auditor could not produce a trustworthy verdict.

Examples:

- authoritative specification missing;
- required reference artifact unavailable;
- application cannot be inspected sufficiently;
- environment dependency unavailable;
- design reference missing;
- audit evidence incomplete.

A blocked audit must never trigger speculative remediation PRD generation.

---

## 23. Finding contract

Every actionable finding should have a stable identity.

Recommended Markdown structure:

```md
## AUD-003 — Responsive sidebar behavior diverges from specification

- **Severity:** major
- **Category:** ux-ui-conformance
- **Requirement:** SPEC-UI-014
- **Status:** new
- **Expected:** ...
- **Observed:** ...
- **Evidence:** ...
- **Recommendation:** ...
```

Minimum fields:

```text
finding ID
severity
category
requirement/spec reference when available
expected behavior
observed behavior
evidence
recommendation
```

Recommended finding lifecycle values for later audits:

```text
new
still_present
resolved
regressed
```

The first implementation only requires stable finding IDs for PRD traceability; lifecycle comparison can be added incrementally.

---

## 24. Report validation

Before publishing a worker report, MACC must validate:

1. file exists;
2. file is inside the worker `.macc/reports/` path;
3. Markdown can be read;
4. YAML frontmatter exists;
5. `schema_version` is supported;
6. `report_type == implementation_audit`;
7. `status` is recognized;
8. `follow_up_required` is consistent with status;
9. `source_prd_sha256` matches the currently executed PRD;
10. `audit_task_id` matches the current audit task;
11. `remediation_cycle` matches the current workflow cycle;
12. the report is non-empty after frontmatter.

Only after these checks may MACC atomically replace the canonical report.

---

# Part VII — Specification convergence gate

## 25. End-of-PRD decision logic

When the current PRD task graph reaches terminal completion, the Coordinator workflow evaluates `spec_convergence.mode`.

### 25.1 Disabled

```text
PRD complete
    ↓
workflow complete
```

No report is required.

### 25.2 Manual

```text
PRD complete
    ↓
canonical audit report
    ↓
pass
  → workflow complete

action_required
  → completed_with_findings
  → no automatic PRD generation

blocked
  → pause
```

### 25.3 Auto

```text
PRD complete
    ↓
canonical audit report
    ↓
pass
  → workflow complete

action_required
  → cycle limit check
  → remediation PRD generation
  → execute remediation PRD

blocked
  → pause
```

---

## 26. Cycle limit

Default:

```yaml
max_remediation_cycles: 2
```

Meaning:

```text
initial PRD                 cycle 0
first remediation PRD       cycle 1
second remediation PRD      cycle 2
```

If the audit after cycle 2 still returns `action_required`:

```text
PAUSE
reason = MAX_REMEDIATION_CYCLES_REACHED
```

The Coordinator must surface:

- the canonical report path;
- outstanding finding count;
- current cycle;
- configured maximum;
- recommended operator action.

Never silently start cycle 3.

---

# Part VIII — Remediation PRD generation

## 27. Input to `macc-prd-planner`

For remediation generation, the canonical audit report is the primary `from_path`:

```text
<project-root>/.macc/reports/implementation-audit.md
```

Conceptually equivalent to:

```bash
macc prd generate \
  --from .macc/reports/implementation-audit.md
```

but the Coordinator must call the shared core PRD-generation service, not spawn the CLI command.

### 27.1 Purpose

Set:

```text
PrdGenerationPurpose::AuditRemediation
```

### 27.2 Additional context

The generated prompt should also provide enough context to avoid an audit report becoming an isolated planning source.

Recommended context:

- remediation purpose;
- previous completed PRD path/content or a bounded relevant summary;
- original workflow source path when available;
- current canonical report path;
- current remediation cycle;
- requirement to preserve finding IDs;
- requirement to create a new PRD rather than mutate the previous one;
- specification-convergence policy.

Do not automatically inject huge repository dumps.

---

## 28. New PRD, not update-in-place

Do not use the existing `update_path` mode to mutate a completed PRD for remediation.

Required behavior:

```text
PRD #1 completed
      ↓
audit report
      ↓
new PRD generation run
      ↓
PRD #2 remediation
```

This preserves:

- task identity;
- history;
- debugging;
- auditability;
- clean cross-PRD dependency semantics.

---

## 29. Remediation PRD validation

A remediation PRD must pass normal PRD validation plus convergence-specific validation.

When `spec_convergence != disabled`, validate:

- a final `implementation_audit` task exists;
- exactly one task writes the canonical implementation audit report;
- the task declares `required_skill = macc-auditor`;
- the task is `artifact_only`;
- the audit task depends on the final implementation/validation barrier;
- remediation implementation tasks contain `source_findings` where applicable;
- no provider-specific model names appear in routing hints.

Failure must prevent promotion/activation.

---

# Part IX — Workflow orchestration architecture

## 30. Keep the existing Coordinator control plane intact

The existing Coordinator task execution loop must remain responsible for:

```text
sync
→ dispatch
→ advance
→ reconcile
→ cleanup
→ task convergence
```

Do not put PRD generation logic into dispatch or task-selection code.

Instead, create a higher-level Coordinator workflow orchestrator.

---

## 31. Recommended workflow service

Add a dedicated core service conceptually named:

```text
core/src/service/coordinator_workflow_run.rs
```

or extend the existing `core/src/service/coordinator_workflow.rs` with a clear high-level API.

Suggested operation:

```rust
pub fn run_coordinator_workflow(
    engine: &dyn Engine,
    paths: &ProjectPaths,
    request: CoordinatorWorkflowRunRequest,
) -> Result<CoordinatorWorkflowRunResult>;
```

### 31.1 Responsibilities

The high-level workflow owns:

1. resolving workflow config + CLI overrides;
2. resolving initial PRD source;
3. generating an initial PRD if required;
4. validating/promoting the PRD;
5. running the existing Coordinator execution for that PRD;
6. waiting for task convergence;
7. evaluating/publishing the final audit report;
8. deciding pass/manual-findings/blocked/remediation;
9. generating remediation PRDs when allowed;
10. enforcing the cycle limit;
11. emitting workflow-level events and final result.

It must not reimplement task-level scheduling.

---

## 32. User-facing `CoordinatorCommand::Run`

`CoordinatorCommand::Run` should become the user-facing **workflow** command.

Conceptually:

```text
CoordinatorCommand::Run
        ↓
workflow orchestrator
        ↓
ensure active PRD
        ↓
existing managed Coordinator PRD execution
        ↓
post-PRD convergence gate
        ↓
optional next PRD
```

The existing internal control-plane run behavior should remain available through the existing internal/native run mechanisms rather than recursively invoking the workflow entry point.

If necessary, extract the current "execute one active PRD until task convergence" behavior into a clearly named internal function, for example:

```rust
run_active_prd_once(...)
```

Then:

```rust
run_coordinator_workflow(...)
    → run_active_prd_once(...)
```

---

## 33. Workflow state without a large new database model

Avoid introducing a complex workflow database in V1.

The implementation only needs to track a small amount of high-level state during one workflow:

```text
initial source path
current PRD generation run ID
current active PRD hash
current remediation cycle
last audit task ID
last canonical report hash/status
```

Recommended V1 approach:

- hold current state in the workflow process;
- persist minimal recovery metadata under existing `.macc/state/` using atomic JSON if required for crash recovery;
- continue treating Coordinator SQLite/task storage as source of truth for task execution.

Suggested file if persistence is required:

```text
.macc/state/coordinator-workflow.json
```

Example:

```json
{
  "schema_version": 1,
  "workflow_id": "wf-20260811-213000",
  "initial_source": "specs/feature.md",
  "active_prd_path": "prd.json",
  "active_prd_sha256": "sha256:...",
  "prd_generation_run_id": "2026-08-11-213001",
  "remediation_cycle": 1,
  "last_audit_status": "action_required"
}
```

Do not persist full prompts or duplicate the task registry here.

---

# Part X — Audit task routing

## 34. `required_skill` contract

The planner-generated audit task should include:

```json
"required_skill": "macc-auditor"
```

The performer prompt/task preparation layer should surface this explicitly:

```text
Required skill: macc-auditor
Use this skill for the task.
```

Do not rely only on accidental skill auto-discovery.

### 34.1 Availability preflight

When convergence is `manual` or `auto`, preflight must verify that `macc-auditor` is available for the selected audit tool.

If unavailable:

```text
pause/fail before dispatching the audit task
```

with an actionable message.

The feature should not silently fall back to a generic reviewer because the audit report contract is part of the workflow protocol.

---

## 35. Tool/model routing for audit

Keep PRD task metadata provider-neutral.

Recommended audit hints:

```json
{
  "reasoning_depth": "deep",
  "context_scope": "cross-cutting",
  "risk_level": "medium",
  "validation_profile": "heavy"
}
```

MACC's model-routing layer remains responsible for selecting the concrete tool/model.

For low-risk/small PRDs the planner may reduce audit scope, but in `auto` convergence mode a final report must still be generated because the Coordinator requires a deterministic gate.

---

# Part XI — Stale report and race protections

## 36. Never consume a report merely because it is present

A previous canonical report may remain at:

```text
.macc/reports/implementation-audit.md
```

The workflow gate must only consume a report that was successfully published from the **current audit task**.

Required checks:

```text
current audit task completed
AND artifact publication succeeded
AND audit_task_id matches
AND source_prd_sha256 matches
AND remediation_cycle matches
```

Only then may the report affect workflow control.

---

## 37. Atomic publication

Publication should use the same safety principles as other MACC filesystem writes:

```text
validate source candidate
→ write temporary file in primary report directory
→ fsync/best effort
→ atomic rename to implementation-audit.md
```

A partial report must never replace a valid canonical report.

Optional later improvement:

```text
.macc/reports/history/<workflow-id>/cycle-<N>-implementation-audit.md
```

History is useful but not required for the minimum V1 behavior because generated PRD history already exists separately.

---

# Part XII — Findings and token-efficient worker context

## 38. Remediation workers should not receive the whole report by default

The planner should translate audit findings into self-contained remediation tasks.

Example:

```json
{
  "id": "AUDIT-FIX-002",
  "source_findings": ["AUD-002", "AUD-004"],
  "description": "...sufficient implementation context...",
  "acceptance_criteria": ["..."]
}
```

Normal worker input remains the task's `worktree.prd.json`.

This keeps context small.

### 38.1 Optional targeted audit context

If a task requires detailed evidence, MACC may later project only the referenced findings into:

```text
<worktree>/.macc/context/audit-findings.md
```

containing only findings listed in `source_findings`.

Do not copy the full global audit report into every worktree by default.

---

# Part XIII — Error handling

## 39. Recommended workflow errors

Introduce structured errors in the existing MACC error model rather than raw strings where practical.

Suggested semantic errors:

```text
PRD_WORKFLOW_SOURCE_MISSING
PRD_WORKFLOW_GENERATION_FAILED
PRD_WORKFLOW_VALIDATION_FAILED
PRD_WORKFLOW_PROMOTION_FAILED
AUDIT_SKILL_UNAVAILABLE
AUDIT_REPORT_MISSING
AUDIT_REPORT_INVALID
AUDIT_REPORT_STALE
AUDIT_REPORT_CONTRACT_MISMATCH
AUDIT_TASK_MODIFIED_SOURCE
SPEC_CONVERGENCE_BLOCKED
MAX_REMEDIATION_CYCLES_REACHED
```

Exact numeric MACC codes should follow the project's existing error-code allocation policy.

---

## 40. Failure policy

### Initial PRD generation failure

Recommended default:

```text
pause workflow
```

Do not execute an older PRD as a silent fallback in `always_generate` mode.

### Audit task failure

Pause/block according to existing task failure policy. Do not consume an old report.

### Missing report after successful tool process

Treat as audit failure:

```text
AUDIT_REPORT_MISSING
```

### Invalid frontmatter

Treat as audit failure:

```text
AUDIT_REPORT_INVALID
```

### `status = blocked`

Pause the workflow. Do not generate remediation.

### Remediation PRD generation failure

Pause. Preserve the report and previous generated PRD run for investigation.

### Cycle limit reached

Pause with outstanding findings; do not mark the implementation as conformant.

---

# Part XIV — Observability

## 41. Workflow-level events

Add high-level events in addition to existing task events.

Recommended event types:

```text
workflow_started
prd_generation_started
prd_generation_completed
prd_generation_failed
prd_activated
prd_execution_started
prd_execution_completed
audit_artifact_published
audit_passed
audit_action_required
audit_blocked
remediation_generation_started
remediation_generation_completed
remediation_cycle_started
spec_convergence_reached
spec_convergence_limit_reached
workflow_completed
workflow_paused
```

Include useful metadata:

```text
workflow_id
prd_run_id
prd_hash
remediation_cycle
audit_task_id
audit_status
finding counts
```

---

## 42. CLI output

Example automatic run:

```text
MACC Coordinator Workflow
PRD generation: generate_if_missing
Specification convergence: auto
Source: specs/feature.md
Maximum remediation cycles: 2

[planning] Generating initial PRD...
[planning] PRD validated and activated.
[execution] Coordinator started.
...
[audit] implementation-audit.md published.
[audit] ACTION_REQUIRED: 3 findings.
[planning] Generating remediation PRD (cycle 1/2)...
[planning] Remediation PRD activated.
[execution] Coordinator resumed with remediation PRD.
...
[audit] PASS.
[workflow] Specification convergence reached.
```

Manual mode example:

```text
[workflow] Coordinator tasks completed.
[audit] ACTION_REQUIRED: 3 findings.
[audit] Report: .macc/reports/implementation-audit.md
[workflow] Automatic remediation disabled (spec_convergence=manual).
[workflow] Completed with findings.
```

---

## 43. Status API / TUI / Web fields

The core status model should eventually expose:

```text
workflow_status
prd_generation_mode
spec_convergence_mode
current_prd_run_id
remediation_cycle
max_remediation_cycles
last_audit_status
last_audit_report
outstanding_findings
```

UI implementation can follow after core behavior is stable.

No separate Web/TUI workflow engine is allowed.

---

# Part XV — Security and execution boundaries

## 44. Audit task permissions

`macc-auditor` should be treated as read-only with respect to tracked source code.

Allowed:

```text
read project files
read specs/design references made available locally
run approved validation/inspection commands where the skill/task permits
write .macc/reports/*
```

Not allowed by the audit contract:

```text
modify production source
commit fixes
merge code
silently rewrite specifications
```

---

## 45. Report content

Audit reports may contain code paths, findings, and technical evidence. They remain under `.macc/` and are not automatically committed.

Normal MACC log/report secret-redaction principles should apply where relevant.

---

# Part XVI — Backward compatibility

## 46. Existing `macc coordinator run`

With defaults:

```yaml
prd_generation:
  mode: existing_only
spec_convergence:
  mode: disabled
```

behavior is unchanged.

### 46.1 Existing PRDs

Existing PRDs do not need audit tasks when convergence is disabled.

### 46.2 Existing PRD + `manual`

Recommended V1 behavior if the PRD lacks a final implementation audit task:

- warn clearly;
- do not invent a report;
- finish using existing execution semantics;
- report that specification convergence could not be evaluated.

### 46.3 Existing PRD + `auto`

Automatic convergence requires a valid final audit task.

Recommended V1 behavior:

```text
preflight error/pause:
PRD does not contain the required implementation_audit task.
Regenerate the PRD with macc-prd-planner under spec_convergence=auto,
or disable automatic convergence.
```

This is safer than silently adding hidden work to a user-authored PRD.

---

# Part XVII — Validation changes

## 47. Extend PRD validation contextually

Current PRD validation is intentionally lightweight.

Add optional workflow-aware validation rather than making all old PRDs fail.

Suggested API:

```rust
pub struct PrdValidationContext {
    pub spec_convergence_mode: SpecConvergenceMode,
    pub generation_purpose: PrdGenerationPurpose,
}
```

Then validate extra contracts only when applicable.

### 47.1 Convergence-enabled checks

For `manual` or `auto` generated PRDs:

```text
exactly one canonical implementation_audit writer
required_skill == macc-auditor
execution_mode == artifact_only
artifact_outputs contains .macc/reports/implementation-audit.md
audit task is dependency-terminal
```

### 47.2 Remediation checks

For `AuditRemediation`:

```text
implementation tasks with actionable audit provenance include source_findings
final audit task exists again
task IDs remain unique
normal routing-hint neutrality rules still apply
```

---

# Part XVIII — Recommended code changes

## 48. Core configuration

### `core/src/config/mod.rs`

Add typed configuration structures, for example:

```rust
pub struct CoordinatorPrdGenerationConfig {
    pub mode: String,
    pub source: Option<String>,
    pub on_failure: String,
}

pub struct SpecConvergenceConfig {
    pub mode: String,
    pub max_remediation_cycles: usize,
    pub on_blocked: String,
}
```

Add optional/defaulted fields to `CoordinatorConfig`:

```rust
pub prd_generation: Option<CoordinatorPrdGenerationConfig>,
pub spec_convergence: Option<SpecConvergenceConfig>,
```

Prefer typed enums during resolution even if YAML deserialization initially uses strings for compatibility with existing config patterns.

---

## 49. PRD generation core

### `core/src/prd_generation/request.rs`

Add:

```text
PrdGenerationPurpose
```

and ensure request fields support internal Coordinator use.

### `core/src/prd_generation/prompt_builder.rs`

Add workflow-policy and generation-purpose sections to the prompt.

Do not hardcode Coordinator config reading inside the prompt builder; pass resolved policy in.

### New/extended generation service

Move orchestration currently duplicated in CLI into core/service and expose through `Engine::prd_generate()`.

### `core/src/engine.rs`

Add the shared PRD-generation facade method.

Keep `prd_invoke_tool()` as the single low-level tool invocation path.

### `cli/src/commands/prd.rs`

Reduce `run_generate()` to request construction, Engine invocation, and presentation.

### Web/TUI

Migrate them to the same Engine operation as part of the same refactor or immediately afterward.

---

## 50. Coordinator workflow

### `core/src/service/coordinator_workflow.rs`

Add high-level workflow orchestration around the existing one-PRD execution loop.

Responsibilities include:

```text
ensure/generate PRD
execute active PRD
convergence gate
remediation generation
cycle management
```

Do not move task scheduler internals here.

### `cli/src/coordinator/command.rs`

Add CLI override fields:

```text
prd_from
prd_generation_mode
spec_convergence_mode
max_remediation_cycles
```

Resolve delayed start first as today, then start the workflow only at the actual scheduled start.

### CLI argument definitions

Add Clap arguments with conflicts/validation.

---

## 51. Coordinator audit artifact support

Recommended module:

```text
core/src/coordinator/audit_artifact.rs
```

Responsibilities:

```text
parse report frontmatter
validate report contract
validate source PRD hash
audit task identity validation
atomic publication
return AuditOutcome
```

Suggested types:

```rust
pub enum AuditStatus {
    Pass,
    ActionRequired,
    Blocked,
}

pub struct ImplementationAuditMetadata {
    pub schema_version: u32,
    pub status: AuditStatus,
    pub follow_up_required: bool,
    pub source_prd_sha256: String,
    pub audit_task_id: String,
    pub remediation_cycle: usize,
    pub critical_findings: usize,
    pub major_findings: usize,
    pub minor_findings: usize,
}
```

---

## 52. Task execution specialization

Where task completion/phase routing is decided, add a narrow helper:

```rust
fn is_artifact_only_audit_task(task: &Task) -> bool
```

Criteria:

```text
category == implementation_audit
required_skill == macc-auditor
execution_mode == artifact_only
```

On success:

```text
verify no tracked source modifications
validate artifact
publish artifact
completion_kind = artifact_only
skip review/merge phases
terminalize task
```

Do not change normal task behavior.

---

## 53. Planner skill

Update the existing `macc-prd-planner` skill in its source repository.

Required additions:

1. understand injected specification-convergence policy;
2. emit exactly one final canonical implementation-audit task in `manual`/`auto` mode;
3. use `macc-auditor` as the required skill;
4. emit `artifact_only` metadata;
5. emit the fixed canonical output path;
6. in audit-remediation mode, preserve `source_findings`;
7. generate a new remediation lot rather than rewrite completed task IDs;
8. include a final audit task in each remediation PRD while convergence is enabled.

---

## 54. Existing `macc-auditor` skill

Do not replace it.

Verify/update only the output contract if necessary:

```text
fixed report path
machine-readable frontmatter
stable finding IDs
pass/action_required/blocked status
source PRD hash/task/cycle metadata
no source modifications
```

---

# Part XIX — Implementation sequence

## 55. Recommended phased implementation

### Phase 1 — Centralize PRD generation

**Goal:** one reusable generation operation.

Tasks:

1. introduce `PrdGenerationPurpose`;
2. introduce `PrdGenerateResult`;
3. implement `Engine::prd_generate()`;
4. move CLI generation orchestration into shared core/service;
5. migrate CLI to the shared method;
6. migrate Web/TUI callers where applicable;
7. preserve existing CLI behavior and tests.

**Exit criterion:** Coordinator can call PRD generation without spawning `macc prd generate`.

---

### Phase 2 — Add initial PRD generation to Coordinator workflow

Tasks:

1. add `CoordinatorPrdGenerationConfig`;
2. add modes `existing_only`, `generate_if_missing`, `always_generate`;
3. add `--prd-from` and `--prd-generation`;
4. resolve source paths from primary project root;
5. generate/validate/promote before task execution;
6. emit planning/workflow events;
7. preserve current behavior under default config;
8. integrate correctly with delayed `--in` / `--at` runs.

**Exit criterion:**

```bash
macc coordinator run --prd-from feature.md
```

can generate, validate, activate, and execute a PRD in one command.

---

### Phase 3 — Planner audit task contract

Tasks:

1. update `macc-prd-planner`;
2. inject workflow policy into generation prompts;
3. add final audit task generation;
4. add workflow-aware PRD validation;
5. add `required_skill`, `artifact_only`, and artifact output contract;
6. ensure one canonical report writer.

**Exit criterion:** convergence-enabled generated PRDs always contain a valid final `macc-auditor` task.

---

### Phase 4 — Artifact-only audit execution

Tasks:

1. add audit-task detection;
2. route performer prompt to `macc-auditor` explicitly;
3. allow successful no-commit artifact-only completion;
4. reject tracked source modifications;
5. validate worker audit report;
6. atomically publish to primary project `.macc/reports/implementation-audit.md`;
7. set `completion_kind = artifact_only`;
8. terminalize/cleanup using existing Coordinator mechanisms.

**Exit criterion:** a final audit task produces a canonical report visible to the primary Coordinator.

---

### Phase 5 — Manual convergence mode

Tasks:

1. add `spec_convergence.mode`;
2. implement `disabled` and `manual` behavior;
3. parse canonical report deterministically;
4. expose `completed_with_findings` workflow result;
5. implement blocked-report pause behavior.

**Exit criterion:** manual mode audits automatically but never generates fixes automatically.

---

### Phase 6 — Automatic remediation

Tasks:

1. add `AuditRemediation` PRD generation purpose;
2. use canonical report as generation input;
3. inject previous PRD/original-source context;
4. validate `source_findings` mapping;
5. generate and promote a new PRD;
6. restart existing Coordinator execution on the new PRD;
7. audit again after completion.

**Exit criterion:** one action-required audit can automatically produce and execute one remediation PRD.

---

### Phase 7 — Bounded convergence and recovery

Tasks:

1. track remediation cycle;
2. enforce `max_remediation_cycles`;
3. validate report cycle/PRD hashes;
4. protect against stale reports;
5. persist minimal workflow recovery metadata if needed;
6. add workflow-level status/events;
7. add pause/resume behavior for blocked/limit cases.

**Exit criterion:** no infinite remediation loop is possible.

---

### Phase 8 — TUI/Web UX

Only after core behavior is stable.

Expose:

```text
PRD generation mode
PRD source
spec convergence mode
current remediation cycle
last audit status
report link
outstanding findings
```

All UI actions must call the shared core operation.

---

# Part XX — Test strategy

## 56. PRD-generation unit tests

Test:

- `PrdGenerationPurpose::Standard` prompt;
- `AuditRemediation` prompt;
- workflow policy injection;
- tool resolution parity with current CLI;
- generated run directory behavior;
- validation failure propagation;
- promotion destination resolution;
- no duplicate client orchestration.

---

## 57. Configuration tests

Test defaults:

```text
existing_only
disabled
max_remediation_cycles = 2
```

Test invalid values.

Test CLI precedence over config.

Test `--prd` / `--prd-from` conflict.

Test `--prd-from` implicit generation behavior.

---

## 58. Planner output validation tests

For convergence-enabled generation:

- exactly one `implementation_audit` task;
- required skill is `macc-auditor`;
- task is artifact-only;
- fixed report path declared;
- audit dependency barrier valid;
- remediation tasks preserve source finding IDs.

---

## 59. Audit artifact tests

Test:

- missing report;
- invalid Markdown/frontmatter;
- unsupported schema version;
- unknown status;
- inconsistent `follow_up_required`;
- wrong PRD hash;
- wrong audit task ID;
- wrong remediation cycle;
- atomic publication;
- old canonical report not consumed after current audit failure;
- tracked source modification causes audit task failure;
- no-commit artifact-only task completes successfully.

---

## 60. Convergence state tests

### Disabled

```text
PRD complete → workflow complete
```

### Manual + pass

```text
PRD complete → audit pass → complete
```

### Manual + findings

```text
PRD complete → action_required → completed_with_findings
```

### Auto + pass

```text
PRD complete → pass → complete
```

### Auto + findings

```text
PRD complete → action_required → remediation PRD
```

### Auto + blocked

```text
PRD complete → blocked → pause
```

### Auto + cycle limit

```text
cycle N audit action_required
N == max
→ pause
```

---

## 61. End-to-end integration tests

### E2E-1 — Existing behavior unchanged

Config:

```yaml
prd_generation:
  mode: existing_only
spec_convergence:
  mode: disabled
```

Expected:

- existing PRD executes;
- no PRD generation;
- no audit report required.

### E2E-2 — Generate initial PRD only

```bash
macc coordinator run --prd-from brief.md
```

Expected:

```text
brief → generation → validation → promotion → Coordinator
```

### E2E-3 — Manual audit

Expected:

```text
brief → PRD → execution → audit → action_required → stop with findings
```

### E2E-4 — Automatic single remediation

Expected:

```text
brief
→ PRD #1
→ execution
→ AUD-001/AUD-002
→ remediation PRD #2
→ execution
→ audit pass
→ complete
```

### E2E-5 — Bounded failure to converge

Expected:

```text
initial audit action_required
→ remediation 1
→ audit action_required
→ remediation 2
→ audit action_required
→ pause, no remediation 3
```

### E2E-6 — Worker report publication

Expected:

```text
worker report exists
primary report absent
→ Coordinator validates
→ atomic publish
→ primary canonical report exists
```

### E2E-7 — Stale report protection

Expected:

- old canonical report exists before run;
- current audit fails before publication;
- workflow must not use old report.

### E2E-8 — Delayed start

```bash
macc coordinator run --in 1m --prd-from brief.md
```

Expected:

- no initial PRD is generated before scheduled start;
- at start, source and project state are re-read;
- generation/execution then proceeds normally.

---

# Part XXI — Acceptance criteria

## 62. Functional acceptance criteria

The feature is complete when all of the following are true.

1. `macc coordinator run` preserves existing behavior with default configuration.
2. Coordinator can generate an initial PRD through the same shared core operation as `macc prd generate`.
3. `macc coordinator run --prd-from <file>` performs generate → validate → activate → execute.
4. No subprocess call to `macc prd generate` is used internally.
5. Generated PRDs in convergence mode contain exactly one final canonical implementation-audit task.
6. The audit task explicitly uses `macc-auditor`.
7. The audit task is supported without requiring a git commit.
8. Audit tasks cannot silently modify/merge tracked source code.
9. A worker-local report is not treated as canonical until the Coordinator publishes it.
10. Canonical report path is exactly `.macc/reports/implementation-audit.md`.
11. The Coordinator validates report schema, PRD hash, task identity, and cycle before using it.
12. `disabled` mode requires no audit.
13. `manual` mode produces/uses the audit but never generates a remediation PRD automatically.
14. `auto` mode generates a remediation PRD only when `status=action_required`.
15. `blocked` never causes speculative remediation generation.
16. Remediation PRDs are new generation runs, not in-place edits of completed PRDs.
17. Remediation tasks preserve `source_findings` traceability.
18. Every remediation PRD includes a new final audit task while convergence remains enabled.
19. Automatic remediation cannot exceed `max_remediation_cycles`.
20. Stale reports cannot trigger new PRDs.
21. Generated PRD run history remains available under `.macc/generated/prd/`.
22. CLI, future TUI, future Web, and Coordinator use one PRD-generation core contract.
23. Delayed one-shot Coordinator runs remain compatible.
24. Workflow events make planning, execution, audit, remediation, and convergence visible.

---

# Part XXII — Recommended V1 configuration examples

## 63. Legacy/current behavior

```yaml
automation:
  coordinator:
    prd_generation:
      mode: existing_only

    spec_convergence:
      mode: disabled
      max_remediation_cycles: 2
      on_blocked: pause
```

---

## 64. Automatic initial planning, no audit loop

```yaml
automation:
  coordinator:
    prd_generation:
      mode: generate_if_missing
      source: specs/feature.md

    spec_convergence:
      mode: disabled
```

Workflow:

```text
spec → PRD → execution → done
```

---

## 65. Automatic planning + manual final conformity check

```yaml
automation:
  coordinator:
    prd_generation:
      mode: generate_if_missing
      source: specs/feature.md

    spec_convergence:
      mode: manual
      max_remediation_cycles: 2
      on_blocked: pause
```

Workflow:

```text
spec → PRD → execution → macc-auditor → report → operator decides
```

---

## 66. Full bounded automatic convergence

```yaml
automation:
  coordinator:
    prd_generation:
      mode: generate_if_missing
      source: specs/feature.md

    spec_convergence:
      mode: auto
      max_remediation_cycles: 2
      on_blocked: pause
```

Workflow:

```text
spec
→ PRD
→ implementation/testing/review
→ macc-auditor
→ remediation PRD if needed
→ implementation/testing/review
→ macc-auditor
→ PASS or bounded pause
```

---

# Part XXIII — Design decisions to keep

## 67. Final recommended decisions

The implementation should explicitly preserve these decisions.

### Decision 1 — PRD generation is part of the Coordinator workflow, not a separate automation system

Coordinator orchestrates the existing PRD-generation service.

### Decision 2 — `macc-prd-planner` remains the only built-in PRD planning skill

No new remediation planner skill is required.

### Decision 3 — `macc-auditor` is the canonical audit skill

No second implementation-auditor skill is introduced.

### Decision 4 — One canonical final audit report in V1

```text
.macc/reports/implementation-audit.md
```

This avoids report-discovery complexity and concurrent writer races.

### Decision 5 — Audit task is artifact-only

It produces operational evidence, not source changes.

### Decision 6 — Coordinator owns publication of runtime artifacts

Workers never rely on Git to propagate `.macc` reports between worktrees.

### Decision 7 — Report decision is deterministic

Frontmatter controls workflow branching; a new LLM call is not used merely to classify the report.

### Decision 8 — Remediation uses the existing PRD-generation path

The audit report becomes the primary remediation input to `macc-prd-planner`.

### Decision 9 — Completed PRDs are immutable workflow history

Remediation creates a new PRD generation run.

### Decision 10 — Convergence is optional and bounded

Backward-compatible defaults remain non-automatic.

---

# Part XXIV — Definition of done

## 68. Definition of done

This initiative is considered complete when MACC can reliably execute the following command:

```bash
macc coordinator run \
  --prd-from specs/feature.md \
  --spec-convergence auto
```

and produce a safe workflow equivalent to:

```text
1. Read the current source specification.
2. Generate a PRD with macc-prd-planner.
3. Validate the PRD.
4. Activate/promote it.
5. Execute it using the existing Coordinator.
6. Run its final artifact-only audit task using macc-auditor.
7. Publish .macc/reports/implementation-audit.md to the primary project.
8. Validate the report against the current PRD/task/cycle.
9. If PASS: complete the workflow.
10. If ACTION_REQUIRED and cycles remain:
    a. generate a new remediation PRD from the report;
    b. preserve finding traceability;
    c. validate/activate the new PRD;
    d. execute it;
    e. audit again.
11. If BLOCKED: pause.
12. If the remediation limit is reached: pause with outstanding findings.
13. Never use a stale report.
14. Never create an unbounded loop.
15. Preserve generated PRD history and existing Coordinator recovery/observability behavior.
```

The resulting capability turns MACC's existing PRD generator and Coordinator into a configurable end-to-end development workflow without introducing a second planning engine, a second audit system, or an unnecessarily complex workflow platform.


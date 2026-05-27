# MACC Reference Branch Preflight Gate Specification

> **Document status:** Proposed modification for MACC v1  
> **Document language:** English  
> **Feature area:** Coordinator, Git safety, Worktrees, TUI/Web operations  
> **Primary command affected:** `macc coordinator run` / `macc coordinator`  
> **Related modules:** coordinator runtime, Git wrapper, configuration, TUI Automation screen, Web Coordinator Console

---

## 1. Executive summary

This document defines a new **Reference Branch Preflight Gate** for MACC.

Before MACC starts a coordinator run, it must verify that the user-designated `reference_branch` is safe to use as the base branch for task dispatch, worktree reuse, PRD reconciliation, and final merge operations.

The gate performs two mandatory checks:

1. **Reference branch existence check**  
   MACC verifies that the configured reference branch exists locally. If it does not exist, MACC notifies the user and offers to create it. If the user declines, the coordinator run is cancelled before any mutation occurs.

2. **Reference branch dirty-state check**  
   MACC inspects any worktree where the reference branch is currently checked out. If staged, unstaged, or untracked changes are found, MACC notifies the user and blocks by default unless the user explicitly chooses to continue.

This prevents the coordinator from starting on an invalid or unsafe base branch and reduces the risk of accidental merges, confusing reconciliation, lost local work, and unstable task dispatch.

---

## 2. Background and rationale

MACC already uses a `reference_branch` in coordinator operations. The branch is used as the base for task selection, PRD reconciliation from commit history, worktree setup, and local merge workflows.

The current coordinator model includes:

- `macc coordinator` running full-cycle mode by default.
- `macc coordinator [run|dispatch|advance|resume|sync|sync-prd|audit-prd|status|reconcile|unlock|cleanup]`.
- `reference_branch` resolved from environment/config/defaults.
- `sync-prd` scanning commits on the reference branch to mark completed tasks as merged.
- Worktree pool reuse based on compatible base branch and clean worktree state.
- Local fallback merges into the base branch when automation is enabled.

Because of this, the reference branch is a critical safety boundary. If it is missing, dirty, checked out in an unexpected location, or only present remotely, the coordinator should not silently proceed.

### 2.1 Problems solved

This change addresses the following failure modes:

| Failure mode | Impact | Preflight mitigation |
|---|---|---|
| Config references a non-existent branch | Coordinator may fail later after partial setup | Detect before run and offer branch creation |
| Reference branch exists remotely but not locally | User may expect it to work, but local Git commands fail | Offer local tracking branch creation |
| Reference branch has uncommitted changes | Local work may be overwritten, conflicts become harder to diagnose | Block by default and show changed files |
| Reference branch is checked out in another worktree | Dirty state may be missed if only current directory is inspected | Inspect all worktrees on the reference branch |
| Coordinator starts before safety checks | Registry/worktree mutations can happen before failure | Run preflight before any coordinator mutation |
| CI/non-interactive runs prompt unexpectedly | Automation can hang | Fail fast unless explicit override flags are provided |

---

## 3. Goals

The Reference Branch Preflight Gate must:

1. Run before `macc coordinator run` performs any state-changing operation.
2. Resolve the same `reference_branch` used by the coordinator runtime.
3. Verify that the local branch exists.
4. Detect whether a matching remote-tracking branch exists when the local branch is missing.
5. Offer interactive branch creation when safe and appropriate.
6. Cancel the coordinator run if the branch is missing and the user declines creation.
7. Inspect all Git worktrees where the reference branch is checked out.
8. Detect staged, unstaged, and optionally untracked changes.
9. Warn and block by default when the reference branch is dirty.
10. Provide explicit non-interactive overrides for automation.
11. Surface the same preflight state in CLI, TUI, and Web UI.
12. Produce structured logs and error codes.

---

## 4. Non-goals

This feature does **not** attempt to:

- Automatically commit, stash, or discard user changes.
- Guess the correct reference branch when the configured branch is wrong.
- Rewrite Git history.
- Replace existing `macc doctor` diagnostics.
- Prevent all possible merge conflicts.
- Validate remote branch freshness unless explicitly added in a later milestone.
- Enforce a specific branching strategy such as GitFlow or trunk-based development.

---

## 5. Terminology

| Term | Meaning |
|---|---|
| `reference_branch` | The branch configured as the coordinator's base branch, usually `main`. |
| Local branch | A branch under `refs/heads/<name>`. |
| Remote-tracking branch | A branch such as `origin/<name>` under `refs/remotes/origin/<name>`. |
| Dirty worktree | A worktree with staged, unstaged, or untracked changes. |
| Preflight gate | A mandatory validation step that must pass before the coordinator run starts. |
| Interactive mode | A CLI/TUI/Web context where MACC can ask the user for a decision. |
| Non-interactive mode | A context such as CI where MACC must never prompt. |
| Mutation | Any write to registry, PRD, worktrees, branches, logs beyond preflight logs, or coordinator runtime state. |

---

## 6. Current MACC integration points

The preflight gate integrates with these existing MACC areas:

### 6.1 Coordinator command

The coordinator command currently supports full-cycle execution and subcommands such as `run`, `dispatch`, `advance`, `resume`, `sync`, `sync-prd`, `audit-prd`, `status`, `reconcile`, `unlock`, and `cleanup`.

The preflight gate should be invoked by:

```bash
macc coordinator
macc coordinator run
```

Recommended extended protection:

```bash
macc coordinator dispatch
macc coordinator advance
macc coordinator resume
```

The minimal MVP can guard only `run`, but the final design should guard all coordinator actions that can dispatch tasks, create task branches, or merge into the reference branch.

### 6.2 Coordinator settings

Coordinator settings are persisted under:

```yaml
automation:
  coordinator:
    reference_branch: main
```

The preflight gate must use exactly the same resolution logic as the coordinator runtime.

### 6.3 Worktree pool mode

Coordinator worktrees are reused as worker slots. Compatibility checks already include selected tool, base branch, optional task scope, clean worktree state, and active task binding.

The new preflight gate adds a separate safety check for the **reference branch itself**, not only the worker worktree slots.

### 6.4 PRD reconciliation

`macc coordinator sync-prd` scans commits on `reference_branch` and transitions matching tasks to `merged`. If the reference branch is missing or misconfigured, reconciliation cannot be trusted. Therefore the preflight gate should run before `sync-prd` when it is invoked as part of the full coordinator run.

---

## 7. Functional specification

### 7.1 Reference branch resolution

MACC resolves the reference branch in this order:

1. CLI override, if provided:
   ```bash
   macc coordinator run --reference-branch integration
   ```
2. Environment override, if already supported by the coordinator:
   ```bash
   MACC_REFERENCE_BRANCH=integration
   ```
3. Project config:
   ```yaml
   automation:
     coordinator:
       reference_branch: integration
   ```
4. Built-in default:
   ```text
   main
   ```

The result is a single branch name string, for example:

```text
main
integration
release/v1
```

### 7.2 Branch name validation

Before passing the name to Git, MACC must validate it using Git itself:

```bash
git check-ref-format --branch <reference_branch>
```

If validation fails, MACC returns a structured error:

```text
E706 Invalid reference branch name
```

The coordinator must not continue.

### 7.3 Local branch existence check

MACC checks whether the local branch exists:

```bash
git show-ref --verify --quiet refs/heads/<reference_branch>
```

Outcomes:

| Result | Behavior |
|---|---|
| Exists | Continue to dirty-state check |
| Missing | Check remote-tracking branches |
| Git failure | Return `E703` or `E705` depending on context |

### 7.4 Remote-tracking branch detection

If the local branch is missing, MACC checks whether a remote-tracking branch exists:

```bash
git show-ref --verify --quiet refs/remotes/origin/<reference_branch>
```

Future enhancement: support multiple remotes by scanning:

```bash
git for-each-ref --format='%(refname:short)' refs/remotes
```

MVP behavior may prioritize `origin/<reference_branch>` only.

### 7.5 Missing branch behavior

If the local branch does not exist, MACC must notify the user before starting the coordinator.

Interactive choices:

```text
Reference branch "integration" does not exist locally.

Options:
  [1] Create from current HEAD
  [2] Create from remote-tracking branch origin/integration
  [3] Create from another existing local branch
  [4] Edit coordinator.reference_branch
  [5] Cancel
```

Rules:

- Option 2 is shown only when a matching remote-tracking branch exists.
- Option 3 prompts for an existing local branch name and validates it.
- Option 4 can open the config editor in TUI/Web or show instructions in CLI.
- Option 5 cancels without mutation.

### 7.6 Branch creation rules

MACC can create the missing reference branch from one of these sources:

| Source | Command equivalent |
|---|---|
| Current HEAD | `git branch <reference_branch> HEAD` |
| Existing local branch | `git branch <reference_branch> <base_branch>` |
| Remote-tracking branch | `git branch --track <reference_branch> origin/<reference_branch>` |

Branch creation must be treated as a **caution-level write action**.

It should be allowed only after explicit user confirmation in interactive mode.

### 7.7 Non-interactive missing branch behavior

In non-interactive mode, MACC must fail fast by default.

Default:

```text
ERROR E701: Reference branch "integration" does not exist locally.
Use --create-reference-branch with --reference-branch-base to create it non-interactively.
```

Explicit override:

```bash
macc coordinator run \
  --create-reference-branch \
  --reference-branch-base main
```

If a remote-tracking branch exists and the user wants to create a local tracking branch:

```bash
macc coordinator run \
  --create-reference-branch \
  --reference-branch-base origin/integration \
  --track-reference-branch
```

MVP may simplify this to:

```bash
macc coordinator run --create-reference-branch --reference-branch-base main
```

### 7.8 Dirty-state check

After confirming the branch exists, MACC inspects all worktrees where the reference branch is checked out.

First, list worktrees:

```bash
git worktree list --porcelain
```

Then match entries where the branch is:

```text
branch refs/heads/<reference_branch>
```

For each matching worktree path, run:

```bash
git -C <worktree_path> status --porcelain=v1 --untracked-files=all
```

Dirty entries include:

- staged modifications
- unstaged modifications
- staged additions
- staged deletions
- unstaged deletions
- renames
- copies
- unmerged/conflict entries
- untracked files, when enabled

### 7.9 Untracked file policy

Default policy:

```yaml
include_untracked: true
```

Rationale: untracked files can still matter during coordinator execution, especially generated files, scratch patches, local notes, or files that a user expects to keep.

Optional stricter/flexible policies can be added:

```yaml
untracked_policy: block # block | warn | ignore
```

MVP can use a boolean:

```yaml
include_untracked: true
```

### 7.10 Dirty reference branch behavior

If dirty entries are found, MACC must notify the user.

Default interactive CLI output:

```text
Reference branch "main" has uncommitted changes in:
/path/to/project

Changes:
  M src/app/page.tsx
  A docs/notes.md
  ?? tmp/debug-output.txt

Running the coordinator may later merge task branches into this branch.
Please commit, stash, or discard these changes before continuing.

Options:
  [1] Cancel
  [2] Show full git status
  [3] Continue once
```

Default selected action should be **Cancel**.

### 7.11 Non-interactive dirty branch behavior

In non-interactive mode, dirty reference branches must fail by default:

```text
ERROR E702: Reference branch "main" has uncommitted changes.
Commit, stash, discard, or rerun with --allow-dirty-reference.
```

Explicit override:

```bash
macc coordinator run --allow-dirty-reference
```

This override must be logged.

### 7.12 Worktree not checked out case

Git uncommitted changes are attached to a worktree, not directly to a branch ref.

If the reference branch exists but is not checked out in any worktree:

- There is no working tree to inspect for that branch.
- The dirty-state check passes.
- MACC should optionally log:

```text
Reference branch "main" is not checked out in any worktree; dirty-state check skipped.
```

This is not an error.

### 7.13 Multiple worktrees on the same reference branch

Git generally prevents the same branch from being checked out in multiple worktrees unless special operations are used. However, MACC should not assume this is impossible.

If multiple worktrees report the same branch:

- Inspect all of them.
- If any one is dirty, the preflight fails or warns according to policy.
- Include each dirty worktree in the report.

### 7.14 Submodules

MVP behavior:

- Do not recurse into submodules by default.
- If the parent repository reports submodule changes via porcelain status, show them as normal status entries.

Future option:

```yaml
submodule_policy: parent_status_only # parent_status_only | recurse | ignore
```

### 7.15 Bare repositories

If MACC is run in a bare repository, coordinator operations that require worktrees should fail through existing worktree/doctor checks.

Preflight should return:

```text
E707 Bare repository unsupported for coordinator run
```

unless MACC later adds explicit bare-repo support.

---

## 8. User-facing CLI design

### 8.1 Clean branch

```text
Reference branch: main
Preflight: OK
Starting coordinator run...
```

### 8.2 Missing branch, no remote

```text
Reference branch "integration" does not exist locally.

Create it now?
  [1] Create from current HEAD
  [2] Create from another existing branch
  [3] Edit coordinator.reference_branch
  [4] Cancel

Selection:
```

If the user cancels:

```text
Coordinator cancelled: reference branch "integration" does not exist.
```

### 8.3 Missing branch, matching remote exists

```text
Reference branch "integration" does not exist locally, but "origin/integration" exists.

Create local tracking branch "integration" from "origin/integration"?
  [Y] Yes
  [N] No, cancel
```

If accepted:

```text
Created local tracking branch "integration" from "origin/integration".
Preflight: OK
Starting coordinator run...
```

### 8.4 Dirty branch

```text
Reference branch "main" has uncommitted changes in:
/Users/alex/project

Changes:
  M src/app/page.tsx
  A docs/notes.md
  ?? tmp/debug-output.txt

Running the coordinator may later merge task branches into this branch.
Please commit, stash, or discard these changes before continuing.

Options:
  [1] Cancel
  [2] Show full git status
  [3] Continue once
```

### 8.5 Dirty branch with explicit override

```bash
macc coordinator run --allow-dirty-reference
```

Output:

```text
WARNING: Reference branch "main" has uncommitted changes.
Override accepted because --allow-dirty-reference was provided.
Starting coordinator run...
```

### 8.6 Suggested CLI flags

| Flag | Description |
|---|---|
| `--reference-branch <branch>` | Override configured reference branch for this run. |
| `--create-reference-branch` | Allow MACC to create the reference branch if missing. |
| `--reference-branch-base <branch-or-rev>` | Base used when creating the missing reference branch. |
| `--track-reference-branch` | Create a local tracking branch from a remote-tracking branch. |
| `--allow-dirty-reference` | Allow coordinator run even when reference branch worktree is dirty. |
| `--preflight-only` | Run preflight checks and exit without starting the coordinator. |
| `--no-reference-preflight` | Disable the gate. Dangerous; should require explicit config or debug build depending on policy. |

Recommended MVP flags:

```text
--create-reference-branch
--reference-branch-base <branch-or-rev>
--allow-dirty-reference
--preflight-only
```

---

## 9. Configuration schema

### 9.1 Minimal MVP schema

```yaml
automation:
  coordinator:
    reference_branch: main
    require_clean_reference_branch: true
```

Semantics:

| Field | Type | Default | Meaning |
|---|---|---:|---|
| `reference_branch` | string | `main` | Coordinator base branch. |
| `require_clean_reference_branch` | bool | `true` | Block coordinator run if checked-out reference branch is dirty. |

### 9.2 Recommended full schema

```yaml
automation:
  coordinator:
    reference_branch: main
    reference_branch_preflight:
      enabled: true
      missing_branch_policy: prompt
      dirty_policy: block
      include_untracked: true
      create_from: remote_tracking_or_current_head
      allow_non_interactive_create: false
      log_clean_result: true
```

### 9.3 Field definitions

| Field | Type | Default | Allowed values | Description |
|---|---|---:|---|---|
| `enabled` | bool | `true` | `true`, `false` | Enables/disables reference branch preflight. |
| `missing_branch_policy` | enum | `prompt` | `prompt`, `fail`, `create` | Behavior when local branch does not exist. |
| `dirty_policy` | enum | `block` | `block`, `warn`, `allow` | Behavior when reference branch worktree is dirty. |
| `include_untracked` | bool | `true` | `true`, `false` | Whether untracked files count as dirty. |
| `create_from` | enum/string | `remote_tracking_or_current_head` | See below | Default source for creating missing branch. |
| `allow_non_interactive_create` | bool | `false` | `true`, `false` | Whether config alone can create branches in non-interactive mode. |
| `log_clean_result` | bool | `true` | `true`, `false` | Whether to log successful preflight details. |

### 9.4 `create_from` values

Supported values:

| Value | Meaning |
|---|---|
| `current_head` | Create branch from current `HEAD`. |
| `remote_tracking` | Create local tracking branch from matching remote branch. |
| `remote_tracking_or_current_head` | Prefer remote-tracking branch; otherwise current `HEAD`. |
| `branch:<name>` | Create from a specific existing local branch. |
| `rev:<sha-or-ref>` | Create from a specific Git revision. |

MVP can support only:

```text
current_head
remote_tracking
branch:<name>
```

### 9.5 Configuration precedence

Runtime behavior should resolve as:

1. CLI flags.
2. Environment variables, if supported.
3. Project config.
4. Built-in defaults.

Example:

```bash
macc coordinator run --allow-dirty-reference
```

This overrides:

```yaml
reference_branch_preflight:
  dirty_policy: block
```

for one run only.

---

## 10. Core Rust design

### 10.1 New module

Add:

```text
core/src/coordinator/preflight.rs
```

This module should contain pure coordinator preflight logic and should not render prompts directly.

### 10.2 Responsibilities

The module owns:

- resolving preflight config
- validating branch names
- checking local branch existence
- detecting remote-tracking branches
- listing worktrees for the reference branch
- collecting porcelain status entries
- building a structured preflight report
- returning decision requirements to CLI/TUI/Web layers

The module does **not** own:

- terminal prompts
- TUI rendering
- Web modal rendering
- branch creation confirmation UI
- long-running coordinator loop

### 10.3 Data structures

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceBranchPreflightReport {
    pub reference_branch: String,
    pub branch_exists: bool,
    pub remote_tracking_branches: Vec<String>,
    pub checked_out_worktrees: Vec<ReferenceWorktreeStatus>,
    pub status: ReferencePreflightStatus,
    pub recommended_action: ReferencePreflightAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceWorktreeStatus {
    pub path: PathBuf,
    pub branch: String,
    pub dirty_entries: Vec<GitStatusEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatusEntry {
    pub index_status: char,
    pub worktree_status: char,
    pub path: String,
    pub original_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferencePreflightStatus {
    Clean,
    BranchMissing,
    Dirty,
    InvalidBranchName,
    GitInspectionFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferencePreflightAction {
    Proceed,
    PromptCreateBranch,
    PromptCleanOrOverride,
    Fail,
}
```

### 10.4 Policy structures

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceBranchPreflightConfig {
    pub enabled: bool,
    pub missing_branch_policy: MissingBranchPolicy,
    pub dirty_policy: DirtyReferencePolicy,
    pub include_untracked: bool,
    pub create_from: BranchCreateSourcePolicy,
    pub allow_non_interactive_create: bool,
    pub log_clean_result: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissingBranchPolicy {
    Prompt,
    Fail,
    Create,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirtyReferencePolicy {
    Block,
    Warn,
    Allow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchCreateSourcePolicy {
    CurrentHead,
    RemoteTracking,
    RemoteTrackingOrCurrentHead,
    LocalBranch(String),
    Revision(String),
}
```

### 10.5 Public API

```rust
pub fn inspect_reference_branch_preflight(
    repo_root: &Path,
    reference_branch: &str,
    config: &ReferenceBranchPreflightConfig,
) -> Result<ReferenceBranchPreflightReport, PreflightError>;

pub fn create_reference_branch(
    repo_root: &Path,
    reference_branch: &str,
    source: BranchCreateSource,
) -> Result<(), PreflightError>;
```

### 10.6 Internal helper functions

```rust
fn validate_branch_name(repo_root: &Path, branch: &str) -> Result<(), PreflightError>;

fn local_branch_exists(repo_root: &Path, branch: &str) -> Result<bool, PreflightError>;

fn remote_tracking_branches(repo_root: &Path, branch: &str) -> Result<Vec<String>, PreflightError>;

fn worktrees_for_branch(repo_root: &Path, branch: &str) -> Result<Vec<PathBuf>, PreflightError>;

fn porcelain_status(
    worktree_path: &Path,
    include_untracked: bool,
) -> Result<Vec<GitStatusEntry>, PreflightError>;
```

### 10.7 Git command wrapper integration

If `core/src/git.rs` already owns Git CLI invocation, place low-level commands there and keep `preflight.rs` as a business-logic module.

Suggested additions to `git.rs`:

```rust
pub fn check_ref_format_branch(repo_root: &Path, branch: &str) -> GitResult<()>;

pub fn branch_exists(repo_root: &Path, branch: &str) -> GitResult<bool>;

pub fn remote_branch_exists(repo_root: &Path, remote_branch: &str) -> GitResult<bool>;

pub fn list_worktrees_porcelain(repo_root: &Path) -> GitResult<Vec<GitWorktree>>;

pub fn status_porcelain(
    repo_root: &Path,
    include_untracked: bool,
) -> GitResult<Vec<GitStatusEntry>>;

pub fn create_branch(repo_root: &Path, branch: &str, start_point: &str) -> GitResult<()>;

pub fn create_tracking_branch(
    repo_root: &Path,
    branch: &str,
    remote_branch: &str,
) -> GitResult<()>;
```

---

## 11. Coordinator integration

### 11.1 Execution order

The coordinator run should execute in this order:

```text
1. Parse CLI arguments
2. Load project config
3. Resolve coordinator settings
4. Resolve reference branch
5. Run reference branch preflight
6. If preflight passes, start coordinator sync
7. sync-prd
8. dispatch
9. advance
10. reconcile
11. cleanup
12. repeat until convergence
```

Preflight must happen before:

- PRD registry writes
- task state transitions
- worktree creation
- performer launch
- merge attempts
- cleanup that depends on reference branch

### 11.2 Full-cycle run pseudocode

```rust
pub async fn run_coordinator(args: CoordinatorRunArgs) -> Result<()> {
    let repo_root = discover_repo_root()?;
    let config = load_project_config(&repo_root)?;
    let coordinator_config = resolve_coordinator_config(&config, &args)?;

    if coordinator_config.reference_branch_preflight.enabled {
        let report = inspect_reference_branch_preflight(
            &repo_root,
            &coordinator_config.reference_branch,
            &coordinator_config.reference_branch_preflight,
        )?;

        handle_preflight_report_or_exit(report, &args)?;
    }

    run_full_cycle_loop(repo_root, coordinator_config).await
}
```

### 11.3 Interactive decision handling

CLI/TUI/Web layer should convert reports to user decisions:

```rust
pub enum UserPreflightDecision {
    Proceed,
    CreateBranch { source: BranchCreateSource },
    Cancel,
    ContinueWithDirtyReference,
}
```

Only after a positive decision should the coordinator continue.

### 11.4 Guarded coordinator subcommands

Recommended guard matrix:

| Subcommand | Preflight required? | Reason |
|---|---:|---|
| `run` | Yes | Full-cycle mutation path |
| `dispatch` | Yes | Creates or reuses worktrees from reference branch |
| `advance` | Yes | May merge task branches |
| `resume` | Yes | Re-enters mutation path |
| `sync` | Recommended | Uses reference branch state |
| `sync-prd` | Recommended | Reads commits on reference branch |
| `audit-prd` | Recommended | Reads commits on reference branch |
| `status` | No | Read-only |
| `reconcile` | Optional | Depends on implementation |
| `unlock` | No, unless it resumes work | Operational recovery |
| `cleanup` | No, unless branch deletion/merge state depends on reference | Cleanup only |

MVP:

```text
run: required
all other commands: warning or future extension
```

Final design:

```text
run, dispatch, advance, resume: required
sync, sync-prd, audit-prd: existence check required, dirty check optional
```

---

## 12. TUI design

### 12.1 Automation / Coordinator screen

Add a dedicated section:

```text
Reference Branch Preflight
──────────────────────────
Reference branch: main
Branch status: Exists
Working tree status: Clean
Preflight: Enabled
Missing branch policy: Prompt
Dirty policy: Block
Include untracked: Yes
```

### 12.2 Missing branch state

```text
Reference Branch Preflight
──────────────────────────
Reference branch: integration
Branch status: Missing
Remote match: origin/integration

Actions:
[Create tracking branch] [Choose another branch] [Edit config] [Cancel]
```

### 12.3 Dirty branch state

```text
Reference Branch Preflight
──────────────────────────
Reference branch: main
Status: Dirty
Worktree: /Users/alex/project

Changed files:
M  src/app/page.tsx
A  docs/notes.md
?? tmp/debug-output.txt

Actions:
[Show full status] [Cancel run] [Continue once]
```

### 12.4 TUI behavior

- The TUI must use the same core preflight report as CLI and Web.
- It must not duplicate Git inspection logic.
- It should allow the user to edit `automation.coordinator.reference_branch` from the same screen.
- It should expose `require_clean_reference_branch` in MVP.
- It should expose the full `reference_branch_preflight` settings in a later release.

---

## 13. Web UI design

### 13.1 Coordinator Console card

Add a preflight card to `/ops/console`:

```text
Reference Branch Preflight
Status: Failed
Reference branch: main
Issue: Uncommitted changes detected

3 changed files in /repo

[View git status] [Cancel] [Continue once]
```

### 13.2 Settings page

Add settings to `/config/settings` under the Coordinator tab:

```text
Reference branch: main
Preflight enabled: on
Missing branch policy: prompt
Dirty branch policy: block
Include untracked files: on
```

### 13.3 Web consent model

Branch creation is a **Caution** action.

Continuing with a dirty reference branch is also **Caution**, or **Dangerous** if merge automation is enabled.

Recommended confirmation copy:

```text
You are about to run the coordinator while the reference branch has uncommitted changes.
This may make merges harder to recover from. Continue for this run only?
```

---

## 14. Web API design

### 14.1 New endpoint

```http
POST /api/v1/coordinator/preflight
```

Request:

```json
{
  "reference_branch": "main",
  "include_untracked": true
}
```

Response, clean:

```json
{
  "reference_branch": "main",
  "branch_exists": true,
  "remote_tracking_branches": [],
  "status": "clean",
  "checked_out_worktrees": [
    {
      "path": "/repo",
      "branch": "main",
      "dirty_entries": []
    }
  ],
  "recommended_action": "proceed"
}
```

Response, missing branch:

```json
{
  "reference_branch": "integration",
  "branch_exists": false,
  "remote_tracking_branches": ["origin/integration"],
  "status": "branch_missing",
  "checked_out_worktrees": [],
  "recommended_action": "prompt_create_branch"
}
```

Response, dirty branch:

```json
{
  "reference_branch": "main",
  "branch_exists": true,
  "remote_tracking_branches": [],
  "status": "dirty",
  "checked_out_worktrees": [
    {
      "path": "/repo",
      "branch": "main",
      "dirty_entries": [
        {
          "index_status": " ",
          "worktree_status": "M",
          "path": "src/app/page.tsx",
          "original_path": null
        },
        {
          "index_status": "?",
          "worktree_status": "?",
          "path": "tmp/debug-output.txt",
          "original_path": null
        }
      ]
    }
  ],
  "recommended_action": "prompt_clean_or_override"
}
```

### 14.2 Branch creation endpoint

```http
POST /api/v1/git/branches
```

Request:

```json
{
  "branch": "integration",
  "source": {
    "type": "remote_tracking",
    "value": "origin/integration"
  }
}
```

Response:

```json
{
  "branch": "integration",
  "created": true,
  "source": "origin/integration"
}
```

Alternative: keep branch creation under coordinator preflight:

```http
POST /api/v1/coordinator/preflight/create-reference-branch
```

This is simpler for MVP.

### 14.3 Running coordinator with preflight override

Existing endpoint:

```http
POST /api/v1/coordinator/run
```

Request addition:

```json
{
  "allow_dirty_reference": true,
  "create_reference_branch": false
}
```

### 14.4 Error model integration

All Web errors must use the existing structured envelope:

```json
{
  "error": {
    "code": "MACC-WEB-3002",
    "category": "conflict",
    "message": "Reference branch \"main\" has uncommitted changes.",
    "retryable": false,
    "recommended_action": "Commit, stash, discard, or run with explicit override."
  }
}
```

---

## 15. Error codes

Add a new Git/preflight error range.

| Code | Name | Retryable | User action |
|---|---|---:|---|
| `E701` | Reference branch not found | No | Create branch or update config |
| `E702` | Reference branch dirty | No | Commit, stash, discard, or override |
| `E703` | Reference branch inspection failed | No | Run `macc doctor`, inspect Git state |
| `E704` | Reference branch creation declined | No | Re-run and choose/create a branch |
| `E705` | Reference branch creation failed | No | Fix Git error and retry |
| `E706` | Invalid reference branch name | No | Correct `automation.coordinator.reference_branch` |
| `E707` | Bare repository unsupported | No | Run MACC in a normal worktree |

### 15.1 Mapping to existing error categories

Suggested mapping:

| E-code | Category |
|---|---|
| `E701` | Coordinator/Registry or new Git/Preflight |
| `E702` | Git/Preflight conflict |
| `E703` | Worktree/FS or Git/Preflight |
| `E704` | User cancellation |
| `E705` | Git command failure |
| `E706` | Validation |
| `E707` | Environment unsupported |

Preferred long-term category:

```text
E700 Git/Preflight
```

---

## 16. Logging and auditability

### 16.1 Coordinator log

Preflight results should be logged to:

```text
.macc/log/coordinator/preflight-<timestamp>.json
```

or included in the current coordinator run log.

Example:

```json
{
  "timestamp": "2026-05-25T20:15:00Z",
  "event": "reference_branch_preflight",
  "reference_branch": "main",
  "branch_exists": true,
  "status": "dirty",
  "dirty_worktrees": [
    {
      "path": "/repo",
      "entries": [
        { "status": " M", "path": "src/app/page.tsx" }
      ]
    }
  ],
  "decision": "cancelled",
  "override_used": false
}
```

### 16.2 Ops audit log

For Web UI mutating actions:

- branch creation must be logged to `.macc/log/ops.jsonl`
- dirty override must be logged
- preflight-only read should not be considered mutating

Example ops event:

```json
{
  "timestamp": "2026-05-25T20:16:00Z",
  "method": "POST",
  "path": "/api/v1/coordinator/run",
  "status_code": 200,
  "duration_ms": 42,
  "preflight_override": "allow_dirty_reference"
}
```

---

## 17. Security and safety considerations

### 17.1 No automatic stash

MACC must not automatically run:

```bash
git stash
git reset --hard
git clean -fd
```

These operations can lose or hide user work. MACC may suggest them but must not perform them automatically in MVP.

### 17.2 Branch creation is explicit

Creating a branch changes repository state. It must require explicit confirmation in interactive mode or explicit flags in non-interactive mode.

### 17.3 Shell argument safety

All Git commands must pass branch names as separate process arguments, never interpolated into shell strings.

Correct:

```rust
Command::new("git")
    .args(["show-ref", "--verify", "--quiet"])
    .arg(format!("refs/heads/{branch}"));
```

Avoid:

```rust
Command::new("sh")
    .arg("-c")
    .arg(format!("git show-ref refs/heads/{branch}"));
```

### 17.4 Path safety

Worktree paths returned by Git should still be normalized and checked where relevant.

### 17.5 Web security

The Web UI must preserve MACC's existing security boundaries:

- localhost binding by default
- no path traversal
- mutating requests audit-logged
- branch creation confirmation
- no secret exposure in logs

---

## 18. Testing strategy

### 18.1 Unit tests

Test pure parsing and decision logic:

1. Branch exists and no worktree is checked out.
2. Branch exists and checked-out worktree is clean.
3. Branch exists and checked-out worktree has unstaged changes.
4. Branch exists and checked-out worktree has staged changes.
5. Branch exists and checked-out worktree has untracked files.
6. Branch missing and no remote exists.
7. Branch missing and `origin/<branch>` exists.
8. Invalid branch name.
9. Dirty policy `block`.
10. Dirty policy `warn`.
11. Dirty policy `allow`.
12. Missing branch policy `fail`.
13. Missing branch policy `create`.
14. Non-interactive mode with missing branch.
15. Non-interactive mode with dirty branch.

### 18.2 Integration tests

Create temporary Git repositories and execute actual Git commands.

#### Test: clean branch

```bash
git init repo
cd repo
echo hello > README.md
git add README.md
git commit -m "chore: init"
macc coordinator run --preflight-only
```

Expected:

```text
Preflight: OK
```

#### Test: missing branch

```bash
git init repo
cd repo
git checkout -b main
git commit --allow-empty -m "chore: init"
macc coordinator run --reference-branch integration --preflight-only
```

Expected:

```text
E701
```

#### Test: dirty branch

```bash
git init repo
cd repo
git checkout -b main
git commit --allow-empty -m "chore: init"
echo change > file.txt
macc coordinator run --preflight-only
```

Expected:

```text
E702
```

#### Test: remote tracking branch

```bash
git init --bare remote.git
git clone remote.git repo
cd repo
git checkout -b integration
git commit --allow-empty -m "chore: init"
git push -u origin integration
git checkout -b main
git branch -D integration
macc coordinator run --reference-branch integration --preflight-only
```

Expected:

```text
Local branch missing, remote origin/integration found
```

### 18.3 TUI tests

- Snapshot test for clean state card.
- Snapshot test for missing branch card.
- Snapshot test for dirty state card.
- Input handling test for cancel.
- Input handling test for create branch.
- Input handling test for continue once.

### 18.4 Web API tests

- `POST /api/v1/coordinator/preflight` returns clean response.
- Missing branch returns `branch_missing` status.
- Dirty branch returns `dirty` status with entries.
- Invalid branch returns structured validation error.
- Branch creation endpoint requires confirmation/intent.
- Coordinator run fails when dirty unless `allow_dirty_reference` is true.

### 18.5 Regression tests

Verify that preflight runs before:

- task registry mutation
- worktree creation
- performer launch
- PRD sync writes
- merge operations

One useful test is to intentionally configure a missing branch and assert that no `.macc/worktree/` entry is created.

---

## 19. Acceptance criteria

### 19.1 MVP acceptance criteria

1. `macc coordinator run` checks the resolved `reference_branch` before any coordinator mutation.
2. If the branch does not exist locally, the user is notified.
3. If the branch does not exist locally and a matching remote-tracking branch exists, MACC offers to create a local tracking branch.
4. If the user declines branch creation, the coordinator exits without dispatching tasks.
5. If the reference branch is checked out in a worktree and has staged, unstaged, or untracked changes, MACC notifies the user.
6. Dirty reference branch state blocks the run by default.
7. Non-interactive runs fail instead of prompting.
8. `--create-reference-branch` and `--allow-dirty-reference` provide explicit automation overrides.
9. `--preflight-only` runs checks and exits without starting the coordinator.
10. Preflight results are logged in the coordinator logs.

### 19.2 Full acceptance criteria

1. The same core preflight logic powers CLI, TUI, and Web UI.
2. TUI Automation screen displays preflight status and actions.
3. Web Coordinator Console displays preflight status and actions.
4. Web API exposes `POST /api/v1/coordinator/preflight`.
5. Web UI branch creation is audit-logged.
6. Dirty override is audit-logged.
7. The preflight gate protects `run`, `dispatch`, `advance`, and `resume`.
8. `sync-prd` and `audit-prd` perform at least a branch existence check.
9. Error codes `E701` through `E707` are documented.
10. `macc doctor` includes a reference branch diagnostic.

---

## 20. Documentation updates

### 20.1 README update

Add a section:

```md
### Coordinator reference branch safety

Before running the coordinator, MACC verifies that the configured reference branch exists and is clean. If the branch is missing, MACC can create it after confirmation. If the branch has uncommitted changes, MACC blocks by default to protect local work.

Useful commands:

```bash
macc coordinator run --preflight-only
macc coordinator run --create-reference-branch --reference-branch-base main
macc coordinator run --allow-dirty-reference
```
```

### 20.2 Coordinator docs update

Add under coordinator execution:

```md
Before full-cycle execution starts, MACC runs a reference branch preflight gate. The gate verifies that `automation.coordinator.reference_branch` exists locally and checks any worktree where that branch is checked out for uncommitted changes. Missing branches and dirty reference branches block execution by default.
```

### 20.3 Error docs update

Add `E700 Git/Preflight` range.

### 20.4 Web API contract update

Add:

```http
POST /api/v1/coordinator/preflight
```

and document the response model.

### 20.5 TUI docs update

Document the new Automation / Coordinator preflight section.

---

## 21. Suggested patch for `MACC.md`

The following text can be inserted after the existing coordinator command section.

```md
### 12.3.6 Reference branch preflight gate

Before executing `macc coordinator run` or the default `macc coordinator` full-cycle mode, MACC performs a reference branch preflight gate.

The gate resolves `automation.coordinator.reference_branch` using the same precedence as the coordinator runtime, then verifies that the local branch exists. If the branch is missing, MACC notifies the user and offers to create it. When a matching remote-tracking branch such as `origin/<branch>` exists, MACC prefers creating a local tracking branch from it. If the user declines branch creation, the coordinator run is cancelled before any task dispatch, registry mutation, worktree creation, performer launch, or merge operation.

After confirming that the branch exists, MACC inspects every Git worktree where that branch is checked out. If staged, unstaged, or untracked changes are found, MACC warns the user and blocks the run by default. The user must commit, stash, discard, or explicitly override the guard with `--allow-dirty-reference`.

Non-interactive runs never prompt. Missing branches and dirty reference branches fail fast unless explicit override flags are provided.

Recommended flags:

- `--preflight-only`: run reference branch checks and exit.
- `--create-reference-branch`: allow creation of a missing reference branch.
- `--reference-branch-base <branch-or-rev>`: base used when creating the branch.
- `--allow-dirty-reference`: continue even if the reference branch worktree is dirty.

Preflight failures use the `E700 Git/Preflight` error range:

| Code | Meaning |
|---|---|
| `E701` | Reference branch not found |
| `E702` | Reference branch has uncommitted changes |
| `E703` | Reference branch inspection failed |
| `E704` | Reference branch creation declined |
| `E705` | Reference branch creation failed |
| `E706` | Invalid reference branch name |
| `E707` | Bare repository unsupported |
```

---

## 22. Implementation roadmap

### Phase 1 — Core and CLI MVP

Deliverables:

- `core/src/coordinator/preflight.rs`
- Git wrapper helpers
- CLI preflight before `run`
- `--preflight-only`
- `--allow-dirty-reference`
- `--create-reference-branch`
- `--reference-branch-base`
- error codes `E701` to `E707`
- coordinator log entry
- tests for clean/missing/dirty branches

### Phase 2 — TUI integration

Deliverables:

- Automation screen preflight section
- Missing branch action modal
- Dirty branch action modal
- Config editor support for `require_clean_reference_branch`
- TUI tests/snapshots

### Phase 3 — Web integration

Deliverables:

- `POST /api/v1/coordinator/preflight`
- Web Coordinator Console preflight card
- branch creation action
- dirty override action
- ops audit logging
- Web API tests

### Phase 4 — Broader coordinator guard coverage

Deliverables:

- Guard `dispatch`, `advance`, and `resume`
- Add existence check for `sync-prd` and `audit-prd`
- Add `macc doctor` reference branch diagnostic
- Add full config schema

---

## 23. Recommended defaults

For MVP:

```yaml
automation:
  coordinator:
    reference_branch: main
    require_clean_reference_branch: true
```

For full implementation:

```yaml
automation:
  coordinator:
    reference_branch: main
    reference_branch_preflight:
      enabled: true
      missing_branch_policy: prompt
      dirty_policy: block
      include_untracked: true
      create_from: remote_tracking_or_current_head
      allow_non_interactive_create: false
      log_clean_result: true
```

Recommended behavior summary:

| Scenario | Default behavior |
|---|---|
| Branch exists and clean | Proceed |
| Branch exists but not checked out | Proceed |
| Branch missing, interactive | Prompt |
| Branch missing, non-interactive | Fail |
| Matching remote branch exists | Offer local tracking branch creation |
| Branch dirty, interactive | Block and prompt |
| Branch dirty, non-interactive | Fail |
| `--allow-dirty-reference` used | Warn and proceed |
| User declines branch creation | Cancel coordinator |

---

## 24. Final proposed spec wording

> Before starting `macc coordinator run`, MACC performs a reference-branch preflight gate. It resolves the configured `automation.coordinator.reference_branch`, verifies that the local branch exists, and inspects any worktree where that branch is checked out for staged, unstaged, or untracked changes. If the branch is missing, MACC notifies the user and offers to create it, preferring an existing remote-tracking branch when available. If the user declines, the coordinator run is cancelled before any task dispatch, registry mutation, worktree creation, performer launch, or merge operation. If the branch has uncommitted changes, MACC warns and blocks by default, requiring the user to commit, stash, discard, or explicitly override the guard. Non-interactive runs fail fast unless explicit override flags are provided.

---

## 25. Quick checklist for implementation PR

- [ ] Add `ReferenceBranchPreflightConfig` to coordinator config model.
- [ ] Add config defaults.
- [ ] Add Git wrapper functions for branch existence, worktree listing, status, and branch creation.
- [ ] Add `core/src/coordinator/preflight.rs`.
- [ ] Add branch-name validation with `git check-ref-format --branch`.
- [ ] Add CLI `--preflight-only`.
- [ ] Add CLI `--allow-dirty-reference`.
- [ ] Add CLI `--create-reference-branch`.
- [ ] Add CLI `--reference-branch-base`.
- [ ] Invoke preflight before coordinator full-cycle run.
- [ ] Ensure missing/dirty preflight exits before registry/worktree mutation.
- [ ] Add structured coordinator logs.
- [ ] Add `E701` to `E707` errors.
- [ ] Add unit tests.
- [ ] Add integration tests with temporary Git repositories.
- [ ] Add TUI display in Automation screen.
- [ ] Add Web API endpoint.
- [ ] Add Web UI preflight card.
- [ ] Update README/coordinator docs/error docs.


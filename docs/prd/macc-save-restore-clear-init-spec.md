# MACC Save, Restore, Init Recovery, and Clear Safety Specification

**Document status:** Proposed extension  
**Target project:** MACC  
**Language:** English  
**Scope:** CLI/TUI behavior, save-bundle model, restore flow, `macc init` recovery suggestion, `macc clear` save gate, optional log backup, exclusions, data model, safety rules, and acceptance criteria.

---

## 1. Executive Summary

This specification proposes a user-facing simplification layer for MACC that makes project setup, experimentation, cleanup, and recovery safer and easier.

The core change is to introduce a coherent **MACC save bundle** workflow:

```bash
macc save <name>
macc restore <name>
```

This workflow complements, but does not replace, existing lower-level commands such as:

```bash
macc config save <name>
macc config restore <name>
macc coordinator sessions save <name>
```

The save bundle is designed to preserve only the meaningful, user-authored or user-selected MACC state required to recreate a project’s MACC setup. It must **not** back up worktrees, caches, or generated files. Logs are excluded by default, but can be included explicitly through an opt-in flag.

The intended user experience is:

```bash
macc save my-setup
macc clear
macc init
# MACC detects "my-setup" and suggests restoring it
macc restore my-setup --apply
```

This makes MACC safer for experimentation because users can clear a project, initialize it again, and recover their preferred configuration without manually remembering profile/session commands.

---

## 2. Design Goals

### 2.1 Primary goals

1. Make `macc clear` less risky by prompting the user to save important MACC state before cleanup.
2. Make `macc init` smarter by detecting and suggesting previous saves for the current repository.
3. Introduce a simple umbrella command:

   ```bash
   macc save <name>
   ```

   that saves the current MACC setup as a restorable bundle.

4. Introduce a matching command:

   ```bash
   macc restore <name>
   ```

   that restores the selected save bundle.

5. Preserve user intent without storing unnecessary, large, stale, or generated artifacts.

6. Keep save bundles portable across machines and repositories where possible.

7. Preserve MACC’s existing safety model: explicit consent for destructive operations, no secrets in Git, no generated files stored as authoritative state, and no execution of remote code.

---

## 3. Non-Goals

This proposal does **not** attempt to:

1. Back up the full repository.
2. Back up worktrees.
3. Back up `.macc/cache/`.
4. Back up tool-generated output files.
5. Back up generated adapter outputs created by `macc apply`.
6. Back up generated remote skill/MCP installed files.
7. Replace Git.
8. Replace existing timestamped safety backups created during file overwrites.
9. Store secrets, API keys, tokens, or real MCP credentials.
10. Restore active running processes.
11. Restore provider-side remote AI sessions beyond local session identifiers and mappings.
12. Guarantee that a restored session ID is still valid in the external AI tool.

---

## 4. Terminology

MACC should distinguish three related but different concepts.

| Term | Command family | Purpose | Storage |
|---|---|---|---|
| **Config profile** | `macc config save/restore` | Portable configuration template | `~/.macc/profiles/<name>.yaml` |
| **Safety backup** | internal backups, `macc backups` | Timestamped copy of overwritten files | project/user backup directories |
| **Save bundle** | `macc save/restore` | Coherent restorable MACC setup for a project | `~/.macc/saves/<name>/` |

### 4.1 Config profile

A config profile is primarily a reusable configuration template. It captures canonical configuration sections and can be applied to another repository.

Example:

```bash
macc config save nextjs-defaults
macc init --profile nextjs-defaults
```

### 4.2 Safety backup

A safety backup is created before MACC overwrites files during operations such as `apply`, restore, or user-level configuration merges.

Safety backups are defensive. They are not the primary UX for saving a reusable MACC setup.

### 4.3 Save bundle

A save bundle is a user-created or MACC-suggested restorable unit that may include:

- canonical MACC configuration,
- coordinator session mappings,
- catalog selections,
- optional logs,
- metadata describing repository identity and included artifacts.

A save bundle must not include worktrees, caches, or generated files.

---

## 5. High-Level Workflow

### 5.1 Save before clear

```bash
macc clear
```

If MACC detects unsaved configuration or coordinator session state, it prompts:

```text
This project has MACC configuration or coordinator session state that has not been saved.

Save before clearing?
  [Y] Save now
  [N] Continue without saving
  [A] Abort clear
```

Recommended default: **Save now**.

### 5.2 Save explicitly

```bash
macc save my-project-defaults
```

This creates a save bundle under:

```text
~/.macc/saves/my-project-defaults/
```

### 5.3 Init suggests previous saves

```bash
macc init
```

If MACC finds a save bundle matching the current repository, it suggests restoring it:

```text
A previous MACC save was found for this repository:

  my-project-defaults
  Saved: 2026-05-25 21:30
  Includes: config, coordinator sessions, catalogs
  Excludes: worktrees, cache, generated files, logs

Restore it now?
  [Y] Restore this save
  [N] Start fresh
  [L] List all matching saves
  [A] Abort
```

### 5.4 Restore explicitly

```bash
macc restore my-project-defaults
```

This restores the selected save bundle.

By default, restore should restore source-of-truth MACC state only. It should not run `macc apply` unless requested or confirmed.

```bash
macc restore my-project-defaults --apply
```

---

## 6. What Must Be Saved

A default save bundle should include only state that is meaningful, compact, restorable, and not generated.

### 6.1 Required by default

| Item | Source | Destination in save bundle | Notes |
|---|---|---|---|
| Canonical project config | `.macc/macc.yaml` | `config/macc.yaml` | Required if present |
| Coordinator/tool session mappings | `.macc/state/tool-sessions.json` | `state/tool-sessions.json` | Included by default unless `--no-sessions` |
| Save manifest | generated | `manifest.yaml` | Required |
| Repository identity metadata | generated | `manifest.yaml` | Required |
| Save hashes | generated | `manifest.yaml` | Required |
| Catalog selections and local catalog overlays | project config / catalog files | `catalogs/` | Include only user-authored or selected metadata |
| MCP templates with placeholders | config/catalogs | `mcp/` or included in config | Never include real secrets |

### 6.2 Optional

| Item | Flag | Default | Notes |
|---|---|---|---|
| Logs | `--include-logs` | Excluded | Optional compressed copy |
| Stateful coordinator runtime data | `--include-state` | Excluded | For advanced recovery only |
| PRD/task registry | `--include-prd` or `--include-state` | Excluded by default | Should be explicit because this can represent active work |
| Description | `--description "..."` | Empty | Human-readable metadata |
| Tags | `--tag <tag>` | Empty | Useful for filtering saves |

### 6.3 Excluded always by default

The following must not be included in default save bundles:

```text
.macc/cache/
.macc/worktree/
.macc/worktrees/
generated tool files
generated adapter outputs
installed remote package outputs
node_modules/
vendor caches
build outputs
temporary files
tool output directories
```

---

## 7. Explicit Exclusion Rules

The user requirement is strict:

> Do not back up worktrees, caches, or any generated files.

MACC should enforce this rule through a denylist and a classification model.

### 7.1 Worktrees

Never save:

```text
.macc/worktree/
.macc/worktrees/
.worktrees/
any path listed by `git worktree list` except the current root
```

Rationale:

- Worktrees are runtime execution environments.
- They can be recreated.
- They may contain partial task work, branch-specific state, or large files.
- They may be dirty or inconsistent.
- They are not portable across machines.

### 7.2 Caches

Never save:

```text
.macc/cache/
tool download caches
remote package materialization cache
HTTP archive cache
Git fetch cache
temporary package extraction directories
```

Rationale:

- Cache content is reproducible from pinned Git refs, tags, commits, or HTTP checksums.
- Caches can be large.
- Cache entries can become stale.
- Cache storage can hide accidental secrets or downloaded third-party content.

### 7.3 Generated files

Never save generated files as authoritative state.

Generated files include:

1. Files created by tool adapters during `macc apply`.
2. Tool-specific instruction files rendered from `.macc/macc.yaml`.
3. Installed skill files generated from remote package manifests.
4. Installed MCP files generated from templates.
5. Tool-specific lock outputs created by MACC.
6. Any file marked in MACC’s managed-path registry as `generated`.
7. Build artifacts or outputs created by tools or scripts.

Generated files should be recreated by:

```bash
macc apply
```

not restored from a save bundle.

### 7.4 Logs

Logs are excluded by default but may be included explicitly:

```bash
macc save my-setup --include-logs
```

Optional log backup should include only MACC logs, not arbitrary project logs:

```text
.macc/log/
```

Recommended log backup destination:

```text
~/.macc/saves/<name>/logs/logs.tar.zst
```

or:

```text
~/.macc/saves/<name>/logs/
```

Compression is recommended because logs can be large.

---

## 8. Optional Log Backup Design

### 8.1 CLI flags

```bash
macc save <name> --include-logs
macc save <name> --include-logs --log-max-size 50MB
macc save <name> --include-logs --log-since 7d
macc save <name> --include-logs --redact-logs
```

Recommended default if `--include-logs` is provided:

```text
--log-max-size 50MB
--redact-logs true
```

### 8.2 What logs to include

Only include:

```text
.macc/log/coordinator/
.macc/log/performer/
.macc/log/ops.jsonl
other MACC-owned logs under .macc/log/
```

Do not include:

```text
application logs
framework logs
external tool caches
terminal scrollback outside MACC logs
logs outside the project root
```

### 8.3 Redaction

Before storing logs, MACC should run best-effort redaction for common secret patterns:

```text
API keys
Bearer tokens
OAuth tokens
JWT-like strings
private keys
database URLs with credentials
.env-style assignments
provider request headers
```

If potential secrets are detected, MACC should warn:

```text
Potential secrets were detected in logs.

Choose:
  [R] Save redacted logs
  [S] Save logs as-is
  [N] Do not save logs
  [A] Abort
```

Recommended default: **Save redacted logs**.

### 8.4 Restore behavior for logs

Logs should not be restored into active runtime locations by default.

Instead:

```bash
macc restore my-setup --include-logs
```

should restore logs into an archival location:

```text
.macc/restored-logs/<save-name>/
```

not:

```text
.macc/log/
```

Rationale:

- Restored logs should not be confused with current runtime logs.
- Current logs may already exist.
- Log restoration is mainly for inspection, debugging, and audit history.

---

## 9. Save Bundle Layout

Recommended storage path:

```text
~/.macc/saves/<name>/
```

Recommended structure:

```text
~/.macc/saves/<name>/
  manifest.yaml
  config/
    macc.yaml
  state/
    tool-sessions.json
  catalogs/
    skills.catalog.json
    mcp.catalog.json
    overlays/
  logs/
    logs.tar.zst
  checksums/
    sha256sums.txt
```

Files or folders should be omitted when not included.

For example, a default save without logs:

```text
~/.macc/saves/my-project-defaults/
  manifest.yaml
  config/
    macc.yaml
  state/
    tool-sessions.json
  catalogs/
    skills.catalog.json
    mcp.catalog.json
  checksums/
    sha256sums.txt
```

A save with logs:

```text
~/.macc/saves/my-project-defaults/
  manifest.yaml
  config/
    macc.yaml
  state/
    tool-sessions.json
  catalogs/
    skills.catalog.json
    mcp.catalog.json
  logs/
    logs.tar.zst
  checksums/
    sha256sums.txt
```

---

## 10. Manifest Format

Each save bundle must include a manifest.

### 10.1 Example

```yaml
version: 1
kind: macc.save_bundle
name: my-project-defaults
description: "Default setup for this repository"
created_at: "2026-05-25T21:30:00+02:00"
updated_at: "2026-05-25T21:30:00+02:00"
macc_version: "0.x.x"

repository:
  root_name: "my-app"
  root_path_hash: "sha256:..."
  git_remote_url_hash: "sha256:..."
  git_default_branch: "main"
  git_current_branch: "main"
  git_head_sha: "..."
  identity_strength: "strong"

includes:
  config: true
  coordinator_sessions: true
  catalogs: true
  logs: false
  prd: false
  automation_state: false

excludes:
  worktrees: true
  cache: true
  generated_files: true
  secrets: true

paths:
  config: "config/macc.yaml"
  coordinator_sessions: "state/tool-sessions.json"
  logs_archive: null

hashes:
  config: "sha256:..."
  coordinator_sessions: "sha256:..."
  manifest_payload: "sha256:..."

security:
  secret_scan:
    performed: true
    findings: 0
    redacted_logs: false

restore:
  recommended_after_restore:
    - "macc apply"
  requires_confirmation:
    user_level_writes: true
    sessions: true
    logs: true
```

### 10.2 Required manifest fields

| Field | Required | Description |
|---|---:|---|
| `version` | Yes | Manifest schema version |
| `kind` | Yes | Must be `macc.save_bundle` |
| `name` | Yes | Save bundle name |
| `created_at` | Yes | Creation timestamp |
| `macc_version` | Yes | MACC version used to create the save |
| `repository` | Yes | Repository identity metadata |
| `includes` | Yes | What is included |
| `excludes` | Yes | Explicitly documents what is excluded |
| `paths` | Yes | Relative paths inside save bundle |
| `hashes` | Yes | Integrity checks |
| `security` | Yes | Secret-scan metadata |

---

## 11. Repository Matching

MACC should detect whether a save belongs to the current repository.

### 11.1 Identity inputs

Recommended repository identity inputs:

1. Git repository root path.
2. Git remote URL, normalized and hashed.
3. Repository directory name.
4. Current branch.
5. Default/reference branch.
6. Optional `.git` root fingerprint.
7. Optional canonical config hash.

### 11.2 Match strength

Use match strengths:

| Strength | Meaning | Example |
|---|---|---|
| `strong` | Same remote hash and compatible root identity | Same repo cloned to same/different machine |
| `medium` | Same repo name and similar config hash | Fork or copied repo |
| `weak` | Same directory name only | Possibly related |
| `none` | No meaningful match | Do not suggest by default |

### 11.3 `macc init` suggestion rules

When running:

```bash
macc init
```

Recommended behavior:

1. If exactly one strong match exists: suggest it as the default.
2. If multiple strong matches exist: list them and ask.
3. If only medium matches exist: list them, but default to fresh init.
4. If only weak matches exist: do not prompt loudly; mention with `--verbose` or `macc save list --matching`.
5. If no match exists: proceed with fresh init.

---

## 12. `macc init` Behavior

### 12.1 Interactive default

```bash
macc init
```

Flow:

1. Detect project root.
2. Detect whether `.macc/` already exists.
3. Detect matching save bundles.
4. If a strong match exists, suggest restore.
5. If the user accepts, restore source-of-truth MACC state.
6. Ask whether to run `macc apply`.
7. If the user declines restore, create a fresh baseline `.macc/`.

### 12.2 Example: one strong match

```text
A previous MACC save was found for this repository:

  my-project-defaults
  Saved: 2026-05-25 21:30
  Includes: config, coordinator sessions, catalogs
  Excludes: worktrees, cache, generated files, logs

Restore it now?
  [Y] Restore this save
  [N] Start fresh
  [L] List all matching saves
  [A] Abort
```

### 12.3 Example: multiple matches

```text
Multiple MACC saves were found for this repository:

  1. my-project-defaults      2026-05-25 21:30  config,sessions,catalogs
  2. macc-web-experiment      2026-05-24 17:08  config
  3. before-clear             2026-05-23 09:41  config,sessions,logs

Choose:
  [1-3] Restore selected save
  [N]   Start fresh
  [A]   Abort
```

### 12.4 Flags

```bash
macc init --fresh
macc init --restore
macc init --restore <name>
macc init --profile <name>
macc init --no-restore-prompt
macc init --restore --apply
```

### 12.5 Flag semantics

| Flag | Behavior |
|---|---|
| `--fresh` | Ignore matching saves and create a new baseline |
| `--restore` | Restore the best matching save, asking if ambiguous |
| `--restore <name>` | Restore a specific save during init |
| `--profile <name>` | Existing behavior: initialize from a config profile |
| `--no-restore-prompt` | Do not suggest saves |
| `--apply` | After init/restore, run `macc apply` |

### 12.6 Recommendation

Do not make `macc init` silently restore state without user visibility. Suggest restore interactively unless the user passes an explicit non-interactive flag.

---

## 13. `macc save` Behavior

### 13.1 Basic command

```bash
macc save <name>
```

Default behavior:

1. Validate save name.
2. Detect repository identity.
3. Load `.macc/macc.yaml`.
4. Load `.macc/state/tool-sessions.json` if present.
5. Load user-authored catalog overlays if present.
6. Exclude worktrees, caches, and generated files.
7. Run secret scan.
8. Write save bundle atomically.
9. Print summary.

### 13.2 Example output

```text
Saved MACC bundle "my-project-defaults".

Included:
  ✓ config
  ✓ coordinator sessions
  ✓ catalogs

Excluded:
  - worktrees
  - cache
  - generated files
  - logs

Location:
  ~/.macc/saves/my-project-defaults/
```

### 13.3 With logs

```bash
macc save my-project-defaults --include-logs
```

Example output:

```text
Saved MACC bundle "my-project-defaults".

Included:
  ✓ config
  ✓ coordinator sessions
  ✓ catalogs
  ✓ logs, redacted and compressed

Excluded:
  - worktrees
  - cache
  - generated files
```

### 13.4 Existing save handling

If the save name already exists:

```text
Save "my-project-defaults" already exists.

Choose:
  [O] Overwrite
  [N] Create a new save name
  [A] Abort
```

For non-interactive use:

```bash
macc save my-project-defaults --overwrite
```

### 13.5 Flags

```bash
macc save <name>
macc save <name> --overwrite
macc save <name> --description "..."
macc save <name> --only config,sessions,catalogs
macc save <name> --no-sessions
macc save <name> --include-logs
macc save <name> --log-max-size 50MB
macc save <name> --log-since 7d
macc save <name> --redact-logs
macc save <name> --dry-run
```

### 13.6 `--only` values

Recommended values:

```text
config
sessions
catalogs
logs
prd
automation_state
```

But default should remain conservative:

```text
config,sessions,catalogs
```

Logs should only be included when explicitly requested.

---

## 14. `macc clear` Save Gate

### 14.1 Current role of `macc clear`

`macc clear` is a guarded cleanup command. It removes MACC-managed project artifacts and cleans worktrees first.

This proposal adds a save gate before cleanup.

### 14.2 Pre-clear detection

Before clearing, MACC should check:

```text
.macc/macc.yaml
.macc/state/tool-sessions.json
local catalog overlays
optional logs if user asks
```

Then compare hashes against existing save manifests.

### 14.3 Prompt

```text
Unsaved MACC setup detected.

The following state has changed since the last save:
  - config
  - coordinator sessions

Save before clearing?
  [Y] Save now
  [N] Continue without saving
  [A] Abort clear
```

Recommended default: **Save now**.

### 14.4 Existing saves for the repository

If saves already exist:

```text
Existing saves found for this repository:

  1. my-project-defaults
     Saved: 2026-05-25 21:30
     Includes: config, sessions, catalogs

  2. before-mcp-experiment
     Saved: 2026-05-24 19:03
     Includes: config, sessions, catalogs, logs

Choose:
  [1-2] Overwrite an existing save
  [N]   Create a new save
  [S]   Skip saving
  [A]   Abort clear
```

### 14.5 Flags

```bash
macc clear --save <name>
macc clear --save <name> --include-logs
macc clear --no-save-prompt
macc clear --force --no-save-prompt
macc clear --dry-run
```

### 14.6 Safety recommendation

`macc clear --force` should not silently skip save prompts unless paired with:

```bash
macc clear --force --no-save-prompt
```

This prevents accidental destructive cleanup in interactive sessions.

---

## 15. `macc restore` Behavior

### 15.1 Explicit restore

```bash
macc restore <name>
```

Because the user provided a specific save name, MACC should restore that save directly after showing a concise summary.

Recommended output:

```text
Restoring MACC save "my-project-defaults"...

Restored:
  ✓ .macc/macc.yaml
  ✓ coordinator sessions
  ✓ catalog selections

Skipped:
  - worktrees
  - cache
  - generated files
  - logs

Next recommended command:
  macc apply
```

### 15.2 Restore and apply

```bash
macc restore my-project-defaults --apply
```

Behavior:

1. Restore source-of-truth state.
2. Run `macc plan`.
3. Show summary.
4. Apply if safe or if confirmed by flag/prompt.

Recommended interactive prompt:

```text
Run `macc apply` now to regenerate tool-specific files?
  [Y] Apply now
  [N] Restore only
```

### 15.3 Restore latest

```bash
macc restore --latest
```

This is ambiguous enough to require confirmation:

```text
Restore latest save for this repository?

  Save: before-clear
  Saved: 2026-05-25 21:30
  Includes: config, sessions, catalogs
  Excludes: worktrees, cache, generated files, logs

Continue? [y/N]
```

For non-interactive automation:

```bash
macc restore --latest --yes
```

### 15.4 Plain `macc restore`

Plain restore with no name should not automatically restore anything.

Recommended behavior:

```text
No save name provided.

Matching saves:
  1. my-project-defaults
  2. before-clear
  3. macc-web-experiment

Run:
  macc restore <name>
```

### 15.5 Restoring logs

Logs should not be restored by default even if included in the save.

To restore logs:

```bash
macc restore my-project-defaults --include-logs
```

Restored logs should go to:

```text
.macc/restored-logs/my-project-defaults/
```

not:

```text
.macc/log/
```

### 15.6 Flags

```bash
macc restore <name>
macc restore <name> --apply
macc restore <name> --config-only
macc restore <name> --sessions
macc restore <name> --no-sessions
macc restore <name> --include-logs
macc restore <name> --dry-run
macc restore --latest
macc restore --latest --yes
```

---

## 16. Save Listing, Inspection, and Deletion

### 16.1 List saves

```bash
macc save list
```

Recommended output:

```text
MACC saves:

  my-project-defaults
    Saved: 2026-05-25 21:30
    Repo: my-app
    Includes: config,sessions,catalogs

  before-clear
    Saved: 2026-05-24 18:10
    Repo: my-app
    Includes: config,sessions,catalogs,logs
```

### 16.2 List matching saves only

```bash
macc save list --matching
```

### 16.3 Show save details

```bash
macc save show my-project-defaults
```

Recommended output:

```text
Save: my-project-defaults
Created: 2026-05-25 21:30
Repository match: strong
Includes:
  ✓ config
  ✓ coordinator sessions
  ✓ catalogs
  - logs
  - PRD
  - automation state

Explicitly excluded:
  - worktrees
  - cache
  - generated files
  - secrets

Recommended restore:
  macc restore my-project-defaults --apply
```

### 16.4 Delete save

```bash
macc save delete my-project-defaults
```

Require confirmation unless `--yes` is provided.

---

## 17. Data Classification Model

MACC should classify files before saving, clearing, or restoring.

### 17.1 Classes

| Class | Description | Save default |
|---|---|---:|
| `source_of_truth` | Canonical user/project config | Yes |
| `portable_state` | Restorable user-level or project-level state | Yes |
| `runtime_state` | Active runtime/task/process state | No |
| `diagnostic_log` | MACC logs | No, unless `--include-logs` |
| `generated` | Generated adapter/tool output | Never |
| `cache` | Reproducible downloaded/materialized artifacts | Never |
| `worktree` | Git worktree execution directory | Never |
| `secret` | Credentials or likely credentials | Never |
| `unknown` | Unclassified path | No |

### 17.2 Default save policy

```text
source_of_truth: include
portable_state: include
runtime_state: exclude unless explicit
diagnostic_log: exclude unless --include-logs
generated: exclude
cache: exclude
worktree: exclude
secret: exclude
unknown: exclude
```

### 17.3 Implementation note

Every path MACC creates should carry metadata:

```rust
enum ManagedPathKind {
    SourceOfTruth,
    PortableState,
    RuntimeState,
    DiagnosticLog,
    Generated,
    Cache,
    Worktree,
}
```

This prevents accidental inclusion of generated or runtime paths.

---

## 18. Security and Privacy

### 18.1 Secrets

Save bundles must not contain secrets.

Before writing a save bundle, MACC should scan included files for likely secret patterns.

If findings exist in required files:

```text
Potential secrets were detected in files selected for saving.

Choose:
  [R] Save with redaction if possible
  [E] Exclude affected files
  [A] Abort
```

Recommended default: **Abort** for config/session files, **Redact** for logs.

### 18.2 Sessions

Coordinator/tool session IDs are not API keys, but they may reveal workflow context or allow continuation of tool conversations depending on the provider/tool.

Recommendations:

1. Include sessions by default only in user-local saves.
2. Never write save bundles into the Git repository by default.
3. Warn when restoring sessions into a different repository.
4. Support:

   ```bash
   macc save <name> --no-sessions
   macc restore <name> --no-sessions
   ```

### 18.3 Logs

Logs may contain sensitive data.

Recommendations:

1. Exclude logs by default.
2. Require explicit `--include-logs`.
3. Redact logs by default when included.
4. Restore logs into an archival directory, not live log directories.
5. Store log inclusion in the manifest.

### 18.4 User-level writes

If restore requires changing user-level files, MACC must:

1. Create a timestamped safety backup.
2. Show a summary or diff.
3. Ask for explicit consent.
4. Preserve existing user-level configuration unless the restore explicitly overwrites it.

---

## 19. Restore Merge Strategy

### 19.1 Config restore

Default behavior should restore the full canonical config from the save bundle.

Alternative flags:

```bash
macc restore <name> --config-only
macc restore <name> --only tools,settings
```

For fine-grained config restore, reuse the same section model as existing config profiles.

### 19.2 Session restore

Default behavior:

- Restore sessions if the save includes them.
- Warn if repository identity does not match strongly.
- Remove active leases that are stale or machine-specific.
- Preserve reusable session mappings.

Recommended session restore normalization:

1. Drop active lease ownership fields tied to old PIDs or old worktree paths.
2. Preserve session IDs and scope mappings when scope is portable.
3. Mark restored leases as `released`.
4. Recompute worktree-specific scopes if worktrees are absent.

### 19.3 Catalog restore

Catalog selections and overlays can be restored.

Do not restore materialized package contents from cache.

After restoring catalog selections, `macc apply` should fetch or reuse remote artifacts according to normal apply behavior.

### 19.4 Logs restore

Restore only with:

```bash
macc restore <name> --include-logs
```

Destination:

```text
.macc/restored-logs/<save-name>/
```

---

## 20. UX Principles

### 20.1 Make the safe path the easy path

Users should be able to do:

```bash
macc clear
```

and be guided to save before destructive cleanup.

### 20.2 Be explicit about exclusions

Every save summary should show:

```text
Excluded:
  - worktrees
  - cache
  - generated files
```

If logs are excluded:

```text
  - logs
```

If logs are included:

```text
Included:
  ✓ logs, redacted
```

### 20.3 Avoid surprising automatic restore

Recommended:

- `macc init`: suggest restore, do not silently restore.
- `macc restore <name>`: restore directly because the user selected a target.
- `macc restore --latest`: ask for confirmation unless `--yes`.
- `macc restore`: list options, do not restore.

### 20.4 Keep concepts separate

Do not blur:

- config profiles,
- safety backups,
- save bundles.

Use precise language in prompts.

Prefer:

```text
A previous MACC save was found.
```

Avoid:

```text
A backup was found.
```

unless referring to file-overwrite safety backups.

---

## 21. Suggested CLI Reference

### 21.1 Save

```bash
macc save <name>
macc save <name> --overwrite
macc save <name> --description "..."
macc save <name> --only config,sessions,catalogs
macc save <name> --no-sessions
macc save <name> --include-logs
macc save <name> --log-max-size 50MB
macc save <name> --log-since 7d
macc save <name> --redact-logs
macc save <name> --dry-run
```

### 21.2 Restore

```bash
macc restore <name>
macc restore <name> --apply
macc restore <name> --config-only
macc restore <name> --no-sessions
macc restore <name> --include-logs
macc restore <name> --dry-run
macc restore --latest
macc restore --latest --yes
```

### 21.3 Init

```bash
macc init
macc init --fresh
macc init --restore
macc init --restore <name>
macc init --profile <name>
macc init --restore --apply
macc init --no-restore-prompt
```

### 21.4 Clear

```bash
macc clear
macc clear --save <name>
macc clear --save <name> --include-logs
macc clear --no-save-prompt
macc clear --force --no-save-prompt
macc clear --dry-run
```

### 21.5 Save management

```bash
macc save list
macc save list --matching
macc save show <name>
macc save delete <name>
```

---

## 22. TUI Integration

### 22.1 Init screen

When matching saves exist, show a restore card:

```text
Previous setup found

Save: my-project-defaults
Saved: 2026-05-25 21:30
Includes: config, sessions, catalogs
Excludes: worktrees, cache, generated files, logs

Actions:
  Restore
  Start fresh
  View details
```

### 22.2 Clear confirmation screen

Add a pre-clear save step:

```text
Before clearing

Unsaved MACC setup detected.

Recommended action:
  Save current setup before clearing.

Options:
  Save and continue
  Continue without saving
  Abort
```

If the user chooses save, show checkboxes:

```text
[x] Config
[x] Coordinator sessions
[x] Catalog selections
[ ] Logs
```

Worktrees, cache, and generated files must not be selectable.

### 22.3 Save details screen

Display:

- name,
- description,
- creation date,
- repository match strength,
- included sections,
- excluded sections,
- secret-scan summary,
- restore command.

### 22.4 Restore screen

Display:

```text
This restore will write:
  .macc/macc.yaml
  .macc/state/tool-sessions.json
  catalog overlays

This restore will not write:
  worktrees
  cache
  generated files
  live logs

Optional:
  [ ] Restore archived logs to .macc/restored-logs/<name>/
  [ ] Run macc apply after restore
```

---

## 23. Web UI Integration

The Web UI should expose save/restore under the Ops or Settings area.

Recommended pages:

```text
/ops/backups
/ops/saves
/config/saves
```

Recommended API endpoints:

```text
GET    /api/v1/saves
GET    /api/v1/saves/{name}
POST   /api/v1/saves
DELETE /api/v1/saves/{name}
POST   /api/v1/saves/{name}/restore
GET    /api/v1/saves/matching
```

Example restore request:

```json
{
  "apply": true,
  "include_logs": false,
  "restore_sessions": true,
  "confirmed": true
}
```

The Web UI must clearly show that worktrees, caches, and generated files are excluded.

---

## 24. Error Handling

### 24.1 Error categories

Recommended error codes:

| Code | Meaning |
|---|---|
| `MACC-SAVE-1000` | Invalid save name |
| `MACC-SAVE-1001` | Save already exists |
| `MACC-SAVE-1002` | No `.macc/macc.yaml` found |
| `MACC-SAVE-1003` | Secret scan failed |
| `MACC-SAVE-1004` | Log archive too large |
| `MACC-SAVE-1005` | Attempted to include excluded path |
| `MACC-RESTORE-2000` | Save not found |
| `MACC-RESTORE-2001` | Manifest version unsupported |
| `MACC-RESTORE-2002` | Repository mismatch |
| `MACC-RESTORE-2003` | Checksum mismatch |
| `MACC-RESTORE-2004` | Restore conflict |
| `MACC-INIT-3000` | Ambiguous matching saves |
| `MACC-CLEAR-4000` | Unsaved state detected in non-interactive mode |

### 24.2 Checksum mismatch

If a save bundle checksum fails:

```text
Save bundle integrity check failed.

The save may be corrupted or modified outside MACC.

Restore aborted.
```

Do not restore by default.

### 24.3 Repository mismatch

If restoring into a different repository:

```text
This save was created for a different repository.

Created for:
  my-app

Current repository:
  another-app

Restore config only?
  [C] Config only
  [F] Full restore
  [A] Abort
```

Recommended default: **Config only**.

---

## 25. Implementation Architecture

### 25.1 Core modules

Recommended additions:

```text
core/src/save/
  mod.rs
  manifest.rs
  bundle.rs
  classifier.rs
  repository_identity.rs
  scanner.rs
  logs.rs
  restore.rs
```

### 25.2 CLI modules

Recommended additions:

```text
cli/src/commands/save.rs
cli/src/commands/restore.rs
```

Update:

```text
cli/src/commands/init.rs
cli/src/commands/clear.rs
```

### 25.3 Core types

Suggested Rust-style conceptual types:

```rust
pub struct SaveBundleManifest {
    pub version: u32,
    pub kind: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub macc_version: String,
    pub repository: RepositoryIdentity,
    pub includes: SaveIncludes,
    pub excludes: SaveExcludes,
    pub paths: SavePaths,
    pub hashes: SaveHashes,
    pub security: SaveSecurity,
}
```

```rust
pub struct SaveIncludes {
    pub config: bool,
    pub coordinator_sessions: bool,
    pub catalogs: bool,
    pub logs: bool,
    pub prd: bool,
    pub automation_state: bool,
}
```

```rust
pub struct SaveExcludes {
    pub worktrees: bool,
    pub cache: bool,
    pub generated_files: bool,
    pub secrets: bool,
}
```

```rust
pub enum ManagedPathKind {
    SourceOfTruth,
    PortableState,
    RuntimeState,
    DiagnosticLog,
    Generated,
    Cache,
    Worktree,
    Secret,
    Unknown,
}
```

### 25.4 Atomic writes

Save creation should be atomic:

1. Write to temporary directory:

   ```text
   ~/.macc/saves/.tmp/<name>.<random>/
   ```

2. Validate manifest and checksums.
3. Rename into place:

   ```text
   ~/.macc/saves/<name>/
   ```

For overwrite:

1. Move existing save to temporary backup.
2. Move new save into place.
3. Delete temporary backup after success.

---

## 26. Migration Strategy

### 26.1 Phase 1: Core save bundle

Implement:

```bash
macc save <name>
macc restore <name>
macc save list
macc save show <name>
```

Default included items:

```text
config
sessions
catalogs
```

Default exclusions:

```text
worktrees
cache
generated files
logs
```

### 26.2 Phase 2: `clear` integration

Add pre-clear save gate:

```bash
macc clear
macc clear --save <name>
macc clear --no-save-prompt
```

### 26.3 Phase 3: `init` integration

Add matching save detection:

```bash
macc init
macc init --restore
macc init --fresh
```

### 26.4 Phase 4: Optional logs

Add:

```bash
macc save <name> --include-logs
macc restore <name> --include-logs
```

with redaction and size limits.

### 26.5 Phase 5: TUI/Web UI

Add visual save/restore screens and confirmation flows.

---

## 27. Testing Strategy

### 27.1 Unit tests

Test:

1. Save name validation.
2. Repository identity hashing.
3. Match strength classification.
4. Manifest serialization/deserialization.
5. Exclusion rules.
6. Path classification.
7. Secret scanning.
8. Log redaction.
9. Checksum generation.
10. Restore conflict detection.

### 27.2 Integration tests

Scenarios:

1. `macc save <name>` saves config and sessions.
2. `macc save <name>` excludes `.macc/cache/`.
3. `macc save <name>` excludes worktrees.
4. `macc save <name>` excludes generated files.
5. `macc save <name> --include-logs` includes logs.
6. `macc restore <name>` restores config.
7. `macc restore <name> --include-logs` restores logs to `.macc/restored-logs/<name>/`.
8. `macc init` suggests a strong matching save.
9. `macc init --fresh` ignores matching saves.
10. `macc clear` prompts to save unsaved state.
11. `macc clear --save <name>` creates a save before cleanup.
12. `macc restore --latest` requires confirmation.
13. `macc restore --latest --yes` works non-interactively.
14. Checksum mismatch aborts restore.
15. Repository mismatch triggers warning.

### 27.3 Safety tests

Ensure save bundles never contain:

```text
.macc/cache/
.macc/worktree/
.macc/worktrees/
generated adapter outputs
tool-generated files
secrets
```

Add regression tests for each excluded class.

---

## 28. Acceptance Criteria

### 28.1 Save

- `macc save <name>` creates `~/.macc/saves/<name>/`.
- Save bundle contains a valid `manifest.yaml`.
- Save bundle includes `.macc/macc.yaml` when present.
- Save bundle includes `.macc/state/tool-sessions.json` when present unless `--no-sessions`.
- Save bundle includes user-authored catalog overlays when present.
- Save bundle does not include worktrees.
- Save bundle does not include caches.
- Save bundle does not include generated files.
- Save bundle does not include logs unless `--include-logs`.
- Save summary explicitly lists included and excluded categories.

### 28.2 Logs

- `macc save <name> --include-logs` includes MACC logs only.
- Logs are redacted by default.
- Log archive size is bounded.
- Restore does not write logs into `.macc/log/` by default.
- `macc restore <name> --include-logs` restores logs into `.macc/restored-logs/<name>/`.

### 28.3 Restore

- `macc restore <name>` restores the selected save bundle.
- Restore validates manifest version and checksums.
- Restore warns on repository mismatch.
- Restore does not restore worktrees.
- Restore does not restore cache.
- Restore does not restore generated files.
- Restore recommends `macc apply`.
- `macc restore <name> --apply` restores then runs apply with normal safety gates.

### 28.4 Init

- `macc init` detects strong matching save bundles.
- `macc init` suggests a matching save interactively.
- `macc init --fresh` bypasses suggestions.
- `macc init --restore <name>` restores a specific save.
- `macc init --restore --apply` restores and applies.

### 28.5 Clear

- `macc clear` detects unsaved MACC state.
- `macc clear` prompts to save before cleanup.
- `macc clear --save <name>` saves before clearing.
- `macc clear --save <name> --include-logs` includes logs in the save.
- `macc clear --no-save-prompt` skips save suggestion.
- `macc clear --dry-run` shows what would be saved and cleared.

---

## 29. Recommended Final UX

### 29.1 Experiment safely

```bash
macc save before-experiment
macc clear
macc init --restore before-experiment --apply
```

### 29.2 Clear safely with automatic save prompt

```bash
macc clear
```

Expected interaction:

```text
Unsaved MACC setup detected.
Save before clearing? [Y/n/a]
> y

Save name:
> before-clear

Include logs? [y/N]
> n

Saved:
  ✓ config
  ✓ coordinator sessions
  ✓ catalogs

Excluded:
  - worktrees
  - cache
  - generated files
  - logs

Proceed with clear? [y/N]
> y
```

### 29.3 Restore on init

```bash
macc init
```

Expected interaction:

```text
A previous MACC save was found for this repository:

  before-clear
  Saved: 2026-05-25 21:30
  Includes: config, sessions, catalogs
  Excludes: worktrees, cache, generated files, logs

Restore it now? [Y/n]
> y

Run `macc apply` now? [Y/n]
> y
```

### 29.4 Include logs only when needed

```bash
macc save debug-session --include-logs
```

Expected interaction:

```text
Logs may contain sensitive information.
MACC will redact likely secrets before saving logs.

Continue? [Y/n]
```

---

## 30. Final Recommendation

Implement the feature as a first-class **save bundle** system rather than treating it as a generic backup mechanism.

The most important product decision is to preserve a clean separation:

1. **Profiles** are reusable config templates.
2. **Safety backups** protect overwritten files.
3. **Save bundles** restore a project’s MACC setup.

The strict exclusion rule should be part of the contract:

```text
Save bundles never include worktrees, caches, or generated files.
Logs are included only with --include-logs.
```

The recommended command behavior is:

```bash
macc save <name>
```

creates a compact restorable setup.

```bash
macc clear
```

prompts to save before destructive cleanup.

```bash
macc init
```

suggests a previous matching save.

```bash
macc restore <name>
```

restores the explicitly selected save.

```bash
macc restore <name> --apply
```

restores and regenerates generated files through the normal MACC apply pipeline.

This design reduces user fear, supports experimentation, avoids bloated or stale backups, and keeps MACC’s source-of-truth model intact.

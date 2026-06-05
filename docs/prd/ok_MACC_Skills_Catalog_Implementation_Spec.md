# MACC Skills & Catalog Motif — Implementation Specification

**Project:** MACC — Multi-Assistant Code Config  
**Document type:** Architecture and product specification  
**Status:** Proposed implementation track  
**Language:** English  
**Date:** 2026-05-26

---

## 1. Executive summary

The current MACC remote Skills/MCP model is directionally correct: it uses catalogs, Git/HTTP/local sources, sparse checkout, `.macc/cache/`, `macc.package.json`, and tool-specific install targets. However, the model needs a stronger lifecycle layer to avoid cache fragility, mutable-reference drift, unclear install state, and silent file conflicts.

This specification applies the recommendations from the Skills & Catalog motif and integrates the previously discussed token-optimization idea: generated hook bundles that summarize or filter noisy tool output before it enters an assistant context.

The proposed evolution turns Skills & Catalog into a reproducible, lockfile-driven package subsystem for AI-tool capabilities.

Key additions:

1. Clear distinction between **available**, **selected**, **installed**, and **locked** skills.
2. New `macc skills` lifecycle commands: `available`, `status`, `install`, `update`, `verify`, `doctor`, `diff`, `prune`, and `uninstall`.
3. Dedicated lockfile: `.macc/skills.lock.json`.
4. Immutable cache keys based on resolved Git SHAs or HTTP checksums.
5. Explicit SHA pinning with `--pin` and project-level `require_pin` policy.
6. Pre-install conflict detection across skills, existing files, adapters, and platforms.
7. Conservative update behavior respecting pinned references by default.
8. Token-saving hook bundles distributed as first-class MACC packages.
9. TUI/Web UI views for Available, Installed, Updates, Conflicts, and Provenance.
10. Acceptance criteria, test strategy, and phased roadmap.

---

## 2. Design goals

### 2.1 Product goals

MACC should make AI-tool skills feel as safe and inspectable as modern package dependencies.

The user should be able to answer these questions without inspecting `.claude/skills/` manually:

- What skills are available?
- What skills are selected in this project?
- What skills are actually installed for each tool?
- Where did each skill come from?
- Which branch, tag, SHA, or checksum was used?
- Has any installed skill drifted from the lockfile?
- Are there updates available?
- Will installing this skill overwrite anything?
- Can this project be reproduced offline?

### 2.2 Engineering goals

The implementation should provide:

- deterministic installs,
- atomic cache writes,
- stable lockfile semantics,
- safe conflict detection,
- clear ownership metadata,
- offline reproducibility,
- platform-aware path validation,
- testable pure-core logic,
- adapter-friendly install planning,
- TUI/Web visibility.

### 2.3 Security goals

Remote packages remain **data-only**.

MACC must not:

- execute post-install scripts,
- write secrets into the repository,
- install files outside approved destination roots,
- silently overwrite unmanaged files,
- silently move pinned dependencies,
- trust mutable Git branches as reproducible sources.

---

## 3. Core state model

MACC should distinguish four different states.

| State | Meaning | Source of truth | Example |
|---|---|---|---|
| **Available** | Known from one or more catalogs | `catalog/skills.catalog.json`, imported catalogs, remote indexes | `nextjs-rsc`, `supabase-rls` |
| **Selected** | Desired by the project/user | `.macc/macc.yaml` | `nextjs-rsc` selected for Claude |
| **Installed** | Materialized into tool-specific files | generated tool directories plus ownership metadata | `.claude/skills/nextjs-rsc/SKILL.md` |
| **Locked** | Exact resolved source and installed file digests | `.macc/skills.lock.json` | Git SHA `9f31c2a...` |

Conceptual flow:

```text
catalogs                -> what exists
.macc/macc.yaml          -> what the user wants
.macc/cache/             -> fetched source material
.macc/skills.lock.json   -> what was resolved and installed
tool directories         -> what exists on disk
```

This is similar in spirit to dependency managers:

```text
catalog + config + lockfile + cache + install tree
```

But MACC is stricter because remote skill packages are configuration/text artifacts, not executable packages.

---

## 4. CLI command surface

### 4.1 `macc skills available`

Lists skills known from configured catalogs.

```bash
macc skills available
macc skills available --tool claude
macc skills available --source official
macc skills available --tag nextjs
macc skills available --json
```

Example output:

```text
Available skills

ID                    Tools              Source      Recommended ref   Risk
nextjs-rsc            claude,codex       official    v0.3.1            low
supabase-rls          claude             official    v0.2.0            medium
github-reviewer       claude,codex       community   main              medium
```

Purpose:

- answer “what can I install?”
- make catalog contents visible from the CLI,
- support future Web/TUI catalog browsing.

---

### 4.2 `macc skills status`

Shows installed skills per tool, including provenance and health.

```bash
macc skills status
macc skills status --tool claude
macc skills status --verbose
macc skills status --json
```

Example output:

```text
Installed skills

Tool     Skill             Source                                  Ref        Pin        Status
claude   nextjs-rsc        github.com/brand201/macc-skills          v0.3.1     9f31c2a    clean
claude   supabase-rls      github.com/brand201/macc-skills          main       unpinned   warning
codex    github-reviewer   github.com/acme/ai-skills                1.4.0      2a77be9    modified

Warnings:
- supabase-rls is installed from mutable ref "main".
- github-reviewer differs from lockfile digest at .codex/skills/github-reviewer/SKILL.md.
```

Status categories:

| Status | Meaning |
|---|---|
| `clean` | Installed files match lockfile digests |
| `modified` | Installed files changed after MACC installed them |
| `missing-files` | Lockfile expects files that are absent |
| `cache-missing` | Lockfile points to a cache entry that no longer exists |
| `unpinned` | Installed from mutable ref without resolved pin policy |
| `source-unreachable` | Remote source cannot be reached during verification |
| `conflict` | Destination collision or ownership mismatch detected |
| `orphaned` | Installed files exist but no longer appear in selected config |
| `unsupported-tool` | Skill is selected for a tool not supported by its manifest |
| `manifest-invalid` | Package manifest cannot be parsed or validated |

---

### 4.3 `macc skills install`

Installs a skill through the catalog/package pipeline.

```bash
macc skills install nextjs-rsc --tool claude
macc skills install github-reviewer --tool claude --source https://github.com/acme/ai-skills --reference v1.2.0
macc skills install github-reviewer --tool claude --source https://github.com/acme/ai-skills --reference main --pin
```

Behavior:

1. Resolve skill metadata from catalog or direct source.
2. Add selection intent to `.macc/macc.yaml`.
3. Resolve source reference.
4. Fetch into immutable cache.
5. Validate `macc.package.json`.
6. Build install plan.
7. Detect conflicts.
8. Install files atomically.
9. Update `.macc/skills.lock.json`.

Important flags:

```bash
--tool <tool>
--source <url-or-catalog-source>
--reference <branch|tag|sha>
--pin
--no-pin
--require-pin
--alias <install-name>
--dry-run
--replace <existing-skill-id>
--force-managed
--allow-user-file-overwrite
```

Default policy should be safe:

- do not overwrite unmanaged files,
- warn on mutable refs,
- fail if project policy requires pinning and no pin is available,
- install only supported tool targets.

---

### 4.4 `macc skills update`

Updates installed skills safely.

```bash
macc skills update
macc skills update --tool claude
macc skills update nextjs-rsc
macc skills update --dry-run
macc skills update --latest
macc skills update --respect-pins
macc skills update --reinstall
```

Default behavior:

| Installed state | Default behavior |
|---|---|
| Pinned to SHA | Verify only; do not move |
| Pinned to tag | Resolve and warn if tag moved |
| Unpinned branch | Show newer commit; require confirmation before moving |
| HTTP with checksum | Re-download only if checksum matches |
| HTTP without checksum | Warn strongly; require confirmation |
| Modified installed files | Do not overwrite unless explicitly confirmed |

Recommended semantics:

```text
macc skills update
```

Means:

```text
Verify pinned skills.
Update only unpinned or policy-allowed movable refs.
Never silently move pinned SHAs.
Never overwrite local modifications without consent.
```

Useful modes:

```bash
macc skills update --dry-run
macc skills update --latest
macc skills update --lockfile-only
macc skills update --reinstall
```

---

### 4.5 `macc skills verify`

Verifies reproducibility and drift.

```bash
macc skills verify
macc skills verify --tool claude
macc skills verify --json
```

Checks:

- lockfile schema is valid,
- selected skills have corresponding lockfile entries,
- lockfile entries have corresponding installed files,
- installed file digests match,
- cache entries exist,
- package manifest digests match,
- source refs are immutable or policy-approved,
- no destination path escapes are present,
- `.macc/cache/` is ignored by Git,
- generated outputs contain no likely secrets.

Recommended CI usage:

```bash
macc skills verify
macc apply --offline --skills-only --dry-run
```

---

### 4.6 `macc skills doctor`

Specialized diagnostics for catalogs, cache, package manifests, lockfiles, and installed skills.

```bash
macc skills doctor
macc skills doctor --fix
```

Checks:

- catalog parse errors,
- duplicate skill IDs,
- missing catalog sources,
- stale cache index entries,
- invalid `macc.package.json`,
- missing lockfile entries,
- orphaned installed directories,
- unsupported adapter targets,
- unsafe paths,
- file conflicts,
- mutable refs in strict mode.

Potential auto-fixes:

- regenerate cache index,
- add `.macc/cache/` to `.gitignore`,
- remove stale cache index entries,
- rebuild lockfile digests from cache when installed files are clean,
- prune orphaned MACC-owned files after confirmation.

---

### 4.7 `macc skills diff`

Compares installed files to the lockfile/cache.

```bash
macc skills diff
macc skills diff nextjs-rsc --tool claude
```

Use cases:

- show local edits made to generated skill files,
- debug why `status` shows `modified`,
- review update impact before applying.

---

### 4.8 `macc skills prune`

Removes installed skills that are no longer selected.

```bash
macc skills prune
macc skills prune --dry-run
macc skills prune --tool claude
```

Safety rules:

1. Only remove files listed in `.macc/skills.lock.json` or `.macc-owned.json`.
2. Do not remove files modified after installation unless confirmed.
3. Do not remove unmanaged files in the same directory.
4. Update `.macc/skills.lock.json` after pruning.
5. Preserve backups when deleting non-trivial directories.

---

### 4.9 `macc skills uninstall`

Safely removes a selected and installed skill.

```bash
macc skills uninstall nextjs-rsc --tool claude
macc skills uninstall nextjs-rsc --all-tools
```

Behavior:

1. Remove selection from `.macc/macc.yaml`.
2. Remove MACC-owned installed files.
3. Preserve modified files unless confirmed.
4. Update `.macc/skills.lock.json`.
5. Report remaining orphaned files, if any.

---

## 5. Data model changes

### 5.1 `.macc/macc.yaml` intent model

The canonical config should represent user intent, not the exact resolved installation state.

Example:

```yaml
catalogs:
  skills:
    - id: official
      kind: git
      url: https://github.com/brand201/macc-skills
      reference: v0.3.1

skills:
  selected:
    - id: nextjs-rsc
      tool: claude
      source: official
      reference: main
      pin: true

    - id: test-output-failures-only
      tool: claude
      source: official
      reference: v0.3.1
      pin: true
      category: hook-bundle

settings:
  skills:
    require_pin: true
    allow_mutable_refs: false
    conflict_policy: fail
    offline_uses_lockfile_only: true
```

Recommended settings:

| Setting | Type | Default | Meaning |
|---|---:|---:|---|
| `require_pin` | bool | `false` for local experimentation, `true` for teams | Require commit SHA/checksum-resolved installs |
| `allow_mutable_refs` | bool | `false` | Allow branch refs such as `main` without pinning |
| `conflict_policy` | enum | `fail` | `fail`, `prompt`, `replace-managed` |
| `offline_uses_lockfile_only` | bool | `true` | Offline install cannot resolve mutable refs |
| `write_ownership_markers` | bool | `true` | Add `.macc-owned.json` markers in generated package roots |

---

### 5.2 `.macc/skills.lock.json`

A dedicated lockfile records exact resolved state.

Example:

```json
{
  "version": 1,
  "generated_by": "macc 0.3.0",
  "generated_at": "2026-05-26T12:00:00Z",
  "skills": [
    {
      "id": "nextjs-rsc",
      "tool": "claude",
      "source": {
        "kind": "git",
        "url": "https://github.com/brand201/macc-skills",
        "requested_ref": "main",
        "resolved_ref": "9f31c2a8f3b6b4e1c7e42c9f4c2f8a2c6b73d911",
        "pinned": true,
        "subpath": "skills/nextjs-rsc"
      },
      "package": {
        "manifest_path": "skills/nextjs-rsc/macc.package.json",
        "manifest_digest": "sha256:manifest-digest",
        "id": "nextjs-rsc",
        "version": "0.3.1"
      },
      "cache": {
        "cache_key": "git/2c92a9/9f31c2a8f3b6b4e1c7e42c9f4c2f8a2c6b73d911"
      },
      "installed": {
        "at": "2026-05-26T12:00:00Z",
        "targets": [
          {
            "src": "claude/SKILL.md",
            "dest": ".claude/skills/nextjs-rsc/SKILL.md",
            "digest": "sha256:file-digest",
            "owner": "macc"
          }
        ]
      }
    }
  ]
}
```

Lockfile policy:

```text
.macc/cache/              ignored
.macc/skills.lock.json    committed
.macc/macc.yaml           committed
```

Rationale:

- cache is machine-local,
- lockfile is reproducibility metadata,
- canonical config is user/project intent.

---

### 5.3 `.macc/cache/index.json`

The cache index maps immutable fetch units to local cache paths.

Example:

```json
{
  "version": 1,
  "entries": [
    {
      "cache_key": "git/2c92a9/9f31c2a8f3b6b4e1c7e42c9f4c2f8a2c6b73d911",
      "kind": "git",
      "url": "https://github.com/brand201/macc-skills",
      "requested_ref": "main",
      "resolved_ref": "9f31c2a8f3b6b4e1c7e42c9f4c2f8a2c6b73d911",
      "fetched_at": "2026-05-26T12:00:00Z",
      "manifest_paths": [
        "skills/nextjs-rsc/macc.package.json"
      ]
    }
  ]
}
```

This file is not required for reproducibility and can be rebuilt, but it improves diagnostics and performance.

---

## 6. Catalog format improvements

A catalog describes available packages, not installed state.

Example `skills.catalog.json`:

```json
{
  "version": 1,
  "sources": [
    {
      "id": "official",
      "kind": "git",
      "url": "https://github.com/brand201/macc-skills",
      "default_ref": "v0.3.1"
    }
  ],
  "skills": [
    {
      "id": "nextjs-rsc",
      "title": "Next.js RSC Expert",
      "description": "Guidance for Server Components, Server Actions, caching, and routing.",
      "tools": ["claude", "codex"],
      "source": "official",
      "subpath": "skills/nextjs-rsc",
      "recommended_ref": "v0.3.1",
      "tags": ["nextjs", "react", "frontend"],
      "risk": "low",
      "requires_mcp": false,
      "writes_user_level_config": false,
      "targets": {
        "claude": [".claude/skills/nextjs-rsc/"],
        "codex": [".codex/skills/nextjs-rsc/"]
      },
      "compatibility": {
        "macc_min": "0.3.0",
        "tools": {
          "claude": ">=1.0.0"
        }
      }
    }
  ]
}
```

Recommended fields:

| Field | Required | Purpose |
|---|---:|---|
| `id` | yes | Stable skill identifier |
| `title` | yes | Human-readable name |
| `description` | yes | TUI/Web explanation |
| `tools` | yes | Supported tool adapters |
| `source` | yes | Catalog source ID |
| `subpath` | yes | Package path inside source |
| `recommended_ref` | yes | Preferred tag/SHA/branch |
| `tags` | no | Search/filter UX |
| `risk` | no | Install review UX |
| `requires_mcp` | no | Dependency hint |
| `writes_user_level_config` | no | Consent hint |
| `targets` | no | Preview of install paths |
| `compatibility` | no | MACC/tool compatibility constraints |

---

## 7. Package manifest requirements

Each remote package must include `macc.package.json` at the package root.

Example skill manifest:

```json
{
  "type": "skill",
  "id": "nextjs-rsc",
  "version": "0.3.1",
  "title": "Next.js RSC Expert",
  "targets": {
    "claude": [
      {
        "src": "claude/SKILL.md",
        "dest": ".claude/skills/nextjs-rsc/SKILL.md"
      }
    ],
    "codex": [
      {
        "src": "codex/instructions.md",
        "dest": ".codex/skills/nextjs-rsc/instructions.md"
      }
    ]
  }
}
```

Example hook-bundle manifest:

```json
{
  "type": "skill",
  "category": "hook-bundle",
  "id": "test-output-failures-only",
  "version": "0.1.0",
  "title": "Test Output Failures Only",
  "description": "Filters test logs so only failures, summaries, and actionable stack traces enter assistant context.",
  "targets": {
    "claude": [
      {
        "src": "claude/hooks/test-output-failures-only.md",
        "dest": ".claude/hooks/test-output-failures-only.md"
      }
    ],
    "gemini": [
      {
        "src": "gemini/hooks/test-output-failures-only.json",
        "dest": ".gemini/hooks/test-output-failures-only.json"
      }
    ]
  }
}
```

Validation rules:

- `type` must be `skill`, `mcp`, `hook-bundle`, or future allowlisted types.
- `id` must match stable identifier rules.
- `version` must be SemVer-like.
- `targets` must map known tool IDs to file install rules.
- `src` must stay inside the package root.
- `dest` must stay inside approved project/tool/user-level roots.
- scripts are forbidden.
- executable files are rejected by default.
- symlinks are rejected by default.
- absolute paths are rejected unless explicitly adapter-allowlisted.
- environment values must be placeholders for MCP packages.

---

## 8. Cache architecture

### 8.1 Problem with mutable cache keys

A fragile cache key might look like this:

```text
.macc/cache/github.com_acme_skills_main/
```

This is unsafe because `main` can move. The same path can contain different content over time, causing drift and test flakiness.

### 8.2 Immutable cache keys

Use resolved immutable identity.

For Git:

```text
.macc/cache/git/<hash-of-url>/<resolved-commit-sha>/
```

For HTTP:

```text
.macc/cache/http/<hash-of-url-and-checksum>/
```

For local development:

```text
.macc/cache/local/<hash-of-path-and-copy-id>/
```

Example:

```text
.macc/cache/git/2c92a9/9f31c2a8f3b6b4e1c7e42c9f4c2f8a2c6b73d911/
```

### 8.3 Atomic fetch flow

```text
1. Resolve source reference.
2. Compute final immutable cache key.
3. Fetch into .macc/cache/.tmp/<uuid>.
4. Validate package manifest.
5. Verify checksum/ref.
6. Compute content digests.
7. Atomically rename temp directory to final cache path.
8. Atomically update cache index.
9. Install only from final cache path.
```

Rules:

- never install from `.tmp`,
- never reuse a partial cache,
- remove stale temp directories during `macc skills doctor --fix`,
- cache index is advisory and rebuildable,
- lockfile is authoritative for reproducibility.

---

## 9. Sparse checkout strategy

Sparse checkout remains the right optimization, but it must not define identity.

Correct order:

```text
1. Resolve requested ref to immutable commit SHA.
2. Compute cache key from source URL + resolved SHA.
3. Sparse checkout required subpaths.
4. Validate manifests.
5. Compute package digests.
6. Install from immutable cache.
7. Record installed file digests in lockfile.
```

Multiple selections can share one fetch unit:

```text
Fetch unit:
  repo: https://github.com/brand201/macc-skills
  resolved_ref: 9f31c2a...

Selections:
  skills/nextjs-rsc
  skills/supabase-rls
  hooks/test-output-failures-only
```

This avoids repeated downloads while preserving deterministic identity.

---

## 10. SHA pinning model

### 10.1 CLI behavior

```bash
macc skills install nextjs-rsc --tool claude --reference main --pin
```

Records:

```text
requested_ref = main
resolved_ref  = 9f31c2a8f3b6b4e1c7e42c9f4c2f8a2c6b73d911
pinned        = true
```

```bash
macc skills install nextjs-rsc --tool claude --reference v0.3.1
```

Records:

```text
requested_ref = v0.3.1
resolved_ref  = <commit behind tag>
pinned        = policy-dependent
```

Recommended policy:

- commit SHA: fully pinned,
- signed immutable tag: acceptable but still record SHA,
- unsigned tag: resolved but warn in strict mode,
- branch: mutable; warn unless `--pin` resolves it to SHA,
- HTTP with checksum: pinned,
- HTTP without checksum: not pinned.

### 10.2 Project-level strict mode

```yaml
settings:
  skills:
    require_pin: true
    allow_mutable_refs: false
```

In strict mode:

- branches without `--pin` fail,
- HTTP without checksum fails,
- tag movement is detected and reported,
- offline apply uses only lockfile + cache.

---

## 11. Conflict detection

### 11.1 Conflict classes

| Conflict | Example | Default action |
|---|---|---|
| Skill vs skill | Two skills write `.claude/skills/review/SKILL.md` | fail |
| Skill vs unmanaged file | Destination exists but is not MACC-owned | fail |
| Skill vs modified managed file | User edited generated skill file | prompt/fail |
| File vs directory | Skill wants file where directory exists | fail |
| Case-insensitive collision | `Skill.md` vs `SKILL.md` on Windows/macOS | fail |
| Path escape | Manifest writes `../../.ssh/config` | reject |
| User-level write | Skill modifies global config | consent gate |
| Adapter protected path | Skill writes adapter-owned core file | fail unless allowlisted |

### 11.2 Conflict algorithm

```text
1. Parse package manifest.
2. Expand install targets for selected tool.
3. Normalize all destination paths.
4. Reject absolute or escaping paths.
5. Apply platform-specific normalization.
6. Build destination map for the full install plan.
7. Compare against:
   - other planned files,
   - lockfile ownership,
   - filesystem state,
   - adapter protected paths,
   - user-level consent rules.
8. Emit structured conflict report.
9. Stop before writing unless a safe explicit resolution is provided.
```

Example output:

```text
Conflict detected

New skill:
  github-reviewer

Target:
  .claude/skills/review/SKILL.md

Already owned by:
  code-reviewer@claude

Resolution options:
  - choose a different alias
  - uninstall existing skill
  - install with --replace code-reviewer
```

### 11.3 Resolution flags

```bash
--replace <skill-id>
--force-managed
--allow-user-file-overwrite
--alias <new-install-id>
```

Default remains safe: no unmanaged overwrite.

---

## 12. Ownership metadata

The lockfile is authoritative, but local ownership markers improve recovery.

Example marker:

```text
.claude/skills/nextjs-rsc/.macc-owned.json
```

Content:

```json
{
  "owner": "macc",
  "skill_id": "nextjs-rsc",
  "tool": "claude",
  "lockfile_entry": "nextjs-rsc@claude",
  "installed_at": "2026-05-26T12:00:00Z"
}
```

Benefits:

- `prune` can recover when the lockfile is damaged,
- `doctor` can detect orphaned managed files,
- manual debugging is easier,
- generated directories are self-describing.

---

## 13. Apply pipeline integration

Update the `macc apply` pipeline for skills.

```text
1. Load .macc/macc.yaml.
2. Load and merge catalogs.
3. Resolve selected skills.
4. Resolve refs:
   - branch/tag -> commit SHA,
   - HTTP -> checksum validation,
   - local -> copy identity.
5. Build fetch plan.
6. Fetch into immutable cache.
7. Validate macc.package.json.
8. Build install plan.
9. Detect conflicts.
10. Secret scan generated outputs.
11. Write files atomically.
12. Update .macc/skills.lock.json.
13. Emit install report.
```

Useful apply flags:

```bash
macc apply --skills-only
macc apply --no-skills
macc apply --require-pins
macc apply --offline
macc apply --dry-run
```

Offline behavior:

```text
- use .macc/skills.lock.json only,
- use existing .macc/cache/ only,
- fail if cache entry is missing,
- never resolve branch names remotely,
- never fetch missing content,
- verify installed output against lockfile.
```

---

## 14. Token-saving hook bundles

The previously discussed token-optimization idea should be implemented as first-class MACC packages, probably with `category: hook-bundle`.

Goal: reduce context pollution by summarizing or filtering noisy tool output before it enters the assistant context.

### 14.1 Default hook bundles

Recommended initial bundles:

| Bundle ID | Purpose |
|---|---|
| `test-output-failures-only` | Keep failed tests, error messages, failing assertions, and summary; drop passing test noise |
| `lint-errors-only` | Keep lint errors and actionable warnings; collapse successful lint output |
| `stacktrace-collapse` | Collapse repetitive stack frames while preserving top error, cause chain, and project frames |
| `git-diff-stat-before-full-diff` | Show diff stat and changed files before exposing full diff |
| `log-grep-error-first` | Surface `error`, `warn`, `fatal`, `panic`, `exception`, and recent surrounding context first |
| `build-output-summary` | Keep build errors and bundle summary; collapse successful compilation logs |
| `package-manager-noise-filter` | Collapse dependency install progress, audit banners, and repeated network retry logs |
| `coordinator-event-summarizer` | Summarize MACC coordinator event streams by task, phase, and state transition |
| `performer-log-summary` | Summarize long performer logs into action/result/error sections |

### 14.2 Configuration in `.macc/macc.yaml`

```yaml
hooks:
  output_summarization:
    enabled: true
    default_token_budget: 1200
    bundles:
      - id: test-output-failures-only
        tools: [claude, gemini]
        applies_to:
          commands: ["pnpm test", "pnpm test:e2e", "pytest", "cargo test"]
        token_budget: 1000

      - id: git-diff-stat-before-full-diff
        tools: [claude, codex, gemini]
        applies_to:
          commands: ["git diff", "git show"]
        token_budget: 1500
```

### 14.3 Adapter-specific generation

MACC should generate hook configs differently per tool.

```text
Canonical hook bundle
        ↓
Adapter renderer
        ↓
Claude hook/instruction files
Gemini hook config or summarizeToolOutput equivalent
Codex instruction/fallback wrapper
Generic shell wrapper fallback
```

For tools with native hook support, generate native hook configuration.

For tools without native hook support, generate:

- wrapper scripts,
- tool instructions,
- command aliases,
- or fallback skill files explaining when and how to summarize.

### 14.4 Hook package manifest

```json
{
  "type": "skill",
  "category": "hook-bundle",
  "id": "stacktrace-collapse",
  "version": "0.1.0",
  "targets": {
    "claude": [
      {
        "src": "claude/hooks/stacktrace-collapse.md",
        "dest": ".claude/hooks/stacktrace-collapse.md"
      }
    ],
    "gemini": [
      {
        "src": "gemini/hooks/stacktrace-collapse.json",
        "dest": ".gemini/hooks/stacktrace-collapse.json"
      }
    ]
  }
}
```

### 14.5 Hook safety rules

Hook bundles must:

- never hide the command exit code,
- never suppress the final failure reason,
- preserve enough context for debugging,
- provide an escape hatch to view full output,
- mark summarized output clearly,
- avoid irreversible log deletion,
- not execute remote code.

Recommended output header:

```text
[MACC summarized tool output]
Original command: pnpm test:e2e
Original exit code: 1
Summary policy: test-output-failures-only
Full output location: .macc/log/performer/<file>.log
```

---

## 15. TUI and Web UI integration

### 15.1 Skills page structure

The TUI/Web Skills page should have five primary views:

```text
Available | Selected | Installed | Updates | Conflicts
```

### 15.2 Available view

Show:

- skill title,
- description,
- supported tools,
- source,
- recommended ref,
- risk level,
- tags,
- install button,
- target preview.

### 15.3 Selected view

Show what `.macc/macc.yaml` currently requests:

- skill ID,
- tool,
- source,
- requested ref,
- pin preference,
- whether it has been applied.

### 15.4 Installed view

Show what exists on disk:

- tool,
- installed files,
- source URL,
- requested ref,
- resolved SHA/checksum,
- status,
- local modifications,
- uninstall/prune actions.

### 15.5 Updates view

Show:

- pinned and unchanged,
- unpinned with newer commit,
- tag movement warning,
- modified local files,
- missing cache,
- unreachable source.

### 15.6 Conflicts view

Before apply/install, show:

- conflicting destination,
- current owner,
- new owner,
- conflict class,
- recommended resolution.

### 15.7 Provenance drawer

Every skill should expose a provenance drawer:

```text
Skill: nextjs-rsc
Tool: claude
Source: https://github.com/brand201/macc-skills
Requested ref: main
Resolved SHA: 9f31c2a8f3b6...
Package version: 0.3.1
Manifest digest: sha256:...
Installed paths:
  .claude/skills/nextjs-rsc/SKILL.md
  .claude/skills/nextjs-rsc/examples.md
Status: clean
```

---

## 16. Web API additions

Add endpoints under `/api/v1/skills`.

| Method | Endpoint | Purpose |
|---|---|---|
| `GET` | `/api/v1/skills/available` | List catalog skills |
| `GET` | `/api/v1/skills/selected` | List selected skills from config |
| `GET` | `/api/v1/skills/installed` | List installed skills from lockfile/filesystem |
| `GET` | `/api/v1/skills/status` | Combined health/status report |
| `POST` | `/api/v1/skills/install-plan` | Build plan without writing |
| `POST` | `/api/v1/skills/install` | Install with confirmation gate |
| `POST` | `/api/v1/skills/update-plan` | Build update plan |
| `POST` | `/api/v1/skills/update` | Apply update with confirmation gate |
| `POST` | `/api/v1/skills/verify` | Run verification |
| `POST` | `/api/v1/skills/prune-plan` | Build prune plan |
| `POST` | `/api/v1/skills/prune` | Apply prune with confirmation gate |
| `DELETE` | `/api/v1/skills/{id}` | Uninstall selected skill |

Use the existing MACC Web error envelope style:

```json
{
  "error": {
    "code": "MACC-WEB-3001",
    "category": "conflict",
    "message": "Skill destination conflicts with an unmanaged file.",
    "retryable": false,
    "recommended_action": "Choose another alias or allow explicit overwrite."
  }
}
```

Suggested skill-specific error codes:

| Code | Meaning |
|---|---|
| `MACC-SKILL-1001` | Catalog parse error |
| `MACC-SKILL-1002` | Skill not found |
| `MACC-SKILL-1003` | Unsupported tool target |
| `MACC-SKILL-2001` | Source resolution failed |
| `MACC-SKILL-2002` | Mutable ref blocked by policy |
| `MACC-SKILL-2003` | Checksum mismatch |
| `MACC-SKILL-3001` | Destination conflict |
| `MACC-SKILL-3002` | Path escape rejected |
| `MACC-SKILL-3003` | Unmanaged file overwrite rejected |
| `MACC-SKILL-4001` | Manifest invalid |
| `MACC-SKILL-4002` | Cache entry missing |
| `MACC-SKILL-4003` | Lockfile drift detected |

---

## 17. Rust module structure

Recommended additions to the existing tree:

```text
core/src/catalog/
  mod.rs
  skills_catalog.rs
  catalog_source.rs
  catalog_merge.rs
  catalog_validation.rs

core/src/skills/
  mod.rs
  model.rs
  commands.rs
  resolver.rs
  lockfile.rs
  status.rs
  verify.rs
  update.rs
  prune.rs
  diff.rs
  conflicts.rs
  ownership.rs
  hooks.rs

core/src/fetch/
  git_fetch.rs
  http_fetch.rs
  local_fetch.rs
  cache_key.rs
  cache_index.rs
  atomic_cache.rs

core/src/packages/
  manifest.rs
  validation.rs
  target_expansion.rs

core/src/install/
  install_plan.rs
  atomic_install.rs
  path_safety.rs
  secret_scan.rs

cli/src/commands/skills.rs

tui/src/screens/skills.rs

cli/src/commands/web/skills.rs
```

Keep most logic in `core` so CLI, TUI, and Web share the same behavior.

---

## 18. Core Rust data structures

Conceptual model:

```rust
pub struct SkillSelection {
    pub id: String,
    pub tool: String,
    pub source: Option<String>,
    pub reference: Option<String>,
    pub pin: bool,
    pub alias: Option<String>,
}

pub struct ResolvedSkillSource {
    pub kind: SourceKind,
    pub url: Option<String>,
    pub requested_ref: Option<String>,
    pub resolved_ref: Option<String>,
    pub checksum: Option<String>,
    pub subpath: String,
    pub pinned: bool,
}

pub struct SkillLockEntry {
    pub id: String,
    pub tool: String,
    pub source: ResolvedSkillSource,
    pub package: LockedPackage,
    pub cache: CacheRef,
    pub installed: InstalledTargets,
}

pub struct InstallTarget {
    pub src: PathBuf,
    pub dest: PathBuf,
    pub digest: Option<String>,
    pub owner: InstallOwner,
}

pub enum SkillStatusKind {
    Clean,
    Modified,
    MissingFiles,
    CacheMissing,
    Unpinned,
    SourceUnreachable,
    Conflict,
    Orphaned,
    UnsupportedTool,
    ManifestInvalid,
}
```

---

## 19. Security model

### 19.1 Package safety

Remote packages must be data-only.

Reject by default:

- post-install scripts,
- executable files,
- symlinks,
- absolute paths,
- path traversal,
- hidden writes outside allowed destinations,
- direct secret values in MCP env fields.

### 19.2 Secret scanning

Before writing files, scan generated outputs for likely secrets:

- API keys,
- private keys,
- tokens,
- `.env`-style secrets,
- cloud credentials,
- OAuth secrets.

If found:

```text
Fail install.
Show file path and pattern class.
Do not print the secret value.
```

### 19.3 User-level consent

Any package that writes user-level config requires explicit consent.

Install review must show:

- destination path,
- backup path,
- merge strategy,
- risk level,
- rollback command.

### 19.4 Future hardening

Post-MVP roadmap:

- signed catalogs,
- signed tags,
- trusted publishers,
- transparency log,
- package reputation metadata,
- allowlisted official sources,
- lockfile signature verification.

---

## 20. Testing strategy

### 20.1 Unit tests

Test pure logic:

- catalog parsing,
- catalog merging,
- source resolution,
- cache key generation,
- manifest validation,
- target expansion,
- path normalization,
- case-insensitive collision detection,
- lockfile read/write,
- status classification,
- update planning,
- prune planning.

### 20.2 Integration tests

Test full flows:

```text
install skill from Git tag
install skill from Git branch with --pin
install skill from HTTP archive with checksum
install multiple skills from same fetch unit
install with sparse checkout
status after install
status after manual file modification
update dry-run
update unpinned branch
verify offline
prune orphaned skill
conflict with unmanaged file
conflict with another skill
```

### 20.3 Cache flakiness tests

Specific tests for the fragility issue:

1. Simulate interrupted fetch leaving `.tmp` directory.
2. Ensure install ignores temp cache.
3. Simulate branch moving between runs.
4. Ensure pinned cache remains unchanged.
5. Simulate cache index pointing to missing path.
6. Ensure `doctor --fix` rebuilds or cleans index.
7. Run concurrent installs for same fetch unit.
8. Ensure only one final immutable cache directory is used.

### 20.4 Cross-platform path tests

Test on Linux, macOS, and Windows:

- slash normalization,
- drive-letter rejection,
- case-insensitive collision,
- reserved Windows filenames,
- symlink behavior,
- file-vs-directory collisions.

---

## 21. Acceptance criteria

### 21.1 CLI

- `macc skills available` lists catalog skills.
- `macc skills status` shows installed skills per tool with source URL and resolved ref.
- `macc skills install` supports `--reference` and `--pin`.
- `macc skills install` records `requested_ref` and `resolved_ref`.
- `macc skills update` respects pinned SHAs by default.
- `macc skills update --dry-run` shows planned changes.
- `macc skills verify` detects lockfile/cache/filesystem drift.
- `macc skills prune` removes only MACC-owned orphaned skill files.
- `macc skills diff` shows local modifications.
- `macc skills uninstall` removes selected skills safely.

### 21.2 Reproducibility

- `.macc/skills.lock.json` is generated and stable.
- `.macc/cache/` is ignored by Git.
- Offline apply uses lockfile + cache only.
- Cache keys are based on immutable resolved refs or checksums.
- Partial cache writes cannot be used as install sources.
- Mutable branch refs warn unless explicitly allowed.
- Strict mode blocks unpinned mutable refs.

### 21.3 Conflict detection

- Conflict detection runs before writing.
- Skill-vs-skill conflicts are detected.
- Skill-vs-unmanaged-file conflicts are detected.
- Modified managed files are not overwritten silently.
- Case-insensitive collisions are detected.
- Path escapes are rejected.
- User-level writes require consent.

### 21.4 Hook bundles

- Default hook bundles are available from catalog.
- Hook bundles can be selected per tool.
- `macc apply` renders hook configs through adapters.
- Hooks preserve exit codes and full-output pointers.
- Hooks clearly mark summarized output.
- Hook bundles can be installed, updated, verified, and pruned like skills.

### 21.5 TUI/Web

- Skills UI has Available, Selected, Installed, Updates, and Conflicts views.
- Provenance drawer shows source, ref, SHA/checksum, digest, and installed paths.
- Install/update/prune flows show plan previews.
- Conflicts are surfaced before writes.
- Consent gates are enforced for risky actions.

---

## 22. Phased roadmap

### Phase 1 — Inventory and lockfile

Scope:

- `macc skills available`
- `macc skills status`
- `.macc/skills.lock.json`
- installed file digests
- ownership markers
- basic catalog parsing

Outcome:

```text
Users can see what is available and what is installed.
MACC can distinguish selected vs installed vs locked.
```

### Phase 2 — Pinning and immutable cache

Scope:

- `--pin`
- `require_pin` policy
- immutable cache keys
- atomic cache writes
- sparse checkout with resolved refs
- offline apply behavior

Outcome:

```text
Installs are reproducible and branch drift is controlled.
```

### Phase 3 — Conflict detection

Scope:

- install plan destination map,
- filesystem conflict checks,
- lockfile ownership checks,
- case-insensitive path handling,
- path escape rejection,
- conflict report UX.

Outcome:

```text
MACC never silently overwrites user files or colliding skills.
```

### Phase 4 — Update, verify, prune

Scope:

- `macc skills update`,
- `macc skills verify`,
- `macc skills prune`,
- `macc skills diff`,
- `macc skills uninstall`,
- drift diagnostics.

Outcome:

```text
Skills have a complete lifecycle.
```

### Phase 5 — Token-saving hook bundles

Scope:

- hook-bundle package category,
- default bundles,
- adapter renderers,
- token budget config,
- fallback behavior for tools without native hooks.

Outcome:

```text
MACC reduces noisy tool output before it consumes assistant context.
```

### Phase 6 — TUI/Web catalog UX

Scope:

- Available/Selected/Installed/Updates/Conflicts views,
- provenance drawer,
- install/update/prune flows,
- Web API endpoints,
- consent gates.

Outcome:

```text
The Skills & Catalog subsystem becomes visible and manageable from the UI.
```

### Phase 7 — Marketplace hardening

Scope:

- signed catalogs,
- trusted publishers,
- signed tags,
- package reputation,
- remote indexes,
- optional transparency log.

Outcome:

```text
MACC can safely scale toward a public skill marketplace.
```

---

## 23. Recommended MVP slice

The smallest valuable implementation should include:

```text
1. .macc/skills.lock.json
2. immutable Git cache key by resolved SHA
3. macc skills status
4. macc skills install --pin
5. conflict detection before writes
6. macc skills verify
7. macc skills update --dry-run
8. initial hook bundles as catalog entries
```

This gives immediate value without requiring the full marketplace vision.

---

## 24. Final recommendation

MACC should treat Skills & Catalog as a reproducible, inspectable, lockfile-driven package subsystem.

The critical architectural shift is to separate:

```text
available   -> catalog
selected    -> .macc/macc.yaml
installed   -> generated tool files
locked      -> .macc/skills.lock.json
cached      -> .macc/cache/
```

This separation solves the current ambiguity and makes future features easier:

- status reporting,
- SHA pinning,
- offline reproducibility,
- safe updates,
- conflict detection,
- pruning,
- Web/TUI observability,
- skill marketplace trust.

The token-saving hook bundles should be implemented through the same mechanism. They are simply a special category of data-only skill package that adapters render into tool-specific hook/config files.

Together, these changes make MACC safer, more deterministic, more debuggable, and more scalable across tools and machines.

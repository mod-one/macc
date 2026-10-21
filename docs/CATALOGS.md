# MACC Catalog Management

MACC uses local catalogs to manage **Skills** and **MCP Servers**. These catalogs act as a bridge between remote sources (Git repositories, HTTP archives) and your local AI tool configurations.

## File Locations

By default, catalogs are stored in your project's `.macc` directory:
- `.macc/catalog/skills.catalog.json`: Registry of available skills.
- `.macc/catalog/mcp.catalog.json`: Registry of available Model Context Protocol (MCP) servers.

## Catalog Schema

Catalogs follow a standard JSON schema:

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | `string` | Version of the catalog schema (currently `1.0`). |
| `type` | `string` | `skills` or `mcp`. |
| `updated_at` | `string` | Last modification timestamp. |
| `entries` | `array` | List of catalog entries. |

### Entry Schema

Each entry in the `entries` array contains:

| Field | Type | Description |
|-------|------|-------------|
| `id` | `string` | Unique identifier for the entry. |
| `name` | `string` | Human-readable name. |
| `description` | `string` | Short description of what it does. |
| `tags` | `string[]` | List of tags for categorization and search. |
| `selector.subpath` | `string` | Path inside the source (e.g., `skills/my-skill`). Use `""` or `"."` for the root. |
| `source.kind` | `string` | `git` or `http`. |
| `source.url` | `string` | URL to the Git repository or HTTP ZIP archive. |
| `source.ref` | `string` | Git branch, tag, or commit SHA (only for `git`). |
| `source.checksum` | `string?` | Optional SHA256 checksum (recommended for `http`). |
| `mandatory` | `boolean?` | When `true`, clients keep the skill selected and refuse user removal from skill menus. |

---

## CLI Workflows

### 1. Manual Add (Advanced)
If you know the exact source details, you can add an entry manually:

```bash
# Add a skill from a specific subpath in a Git repo
macc catalog skills add \
  --id my-skill \
  --name "My Skill" \
  --description "A custom skill" \
  --kind git \
  --url "https://github.com/user/repo.git" \
  --reference "main" \
  --subpath "path/to/skill"

# Add an MCP server from a ZIP archive
macc catalog mcp add \
  --id brave-search \
  --name "Brave Search" \
  --description "Search the web" \
  --kind http \
  --url "https://example.com/mcp.zip" \
  --checksum "sha256:..."
```

### 2. Import from URL (Recommended for Git)
MACC can automatically parse GitHub tree links to extract the repository URL, reference, and subpath.

```bash
macc catalog import-url \
  --kind skill \
  --id custom-skill \
  --url "https://github.com/org/repo/tree/v1.2.0/skills/target-skill"
```

### 3. Remote Search & Discovery
Search for entries on a remote MACC registry and import them directly.

```bash
# Search for skills related to 'testing'
macc catalog search-remote --kind skill --q "testing"

# Search and add all matches to your local catalog
macc catalog search-remote --kind skill --q "workflow" --add

# Add specific IDs from search results
macc catalog search-remote --kind skill --q "aws" --add-ids "aws-s3,aws-lambda"
```

### 4. Direct Installation
Once an entry is in your catalog, you can install it into your AI tool's directory.

```bash
# Install a skill for Claude Code
macc install skill --tool claude --id my-skill

# Install an MCP server (automatically merges into .mcp.json)
macc install mcp --id brave-search
```

---

## Security Constraints

MACC is designed with safety as a priority:

1. **No Post-Install Scripts**: MACC never executes code downloaded from remote sources. It only materializes files and merges configurations.
2. **Symlink Rejection**: During ZIP extraction or file-walks, MACC explicitly rejects symlinks to prevent directory traversal attacks.
3. **Checksum Verification**: For `http` sources, it is strongly recommended to provide a `checksum` (SHA256). MACC verifies the download before use.
4. **Secret Scanning**: All generated files and merged JSON/YAML outputs are scanned for common secret patterns (API keys, tokens) before being written to disk.
5. **Atomic Writes & Backups**: Every change made by `macc apply` or `macc install` is atomic and backed up in `.macc/backups/`.
6. **MCP template placeholders**: Template definitions listed under `mcp_templates` in `.macc/macc.yaml` may reference commands/arguments and environment variables, but the entries must use placeholder values such as `${BRAVE_API_KEY}` or `YOUR_API_KEY_HERE`. The `auth_notes` field should explain where real secrets must be provided locally; MACC never writes real credentials to disk.

## Automatic skill availability checks

MACC checks declared skill sources without installing or running their content. The shared core checker is independent of AI tool adapters; CLI, TUI and Web consume the same results.

| State | Meaning | Selection and installation |
|---|---|---|
| `available` | The declared directory contains a supported entry point: `SKILL.md`, `skill.md`, `README.md` (legacy compatibility), or `macc.package.json`. | Enabled. |
| `unavailable` | A local directory is missing/invalid, or a path/entry point is absent in an accessible GitHub reference. | New selection and installation are blocked, with the reason displayed. |
| `unknown` / not verified | Network timeout, API rate limit, unreadable/private repository, invalid API response or unsupported transport. | Remains selectable; ordinary fetch validation still applies. |

A GitHub 404 only proves a missing path when its repository reference can also be read. An inaccessible repository is never automatically classified as deleted. This is an availability check, not a security audit or a guarantee that a package will install successfully.

### Client workflow

- **CLI:** `macc skills available` checks stale entries before listing. `macc catalog skills list` and `search` use the same cache. `macc skills install` checks the actual requested reference, including `--reference`; the legacy `macc install skill` path uses the same gate. JSON output includes the optional `availability` object per entry.
- **TUI:** checks start in the background at launch and when browsing Skills. Results update without losing the current selection. Skills show an unavailable badge and details. `[r] Recheck sources` is available in the footer, help and command palette.
- **Web:** Settings → Skills & Catalog shows availability and reasons, disables Install for confirmed missing skills, and updates results while a background check runs. **Recheck availability** forces a new check. The server also rejects new unavailable selections through the config API.
- **Apply:** shared fetch resolution rejects a selected skill with a confirmed unavailable status before attempting materialization. Existing selections are retained for an explicit user decision; mandatory skills remain mandatory and produce an actionable error if unavailable.

```bash
macc skills available
macc skills available --refresh
macc skills available --refresh --json
```

### Cache and offline behavior

Results are stored separately in `.macc/state/catalog-availability.json`, keyed by source, reference, selector, category and skill ID. No catalog declarations, config selections, installed files or lockfile entries are deleted by a check. Runtime status is stripped when saving a source catalog. Editing a declaration invalidates its previous result; a successful recheck restores a repaired entry automatically.

Confirmed results are cached for six hours; unknown/failed checks retry after five minutes. A transient refresh failure retains the last confirmed result, labels it stale and records `last_error`. Offline mode skips all remote checks and retains cached results. Local sources are checked on each catalog read. Unchecked transports remain unknown and use the existing installer validation.

Checks use bounded anonymous GitHub metadata requests through `curl` (four workers, eight seconds per request, a 30 second scheduling budget). In-flight requests may finish after that budget; deferred checks remain unknown. Missing `curl`, rate limits or network failures cannot disable an otherwise unverified entry. There is no login requirement or execution of remote scripts. The Web endpoint `GET /api/v1/catalog/skills/available?refresh=true` starts a forced background refresh; its `checking` flag indicates work in progress.

## Reproducible installation and explicit updates

### Version contract

A confirmed Apply records the actual Git commit SHA, original tracking ref, source URL, selected subpath and copied file digests in `.macc/skills.lock.json`. Subsequent CLI, TUI, Web and worker Apply plans resolve selected skills from these pins. Different tools can retain different versions of the same skill. A newly used tool inherits an existing project pin for that skill until explicitly updated. A catalog branch change does not silently replace an installed version.

Existing intent-only locks for selected skills without a resolved commit are completed by the next confirmed Apply; an explicit reinstall completes older unselected installation intents. Existing pins remain authoritative. Invalid pinned SHAs or unsupported future lock versions fail with an actionable error. Legacy lock version 0 is accepted as version 1. HTTP archives are pinned by their downloaded SHA-256 checksum; local source directories retain their live development semantics.

### Cache contract

Sources are prepared under an advisory lock and published with an atomic directory rename:

```text
~/.macc/cache/
  sources-v2/<source-and-version-hash>/
    receipt.json
    tree/
  source-aliases/<request-hash>.json
  source-locks/
```

The project `.macc/cache/` is the fallback when the user cache is unavailable. Workers can also read the reference project's fallback cache. The cache key includes source identity, immutable version/checksum and selected subpaths. A cached superset of Git paths can satisfy narrower requests at the same commit.

Published trees contain copied content, have read-only regular files and are never checked out in place. A tree fingerprint is verified on reuse; detected corruption stops installation rather than rewriting an artifact another worker may be reading. Git metadata is excluded from installed snapshots. Each project/worktree receives independent file copies. Old mutable Git caches can be imported using committed objects without altering their checkout, including offline when the required objects exist.

A verified cached SHA/checksum requires no new source download or Git fetch. Normal Apply reuses frozen versions; mutable request aliases also reuse the last resolved cached version until an explicit update. The separately cached availability check can still perform metadata requests after its expiry. Offline mode disables remote source and availability requests. Project preparation may independently check enabled tools/providers.

### Commands

Both installation spellings use the same real installation, preview and confirmation:

```bash
macc skills install <id> --tool <tool>
macc install skill --tool <tool> --id <id>
macc skills install <id> --tool <tool> --reference <branch-tag-or-sha>
macc skills install <id> --tool <tool> --source <url>
macc skills install <id> --tool <tool> --dry-run

macc skills update
macc skills update <id> --tool <tool>
macc skills update <id> --tool <tool> --dry-run
macc skills update <id> --tool <tool> --latest
```

All Git installations pin automatically; `--pin` remains accepted for compatibility. Updates resolve the saved tracking branch/tag explicitly. A skill explicitly requested at a SHA keeps that SHA unless `--latest` selects the current catalog tracking ref. Installation records the tool-specific selection, copies files and writes the lock receipt; it no longer merely records an intent and asks for another Apply to obtain skill files. Package installation validates package content and confines writes to skill directories and the lockfile. Full tool authentication, project configuration generation and O2 preparation belong to `macc apply`.

Dry runs can populate source caches but leave project selections, installed files and the lockfile unchanged. Cancellation before application restores the staged selection using a configuration revision check. Local edits covered by recorded digests, and differing pre-existing skill files without ownership digests, are protected against automatic overwrite. Concurrent lock changes or file edits after review require a fresh preview. A failed application retains its selection and any partial work for recovery.

### Preservation and sharing

New worker worktrees inherit the reference project's lockfile. An existing worker lock is retained to preserve its in-progress context. `macc clear` retains version pins so the next Apply restores the same versions. Configuration saves include `config/skills.lock.json`, validate it on restore and keep source caches excluded from bundles.

Share `.macc/skills.lock.json` with the project when reproducibility across machines is required. With an existing blanket `.macc/` ignore rule, track it explicitly:

```bash
git add -f .macc/skills.lock.json
```

Lock receipts are deterministic: incidental planning timestamps are not regenerated on each preview; Apply logs hold execution times. Unavailable locked content fails explicitly instead of falling back to a newer branch version.

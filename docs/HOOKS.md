# Managed hooks

MACC maintains a dedicated `catalog/hooks.catalog.json`. Hooks are deterministic
processors, separate from skills and AI agents. Seven hooks are bundled with the
binary and need no network, account or vendor-specific installation:

| Hook | Boundary |
| --- | --- |
| `test-output-failures-only` | Explicit test output contract |
| `lint-errors-only` | Explicit lint output contract |
| `stacktrace-collapse` | Explicit stack trace contract |
| `git-diff-stat-before-full-diff` | Explicit diff contract |
| `log-grep-error-first` | Explicit log output contract |
| `coordinator-event-summarizer` | Coordinator evidence sent to supervisor analysis |
| `performer-log-summary` | Performer evidence sent to supervisor analysis |

The processors run in MACC-owned executions through the common adapter layer.
Selected supported adapters also deploy a native shell-output bridge during Apply
(see Native tool integration below). Other adapters retain MACC-only processing.
Markdown copied into a vendor hook directory is not an executable registration.
Existing merge-repair scripts retain their separate adapter lifecycle; this catalogue
manages output and evidence processors.

## Activation and ownership

All seven built-ins are available and enabled by default. A tool-output processor
runs only when explicitly selected in `context.summarization.default_bundles` or a
per-skill output contract. It is never inferred from arbitrary command output.
The context engine must have output reduction enabled, or legacy summarization
must be enabled, for that execution boundary to reduce output.

O2 output reduction requires `performer-log-summary`. A configured supervisor
requires `coordinator-event-summarizer` and `performer-log-summary`. Configured output contracts used by O2 reduction or enabled legacy summarization claim
the hooks they name. The UI shows every active claim and rejects a disable/remove
request while any claim remains. Disabling the owning feature releases its claim;
manual preferences remain intact. A prior manual opt-out is masked while a feature
requires the hook and becomes effective again when that feature releases it.
Restoring defaults removes only the manual override.

Feature claims are derived, not writable metadata. External packages cannot declare
mandatory status or feature ownership. Generic configuration writes cannot turn off
an effective required hook. CLI/TUI/Web mutations use the same revision-checked
configuration transaction and existing project-control ownership gates. A stale
client must reload and review again; MACC does not silently overwrite another edit.

## Clients

- CLI: `macc hooks list` or `macc hooks list --json`.
- TUI: `[H] Hooks` from Home, or search Hooks in `[F2] Commands`.
  Select a row for integration, owner and source details. Space reviews activation,
  `d` restores defaults, PgUp/PgDn scroll details or a review, `i` imports a local manifest, Delete removes an external
  snapshot, `/` filters and `r` reloads. Enter confirms a reviewed change.
- Web: **Setup → Hooks** (`/config/hooks`), with search, owner explanations,
  confirmation dialogs and a local `hook.json` file picker.

Hook settings are canonical configuration, included in full `macc save` profiles
and copied with configuration into worktrees. Built-in execution is embedded;
external snapshots are stored with configuration and do not need source checkouts
at runtime. Use Apply after configuration changes to refresh preparation receipts
before starting the coordinator.

```sh
macc hooks disable lint-errors-only
macc hooks enable lint-errors-only
macc hooks reset lint-errors-only
# Optional for automation: pass --revision <revision-from-list> to mutations.
```

## External hooks, schema v1

External packages use portable declarative rules. Importing is explicit, defaults
to disabled, and snapshots the reviewed manifest by canonical SHA-256. Discovery,
import and execution do not fetch remote content. Obtain a manifest from a reviewed
repository checkout or downloaded artifact, then inspect and import it. A new
version requires explicit removal/reimport; a used output contract must first be
released. There are no implicit updates or mutable runtime references.

```json
{
  "id": "project-warning-filter",
  "name": "Project warnings",
  "description": "Keep warnings from the project-check skill.",
  "version": "1.0.0",
  "event": "tool_output",
  "default_enabled": false,
  "handler": {
    "kind": "rules",
    "steps": [{"kind": "retain_lines", "pattern": "(?i)warning|error"}]
  },
  "skills": ["project-check"],
  "source": {
    "repository": "https://example.org/team/hooks.git",
    "subpath": "hooks/project-warning-filter",
    "distribution": "snapshot"
  }
}
```

```sh
macc hooks inspect ./hook.json
macc hooks import ./hook.json --sha256 <canonical-sha256-from-inspect>
macc hooks enable project-warning-filter
macc apply
```

Supported events are `tool_output`, `coordinator_evidence` and `performer_evidence`.
For tool output, `skills` specifies exact skill IDs; empty means explicit bundle
selection only. Evidence processors run at their named supervisor boundary.
`retain_lines` keeps matching lines. `replace` replaces each regex match with a
**literal** replacement string (no shell execution or capture expansion). Up to
16 steps, 128 catalogue entries, 256 KiB per imported manifest and 8 MiB per
processing input/output are allowed. Regex compilation has an explicit size limit.
Builtin IDs cannot be shadowed. Arbitrary executable and imported native vendor hook
plugins are outside this v1 contract.

## Preservation and failure behavior

Live protocol JSON, quota/session markers and failed tool commands bypass output
filtering. Adapter transforms cannot bypass the enabled managed processors.
External rules run on copies, never rewrite raw logs, and fall back to bounded
raw output on failure. Existing raw artifact links, permissions and output budgets
remain in force. Coordinator evidence only collapses identical events; distinct
failures and state transitions remain in their original order. Always inspect the
raw evidence for complete logs.

## Distribution and compatibility

`/home/brand/skills/hooks/` contains package manifests, HOOK documentation, and
legacy SKILL discovery aliases for the seven built-ins. Legacy hook-bundle entries
in the skills catalogue remain compatibility aliases; use the Hooks clients for
activation. MACC embeds its catalogue and implementation, so a release works
offline. Local package changes become remotely available only after the skills
repository is committed and published; MACC does not claim an unpublished SHA.


## Native tool integration

Apply now deploys native lifecycle bridges through `ToolAdapter::native_output_hook`.
This capability is optional: unsupported adapters explicitly report unsupported.
Current implementations cover Unix Claude and Codex:

| Adapter | Registration | Launcher | Result protocol |
| --- | --- | --- | --- |
| Claude | `.claude/settings.json` | `.claude/hooks/macc-output.sh` | `PostToolUse.updatedToolOutput`, preserving typed result fields |
| Codex | `.codex/hooks.json` | `.codex/hooks/macc-output.sh` | `PostToolUse.continue: false` and replacement feedback |

Both register `SessionStart` and `PostToolUse` for `Bash`. Apply preserves unrelated
settings, events and handlers, replaces its own managed handlers without duplicates,
and removes only its registrations when no output hooks are enabled or a tool is
removed from selection. An inactive launcher can remain on disk without a registered
handler. Reapplying in a worktree rebinds managed registrations to that worktree.
The launchers call the installed MACC executable; there is no runtime download.
Supervisor evidence hooks remain internal and do not get native registrations.

### Output contracts and protection

SessionStart tells the AI which output processors are available. The agent declares
a known output contract on the first line of its Bash command, for example:

```bash
# macc-hook: log-grep-error-first
cat application.log
```

The first line is a shell comment. MACC never executes or rewrites the command.
The hook operates only on its completed output. Imported enabled declarative hooks
can be selected by the same ID syntax. Multiple IDs are comma separated.
O2 output reduction or legacy summarization must be active; per-tool opt-outs apply.
Unannotated commands are unchanged. No arbitrary shell output is classified by
content. Native annotations are separate from per-skill contracts used by internal
MACC executions.

Known failed, interrupted, image, protocol and unknown response shapes pass through.
A numeric `exit_code`/`exitCode` of zero is accepted. Claude's documented successful
Bash PostToolUse shape without an exit code is also accepted when its typed flags
are false and stderr is empty; failures use PostToolUseFailure, which is not
registered. Unknown Codex result shapes pass through. MCP output is not intercepted.
Transforms must save at least 128 bytes and produce at most 32,000 bytes.
Full original events for replaced output are saved with private file permissions
under `.macc/log/hooks/`, and the replacement includes that raw evidence path.
Handler failures leave the original output available through provider fail-open
behavior; no blocking exit code is used. No gain is claimed until measured with
actual provider versions and output shapes.

### Status and trust

CLI (`macc hooks list --json`), TUI and Web expose these separate native states:

- **Supported**: the adapter and platform have a native bridge.
- **Configured**: both expected lifecycle handlers are registered.
- **Installed**: the generated launcher matches the current MACC launcher.
- **Locally verified**: registration and launcher integrity checks passed after Apply.
- **Execution observed**: the bridge recorded an event for the current configuration.
- **Last event reducers**: IDs actually used by the latest observed event.

`[MACC on]` means enabled in MACC, not installed or executed by a provider.
Local verification does not start a paid model session or establish provider trust.
Receipts under `.macc/state/native-hooks/` contain revision/event/time/IDs, without
raw output. They describe bridge invocation, not an independent provider attestation.
A configuration revision change invalidates observed execution until a new event.

Codex requires explicit review/trust of the exact hook definition using `/hooks`.
MACC reports this requirement and never writes trust records or bypasses approval.
New or changed hooks may require review again. Claude policies or older versions
can also prevent registration or result replacement. Restart the tool session after
Apply, then refresh Hooks to see observed execution. Existing sessions are not
silently restarted. Apply reports pending runtime verification as a warning.

Primary protocol references (checked 2026-10-23):
[Codex hooks](https://learn.chatgpt.com/docs/hooks),
[Claude hooks](https://code.claude.com/docs/en/hooks).

# Apply: prepare and verify enabled features

## Operator workflow

Save the desired settings, then run **Apply** in CLI, TUI or Web. Review the file
changes, storage migration, provider origins and preparation steps before
confirming. Once Apply succeeds, start the coordinator. Separate O2 schema,
provider install/deploy/index and MCP registration commands are no longer the
normal activation procedure.

```sh
macc apply --dry-run    # Optional preview; creates no O2 database or index
macc apply             # Review, confirm, prepare and verify
macc coordinator run
```

Apply only prepares enabled features. It preserves existing sessions, memory,
source changes and task delivery rules. It does not start AI tasks, commit or
merge project code. Existing stop/lease/authority gates still protect storage
migration and native settings. Restore saved session/O2 history before Apply if
that history must be recovered from an earlier installation.

## Shared preparation contract

The Engine plan includes a configuration revision, selected ToolSpecs, enabled
provider requirements and explicit preparation steps. Changes to requirements
require another preview/confirmation. CLI, TUI and Web execute this same plan.

1. Activate SQLite/O2 when required, with the existing migration preview,
   source fingerprint, backup and stopped-owner checks.
2. Apply generated project files and scoped MCP registration supplied by each
   selected tool's declarative adapter.
3. Check tool availability and version. If missing, run its declared installer.
   Use adapter-declared account checks and, when necessary, the declared login
   flow. No generative prompt is used to check an account.
4. For each enabled Code Intelligence adapter, verify or prepare its pinned
   runtime, deployment and local index through the provider lifecycle port.
   Healthy compatible receipts are reused. Native indexing requires no download.
5. Verify the O2 schema, rebuild/validate full-text search, check scoped MCP
   configurations and prepare/probe declared native compaction settings. Tools
   without native compaction capability retain checkpoint rotation.
6. Check current semantic quality approval when semantic search is enabled.
   Missing evidence opens the assistant to collect a human-labelled suite,
   execute the benchmark and review its measured results. Only an explicit
   approval of the exact passing report/checksum activates this gate.
7. Publish `ready` only after all required steps succeed and the configuration
   revision remains current.

Provider operations retain their existing checksums, locks, runtime/source
fingerprints, bounded subprocesses and receipts. Apply confirmation covers the
listed operations and pinned origins; manual provider commands remain available
for maintenance. Settings pages alone do not install anything.

## Interactive assistance

- **CLI:** progress and login instructions appear while preparation runs. If a
  prerequisite fails, retry, edit a typed O2 setting inside Apply, supply a labelled
  suite file, approve a passing report, or cancel. Each refreshed plan requires
  confirmation. `--json` does not grant mutation consent.
- **TUI:** Apply runs preparation outside the terminal event loop. Its assistant
  offers validated settings, suite entry, benchmark evidence and confirmations.
  Page Up/Down scrolls evidence and the Apply confirmation. Escape closes the
  assistant; completed steps and logs remain available for a later attempt.
- **Web:** the Apply page displays live progress, login instructions, completed
  steps and errors. The inline assistant accepts suite JSON and exposes the O2
  settings editor. Continuing refreshes the plan and opens its confirmation.
  Viewing preparation output and changing network permission require ownership.

Offline installation/account setup opens an explicit project network permission
question. Refusing leaves offline mode intact. Acceptance uses the exact config
revision and refreshes the reviewed plan. It is not a global network setting.

Python/uv system prerequisites, unavailable platforms, missing account access and
unsupported adapter setup cannot be fabricated. Apply reports these as blocked;
the assistant can update configured executable paths or disable an integration.
A feature with missing prerequisites is never reported as operational. Likewise,
semantic search on an empty/unreviewed corpus needs meaningful labelled evidence
or must remain disabled. Collection for policy trials can run without an approved
trial; promotion keeps its separate statistical evidence and review requirements.

## Scope, credentials and cleanup

Scoped MCP registration is project-local. Codex uses `.codex/config.toml`, Claude
uses `.mcp.json`, and AGY uses `.agents/mcp_config.json`. MACC only writes the
`macc-o2` server and inherited environment variable names. The common runner
provides the project root and a session/generation capability after acquiring its
lease. No credential is stored in a project configuration. Standalone MCP startup
without a live runner capability remains denied.

Disabling MCP or deselecting an adapter removes only a recognizable MACC-owned
registration, backing up its complete configuration first. Other MCP servers and
unrelated typed settings remain intact. Tool-specific configuration formats and
account commands live in ToolSpecs, not coordinator or client code.

## Readiness and evidence

`.macc/state/apply-preparation.json` records status, config revision, completed
steps, errors and pending questions. Bounded command logs are retained under
`.macc/log/apply/` with private file permissions; UI output is redacted. Existing
provider plans/receipts, schema backups and semantic reports remain authoritative.

Doctor performs read-only O2 readiness checks. Coordinator startup checks tool
readiness and enabled integrations before dispatch; it refreshes an existing
compatible local index when tracked sources changed. It does not install tools,
log in or download provider packages. A changed executable, revoked account or
invalidated semantic approval requires Apply again. Readiness is verified against
the current configuration, source corpus and runtime; external changes after
Apply can invalidate it.

### Adapter references

- [Codex MCP environment forwarding](https://developers.openai.com/codex/mcp/)
- [Codex configuration reference](https://developers.openai.com/codex/config-reference/)
- [Antigravity workspace MCP configuration](https://docs.cloud.google.com/application-design-center/docs/design-deploy-antigravity-cli)

# Apply: prepare and verify enabled features

See [Apply output and degraded preparation](APPLY_OUTPUT.md) for output modes,
consent in scripts and partial-index decisions.

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

## Default local features

With no O2 overrides, Apply prepares native code intelligence, its incremental
symbol index, Context Engine and bounded output reduction. It reviews any SQLite
activation/upgrade, uses the bundled native adapter's install/deploy/index
operations, then verifies readiness. No external provider installation or AI call
is needed for these defaults. Native partial-index warnings retain the existing
interactive review flow; success must not hide degraded indexing.

Explicit disabled settings remain disabled. Missing sections or fields inherit
the current defaults. Memory and its injection/extraction/MCP/semantic features,
adaptive compaction, policy tuning, Serena and Graphify remain opt-in. Output
reduction works independently of memory. These defaults and preparation steps are
shared by CLI, TUI and Web; no additional provider commands are needed.

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
   configurations and verify a registered-server MCP handshake/catalog/authorized
   control read under a temporary revocable lease; prepare/probe declared native
   compaction settings. Tools
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
`.macc/log/apply/<run-id>/` with private file permissions; UI output is redacted. Existing
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

## Runtime check failures and coordinator startup

Tool readiness failures retain their category, process exit status or launch
cause, and evidence log path across CLI, TUI and Web. A timeout, network error,
invalid/empty response, output limit or unknown exit is not an authentication
failure. Only explicit adapter evidence can trigger a declared account setup;
startup never installs tools or starts login. AGY model-list failures therefore
no longer automatically ask the operator to configure an account.

ToolSpec `performer.preparation` supports optional `timeout_seconds` (default 15,
range 1–120), `require_output`, `authentication_errors`, `network_errors`, and
`authentication_json`. String patterns match failed-command output only;
transport/process failures keep their original category. `authentication_json`
maps JSON pointers to explicit denial values, separately from `expected_json`
readiness predicates. Missing/malformed JSON is an invalid response. Pattern
classification never turns successful diagnostic chatter into a failed login.

Coordinator readiness uses the sum of declared tool version/probe budgets and
enabled provider startup budgets, plus a 30-second storage/recovery margin.
Provider adapters own their startup bound; it excludes installation and login.
This changes the launcher's waiting budget, not the individual probe deadlines.
Static O2/configuration prerequisites are checked before spawning, and live tool
checks finish in the child before it publishes readiness. The reference-branch
preflight message explicitly refers to Git checks only.

Managed children retain a private `log/coordinator/run-<pid>.stderr.log` for the
current launch. Startup errors include its bounded, redacted tail and path,
rather than replacing the real failure with a generic readiness timeout or
reading an unrelated earlier daemon failure. CLI and graphical launch paths
roll back a failed startup; the supervisor attaches only after readiness.

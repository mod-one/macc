# O2 P2-C: memory proposals and scoped MCP

P2-C adds a restricted agent port over the existing SQLite memory/checkpoint
services. It uses the common session runner and code-provider abstraction: no
AI vendor names, model selection, extra AI workers or external memory database.
These features are optional and disabled by default.

## Configuration and clients

All settings are editable in TUI **O2 → Policies** and Web **Ops → Policies**,
using the shared revision-checked field catalog. Enable `memory.enabled` first.

| Setting under `memory` | Default | Bounds / behavior |
| --- | --- | --- |
| `phase_extraction_enabled` | `false` | Propose observations when checkpoints record failure or changes requested |
| `mcp_enabled` | `false` | Allow local stdio MCP under a live session capability |
| `agent_proposals_allowed` | `true` | Expose proposal/challenge tools after MCP is enabled |
| `mcp_output_tokens` | `1000` | 256–4000 estimated tokens per complete tool response |
| `mcp_input_bytes` | `65536` | 4096–262144 bytes per request frame |
| `mcp_max_proposals_per_session` | `20` | 0–100, across lease generations of a logical session |

Policy edits do not start workers or install/register providers. Confirm
[Apply preparation](APPLY_PREPARATION.md) to register enabled scoped MCP adapters. Disabling MCP or
agent proposals takes effect on subsequent calls. After enabling additional tools,
refresh/reconnect the MCP client if it caches the tool catalog. Extraction applies to subsequent
checkpoint finishes and explicit backfills. New databases already contain the
required O2 tables. Existing O2 databases need no new migration for P2-C; pre-O2
SQLite databases still need the explicit O2 preview/backup/upgrade.

TUI **Memory → h** and Web **Memory → Agent access and phase proposals** show the
redacted MCP registration recipe. TUI **Memory → e**, then **a** to apply, and the
Web checkpoint extraction form backfill a recorded phase outcome. Both use the
same application service as:

```sh
macc o2 agent-status
macc o2 memory-extract CHECKPOINT_ID
```

## Phase extraction

Checkpoint persistence creates a deterministic, task-scoped `failed_attempt`
proposal for `failure` or `changes_requested`. The common runner recognizes the
existing provider-neutral `REVIEW_VERDICT: CHANGES_REQUESTED` contract for reviewer
sessions. This is an observation; coordinator review postchecks and task transitions
remain authoritative. Successful tests, fixes and delivery are never inferred.
Quota, interruption and precondition outcomes alone do not generate lessons.

Candidates reference the checksummed checkpoint and up to three allowed changed
source snapshots. PRD/acceptance context, blockers and available normalized review
findings remain in the referenced checkpoint. Raw transcripts and private reasoning
are not imported. Repeated observations with the same task/role/phase, revision,
source-content fingerprint, outcome and review evidence reuse the candidate ID.
Operator review is preserved. Backfill accepts only a project-owned, verified or
reconciled checkpoint with a recorded finish outcome; callers cannot assert one.
Unsafe candidate content is skipped with an audit record; database errors fail the
transaction. Checkpoint evidence remains available.

Extraction runs within checkpoint persistence, without another AI invocation,
retry loop, timer or worker. It cannot extend a drain or restart a stopped run.
Candidates remain `proposed` until the existing explicit operator review activates
them. A candidate never marks a task complete or grants permission to merge.

## MCP registration and scope

`macc o2 mcp` serves newline-delimited JSON-RPC 2.0 on local stdio only. Register
that command automatically through the selected tools’ declarative adapters during
Apply. Manual registration remains an advanced maintenance path.
The common SQLite runner exports these runtime values after acquiring a lease:

- `MACC_O2_PROJECT_ROOT`
- `MACC_O2_MCP_SESSION`
- `MACC_O2_MCP_GENERATION`
- `MACC_O2_MCP_TOKEN`

**The client's MCP configuration must forward these inherited environment values.**
Some clients filter subprocess environments. The registration adapter declares
the appropriate environment allowlist or inherited environment behavior. An
enabled tool without a scoped registration adapter is reported as unsupported. Never copy a
live token into project settings, logs, reports or committed files. The token is a
derived capability, separate from the runner's private lease token.

Each request checks project authority, session task/role/tool, retained execution
worktree, generation, token, live owner identity and lease expiry. Release, fencing,
owner death or disabled policy revokes access. Feature scope comes from the
checkpoint granted at reservation; tool arguments cannot supply alternate scope.
No capability means startup fails closed. Coordinator services still use Rust
ports directly. These checks protect this MCP boundary; they do not sandbox an
AI process that already has the same Unix user's unrestricted shell access.

## Tool surface

| Tool | Access |
| --- | --- |
| `memory.search` | Approved/current/in-scope memories, bounded cursor pages |
| `memory.get` | One approved record in checksum-linked JSON fragments |
| `memory.related_to_diff` | Approved memories referencing current changed/untracked paths |
| `memory.propose` | Task-scoped proposal from 1–10 allowed source paths |
| `memory.challenge` | Proposed challenge; leaves the approved target unchanged |
| `context.get_checkpoint` | Granted invocation checkpoint summary |
| `context.search_sources` | Existing scoped file-search provider port |
| `code.query` | Existing bounded provider query |
| `code.path` | Dependency-path query; typed unsupported if unavailable |

Proposal tools are omitted when disallowed. Code tools require
`code_intelligence.enabled`; existing provider flags, routing, freshness checks and
fallbacks remain effective. MCP never installs or mutates a provider/index.

Read operations use SQLite read-only/query-only connections and recheck the lease
before returning evidence. They exclude inactive, stale, low-confidence, foreign
scope and conflicted memories. No read updates canonical memory staleness.
Source paths reject traversal, symlinks, generated/internal directories and secrets.
Secret/control-bearing evidence fails closed rather than altering its checksum.
Results are untrusted evidence; they cannot override the PRD or operator decisions.
This is not a guarantee against every semantic prompt injection in source text.

Proposals require bounded idempotency keys within a lease generation. Replays preserve any operator decision;
changed content under the same key is rejected. The session-wide proposal cap
covers challenges too. Neither tool can promote, reject, invalidate, expand scope,
create a blocking conflict or change task state. Export/import, arbitrary commands,
provider lifecycle, lease reassignment and checkpoint writes are absent from the
fixed MCP catalog.

## Protocol and response limits

Supported lifecycle: `initialize`, `notifications/initialized`, `ping`,
`tools/list`, `tools/call`. Protocol versions 2025-03-26, 2025-06-18 and 2025-11-25
are accepted for this common subset. Unsupported version negotiation returns
2025-11-25. Notifications never perform tool writes. Invalid RPC uses protocol
errors; tool failures use `isError`. Oversized input closes the transport.

The token estimate covers the complete serialized `tools/call` response, including
JSON escaping, the request ID and text-content envelope. Oversized search results
shrink to references with pagination; `memory.get` fragments can be joined only
with an unchanged checksum. Code hits are trimmed or a bounded error requests a
narrower query. Provider-internal estimates describe the original provider result;
`transport_truncated` identifies further MCP trimming. Fixed initialization/catalog
control replies are outside the configurable tool-response budget. No whole PRD
or full mandatory checkpoint restore is silently truncated: the ordinary runner
still supplies authoritative mandatory context.

The CLI sends tracing to stderr for this subcommand so stdout stays JSON-RPC.
There is no HTTP listener, network authentication, sampling, resource subscription
or vendor-native MCP proxy in this milestone.

Protocol references: [stdio transport](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports),
[lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle),
[tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools).

## P3-A freshness extension

[Evidence freshness and retrieval quality](O2_P3_FRESHNESS_QUALITY.md) adds optional
symbol/version bindings and stronger operator conflict resolution. MCP keeps its
fixed scoped catalog and proposal-only writes; freshness checks apply to agent
search/read as well as automatic context. Capture, promotion and conflict resolution
remain separate operator actions.

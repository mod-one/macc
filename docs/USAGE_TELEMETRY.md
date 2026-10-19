# P0-D: provider-neutral invocation usage

## Contract and boundaries

`core::session_store::usage` owns telemetry in the existing `.macc/memory.db`.
Adapters declare machine-output schemas in `performer.usage`; the shared Rust
collector, shell transport, CLI, TUI and Web contain no provider-name dispatch.
Session ownership, generation fencing, model/effort resolution, permissions
(including Codex `--yolo`), explicit native resume and worktree preservation retain
their existing contracts. Telemetry never commits, resets or cleans Git work.

Each acquired lease generation has one invocation ID. Before launching the tool,
the runner persists a pending receipt and a snapshot of the capture policy and
adapter profile. After execution it stores the outcome, elapsed wall time
(second resolution), provider-reported counters and normalized incremental counters.
Interrupted calls remain visible as pending or interrupted with unknown metrics.
Capture failures warn without replacing the task's outcome. Structured text
projection failures stop completion acceptance and preserve the worktree.

Receipt finalization validates the lease. Identical retries are idempotent,
including after release; different payloads conflict. Transactions and the existing
project lock serialize concurrent updates. No database transaction spans an AI call.

## Measurements and normalization

- Dimensions: uncached input, cached reads, cached writes, output, optional
  reasoning, and cost. Missing values are `null`, distinct from measured zero.
- `token_quality: exact` means at least one incremental token dimension was
  explicitly reported. It does not claim complete coverage of every dimension.
  No character-based token estimates are introduced.
- The final matching terminal aggregate is authoritative for a one-shot runner
  invocation. Intermediate messages and repeated summaries are never added to it.
  An adapter emitting multiple distinct terminal turns needs a different contract.
- Profiles specify invocation or cumulative-session scope separately for tokens
  and cost. Cumulative values use durable watermarks scoped by tool, native
  session, profile schema and model label. Different sessions/tools never share
  a baseline. Missing baselines, missing fields, decreasing counters, mismatched
  native IDs or earlier pending invocations produce unknown deltas. A zero
  baseline is allowed only for an explicitly reserved new session on its first
  use, with a matching reported native ID. Imported transcripts cannot establish
  a zero baseline from a missing or zero legacy use count.
  Unknown gaps are not assigned to a later call. Earlier pending receipts
  conservatively prevent cumulative attribution on that logical session.
- Costs use integer **nanocurrency units** (1 currency unit = 1,000,000,000 units).
  Currency and `exact`, `estimated` or `unknown` quality remain separate. There
  is no bundled price table, automatic token pricing or subscription quota estimate.
  Provider estimates are not invoices.
- Totals group task/tool/phase/currency/cost quality, sum known incremental values
  and expose coverage per dimension. Partial sums are not complete billed totals.
  Latest receipts are limited to 200; grouped totals cover the whole store.

Usage records contain selected numeric fields, bounded model metadata, profile
schema, provenance, notes and a receipt digest. They do not contain raw output,
prompts, credentials or lease tokens. Raw task logs retain their existing logging
policy. Temporary structured captures use private `mktemp` files and normal runner
cleanup; a hard process crash may leave a private temporary file for OS cleanup.
For new runner invocations, `model` comes from the actual prepared `--model`/`-m`
arguments (after tier routing), or the adapter-declared JSON/TOML model file when
no model argument is present. A model reported in matching provider output takes
precedence, including multi-model aggregates. A configured tier alone is not
proof that a model was applied. Notes record the metadata source.

`provider_version` uses a fresh local `--version` observation when the adapter
declares a compaction version pattern. This reuses the bounded capability probe
(three seconds, 64 KiB), outside session authority/database locks; it does not
enable compaction, change provider arguments or invoke a model. Unavailable,
unrecognized or unsafe metadata remains unknown. Raw argv is not stored. These
observations do not certify other CLI capabilities and do not backfill historical
receipts or infer prices.

## Rollout and configuration

JSON session mode retains its existing behavior and does not record this ledger.
New SQLite migrations include usage schema v1. Existing SQLite projects require
a reviewed upgrade while coordinator, supervisor and runners are stopped, with
outstanding leases recovered first. The upgrade takes a consistent backup and
rejects stale source hashes. It does not rewrite existing session/checkpoint rows.

```bash
macc session-store usage-upgrade
macc session-store usage-upgrade --apply --expected-sha256 <reviewed-source-hash>
macc session-store usage
```

Both settings use the shared field catalog, revision checking, preview and Apply:

```yaml
session_management:
  usage:
    capture_mode: 0           # 0: passive; 1: request declared machine output
    max_output_bytes: 1048576 # 4096..16777216 bytes per invocation
```

Default passive mode preserves current CLI arguments except adapter-required
checkpoint protocol output (`required_for_checkpoint`), and parses structured output
only if already present; plain text typically produces unknown counters. Mode 1
changes only declared output flags, on the next invocation. It does not change
session creation/resume, permissions, model or effort. Capture settings are snapped
at invocation start. Oversized numeric capture is recorded as unknown; structured
text projection additionally has a fixed 16 MiB safety limit. Internal JSON stdin
transport is bounded to 32 MiB; escaping may exceed that bound and leave a pending
unknown receipt rather than falsely accepting measurements.

- **TUI:** `J` → Sessions edits settings; `t` opens usage, refreshed every five
  seconds. Arrows select, Enter inspects reported/incremental counters and
  provenance; `r` refreshes; `u` previews an upgrade, `a` applies the reviewed
  hash, `x` discards; PageUp/PageDown scroll, Escape returns.
- **Web:** Setup → Sessions (`/config/sessions`, also `/ops/sessions`) includes
  usage settings, task/tool/phase filtering, known totals, coverage, receipt
  inspection and reviewed schema upgrade. Refresh is every five seconds.
- **API:** `GET /api/v1/sessions/usage` is read-only. Upgrade uses existing
  `GET/POST /api/v1/sessions/authority/usage-upgrade` ownership/review gates.
- Internal `usage-begin` and `usage-end` accept bounded JSON on stdin, using the
  existing internal session-service transport and fenced lease keys.
  `usage-begin` optionally accepts `invocation: {command, args}` before prompt
  insertion; older callers may omit it. Repeated begins retain the first receipt's
  plan and metadata. Observation failures preserve normal tool execution.
- `macc save` includes the SQLite usage ledger by default; see
  [Save and Restore](SAVE_RESTORE.md). Restores keep receipts and clear cumulative
  watermarks while quarantining native transcripts. Standalone consistent SQLite
  backup includes receipts/watermarks. JSON rollback preserves
  them in SQLite and its backup, but JSON runners do not update them.

## Adapter mappings and evidence

Mappings are declarative and tested with fixtures; no paid provider calls were
used for regression tests. Verify installed CLI compatibility before enabling
structured mode in production.

| Adapter | Structured mode | Counters and scope | Cost |
| --- | --- | --- | --- |
| Codex | Append `--json` | `turn.completed.usage`, invocation; subtract cached input from total input | Unknown when not reported |
| Claude | Replace `--output-format` with `stream-json`, append `--verbose`; required for active checkpoints | Terminal `modelUsage`, aggregate across models including subagents; cumulative session watermarks | `total_cost_usd`, cumulative, estimated USD |
| Gemini | Append `--output-format stream-json` | Terminal `result.stats`, invocation; `input` uncached, `cached` separately | Unknown when not reported |
| Other tools, including Vibe | Existing arguments | Unknown until an explicit verified usage profile is supplied | Unknown |

Assistant text is projected through declared JSON pointers and event predicates
before existing checkpoint acknowledgement, completion and quota processing.
Reasoning/tool messages are not treated as assistant completion text. Gemini
streamed text chunks concatenate without inserted newlines. Original machine
output remains in task logs; appended human text is never parsed a second time
as numeric telemetry.

Primary references used for the mappings:

- [Codex non-interactive mode](https://developers.openai.com/codex/noninteractive/)
- [Claude headless execution](https://code.claude.com/docs/en/headless)
- [Claude cost tracking and cumulative model usage](https://code.claude.com/docs/en/agent-sdk/cost-tracking)
- [Gemini headless output](https://geminicli.com/docs/cli/headless/)

## Verification

```bash
cargo test -p macc-core session_store::usage --locked
cargo test -p macc-cli --test session_store_runner --locked
make test-tool-runner
npm --prefix web test -- --maxWorkers=1 src/pages/ops/Sessions.test.tsx src/pages/ops/Usage.test.tsx
```

Adaptive context-pressure decisions, native compaction probes, retrieval/context
item attribution, pricing catalogs and billing reconciliation remain later work.
This ledger supplies the normalized baseline without claiming those features.

The multi-turn assistant projection, checkpoint protocol requirement and unchanged
terminal-counter semantics are described in [runner reliability](COORDINATOR_RUN_RELIABILITY.md).

# Short paired O2 performance experiments

## Scope

`scripts/o2_benchmark.py` runs one feature experiment at a time through public MACC
CLI commands and the registered ToolSpec adapter runner. The first delivered
experiment is **session reuse**. Other O2 features are intentionally not bundled
into this treatment. This is a descriptive compatibility/performance pilot, not
a production saving guarantee or a coordinator scheduling benchmark.

The next delivered experiment is **memory/context injection** (`--feature
memory-context`), described below. Each experiment retains its own tasks and
treatment contract; results from different features are not pooled.

The default tool roster is Codex and Claude, but execution and usage interpretation
have no provider-name branches. Additional registered adapters can use the same
protocol. Optional `--tool-spec` accepts a full JSON ToolSpec for a selected custom
adapter. Resolved model/effort, profile hash, executable version, MACC binary hash,
source revision, configuration and deterministic starting Git commit are retained.

## First experiment: reuse versus fresh sessions

Each arm receives a separate disposable Git repository and a real MACC worktree.
Both start from exactly the same tracked files and use SQLite session ownership,
checkpoint validation and structured usage capture. Optional memory/context,
adaptive compaction and code-intelligence policies are disabled in both arms.

- **Fresh:** complete step 1, close the available logical session through
  `macc session-store close`, then complete step 2 in a fresh conversation.
- **Reuse:** complete the identical two steps using the same logical/native
  conversation. Actual IDs are checked; successful output alone does not prove reuse.

The implementation work from step 1 stays in the worktree in both arms. The
checkpoint system stays enabled in both arms, so this isolates session reuse
relative to safe checkpoint-backed fresh sessions. It does not compare against
disabling ownership or mandatory task restoration.

Two fixed, dependency-free Python tasks are available:

| Case | Step 1 | Step 2 |
|---|---|---|
| `slug-list` (default) | Normalize whitespace/case into slugs | Deduplicate normalized nonempty slugs, preserving order |
| `bounded-values` | Clamp an integer with invalid-bound validation | Map across a list without mutation, including empty-list validation |

Only `solution.py` may change. Behavioral assertions are held outside the agent's
project and run in a separate bounded process after each step. Tracked baseline
files and untracked additions are checked; Git HEAD and execution profiles must
remain unchanged. Tasks use the standard library, with no installation or network
required to implement them. The synthetic runtime task contract is not added to
the project's real PRD queue.

Each tool/case pair uses four invocations at most; the default pilot uses eight
across Codex/Claude. There are no automatic billed retries. Failure, quota,
timeout, missing completion or failed assertions stop that tool, preserving every
attempt and all available telemetry. Default per-invocation timeout is 120 seconds,
configurable from 10 to 300. Fixtures and unfinished work are retained.

## Commands

Build the MACC revision under test:

```bash
cargo build -p macc-cli --bin macc --locked --offline
```

The default binary is `target/debug/macc`; no release installation is necessary.
Use `--macc /absolute/path/to/macc` to test another installed binary explicitly.

Create a reviewed `benchmark-settings.json`. The values below are an example;
use models actually available to your accounts. Each selected tool needs an
explicit model and effort in the requested tier. Normal adapters perform the
runtime routing; the benchmark does not construct provider CLI flags.

A ready-to-copy example is available at
`automat/benchmarks/o2/settings.example.json`.

```json
{
  "codex": {
    "model_tiers": {"standard": {"model": "gpt-6.1-sol", "effort": "medium"}}
  },
  "claude": {
    "model_tiers": {"standard": {"model": "sonnet", "effort": "medium"}}
  }
}
```

Review a plan; it performs no tool probe or model call:

```bash
python3 scripts/o2_benchmark.py \
  --settings benchmark-settings.json \
  --output /tmp/o2-session-plan
```

Run the reviewed default pilot explicitly:

```bash
python3 scripts/o2_benchmark.py \
  --settings benchmark-settings.json \
  --output /tmp/o2-session-results \
  --execute
```

Run the second task as a separate new experiment, optionally for a single tool:

```bash
python3 scripts/o2_benchmark.py \
  --settings benchmark-settings.json \
  --case bounded-values --first-arm reuse \
  --output /tmp/o2-bounded-results --execute
```

The output directory must not exist. It is private (0700); reports and raw output
can contain model responses and should not be published without inspection.
Normal execution requires the tools' existing authentication, native session
storage and network access. It never installs or logs into tools automatically.

`--first-arm reuse` reverses the first pair; later tool/case pairs alternate.
For two separate short pilots, run `slug-list` with the default order, then
`bounded-values --first-arm reuse` to counterbalance order within each tool.
This is still only two descriptive pairs per tool, not a confirmatory sample.

Equivalent Make targets: `make test-o2-benchmark` for deterministic tests and
`make o2-benchmark O2_BENCHMARK_ARGS='...'` for a plan or explicit execution.
The standard runner validation includes only the fake-adapter tests; no paid
model calls are added to `make check`.

## Evidence and interpretation

Each output contains:

- `report.json`: configuration/provenance, every attempted arm, invocation
  receipts and comparison eligibility;
- `analysis.md`: concise results and missing-measurement explanations;
- `commands.json`: exact argv, working directory, environment overrides, stdin,
  exit status, duration and log for every MACC/Git/check command;
- `procedure.sh`: shell representation of the captured commands against the
  retained fixtures; it is an audit procedure, not an idempotent retry script;
- `command-*.log`: complete bounded subprocess output, including runner text;
- isolated repositories and worktrees with native task logs and SQLite receipts.

The driver uses `macc init --fresh`, `worktree create --skip-apply`, reviewed
`session-store migrate`, `worktree exec` with the resolved adapter runner, and
read-only `session-store status`/`usage`. The fresh arm additionally uses `close`.
It does not edit SQLite, forge usage receipts, change provider parsers or bypass
ownership with internal-invocation overrides. `--skip-apply` deliberately avoids
unrelated skill downloads, global setup and O2 provider installation in this
session-only experiment. Full Apply readiness needs its own experiment.

Metrics use MACC's [usage normalization](USAGE_TELEMETRY.md), including cumulative
session watermarks. The report retains reported and incremental counters plus
per-dimension coverage. Missing values remain unknown, distinct from measured
zero. Partial known sums are not complete token consumption. Reasoning counts
remain separate and are not blindly added to output tokens. Estimated provider
cost is visible as estimated; financial reductions require exact, complete,
same-currency measurements. No bundled prices or quota-to-cost conversion exists.

Paired reductions require both arms to pass, two usage receipts each, matching
starting revision/configuration/profile/model/version and verified session
behavior. A reduction for a token dimension additionally requires complete
coverage for that dimension. Otherwise the comparison is ineligible or that
dimension remains unknown. Raw results still remain visible. An optional manual
token reading should be collected only for a missing dimension and tied to the
specific retained invocation; it must not overwrite canonical receipts.

Wall time includes the two invocations, checks, inspection and the fresh arm's
close command; fixture preparation is reported separately through command timings
and total arm time. Summed invocation duration uses the existing normalized
telemetry and retains its second-resolution precision. No provider-side cache
flush is claimed. Order alternates across tool/case pairs; tool results remain
separate. One pair per tool cannot control service variance or establish a causal
or statistically reliable saving. Confirmatory runs need a predeclared larger
roster, counterbalanced repetitions and comparable cold/warm cache conditions.

## Next experiments, one at a time

1. Session reuse (delivered here).
2. Approved memory/context injection versus no optional injection (delivered),
   with a frozen evidence-backed corpus and short policy-dependent tasks.
3. Native parsed navigation versus literal references (delivered below).
4. Explicit output reduction, preserving raw evidence and essential diagnostics
   (delivered below).
5. Scoped memory MCP pull versus automatic push (delivered below).
6. Checkpoint rotation/recovery, verifying preserved work and acceptance after
   interruption. Native compaction requires independently observable evidence.
7. Semantic retrieval and alternative providers, each with its own prerequisites
   and exact benchmark settings.

The managed-provider experiments below cover Graphify and Serena separately.

## Native navigation and index preparation

This controlled experiment supplies public CLI navigation results to the agent
before each task step. Both variants enable code intelligence with native routing;
only the candidate enables `native_map_enabled` and prepares its parsed index.
Memory injection and adaptive compaction remain disabled, and both variants reuse
their task session. The baseline queries literal `references`; the candidate
queries `symbol_definition`. These are different documented navigation workflows,
not interchangeable semantic reference engines. The agent still inspects source.
This does not measure whether an agent spontaneously discovers or selects O2 tools.

Two tiny Python tasks delegate to an existing policy class, then add a list wrapper.
Each frozen corpus contains 27 Python files, six historical comment/string
distractors, and one authoritative definition. Only `solution.py` may change.
Independent checks cover behavior, order, duplicates, input preservation and actual
delegation to the policy, with scope/session/profile/usage gates unchanged.

Before paid calls, each arm records one query without a persisted index. The
candidate then previews and applies `provider-plan native deploy`, `index`, and
`update`, retaining the plan checksums and lifecycle receipts. Initial indexing
must parse files with zero failures and complete coverage; unchanged update must
reuse the initial parsed corpus without reparsing. Four subsequent read queries
record public CLI wall time. Neither OS caches nor provider caches are flushed;
"without a persisted index" must not be described as a physically cold disk.

Task-step queries use `--worktree` to inspect the actual worker sources. Candidate
hits must have Tree-sitter provenance, complete map coverage and exactly one
definition; a changed solution must be reflected by the read-only overlay. Raw
results, output token estimates, timing and parse/reuse diagnostics are retained.
Unsupported/fallback/mismatched evidence cannot establish a successful treatment.
CLI startup, Git/source hashing and policy/SQLite reads remain part of wall time;
the parser's own receipt elapsed time is reported separately.

```sh
python3 scripts/o2_benchmark.py --feature native-navigation \
  --settings automat/benchmarks/o2/settings.example.json \
  --output /tmp/macc-o2-benchmark/native-navigation-live-01 \
  --archive-root "$PWD/.macc/reports/o2-performance-2026-10-21" --execute

python3 scripts/o2_benchmark.py --feature native-navigation \
  --case retention-policy --first-arm indexed \
  --settings automat/benchmarks/o2/settings.example.json \
  --output /tmp/macc-o2-benchmark/native-navigation-live-02 \
  --archive-root "$PWD/.macc/reports/o2-performance-2026-10-21" --execute
```

Omit `--execute` for a no-probe/no-call plan, using a new output name. Each command
permits at most eight model invocations with zero retries. Setup and local probes
are reported separately from task execution. End-to-end arm time includes setup;
a reduction in query payload or warm parsing does not itself prove overall token,
latency or financial savings. Full commands and stdin are in each `procedure.sh`
and `commands.json`; all earlier feature archives remain unchanged.
Do not enable the next feature while interpreting the previous comparison.
Use [P3-D tuning](O2_P3_POLICY_TUNING.md) for supported numeric policy trials;
its exact-cost adoption gate is separate from this pilot and does not certify
an O2 feature-toggle comparison.

## Memory/context injection experiment

Use `--feature memory-context`. Both variants reuse their own task session across
two short steps. Both receive identical committed `docs/policy.md`, task contracts
and an identical semantic corpus containing one source-backed constraint. The
driver captures evidence through `macc o2 memory-evidence`, proposes the fact,
inspects its checksum and explicitly activates it through `memory-review`.
This is a reviewed synthetic fixture, not a claim of human approval for product work.

- **`no-push`:** Memory Ledger enabled, Context Engine disabled.
- **`push`:** same state, enable only `context_engine.enabled` through the public
  revision-checked policy preview/apply commands.

The budget is 600 estimated optional context tokens in both variants. Memory MCP,
phase extraction, semantic search, code intelligence, adaptive compaction and
output reduction remain disabled. This isolates automatic push of an approved
fact; it does not benchmark MCP pull, semantic ranking or memory extraction.
The source document is available to the agent in both variants, so the baseline
is not deprived of the information needed for correct implementation.

| Case | Step 1 | Step 2 |
|---|---|---|
| `reference-code` | Format/validate a reference using project namespace/number rules | Format a list, preserving leading zeroes, order and input data |
| `display-label` | Format names using project case/whitespace/empty-value conventions | Format a list, preserving order, duplicates and input data |

The same independent verifier and write-scope checks apply. A comparison requires
matching source revision, actual tool/model/version/profile, normalized effective
configuration and semantic corpus checksum. Only the predeclared context-enabled
flag is masked for configuration comparison. Full unmasked configuration hashes
are still retained and checked during each arm. Memory UUIDs differ by isolated
project; the corpus fingerprint uses the canonical proposal/evidence instead.

Treatment verification inspects `macc o2 context-list` and canonical usage receipts:
the baseline must have no bundles or bundle attribution, while the candidate must
have two bundles attributed to the two invocations, each selecting exactly the
expected active memory and checksum. Optional injected-size estimates remain
separate from actual provider tokens. The fake-adapter integration test also
checks that optional memory text actually reaches the adapter prompt. Neither
these checks nor a successful task prove that a real model semantically used the
fact instead of independently reading its source.

Run a no-call plan, then the first pilot:

```bash
python3 scripts/o2_benchmark.py --feature memory-context \
  --settings automat/benchmarks/o2/settings.example.json \
  --output /tmp/o2-memory-plan

python3 scripts/o2_benchmark.py --feature memory-context \
  --settings automat/benchmarks/o2/settings.example.json \
  --output /tmp/o2-memory-reference --execute \
  --archive-root .macc/reports/o2-performance

python3 scripts/o2_benchmark.py --feature memory-context \
  --case display-label --first-arm push \
  --settings automat/benchmarks/o2/settings.example.json \
  --output /tmp/o2-memory-display --execute \
  --archive-root .macc/reports/o2-performance
```

Each pilot permits eight invocations total across Codex/Claude. The second reverses
the first-arm order within each tool. Successful and failed outcomes remain in
the report; no unfavorable arm is silently retried or replaced.

### Evidence retention

`--archive-root` reserves a new private directory named after the execution output
before any model call. If it already exists, the experiment refuses to start.
On normal completion or a caught failure, the driver copies the complete evidence
tree there, including reports, logs, source fixtures, resolved profiles, policies,
runtime task contracts, normalized usage and harness source snapshots. An
`archive_manifest.json` lists checksums and marks completed copies. Existing
archives are never overwritten. Hard process termination can prevent final
archiving; the original execution directory still retains incremental reports and
command logs. SQLite files in this forensic copy are not a substitute for a
reviewed, consistent MACC backup/restore operation.

An operator interrupt stops the active command group, retains the interrupted
arm and ends the whole experiment before another tool starts. Timeout/failure
also retains evidence; a failed tool is not automatically retried on another case.

Disposable Git worktree pointers still refer to their retained original paths;
archives preserve evidence rather than relocating active worktrees. Keep the
original execution tree when replaying its captured procedure. Plans can also be
archived without tool probes or model calls. Earlier reports are never rewritten
to match a newer harness version.

## Explicit output reduction experiment

`--feature output-reduction` compares `full` and `reduced` diagnostics with Codex
and Claude through the same registered adapter/runner path. Both arms retain
their task session; memory push, code intelligence and adaptive compaction stay
disabled. Only `context_engine.output_reduction` differs, with a shared 2,000
estimated-token/65,536-byte policy. The explicit reduction endpoint operates
independently of automatic context injection.

Each task has two short implementation steps. `slug-diagnostics` normalizes text
then deduplicates slugs; `clamp-diagnostics` bounds integers then maps a list. Before
each step, tracked `diagnostics.py` actually executes 200 successful background
checks and one failing contract test using Python's standard unittest runner. A
wrapper returns the real test result (`exit_code: 1`), test count and complete text
in JSON; collecting this result does not claim the tested code succeeded.

The driver sends the full text in `full`, or supplies that same invocation's text
through public `macc o2 reduce test` in `reduced`. It verifies test identity,
source file, exception/assertion message and failure verdict survive reduction,
and checks the raw artifact byte-for-byte. Reduced output must shrink and remain
within budget. Both variants receive identical task contracts and may read their
explicitly cited raw artifact if necessary. Independent behavioral/scope/session
checks validate the implemented result after each call. Raw output hashes are
retained but not required to match between isolated arms: traceback roots, unittest
timings and subsequent implementations can differ. Fixture/corpus and normalized
effective-policy fingerprints must match.

This is controlled assistance using explicit CLI reduction. Provider-internal
shell/test outputs are outside MACC's capture boundary and are not automatically
compressed by enabling this flag. The JSON completion/usage protocol is preserved;
only the deliberately extracted test text is submitted to the reducer. A smaller
summary does not prove lower total provider tokens, financial savings, or preserved
meaning for every possible output format.

Run local checks with no AI calls, then review no-call plans (omit `--execute`)
before the two paid pilots. Each paid command allows at most eight invocations and
no automatic retries. Use new output names for every execution:

```sh
PYTHONPATH=automat python3 -m benchmarks.o2.output_checks \
  --macc target/debug/macc \
  --output /tmp/macc-o2-benchmark/output-reduction-local-02 \
  --archive-root "$PWD/.macc/reports/o2-performance-2026-10-21"

python3 scripts/o2_benchmark.py --feature output-reduction \
  --settings automat/benchmarks/o2/settings.example.json \
  --output /tmp/macc-o2-benchmark/output-reduction-live-01 \
  --archive-root "$PWD/.macc/reports/o2-performance-2026-10-21" --execute

python3 scripts/o2_benchmark.py --feature output-reduction \
  --case clamp-diagnostics --first-arm reduced \
  --settings automat/benchmarks/o2/settings.example.json \
  --output /tmp/macc-o2-benchmark/output-reduction-live-02 \
  --archive-root "$PWD/.macc/reports/o2-performance-2026-10-21" --execute
```

The local matrix covers representative test/lint/trace/diff/log samples and a
large UTF-8 budget case. Those format samples are synthetic regression fixtures;
the model pilots use actual failing test execution. Neither certifies universal
semantic preservation. Matrix failures, paid failures, full procedures, raw
artifacts and original earlier-feature reports are all retained.

## Scoped memory MCP versus automatic push

`--feature memory-mcp` reuses the same `reference-code` and `display-label` source
policies, explicit memory proposal/review, task sessions and independent acceptance
checks as the memory/context experiment. Each arm has one identical approved fact.
Both enable scoped MCP, disable agent proposals, use a 1,000 estimated-token MCP
response budget and a 600-token optional-push budget, and leave other treatments
disabled. The sole effective policy difference is `context_engine.enabled`.

- **push:** the approved policy is injected automatically at both steps; the MCP
  reader exists but must remain unused.
- **pull:** optional injection is disabled. The real agent runs
  `python3 .macc/memory_mcp_client.py search "policy"` before its first edit. It may
  reuse that retrieved policy in step two, querying again only if needed.

The common client launches public `macc o2 mcp`, negotiates stdio JSON-RPC, sends
the initialized notification and calls `memory.search` (or `memory.get` for a
reference). Runtime credentials are inherited from the actual runner lease; the
driver neither creates an internal-invocation bypass nor writes credentials into
settings, command arguments or evidence. The client logs public RPC frames,
response bytes, wall time and logical-session/generation attribution only.
Acceptance requires an actual successful scoped response delivering the approved
fact with matching ID, checksum and body, plus absence of injection in pull. Push
requires the two matching context bundles and zero reader calls. Both variants
have the authoritative policy file available for checking.

This benchmark uses the agent's ordinary shell to invoke a common MCP client.
It measures real MCP transport/access/retrieval and model usage, while avoiding
provider-specific command flags and global configuration changes. It does **not**
measure native Codex/Claude MCP tool catalogs, server discovery or their startup
cost. These require a separate client-integration comparison. The short facts fit
the search response budget; fragmented large-record retrieval is not measured.
The one-shot reader starts a server for each query, so measured RPC latency
includes initialization and shutdown rather than assuming a persistent connection.

```sh
# Omit --execute and choose a new output name for a no-call plan.
python3 scripts/o2_benchmark.py --feature memory-mcp \
  --settings automat/benchmarks/o2/settings.example.json \
  --output /tmp/macc-o2-benchmark/memory-mcp-live-01 \
  --archive-root "$PWD/.macc/reports/o2-performance-2026-10-21" --execute

python3 scripts/o2_benchmark.py --feature memory-mcp \
  --case display-label --first-arm pull \
  --settings automat/benchmarks/o2/settings.example.json \
  --output /tmp/macc-o2-benchmark/memory-mcp-live-02 \
  --archive-root "$PWD/.macc/reports/o2-performance-2026-10-21" --execute
```

Each pilot permits at most eight native invocations, no automatic billed retry,
with reversed order on the second case. Report execution/setup/total time,
per-dimension actual provider usage, optional injected-size estimates, RPC count,
response sizes and retrieval elapsed time separately. No exact cost saving follows
from unknown or estimated monetary counters; no statistically general saving
claim follows from a single pair per tool/case.

## Real managed Graphify and Serena experiments

Use an optimized binary for every arm of these experiments. Record its checksum;
do not pool these results with earlier debug-binary pilots. Graphify's package
verification and source checks are material costs, so debug-only timings can be
misleading. Rebuild without changing the installed release:

```sh
cargo build -p macc-cli --bin macc --release --locked --offline
```

`graphify-navigation` compares native literal `references` with real managed
Graphify graph traversal for the same symbol query. `serena-navigation` compares
a prepared native parsed definition with managed Serena Python/Jedi definitions
using the same definition-file scope. Each uses the existing two short frozen
navigation tasks (`charge-policy`, `retention-policy`) and independent behavioral,
delegation, allowed-file, session, profile and usage gates.

Both variants enable the respective provider policy and Code Intelligence. Only
the declared routing mode differs (`native_only` versus `adaptive`); unrelated
O2 treatments remain disabled. Candidate projects explicitly preview/apply the
real pinned `install` and `deploy` operations; Graphify also requires `index`.
The two Serena variants both prepare their native map. No PATH provider is adopted,
no global package/skill is installed and no upstream version is upgraded.
Installation may download reviewed lockfile wheels into project-owned generations.
Every command, plan/checksum, receipt, operation log and failure is retained.

Before any AI invocation, check preparation with `--prepare-only --tool codex`.
This enables actual lifecycle operations/calibration but reports a model-call
budget of zero, stops before the runner invokes AI, and produces no model saving
pairs. Without this flag or `--execute`, a plan performs no probes or downloads.

```sh
python3 scripts/o2_benchmark.py --feature graphify-navigation --tool codex \
  --settings automat/benchmarks/o2/settings.example.json --macc target/release/macc \
  --output /tmp/macc-o2-benchmark/graphify-preflight-release-01 \
  --archive-root "$PWD/.macc/reports/o2-performance-2026-10-21" --prepare-only

python3 scripts/o2_benchmark.py --feature graphify-navigation \
  --settings automat/benchmarks/o2/settings.example.json --macc target/release/macc \
  --output /tmp/macc-o2-benchmark/graphify-live-01 \
  --archive-root "$PWD/.macc/reports/o2-performance-2026-10-21" --execute

python3 scripts/o2_benchmark.py --feature graphify-navigation \
  --case retention-policy --first-arm provider \
  --settings automat/benchmarks/o2/settings.example.json --macc target/release/macc \
  --output /tmp/macc-o2-benchmark/graphify-live-02 \
  --archive-root "$PWD/.macc/reports/o2-performance-2026-10-21" --execute
```

Repeat with `--feature serena-navigation` and separate `serena-*` output names.
Each paid pilot permits eight native invocations; two cases across both providers
permit at most 32. No automatic billed retry is allowed. A missing/unsupported/
fallback response cannot establish a successful candidate treatment.

Preparation records installation/deployment/index cost and two unchanged-source
query probes. The task receives one fresh query response before its first edit;
the second step reuses the evidence and checks current source, equally in both
arms. This keeps Graphify's snapshot contract explicit. A separate post-task query
must fall back to native after an edit for Graphify, while Serena must still return
current LSP evidence. This diagnostic query is excluded from task execution time
and recorded separately; total arm time includes all setup and diagnostics.
No stale graph output is delivered as current evidence.

Report source/hash/provenance correctness, result paths, output-byte/token
estimates, per-query time, actual normalized provider tokens and task/total time
separately. Native literal references and Graphify graph relations have different
semantics; a shorter result is not automatically higher precision or a billing
saving. These small policy-class fixtures do not certify large-repository or
multi-hop architecture benefits. No statistically general or exact financial
claim follows from one pair per tool/case.

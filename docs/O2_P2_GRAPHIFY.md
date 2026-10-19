# O2 P2-A: managed Graphify

For normal activation, use [Apply preparation](APPLY_PREPARATION.md). It prepares
and verifies enabled features across CLI, TUI and Web; the individual commands
below remain available for inspection and maintenance.

Graphify is an optional read-side adapter for the existing Code Intelligence port.
SQLite remains the authority for sessions, checkpoints, memories and reviewed
provider receipts. No assistant-specific integration or global skill is installed.
Serena and MCP remain separate P2 increments.

## Architecture decision

Keep process execution, operation progress and plan activation in the shared
provider manager. Keep Graphify package commands, schema parsing and ownership
manifests in its registered adapter. External work uses a per-provider lock;
only plan/receipt transactions take the session authority lock. This avoids
coupling a new read-side engine to assistant adapters or delaying session leases
for a package download. Optional JSON receipt/plan metadata preserves existing
serialized checksums when absent, so no SQLite schema upgrade is introduced.

## Enable and operate

Requires an O2-ready SQLite project, Linux x86_64, Python 3.12 and `uv` already
installed by the operator. Other environments retain the native provider.
Graphify is disabled by default. Opening the manager performs no network request.

In **TUI → Sessions → o → 5 Policies** or **Web → O2 Policies**, enable Code
Intelligence and managed Graphify. Select `adaptive` routing to use graph queries;
`native_only` always retains the original route. Preview and apply settings first.
Python and uv accept an absolute executable path or a PATH basename.

In **TUI → Code Intelligence**, `p` prepares a provider operation, `a` applies the
reviewed plan, `/` queries, `v` compares against native, and `d` inspects an
operation UUID. The Web manager exposes the same controls, confirmation and
progress. Installation/indexing run outside the TUI event loop and Web async
executor. The shared service releases the session authority lock and SQLite
transaction during external work. One kernel lock serializes a provider's work;
a concurrent query falls back immediately rather than waiting for installation.

Equivalent CLI:

```sh
macc o2 providers
macc o2 provider-plan graphify install
macc o2 provider-apply <plan-id> <checksum-from-preview>
macc o2 provider-operation <plan-id>
# Repeat plan/apply for deploy, then index.
macc o2 code-query preserve_contract --kind references
macc o2 code-compare graphify preserve_contract --kind references
```

Every lifecycle mutation requires a reviewed checksum. Plans include origin,
release, lock fingerprint, paths, runtime fingerprints, download estimate,
network permission and limits. Source, policy, receipt or executable changes
invalidate a pending plan. A completed plan replays its recorded result.

## Runtime and resource contract

The bundled wheel-only requirements lock pins **graphifyy 0.9.92** and all 30
runtime packages with SHA-256 hashes from PyPI. No source distributions/build
scripts or optional semantic/cloud extras are installed. Only explicit `install`
and `update` may download packages; canonical offline mode blocks both. There is
no release discovery, arbitrary version selection or automatic runtime download.
`update` stages a fresh copy of the supported pinned release and rebuilds its
index; changing that release requires a reviewed MACC adapter/lock update.

`uv venv` and `uv pip install --require-hashes --only-binary :all:` target a new
project-owned generation under `.macc/code-intelligence/graphify/generations/`.
The download preview's 34,153,890-byte estimate sums the largest published wheel
for each pinned package; it excludes transport overhead and installed disk size.
Python/uv themselves are operator prerequisites and their files/version output
are fingerprinted in the plan. No `graphify install` command is executed.

| Canonical setting | Default | Allowed range |
| --- | --- | --- |
| `code_intelligence.graphify_enabled` | false | Boolean |
| `code_intelligence.graphify_python` | `python3` | Executable, 1–512 bytes |
| `code_intelligence.graphify_uv` | `uv` | Executable, 1–512 bytes |
| `code_intelligence.graphify_timeout_seconds` | 120 | 5–600 seconds per query/extraction |
| `code_intelligence.graphify_install_timeout_seconds` | 600 | 30–1800 seconds per installer command |
| `code_intelligence.graphify_memory_mb` | 2048 | 512–8192 MiB per process |
| `code_intelligence.graphify_index_max_bytes` | 33554432 | 1–256 MiB graph JSON |
| `code_intelligence.graphify_corpus_max_bytes` | 268435456 | 1 MiB–1 GiB exported sources |

Existing shared `max_files`, `max_file_bytes`, `query_limit` and `output_tokens`
continue to apply. Both clients use the same typed field catalog, validation,
preview and revision-checked save. Changing query budgets does not invalidate
parser settings; changing source selection or ignore rules does.

Commands receive a credential-free environment, private HOME/cache/tmp,
wall-clock/output bounds, CPU/address-space limits, serialized package downloads/
installation and one AST worker. Linux
parent-death signals and process-group cleanup terminate failed/overdue commands.
The Python launcher rejects network sockets, subprocesses, exec/fork and writes
outside the operation workspace. A shared Python guard also checks directory
creation/removal, unlink, both sides of rename/hard-link operations, metadata
changes, truncation and paths through symlinks. Creating symlinks is disabled.
Descriptor-relative mutations/open calls are rejected; writable file descriptors
must resolve to an owned scratch path. Byte paths obey the same checks.
MACC pins `PYTHONHASHSEED=0` before invoking
Graphify, preventing its upstream hash-seed re-exec from discarding `-I`, `-B`
and the audit hook. Bytecode caches are disabled and their prefix is confined
to scratch storage. These are defense-in-depth controls around trusted pinned
code, **not a hardened OS sandbox against malicious native extensions**. Do not
use this runtime as an isolation boundary for untrusted package code.

## Snapshot, output and provenance

MACC exports only permitted tracked UTF-8 source files into a private corpus.
Git ignores, `.graphifyignore`, MACC-private paths, secrets and existing native
exclusions apply. Supported export extensions: py, rs, js, jsx, ts, tsx, go, java,
c, h, cpp, hpp, cs, rb, swift, kt, php, scala, sh and lua. Truncated snapshots
fail explicitly instead of silently activating an incomplete graph.

The adapter invokes `extract --code-only --no-cluster --no-dedup --no-gitignore
--max-workers 1 --out <generation>`. `--no-gitignore` applies only to the already
filtered private export; Graphify otherwise excludes its `.macc` parent. The
verified release appends `graphify-out/graph.json` to that output directory.
MACC wraps its raw nodes/edges with a schema-1 index manifest containing HEAD,
source hashes, parser/ignore settings and graph hash. Installation and index
activation each require a successful local extraction/query smoke.

Only healthy, exact-version, exact-revision/source snapshots are eligible.
Changes to tracked dirty files, deletions, ignore rules, runtime files or graph
bytes force native fallback until an explicit rebuild. Matching worktrees may
reuse a graph; divergent worktrees query current native sources. There is no
incremental updater or automatic rebuild during coordinator merges in P2-A.

`references` and `architecture_summary` expose bounded graph traversal.
`file_search` prioritizes native literal search. Precise symbol definitions,
dependency paths and impact analysis return Unsupported. Node/edge lines retain
`EXTRACTED`, `INFERRED` and `AMBIGUOUS` labels; MACC validates cited live source
hashes/line ranges and never promotes graph output to approved memory. Querying
uses a scratch graph copy because upstream writes query logs beside its input.
Completed generation bytes remain immutable.

Generation/operation roots are owner-only directories; externally generated graph
files stay inside that boundary. All query outcomes use the shared complete-envelope
token estimate bound.
Elapsed time includes receipt/source checks and runtime fingerprints, rather
than only the upstream CLI. The opt-in smoke uses a debug test build and does
not predict release performance. A/B output contains **two separately bounded
responses** plus elapsed time; it
is an operator comparison, not a single model payload or token/cost-saving claim.

## Recovery, rollback and removal

A successful operation atomically activates its SQLite receipt after rechecking
source/settings. Prior receipts remain authoritative during staging. The exact
previous deployment can be restored after verifying its runtime, index and
current source compatibility. The receipt keeps ownership proofs for superseded
generations so later uninstall can remove them.

`disable` preserves everything. `undeploy` removes owned indexes and retains the
runtime. `uninstall` removes unchanged receipted generation files only. Added or
modified user files remain on disk with their original ownership manifest;
symlink parents are rejected and interpreter links are never traversed while
removing files. Removal requires stopped runners and no session leases. Memory,
checkpoints, Git changes and outside/PATH installations are preserved.

Progress is recorded at `.macc/code-intelligence/operations/<plan-id>.json` and
may be inspected from all clients. SQLite receipts, rather than progress files,
determine activation. A killed client/process can leave `running` progress and
an unactivated attempt directory. Reapplying an unapplied plan revalidates its
preconditions and stages a fresh attempt; a completed plan replays its receipt.
Failed/interrupted attempts and their logs are retained for inspection, outside
active ownership receipts. They are never automatically adopted or recursively
deleted; operator cleanup must inspect them first. No daemon is added.

## Launcher compatibility after a MACC update

Runtime receipts and index compatibility include the SHA-256 of the complete
launcher, including the shared filesystem guard. A missing/different digest in an
older receipt makes that runtime ineligible for query/deploy/index/rollback;
queries retain native fallback and health explains the mismatch. No SQLite schema
migration or silent launcher overwrite occurs. Recreate pending plans, then
preview/apply **update** for a deployed runtime, or **install** followed by
**deploy** and **index** for an installed/disabled runtime. Existing ownership
proofs still govern cleanup and preserve user changes. Rollback to a generation
with an obsolete launcher is rejected; retain it for inspection or reviewed removal.

## Validation and upstream references

Deterministic subprocess doubles test reviewed lifecycle operations, replay,
side-by-side updates/rollback, source/configuration drift, bounded results,
busy/corrupt provider fallback and non-destructive removal. A separately invoked
real-package smoke test exercises the pinned lock, offline launcher, AST output,
query normalization and native A/B comparison in a disposable project:

```sh
cargo test -p macc-core o2::providers --locked
MACC_GRAPHIFY_TEST_UV=/absolute/path/to/uv cargo test -p macc-core \
  real_pinned_graphify_code_only_smoke --locked -- --ignored --nocapture
```

The second command explicitly downloads pinned wheels into a disposable `/tmp`
project. It retains the fixture and `.macc/graphify-ab.json` comparison for
inspection; a successful run uninstalls its owned package/index generations.
The default suite uses no Graphify package or AI service. It validates
this fixture and platform, not retrieval quality or savings on production repos.

Primary sources: [Graphify](https://github.com/Graphify-Labs/graphify),
[release metadata](https://pypi.org/pypi/graphifyy/0.9.92/json),
[CLI source](https://github.com/Graphify-Labs/graphify/blob/v8/graphify/cli.py),
[uv concurrency controls](https://docs.astral.sh/uv/reference/environment/).

### Delivery verification (2026-10-16)

- `make check`: 1,487 workspace tests, 133 standalone adapter tests and 171 Web
  tests passed, plus runner, contract, architecture, formatting and lint checks.
  The opt-in real-package smoke is excluded from the normal suite.
- The real pinned-package smoke passed separately: isolated install, deployment,
  Rust AST index, verified call relations, no-match fallback, comparison and owned
  uninstall. It made no AI/model calls.
- Final provider regressions: 13 passed; the opt-in real smoke remained ignored.
  These also cover changes to a non-returned source file during a query, local
  worktree ignores, exec/fork prevention, dependency-lock drift, private generation
  permissions, interrupted removal and preservation of added empty directories.
- The observed debug fixture returned two native literal hits in 29 ms and six
  Graphify node/relation hits in 23,443 ms. Those different result counts do not
  establish higher accuracy or token savings. Runtime verification is a material
  cost; measure the optimized release on a representative repository before
  enabling adaptive graph routing. Defaults remain `native_only` and Graphify off.

Full provider integrity inspection remains available in the manager. Routing uses
cheap receipt eligibility, and each graph query verifies its runtime once before
execution. Sources and ignore settings are rechecked after the query so changes
in files outside the returned hits also force native fallback.

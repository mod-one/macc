# O2 P2-B: managed Serena symbol navigation

For normal activation, use [Apply preparation](APPLY_PREPARATION.md). It prepares
and verifies enabled features across CLI, TUI and Web; the individual commands
below remain available for inspection and maintenance.

## Delivered contract

The optional `serena` code-intelligence provider implements precise Python symbol
definitions and references using **Serena 2.0.0 SolidLSP** and
**jedi-language-server 0.46.0**. It uses the existing provider registry, reviewed
plans, SQLite receipts, operation locks, progress, token bounds and shared
CLI/TUI/Web actions. It has no dependency on the selected AI tool or model.

Supported runtime: **Linux x86_64, an existing Python 3.12, and uv**. The other
languages supported by upstream Serena are explicitly disabled in this adapter.
A future language backend must verify its dependencies, offline behavior and
accuracy before advertising support. No language server is downloaded at query
time. Provider flags remain off by default; native routing remains the default.

The managed environment is a **SolidLSP read-side subset**, not a full Serena
agent installation. `requirements.lock` explicitly pins and hashes 35 wheel-only
packages, including Jedi. Installation uses `--no-deps` with this complete tested
subset. Unused Serena UI, AI client and MCP dependencies are excluded. This avoids
source builds and optional graphical application dependencies. The reviewed plan
identifies this boundary, Python/uv executable hashes, package origin, the lock
hash, resource limits and a 9,572,005-byte upper bound for approved package wheel
downloads (excluding registry metadata and transport overhead). The lock targets
Python 3.12 on the supported platform; it is not a universal environment lock.

Upstream references: [Serena](https://github.com/oraios/serena),
[released package](https://pypi.org/project/serena-agent/2.0.0/), and
[Jedi language server](https://github.com/pappasam/jedi-language-server).
The runtime bridge is pinned to the APIs in the published wheels, rather than
assuming that the development branch has the same interface.

## Interactive configuration and use

In **Ops → Policies**, enable Code Intelligence, choose `adaptive` routing and
enable Serena. All seven settings below use the same typed catalog in TUI and Web:

| Setting under `code_intelligence` | Default | Accepted values |
| --- | --- | --- |
| `serena_enabled` | `false` | Boolean |
| `serena_python` | `python3` | Existing Python 3.12 absolute path or PATH basename |
| `serena_uv` | `uv` | Existing uv absolute path or PATH basename |
| `serena_timeout_seconds` | `60` | 5–600, whole query subprocess timeout |
| `serena_install_timeout_seconds` | `600` | 30–1800, per installer command |
| `serena_memory_mb` | `2048` | 512–8192, per process address-space limit |
| `serena_corpus_max_bytes` | `33554432` | 1–256 MiB, exported Python source limit |

Shared `max_files`, `max_file_bytes`, `query_limit` and `output_tokens` also apply.
Executable settings are bounded strings, never shell argument fragments.

In **Ops → Code Intelligence**, select provider `serena`, preview **install**, inspect
and apply the exact plan, then preview/apply **deploy**. Both steps smoke-test an
actual definition through the pinned language server. Only a successfully deployed
receipt is eligible for routing. Merely opening a client or enabling a flag never
installs or deploys a provider. Explicit health refresh checks runtime ownership,
base Python, dependency versions and a real definition smoke. Receipts also bind
the bridge implementation hash: upgrading MACC cannot silently reuse an obsolete
launcher with unchanged package versions.

CLI equivalents (apply uses the ID and checksum returned by each preview):

```sh
macc o2 providers
macc o2 provider-plan serena install
macc o2 provider-apply PLAN_ID REVIEWED_CHECKSUM
macc o2 provider-plan serena deploy
macc o2 provider-apply PLAN_ID REVIEWED_CHECKSUM
macc o2 code-query 'Account/save' --kind symbol_definition --path src/account.py
macc o2 code-query '/save' --kind references --path src/account.py
macc o2 code-compare serena '/save' --kind references --path src/account.py
```

`Query.path` is an optional, backward-compatible field in the shared query DTO;
**Serena requires it to identify a Python definition file**. This prevents a
Python-only provider from returning incomplete results for an unscoped query in a
mixed-language project. Web and TUI expose this field for query and comparison.
`--worktree` selects another worktree from the same Git repository.

Names are exact, case-sensitive identifiers, optionally qualified as `Class/method`.
`/name` selects a top level symbol. An unqualified name may match several symbols
in the file: definitions return the matches; references refuse ambiguity and
request a qualified name. References can cite other exported Python files.
A successful lookup with no matches returns an empty LSP result. It does not
fabricate semantic results from textual mentions.

## Routing and source freshness

With `adaptive` routing, eligible Serena runs before Graphify for definitions and
references. Native remains first for `file_search`. Automatic queries under `native_only`
never launch Serena; an explicit provider comparison can still assess an enabled,
healthy candidate before changing automatic routing. Unscoped queries, other languages, missing dependencies, corrupt runtimes,
provider contention or failed LSP requests retain fallback diagnostics and continue
to Graphify/native where their capabilities permit. Native's reference fallback
remains literal search; native cannot supply precise symbol definitions.

The provider trait explicitly distinguishes cached-snapshot providers from live
providers. Graphify retains all existing artifact/version/source checks. Serena
verifies its runtime and creates a fresh source copy for every query, so an ordinary
source edit does not require a durable index rebuild. `index` is a typed unsupported
operation for Serena.

Only tracked, bounded, permitted UTF-8 Python files enter the transient workspace.
Git ignore rules, generated/secret exclusions and symlink rejection apply before
export. A truncated corpus fails explicitly. Full source digests and Git revision
are checked before/after execution; relevant ignore decisions are checked again.
Citations must name exported files with unchanged hashes and valid source lines.
The shared response limiter bounds the entire JSON envelope, including diagnostics.

## Runtime ownership, process lifetime and limitations

Runtime generations live below `.macc/code-intelligence/serena/generations/`.
Installation/update stages a new generation; activation uses the shared receipt CAS.
External work holds neither the session authority lock nor a SQLite transaction.
Failed stages remain unactivated for inspection. Update reinstalls the same pinned
release; automatic release discovery is future work. Rollback validates the prior
runtime and keeps ownership of superseded generations.

`disable` blocks future routing. `undeploy` deactivates the receipt and keeps the
runtime. `uninstall` uses the shared file-level ownership proofs and removes only
unchanged managed content, preserving added/modified user files and retained
ownership records. Existing stopped-owner/session gates apply to removal. No
system/user Python installation or PATH package is adopted or removed.

Each query uses private scratch, a cleared credential environment, bounded output,
wall time and inherited CPU/address-space limits. Its Jedi stdio child stays in the
managed process group and establishes Linux parent-death protection. There is no
persistent provider worker to stop. Normal completion, errors and timeouts clean up
the query scratch; operator-facing error reasons remain in retrieval diagnostics.
Provider lifecycle audit and progress use the existing O2 journal. TUI provider
health refresh also runs in a background job; polling updates rows without
replacing a pending reviewed operation or scheduling an endless refresh loop.

Both Python processes use the managed bridge. Python audit hooks reject network
connection/bind/name resolution, arbitrary child commands and file writes outside
scratch. The same shared filesystem guard as Graphify checks unlink, directory
creation/removal, both rename/link paths, metadata/truncation and symlink escapes.
Creating symlinks and descriptor-relative mutations/open calls are disabled;
writable descriptors must resolve to scratch. The full bridge digest includes
this guard, so an older installation requires a reviewed **install**/**update**
before reuse. A query never silently patches the installed bridge.
The one allowed subprocess is the fixed managed Jedi launch command;
no query text or PRD string enters that command. Upstream's launcher uses a shell
for this fixed, quoted command. Jedi uses a same-process inference environment with
unsafe interpreter execution disabled, preventing its unguarded helper interpreter.
The pinned pygls synchronous stdio loop avoids an observed asynchronous reader stall.
These are version-specific shims covered by the opt-in real-runtime test.

These controls are **not an OS sandbox for malicious native dependencies**. The
provider receives curated source copies, never the live repository as its LSP
workspace. Project configuration, hooks, arbitrary edits, MCP exposure, runtime
installation for other languages, persistent LSP caches and production latency or
token-savings guarantees are outside P2-B.

## Compatibility and validation

No SQLite schema upgrade is needed: existing optional provider receipt metadata
carries runtime ownership. Old queries without `path` still deserialize; old
configuration receives disabled defaults. Recreate pending provider plans after
updating policy fields because reviewed policy hashes bind the exact defaults.
P0 session, quota and coordinator behavior is untouched.

Deterministic tests cover routing priorities, legacy DTOs, typed settings, lifecycle,
source edits, ignore handling, missing/corrupt dependencies, rollback, ownership,
invalid citations, in-flight source drift, and bridge restrictions. Web tests cover
the scoped symbol query action. Existing Graphify regressions cover the extracted
shared generation ownership and Git-ignore helpers.

The real package fixture is opt-in, uses only a disposable temporary project and
requires public PyPI access:

```sh
cargo test -p macc-core real_serena_symbol_accuracy_and_lifecycle --lib -- --ignored
```

It exercises managed installation/deployment, qualified symbol accuracy, references
excluding text-only matches and unrelated definitions, empty lookups, ambiguous
symbol fallback, and clean uninstall. Run `make check` for the standard validation
pipeline. Fixture results do not establish accuracy for other languages or projects.

### Maintaining the runtime pin

`core/src/o2/providers/serena/requirements.in` records the direct dependencies of
the tested read-side subset. To regenerate its transitive lock for review:

```sh
uv pip compile --python-version 3.12 --only-binary :all: --generate-hashes \
  --no-header --no-annotate --output-file /tmp/serena-subset.lock \
  core/src/o2/providers/serena/requirements.in
```

Append the separately verified Serena wheel version and SHA256, preserving the
subset header in `requirements.lock`. Review all dependency changes and wheel
sizes before updating the committed lock. Update the runtime version, bridge
version checks, plan size/count disclosures and documentation together. Run the
real package fixture before accepting a version bump; a successful full Serena
application dependency check is not a substitute for testing this read-side API.

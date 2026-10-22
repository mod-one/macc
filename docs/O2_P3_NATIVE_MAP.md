# O2 P3-B: optional native symbol map

For normal activation, use [Apply preparation](APPLY_PREPARATION.md). It prepares
and verifies enabled features across CLI, TUI and Web; the individual commands
below remain available for inspection and maintenance.

P3-B extends the bundled `native` code-intelligence adapter with pinned Tree-sitter
grammars for Rust, Python, TypeScript and TSX. It runs locally inside MACC: no AI
provider, external language server, model call, runtime install or network upload
is needed. Native code intelligence and its incremental symbol map are **enabled
by default**. Apply prepares and verifies the bundled provider and index through
the same lifecycle port as other providers. Ordinary literal search and P3-A's
Rust evidence fingerprints remain independently available. Explicit `false`
settings still disable code intelligence or its symbol map.

## Enable and use

Run `macc apply` to prepare the default native provider and index. To restore a
previous explicit opt-out, enable Code Intelligence and
`code_intelligence.native_map_enabled` in the shared O2 policy editor. All controls below are available through CLI policy preview/apply,
**TUI → O2 → 5 Policies**, and **Web → Configure Code Intelligence**. Existing
revision checks and owner gates apply. Search for `native_` to locate the settings.

The provider port accepts `symbol_definition` and `symbol_signature` queries:

```sh
macc o2 code-query 'User/send' --kind symbol_signature --path src/user.py
macc o2 code-query 'send' --kind symbol_definition
macc o2 code-query '/top_level' --kind symbol_definition
```

A bare name returns exact matching declarations across eligible files. A qualified
`module/Class/method` selector chooses that syntactic nesting. A leading slash
requires a top-level declaration. An optional file path disambiguates same-named
symbols; `--worktree` must name a worktree of the same Git project. There is no
fuzzy-name remapping. Each result carries a source location, current whole-file
SHA-256, qualified name/signature and `EXTRACTED` parser provenance.

**Web → Code Intelligence** offers the new signature query type, current-code
queries, explicit comparison, operation preview/apply and deployment receipts.
**TUI → O2 → Code Intelligence** uses the existing code-query/compare forms and
provider lifecycle actions; enter `symbol_signature` as the query kind. These
operations run in background jobs. The agent's existing scoped `code.query` port
can request the same capability; its capability catalog/lease checks are unchanged.

## Incremental persistence and lifecycle

Native `deploy`, `index`, `update`, `rollback`, `disable`, `undeploy` and `uninstall`
use the existing reviewed plan, operation journal, per-provider writer lock and
SQLite receipt publication. For example:

```sh
macc o2 provider-plan native deploy
macc o2 provider-apply '<returned-plan-id>' '<returned-checksum>'
macc o2 provider-plan native index
macc o2 provider-apply '<returned-plan-id>' '<returned-checksum>'
```

Use `update` the same way after edits/merges. `index` and `update` both reuse
unchanged files when enabled. The incremental unit is a **file**, not an edited
syntax-tree range. The persisted map contains declaration/signature records and
source hashes, not full syntax trees or source bodies. Its recipe binds the exact
parser/grammar versions and its settings digest binds the policy. Previous
compatible files are reused only when their content hashes match. Removed files
are pruned; renamed or changed files are parsed again. Receipt metadata exposes
parsed/reused/removed/unsupported/failed counts, corpus bytes and elapsed time.

Queries are read-only. They verify current tracked sources, reuse matching base
records and parse changed files into a temporary worktree overlay. They never
publish this overlay or modify the base receipt. Query completion checks the
source snapshot again; detected concurrent changes discard symbol results and
request a retry. Rollback restores the old immutable base, while live queries
still validate current files. Missing/corrupt/legacy/incompatible caches produce a
diagnostic and a current-source rebuild, not stale hits.

No merge hook, timer, watcher, independent database or automatic write is added.
Index publication remains an explicit reviewed operation. Superseded artifacts
remain owned for rollback and are removed by the existing teardown operations.
The map is expendable and never proves task acceptance or promotes a memory.

## Supported syntax and limitations

- Rust: functions, structs, enums, traits, aliases, constants/statics, inline
  modules and implementation methods.
- Python: functions, classes, methods and nested definitions.
- TypeScript/TSX: functions, classes, interfaces, types, enums, namespaces,
  methods/signatures and named variable declarations with function/arrow values.
- Syntactic nesting determines qualified names; it does not resolve imports,
  aliases, traits, runtime decorators, inferred types, computed names or macros.
- Signatures are bounded source headers. Bodies and decorator semantics are not
  expanded; this is navigation evidence, not a proof of behavior.
- References continue to use literal current-file matches. Call/import/inheritance
  relationships, dependency paths and architecture inference are not supplied by
  this map. Other verified providers retain their capabilities and routing.
- Only tracked, bounded, UTF-8, non-private, non-generated regular files from the
  shared source reader are eligible. Source symlinks and foreign worktrees are
  rejected. Untracked files are intentionally absent.
- Parse errors, timeouts, excessive nesting, overly long declarations or excessive
  symbol counts exclude **all** symbols from that file with a diagnostic. No
  partial syntax tree is represented as an authoritative definition.

P3-A bound memory evidence still uses its documented Rust `syn` fingerprint recipe.
Tree-sitter navigation for another language does not silently enable that language
for memory freshness evidence or change existing fingerprints.

## Shared settings

All keys are under `code_intelligence`:

| Key | Default | Bounds / behavior |
|---|---|---|
| `native_map_enabled` | false | Opt in to parsed definitions/signatures |
| `native_incremental` | true | Reuse compatible unchanged files |
| `native_rust` | true | `.rs` grammar |
| `native_python` | true | `.py` grammar |
| `native_typescript` | true | `.ts` grammar |
| `native_tsx` | true | `.tsx` grammar |
| `native_corpus_max_bytes` | 16777216 | 1–256 MiB supported corpus |
| `native_index_max_bytes` | 16777216 | 1–64 MiB persisted artifact |
| `native_symbols_per_file` | 1000 | 1–10000 |
| `native_parse_timeout_ms` | 100 | 1–1000 ms per file parser |
| `native_build_timeout_ms` | 15000 | 100–60000 ms parse/reuse loop |

Existing `max_files`, `max_file_bytes`, `query_limit` and `output_tokens` still
apply. Corpus/output/loop budget exhaustion marks coverage truncated explicitly;
a bounded result is not a claim that no other matching symbol exists. The loop
budget includes preparation time but does not interrupt shared Git enumeration or
source hashing. File hashing remains necessary to prove reuse; incremental
indexing reduces parsing work rather than eliminating source I/O. Grammar parsing
has a separate timeout, and traversal has a fixed nesting limit.

`map_truncated` describes index coverage. Query traversal preserves one extra
matching definition so the shared response wrapper can emit **Output bounded**
when `query_limit` omits alternatives. A fully parsed map can still yield a
bounded query response. Exactly one match at a limit of one produces no false
truncation warning; multiple matching definitions do.

The recipe pins Tree-sitter 0.25.10, Rust grammar 0.24.0, Python grammar 0.25.0 and
TypeScript/TSX grammar 0.23.2. Dependencies are locked in both workspace lockfiles.
The upstream [Rust API](https://docs.rs/tree-sitter/0.25.10/tree_sitter/) and
[TypeScript grammar](https://docs.rs/tree-sitter-typescript/0.23.2/tree_sitter_typescript/)
document the language-loading interface. A dependency change must update the
recipe and run the accuracy/cost gate again.

## Validation and compatibility

Run `make check`. Focused native-map tests exercise grammar locations/signatures,
qualified names, literal false positives, changed/deleted files, read-only dirty
overlays, lifecycle/rollback, cache corruption, restricted paths, parser limits
and policy catalog coverage. The deterministic 128-file Python gold corpus checks
all expected definition locations and compares full versus incremental records;
the unchanged pass must parse zero files and reuse every eligible file. Tests
assert work counters and record structured timing events, without imposing a machine-dependent speed threshold
or claiming a production accuracy/cost benchmark.

The fixed `core/tests/fixtures/o2-native-map` service/UI corpus adds independently
listed locations and signature expectations in Rust, Python, TypeScript and TSX.
Its 12 queries compare parsed declarations with ordinary literal search, including
comments, calls and string false positives. It records precision/recall, complete
query timings, full/incremental build timings, artifact/corpus bytes and parsed/
reused file counts. Incremental results must equal full results and perform zero
unchanged-file parses. Generate an inspectable local report with:

```sh
MACC_O2_BENCHMARK_REPORT=/tmp/macc-native-map-benchmark.json \
  cargo test -p macc-core --locked \
  representative_multilanguage_accuracy_and_resource_comparison --lib
```

The baseline is literal lookup, which also returns references; its precision is
measured against definition-only labels. These are local fixture comparisons,
not evidence of superiority over an external semantic provider, process RSS
limits or paid task savings. Parsing may cost more than literal search; retain
the optional flag and inspect the project's tradeoff before enabling it.

There is no SQLite migration. Legacy native file indexes remain valid for literal
search and are rebuilt for the new map recipe. New policy defaults are compatible
with existing configurations. The artifact keeps the shared source-snapshot
fields for receipt health checks. `macc save` continues to capture policy/receipts
through the existing mechanisms; derived indexes remain rebuildable caches. Binary
rollback requires removing unsupported policy keys or restoring a compatible
configuration snapshot, as with other new strict policy fields.

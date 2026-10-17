# O2 P3-C: benchmark-gated local semantic memory search

P3-C adds an optional local second pass to the Memory Ledger's FTS retrieval.
The bundled `local_lsa` backend computes corpus-local TF-IDF/latent semantic
vectors with a deterministic truncated-SVD approximation (12 orthogonal power
iterations per component). This is classical LSA, **not a pretrained neural or
multilingual model**. There are no embedding model downloads, API keys, model API
charges, subprocesses or network calls in this backend. Existing evidence checks
still run through their registered version/symbol adapters before retrieval.

LSA can connect terms that co-occur in related documents; it cannot recover a
concept absent from its eligible corpus. The
[Stanford IR book](https://nlp.stanford.edu/IR-book/html/htmledition/latent-semantic-indexing-1.html)
and [TruncatedSVD reference](https://scikit-learn.org/stable/modules/generated/sklearn.decomposition.TruncatedSVD.html)
explain the underlying approach. Neither source establishes a benefit for a MACC
project: activation requires that project's measured evidence.

## Two independent activation conditions

1. Opt in with `memory.semantic_enabled` in the shared policy editor.
2. Run, inspect and explicitly approve a passing labelled benchmark for the exact
   current eligible corpus and settings.

The benchmark can run while the flag is false. Toggling only that flag does not
invalidate its evidence. Changing other memory policy values, the backend recipe,
eligible records or their verified content requires a matching new benchmark.
Approval does not change any memory's status, evidence or review. A benchmark
never approves itself and cannot be approved by a runner/agent session.

Benchmark method `semantic-benchmark-v2:runtime-result-cap` binds reports and gate
settings to runtime result limits. Earlier reports and review checksums remain
inspectable and revocable, but cannot enable retrieval. Earlier approvals no
longer match current gate keys. Re-run the labelled benchmark and explicitly
review it; no database migration or automatic reapproval occurs. Status returns
an `approvable` flag for current passing reports, and clients disable approval
of retired or changed evidence.

Without both conditions, search retains FTS. Corpus, time, memory, malformed vector
or backend failures also retain FTS with a diagnostic in retrieval exclusions.
A corpus exceeding the document cap, or reaching the shared bounded source
enumeration limit, is not silently truncated for semantic use; it retains FTS.

## Benchmark contract

Supply 4–100 distinct queries, at least two relevant reviewed memories across the
suite and explicit relevant memory IDs per query. All labels must refer to the
active, confident, conflict-free, in-scope and currently verified corpus. The
operator must judge whether the labelled queries represent real project work.
The system cannot prove that user-supplied relevance labels are representative.

Both arms use the same fresh authorized records and existing deterministic rank
factors. The baseline admits only FTS matches. The hybrid admits FTS matches plus
positive local cosine matches, then adds the configured cosine score weight.
Both arms apply the same canonical `memory.search_limit` as runtime search before
computing metrics or reporting IDs. The measured window is at most
`min(top_k, search_limit)`; increasing `top_k` cannot fabricate a gain from results
that the configured search would omit. Reports state the applied runtime cap.

A report passes only when:

- Mean recall at `top_k` improves by at least `semantic_min_recall_gain`.
- Mean reciprocal rank does not regress; neither recall nor reciprocal rank
  regresses on an individual labelled query.
- Freshness preparation, training and the slowest measured query together fit the configured time budget.
- Input, matrix and estimated resident model payload fit their byte bounds.
- All input authority/freshness proofs are rechecked before recording the report.

Reports include per-query result IDs, quality scores, corpus/settings digests,
backend recipe, counts, estimated model bytes and measured preparation/training/query time.
The entire benchmark also has a bounded runtime. Measurements are local runtime
and estimated data payload, not OS-enforced process RSS, energy, cloud billing or
an end-to-end task-acceptance/cost trial. No production savings is claimed.

## CLI workflow

Use the existing O2 policy preview/apply actions for all settings. Review the typed
policy revision before applying. Then provide a suite on standard input:

```sh
macc o2 semantic-status
macc o2 semantic-benchmark < reviewed-suite.json
macc o2 semantic-gate '<report-id>' '<report-checksum>' --confirm
macc o2 memory-search 'paraphrased query'
macc o2 semantic-gate '<report-id>' '<report-checksum>' --revoke --confirm
```

Suite shape (replace IDs with actual reviewed records):

```json
{
  "access": {},
  "top_k": 3,
  "cases": [
    {"query": "sign in verification", "relevant_ids": ["memory-a", "memory-b"]},
    {"query": "credential identity", "relevant_ids": ["memory-a", "memory-b"]},
    {"query": "durable storage", "relevant_ids": ["memory-c"]},
    {"query": "job processing", "relevant_ids": ["memory-d"]}
  ]
}
```

`access` supports the existing task/tool/worktree/feature/user selectors. CLI
`semantic-status` has the matching scope flags. Relevant IDs outside this scope
are rejected. Approved evidence can apply to another access request only if its
entire eligible verified corpus and task-affinity ranking factors are identical. Added task-private records therefore
require a new corresponding benchmark, rather than reusing a broader model.

## Interactive clients

**Web → Memory → Local semantic search and benchmark gate**:

- Inspect current corpus, gate and stored reports.
- Add labelled query rows, select `top_k` and run the benchmark.
- Inspect the full report, acknowledge representative labels/quality/resources,
  then approve its exact checksum. Failed reports cannot be approved.
- Revoke the corpus gate independently, or disable the shared policy flag.
- Use **Search approved facts with local semantics** to inspect actual retrieval.

**TUI → O2 → Memory**:

- `S`: inspect status/reports for the current access scope.
- `B`: choose 4–100 queries (blank selects four), then enter the repeated guided
  query/ID pairs and `top_k`; inspect the complete prepared action, then `a` to run
  the benchmark. Shift+Tab returns to the previous field; invalid counts or `top_k`
  retain the inputs for correction. Esc cancels the form. Larger suites need no
  JSON editing. Active forms stay above report details in small terminals.
- `G`: prepare approval of the inspected latest passing report; inspect it and
  press `a` to confirm. `X` prepares revocation in the same way.
- `Q`: search approved facts with FTS plus eligible local semantics.

Scope changes invalidate inspectors and confirmation. Slow operations use existing
background transports/jobs. Late results cannot restore another task's report or
approval. Normal operator inbox search still includes inactive proposals and uses
FTS; automatic semantic retrieval never trains on that inbox. CLI default search,
scoped agent memory search/pull and Context Engine retrieval share the approved
active-fact path. The restricted MCP tool catalog is unchanged.

## Shared configuration

CLI policy actions, TUI Policies and Web Configure Memory use the same typed
catalog, range validation and revision-checked preview/apply. Search for
`semantic_` in the policy editor.

| Key under `memory` | Default | Bounds / behavior |
|---|---|---|
| `semantic_enabled` | false | Opt in; a reviewed passing gate is also required |
| `semantic_backend` | `local_lsa` | Bundled local backend only in this release |
| `semantic_dimensions` | 8 | 2–32; capped by corpus/vocabulary rank bounds |
| `semantic_candidates` | 128 | 3–256 eligible documents; overflow retains FTS |
| `semantic_vocabulary` | 256 | 32–512 frequency-selected terms |
| `semantic_weight` | 100 | 0–1000 cosine ranking contribution |
| `semantic_min_similarity` | 0.15 | 0.01–1 positive cosine threshold |
| `semantic_max_ms` | 1000 | 10–10000 ms local preparation/training/query budget |
| `semantic_max_bytes` | 8388608 | 32768–33554432 input/matrix/model payload budget |
| `semantic_min_recall_gain` | 0.05 | 0.01–1 required measured recall improvement |

The embedding port takes documents and numeric limits, independently of performer
providers/models. It returns finite cosine scores in document order. Remote
backends and malformed shapes/non-finite scores are rejected. Future compiled
backends require a new reviewed recipe/configuration and must honor their local
resource contract; this interface is not an OS sandbox for arbitrary plugin code.

## Authority, privacy and compatibility

Only titles/bodies of eligible reviewed memories are embedded. Draft, stale,
conflicted, low-confidence and foreign-scope records cannot influence the model.
Source privacy checks and credential scans remain mandatory. The entire labelled
suite, including access selectors, is bounded and credential-scanned before recording. Computation occurs
before authority locks. During retrieval, MACC rechecks the whole embedding input
pool, exact query, task-affinity factors, canonical record digests, policy, source metadata and the current gate event
inside the transaction. A revoked gate or changed non-selected input disables the
entire semantic contribution, while individually valid FTS results remain usable.

Vectors are request-local and discarded. Reports and explicit approval/revocation
events use the existing canonical `o2_audit` table. Query text and relevance labels
are retained there for review and backups; do not enter private credentials.
No vector database, background worker, second authority or SQLite migration is
introduced. `macc save` retains the typed configuration and gate/report audit data
through existing consistent snapshots. New strict policy keys require a compatible
configuration snapshot when rolling back to an older binary.

The Context Engine still restores mandatory PRD/acceptance material independently
of optional memories. Semantic scores cannot promote a claim, bypass scope or
freshness, complete a task, or change session/lease/tool handoff rules. LSA quality
is corpus-dependent; volatile corpora may require frequent renewed benchmarks.

## Validation

Run `make check`. Deterministic tests compare FTS with real local LSA on a labelled
three-topic corpus, then exercise separate approval, rejection, revocation during
retrieval, changed corpus/settings, scoped input exclusion, stale/conflicting
facts, byte/time limits, non-finite vectors and forbidden remote backends. Client
contract tests cover typed forms, confirmation, report checksums and late replies.
These fixtures demonstrate a retrieval class where the local second pass helps;
they are not a substitute for a representative project benchmark.

## Task-level policy experiments

Semantic retrieval benchmarks measure labelled search quality. For paired task
quality, measured incremental cost and explicit policy adoption, use the separate
[P3-D policy trial workflow](O2_P3_POLICY_TUNING.md). A semantic benchmark alone
cannot establish task-level cost savings or authorize tuning adoption.

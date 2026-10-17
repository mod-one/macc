# O2 P3-A: evidence freshness and retrieval quality

P3-A extends the existing Memory Ledger, provider port, registered tool observations
and shared client actions. It does not add a second memory authority, AI worker,
timer, task-delivery rule or automatic promotion policy. The optional persistent
native symbol map and incremental indexing are documented separately in the
[P3-B guide](O2_P3_NATIVE_MAP.md).

## Symbol evidence

The code-intelligence port now advertises `symbol_languages` and an optional
`symbol_fingerprint` operation. Unsupported adapters fail explicitly; literal
search or inferred graph edges cannot stand in for a semantic fingerprint.
The bundled native implementation currently supports **Rust** parsed with `syn`:

- Exact functions, structs, enums, traits, aliases, constants, statics and modules.
- Inline module paths and implementation methods, such as `module/Type/method`.
- Attributes, signature and body tokens participate in the digest. Implementation
  method evidence also includes its enclosing impl's generics, trait and where
  clause, without including unrelated methods.
- Ordinary comments, whitespace and changes outside the selected item do not
  invalidate it. This is a parsed-token fingerprint, not semantic equivalence,
  type resolution, macro expansion or dependency inference. Doc attributes and
  equivalent rewrites can change the fingerprint.
- Missing/moved/renamed, ambiguous or unparseable symbols fail closed. No automatic
  remapping or guessed citation occurs. Recapture the new path/name after review.
- Other languages and symbol forms require explicit whole-file evidence until a
  verified provider implements the fingerprint port. Serena's Python LSP queries
  and Graphify graph traversal remain their existing, separate capabilities.

Enable Code Intelligence before capturing/using symbol evidence. Source paths use
the existing bounded, non-private, non-symlink worktree reader. A symbol digest
cannot prove behavior that depends on another function, configuration, dependency
or lockfile: cite those sources separately as whole-file evidence.

```sh
macc o2 memory-evidence-sources
macc o2 memory-symbol-evidence src/lib.rs 'module/Type/method' --provider native
macc o2 memory-evidence Cargo.lock
```

The output is evidence for a proposal/revalidation, not an active memory. The
optional `--worktree` selects an existing validated worktree. Unsupported symbols
produce a diagnostic; there is no automatic installation or fabricated fallback.

## Provider-note observations and TTL

`memory-tool-evidence TOOL_ID` explicitly invokes the existing registered adapter's
bounded version/help probe. Only tools with a declared version profile are offered.
The resulting `provider` evidence binds the registered tool, canonical executable,
executable/profile digests, full version/help output digests and observation time.
The observation establishes identity, not truth of the proposed behavior note.
For the readable observed version, inspect Ops → Compaction or `macc o2 compaction-list`.

```sh
macc o2 memory-tool-evidence '<registered-tool-id>'
```

`provider_note` activation now requires version-bound evidence. Legacy notes without
it remain inspectable, but are excluded from automatic retrieval until reviewed
with a fresh observation. On retrieval the shared observation port checks the
current registered command/profile, executable and a bounded live `--version`.
This detects package upgrades behind an unchanged launcher. A later explicit
probe with changed help/identity also invalidates the old evidence. Environment
or dependency assumptions beyond this identity need their own cited evidence.

TTL is measured from **observation time**, never from a status update, search or
repeated reading. Re-probing the same version does not renew existing memories;
recapture and explicitly revalidate the claim. Static reviewed decisions with
ordinary source evidence have no age-only expiry.

Each retrieval caches successful/failed tool checks and symbol fingerprints. Its
limits exclude unverified candidates with a reason; they never bypass freshness.
Verification is prepared before the memory/session authority lock and before
SQLite transactions. The transaction then checks record, policy, file and cached
adapter identities against that request-local proof; changed inputs are excluded
or require renewed operator review. Proofs expire after a fixed 60-second safety
bound and are never persisted as another authority. This also keeps lease
heartbeats available while an adapter responds. Native AST parsing runs in this
preflight rather than inside the authority transaction.
File/executable content hashes are computed outside the lock. Inside the transaction,
MACC checks file metadata identities (including inode and change time on Unix),
registered adapter identity and record/policy checksums. This is an optimistic
request snapshot for normal concurrent edits, not an atomic filesystem snapshot.

Live version checks use the existing three-second bounded probe, once per distinct
eligible tool up to the configured cap. No model invocation or provider install is
performed. TUI operations that can perform these checks run in background jobs;
Web uses its existing blocking-worker transport. A blocked/unavailable tool can
exclude its notes without stopping native code lookup or completing any task.

## Reviewed contradiction resolution

Open conflicts link two claims and exclude both from automatic context. Repeating
an open conflict in reverse order returns the existing ID. Conflict inspection is
project-bound and returns a checksum over the conflict and both current records.

Resolution requires a reason and confirmation. It is atomic and records the
before/after states and selected outcome in `memory_conflict_resolved`:

| Outcome | Effect |
|---|---|
| `both_stale` | Mark active claims stale; preserve other existing states. This is the conservative default for older clients. |
| `keep_left` / `keep_right` | Retain an already active, currently valid claim; invalidate the other active/stale claim or reject its proposal. Never promote an unreviewed winner. |
| `dismiss` | Explicitly conclude the conflict was spurious/compatible; preserve states and recheck the evidence of every active claim. |

A changed conflict/claim checksum rejects the operation and requires a new
inspection. A checksum is mandatory for outcomes other than the conservative
legacy default. No claim/history is deleted. Another still-open conflict continues
to exclude an otherwise retained claim. These are operator actions, outside the
restricted agent MCP catalog and available only through the existing owner gate
for Web/TUI mutations.

```sh
macc o2 memory-conflicts
macc o2 memory-resolve-conflict '<id>' --resolution keep_left \
  --checksum '<checksum-from-inspection>' --reason 'Reviewed both sources' --confirm
```

In **Web → Memory → Contradictory claims**, inspect a conflict, read both claims,
choose the outcome, enter the reason and explicitly confirm. The claim accesses
come from their recorded scopes, allowing comparison across tasks in this project.
Late replies from another task scope cannot restore the inspector/confirmation.

In **TUI → O2 → Memory**, `f` lists conflicts, Enter inspects the selected pair,
and `v` prepares its reason/outcome. Review the exact pending action and press `a`.
Blank outcome selects `both_stale`; the existing claims remain available for audit.

## Guided capture and revalidation

- Web proposal forms offer **Symbol or provider version evidence**. Load registered
  evidence adapters, then select the provider/tool and capture. The captured proof
  is shown explicitly; changing its source/selector/scope clears it. A proposal
  uses captured evidence when present, otherwise the existing source-file input.
- TUI `y` captures a symbol; `Y` captures a registered tool observation. Leave the
  proposal source path blank to use the inspected captured evidence. A new search
  scope clears the capture. Errors preserve form inputs; obsolete job results do
  not replace another scope's rows/details or a newly prepared action.
- Search freshness is evaluated against the requested worktree, and derived
  `stale` results explain mismatches. New bound evidence does not automatically
  rewrite canonical lifecycle state on a timeout, exhausted check budget or local
  source mismatch. Other worktrees may still satisfy the evidence.
- Inspect the canonical record before a transition. If it is still stored as
  active, explicitly mark it stale first. Then capture/review replacement evidence
  and reactivate. In Web, select **Use captured evidence for revalidation**; in TUI,
  use `R` after capture. All revalidation uses the inspected memory checksum and
  the existing explicit owner review. A stale provider observation must be
  recaptured; changing a status does not extend its TTL.

## Configuration in all clients

The shared typed policy catalog supplies validation, defaults and revision-checked
preview/apply for CLI actions, **TUI → 5 Policies** and **Web → Configure Memory**.
No client has separate persisted values or a way to disable safety filters.

| Canonical key under `memory` | Default | Range / interpretation |
|---|---|---|
| `provider_note_ttl_days` | 14 | 1–90 days; existing setting, now tied to observation time |
| `provider_note_probe_limit` | 2 | 1–8 distinct live tool checks per retrieval |
| `symbol_check_limit` | 50 | 1–500 distinct symbol checks per retrieval |
| `ranking_task_weight` | 100 | 0–1000; same source task |
| `ranking_reference_weight` | 30 | 0–1000; exact queried evidence path or qualified symbol |
| `ranking_authority_weight` | 40 | 0–1000; existing constraint/decision/security-note priority |
| `ranking_confidence_weight` | 10 | 0–100; confidence multiplier, separate from authority |
| `ranking_evidence_weight` | 15 | 0–100; each unique evidence tuple, capped at five |
| `ranking_length_penalty` | 20 | 0–100; per 400 body characters |
| `ranking_duplicate_penalty` | 80 | 0–1000; duplicate normalized claim body |

Hard scope, active/review, freshness, conflict and minimum-confidence filters run
before automatic selection. Scores cannot override the PRD or admit stale facts.
The strongest duplicate is retained first, with deterministic ID tie-breaking.
Every score records its factors in `rank_reason` and the context manifest. The
pushed payload contains citations/title/body; detailed score explanations stay in
the manifest. Its derived cache recipe is versioned to recompute exact framing
budgets. No automatic policy tuning or measured production savings is claimed.

## Compatibility and validation

There is **no new SQLite schema migration**. New evidence has an optional typed
`binding`; omitting it preserves legacy JSON/checksums. Existing policies receive
the new default values through the shared catalog. `macc save` continues to include
these records/configuration through the existing consistent O2 snapshot. Older
binaries cannot interpret new bound evidence; binary rollback needs a compatible
snapshot/export rather than silently discarding provenance.

Run `make check` for the standard pipeline. Deterministic P3-A coverage includes
symbol drift/rename/ambiguity, explicit revalidation, unchanged-wrapper upgrades,
TTL versus review timestamps, non-expiring static decisions, conflict CAS/defaults
and no implicit promotion, ranking/deduplication and policy tuning, bounded push
rendering, async TUI scope/form preservation and routed Web capture/conflict flows.
Tests use a temporary registered future-tool fixture, never a paid AI service.

## Native map extension

[P3-B native map](O2_P3_NATIVE_MAP.md) adds optional Tree-sitter navigation and
incremental declaration/signature artifacts for Rust, Python, TypeScript and TSX.
It preserves the Rust memory-fingerprint recipe and explicit evidence review.

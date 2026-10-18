# P0-C: task checkpoints and session rotation

## Ownership and boundaries

`core::session_store::checkpoints` owns provider-neutral task context. It uses the
same canonical `.macc/memory.db`, authority lock, transactional writes and private
consistent backups as P0-B. The coordinator runtime remains authoritative for task
states, review cycles, approval, testing and merge. A checkpoint never completes a
task, satisfies dependencies, creates a commit or changes Git files.

The module separates the typed record, Git/source inspection, schema rollout,
persistence and invocation lifecycle. Shared adapter runners call the local
`macc session-store` transport; no provider-name dispatch or undocumented native
compaction commands are introduced.

Checkpoints are active on newly migrated SQLite stores. Existing P0-B SQLite
stores retain their previous behavior until an explicit checkpoint schema
upgrade. JSON projects retain P0-A behavior. Stop coordinator, supervisor and
runners before the reviewed upgrade; it creates a SQLite-consistent backup first.
The checkpoint schema has its own additive version (`checkpoint_schema_version=1`)
and does not replace or downgrade the P0-B ownership schema.

## Record and evidence

Each immutable, checksummed `task_checkpoints` row includes:

- Project UUID, task, semantic role, tool, phase, canonical execution root and branch.
- Git HEAD, content digest and a separate combined index/content digest. Partially
  staged changes, unstaged edits, deleted files and untracked nonignored files
  contribute to evidence. Runtime `.macc/` files and `worktree.prd.json` are
  excluded from the implementation-content digest.
- The mandatory scoped PRD task plus global constraints and source policies.
  Mutable dispatch/review fields are excluded from its source hash. Explicit
  task `source_refs`, `spec_files` and `references` file arrays are pinned by hash;
  references outside the execution root are rejected. Remote/log/PRD references
  remain references, not fetched content.
- Acceptance IDs (existing IDs or stable ordinal labels within the pinned PRD),
  changed files, remaining work, decisions, test results, blocking issues, review
  evidence, source/memory references and next action.
- Schema/source validation and a reason. Narrative fields are deterministic.
  Unobserved test commands are labelled `not_observed`; provider exit success is
  never converted into passing acceptance tests or delivered work.

Current checkpoint generation does not synthesize semantic completed steps from
conversation logs. It preserves current Git evidence and unresolved outcomes and
asks the next invocation to revalidate criteria. The normalized provider outcome
and current coordinator review/error evidence survive continuation. No raw
provider transcript is imported. Known secret patterns and credential keys are
redacted before persistence or display; this is defense in depth, not permission
to put secrets in a PRD.

A record exceeding its configured byte budget fails explicitly. Mandatory task
criteria are never silently truncated. The Git reader rejects non-UTF8 names,
submodules/special files, inventories above 100,000 files and individual files
above 64 MiB; these need operator reconciliation or a future dedicated evidence
adapter. Ignored build outputs are outside the implementation digest.

## Invocation lifecycle

1. The shared SQLite runner prepares a checkpoint before acquiring a provider
   lease. It verifies repository membership, task sources, branch and retained
   worktree. No other lease may own that execution root.
2. An unchanged task/role/tool/configuration resumes the eligible P0-B transcript.
   A tool/configuration change or the invocation limit rotates eligible old
   transcripts, preserving lineage and all Git work. Switching back to a prior
   tool also starts a fresh transcript; transcripts are never portable.
3. Acquire rechecks the prepared context inside the lease transaction, preventing
   a stale context handle from being transplanted to another task or Git state.
4. The invocation receives the checkpoint plus its normal task/phase prompt.
   A fresh provider session must emit:
   `MACC_CHECKPOINT_ACK: <checkpoint-id> task=<task-id> phase=<phase>`.
   Plain text and text contained in JSON/JSONL are accepted. Missing
   acknowledgement prevents accepting a successful invocation and retires that
   transcript. Nonzero/quota failures retain their original failure semantics.
5. After provider exit, a fenced checkpoint write records current Git evidence and
   outcome before lease release. Signal cleanup stops the provider, captures an
   interrupted checkpoint where possible, and retires the transcript.

`SIGKILL`, machine loss or a failed checkpoint write cannot produce a trustworthy
post-invocation record. Existing lease quarantine remains mandatory. Once the
retained worktree is idle, preparation deterministically reconciles changed Git
state against the same branch and pinned sources, preserving objections and
requiring acceptance revalidation. Unexpected content changes rotate the old
transcript. A coordinator-created commit with unchanged file content can retain
the same performer transcript; index/HEAD reconciliation is still recorded.

Source/branch/worktree changes, history rewrites, missing evidence and checksum
mismatches stop invocation for reconciliation. The shared runner reports the
existing `precondition_unmet` contract with concrete conditions; development
stops retrying E903, and native phases check it before any automatic commit. Explicit reconciliation shows prior
and current evidence, requires the reviewed current SHA-256, and checks it again
on Apply. It captures current sources without changing files or treating the old
record as completion. Reconciliation rotates eligible old transcripts.

Adapters without native session resume still use checkpoint ownership and fresh
invocations; no resume syntax or synthetic native capability is assumed.
Changes to mandatory sources during an invocation are retained as evidence but
marked `needs_reconciliation`; they cannot silently replace the prior contract.

Independent reviewers keep a separate role. Development and correction use the
performer role, retain review objections and keep the existing review budget:
one completed rejected review at `max_review_cycles: 1` still permits its
correction and configured validation. WIP commits remain partial delivery.

The low-level ownership `acquire` protocol remains backwards compatible for
non-runner consumers. A supplied checkpoint handle is verified and fenced; MACC's
shared SQLite runner always supplies one when this feature is active. New
invocation integrations must use prepare/acquire/finish together.

## Drain semantics

`coordinator::drain` is shared by stop requests, selection and completion checks.
The snapshot includes claimed/development, testing, review, requested corrections,
PR-open, merge-queued tasks and parked same-worktree continuations. Active
performer/merge jobs hold drain even when their task state has already changed.
A captured task parked back in `todo` remains pending and may resume in its
retained worktree; unrelated tasks cannot dispatch. Terminal/blocked tasks leave
the active set. Missing/corrupt drain evidence requires reconciliation rather than
silently declaring completion. Task failures still follow existing coordinator
blocking semantics; a drain does not convert them into success.

## Configuration and interactive clients

Both clients use the shared typed field catalog and canonical revision-checked
settings transaction:

```yaml
session_management:
  checkpoints:
    max_content_bytes: 16384   # 2048..65536; fail rather than truncate criteria
    rotate_after_uses: 20       # 1..1000; next invocation boundary
```

These settings are editable in **TUI J → Sessions** and **Web Setup → Sessions**,
with validation/preview, Apply, defaults, override removal and stale-draft
protection. Rotation never occurs in the middle of a provider call.

In the TUI, press **h** within Sessions for checkpoint inventory and recovery:
Up/Down selects, Enter inspects, **p** previews reconciliation, **a** applies the
reviewed plan, **x** cancels, **u** previews schema upgrade, PageUp/PageDown scrolls.
The Web checkpoint section offers filtering, inspection, upgrade planning and
reviewed reconciliation. Both use core services; mutations retain ownership gates.
Existing session Close remains available to deliberately start a fresh transcript
at the next invocation while keeping checkpoint and session metadata.

```bash
macc session-store upgrade
macc session-store upgrade --apply --expected-sha256 <reviewed-source-hash>
macc session-store checkpoint <checkpoint-id>
macc session-store reconcile <checkpoint-id>
macc session-store reconcile <checkpoint-id> --expected-sha256 <reviewed-current-hash>
```

`checkpoint-prepare` and `checkpoint-finish` are bounded JSON-on-stdin runner
operations. Web uses `GET /api/v1/sessions/checkpoints/{id}` and
`GET/POST /api/v1/sessions/checkpoints/{id}/reconcile`; upgrade reuses the reviewed
`GET/POST /api/v1/sessions/authority/upgrade` contract. Read-only views do not
expose lease tokens. SQLite backups include checkpoints. JSON rollback retains
them in SQLite and its consistent snapshot; JSON execution does not use them.

P0-D [usage accounting](USAGE_TELEMETRY.md) now shares invocation ownership and
checkpoint phase boundaries. Adaptive token-pressure triggers, semantic memory
promotion and native compaction capability probes remain later work packages.


## Integration merge repair

The coordinator's private `<git-common-dir>/macc/integration` slot has a dedicated
merge-repair contract. Before invoking its ordinary adapter runner, the merge hook
calls `session-store merge-repair-prepare` with the source task worktree and pinned
base/feature revisions, then explicitly selects role `merge_fix`, phase `merge`.
The checkpoint validates the same Git project, exact integration slot, task ID,
mandatory sources, HEAD ancestry and any MERGE_HEAD against those pinned inputs.
It captures index stages, conflict file content and untracked work; it never stages,
resets or resolves files itself. Successful completion can advance detached HEAD
only to a commit containing both inputs. An ordinary performer cannot opt into
this contract in its normal task worktree.

Interrupted repair evidence remains capturable. Changed mandatory sources require
reviewed reconciliation through the existing checkpoint workflow, which understands
the same pinned repair contract. The manifest is runtime evidence in the private
slot's `.macc/merge-repair.json`; it does not replace lease fencing or grant task
acceptance. No SQLite schema change is required. JSON-mode projects retain their
existing runner path with an explicitly independent merge-repair role.


Merge-hook output is captured in private stdout/stderr log files while the child
runs, avoiding a full-pipe deadlock with verbose agents. Execution is bounded by
its timeout/output limit and terminated as an owned process group. Publishing
requires a successful hook exit and no explicit failure/precondition verdict,
plus a completed conflict-free merge. A commit alone cannot bypass a failed
checkpoint/source validation. Existing successful legacy hooks without markers
remain compatible.

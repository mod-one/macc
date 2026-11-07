# Diagnostic delivery and external verification

A verification task may complete an honest negative diagnostic report while its
feature acceptance and release authorization remain withheld. MACC records these
as separate outcomes; a completed report does not complete the task, advance its
PRD queue, merge its branch, or approve an intervention.

## Explicit task contract

The task must have category `verification` and declare
`execution_contract.diagnostic_completion: true`. For existing task contracts,
MACC also recognizes `completion_vs_acceptance` beginning with the exact sentence
`Implementation and honest diagnostic review completion are distinct from feature acceptance.`
Ordinary implementation tasks and failed ordinary decision bindings cannot use
this outcome.

The review prompt protocol v4 describes this optional terminal marker:

```text
MACC_DIAGNOSTIC_RESULT: {"report_path":"reports/task-review.json","report_digest":"sha256:<exact-file-byte-digest>","completed":true,"feature_accepted":false,"release_authorized":false}
```

The reviewer must also emit its exact decision acknowledgment, explicit compliance
entries and `REVIEW_VERDICT: OK` for the diagnostic work. The committed JSON report
must name the current task in `task`, declare `verdict: changes_requested`, and
set `featureAccepted: false` and `releaseAuthorized: false`.

The native service checks the task contract, allowed paths (using the existing task-scope glob matcher), canonical worktree containment, committed report, exact digest, 1 MiB size
limit, unchanged source commit and clean worktree. Every non-passing non-advisory
binding must be an intervention with an explicit compliance entry. It then stores
a native `diagnostic_complete_external_pending` receipt, including the unresolved
decision identifiers and reviewed commit. Runtime metadata is loaded from that
receipt, never inferred from JSON inside a tool error. Receipt readback rechecks
the context, source revision, cleanliness and report bytes. Merge still requires
an accepted decision receipt; this diagnostic receipt cannot satisfy that gate.

New contexts use rendering version 4. Existing version 2/3 contexts retain their
rendering and digest; no database migration is required.

## Recovery and clients

The current failure is `E903`, origin `decisions`, phase `review`, tagged
`DM_EXTERNAL_VERIFICATION`. CLI/API run summaries include `phase` and optional
`diagnostic_delivery`. TUI and Web distinguish:

- Diagnostic report complete.
- Feature changes requested.
- External verification pending.
- Release not authorized.

Open Decisions and independently verify the selected assistance, or explicitly
revise/reconcile the decision through the existing governed workflow. A human
`performed=yes/result=ok` is attributed evidence, not an independent observation.
MACC never silently changes `assist` to `defer`. Retry the retained review after
the prerequisite changes; keep the task branch and commits. The supervisor can
produce an actionable native diagnosis from this receipt without another model
call or speculative application repair. Existing meaningful-change waiting rules
apply. An unchanged retry cannot supply missing external evidence.

A later failed phase replaces active diagnostic metadata and retains it as history;
a successful continuation clears active errors. Legacy stored decision-compliance
errors are projected as E903 without rewriting application state.

## Terminal result attribution

Installed performer automation includes `result_protocol.sh`. Task result fields
must be unindented, outside code fences, prompt delimiters and command transcripts,
and belong to the final contiguous result block. Template placeholders such as
`<reason>` anywhere in an explanation are discarded. A later result cannot inherit
an earlier attempt's explanation. Adapter-declared assistant/result projection
remains the boundary for structured provider output; quota detection still takes
precedence over success markers. No provider-specific exception is required.

Previous development outcomes remain history when review or another phase fails.
The current task/phase/error cause takes precedence in summaries and retry targeting.

## Optional Git housekeeping

Worktree preparation detaches directly instead of trying to check out a reference
branch already occupied by another worktree. Optional remote fetch runs without
interactive Git prompts and logs operation, remote, path, exit status and an
authentication/network/Git-exit classification. Required fetch failures still fail
preparation. Missing-ref probes are quiet, and branch cleanup skips `HEAD` and the
reference branch. These changes do not bypass Git delivery or compliance checks.

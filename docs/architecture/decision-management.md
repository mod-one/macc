# Decision management: local implementation

The architecture baseline remains the target contract. The implementation is opt-in and uses explicit `local_trust`: Git identity attributes local operator actions; it does not authenticate an independent human principal. `secure_approval` is rejected rather than silently downgraded. Signed Git publication and an isolated approver broker are separate integrations; this release does not claim those guarantees.

## Enable and operate

Stop the coordinator before first activation or disabling the feature. Consultation thresholds can change while it runs; affected contexts are checked again before invocation, completion and publication.

```sh
macc decisions config --enabled true --mode high
macc decisions list
macc decisions answer REQUEST_ID --version 1 --text 'Exact answer and conditions'
macc decisions feedback --text 'Retain the existing public contract'
macc decisions history
macc decisions task-context TASK_ID
macc decisions verify
macc decisions recover
```

TUI: `D` opens the decision center; `Tab` selects an option, `Enter` inserts a newline, `F5` submits, `Esc` defers, `F4` inspects issued contexts/receipts, `F6` changes autonomy, `F7` enables/disables, `F8` adds project direction. Drafts survive closing and restarting the client. The input remains visible on smaller terminals; details scroll separately.

Web: **Decisions** opens the inbox/history. New questions open only when another editor/dialog is not active. A preselected recommendation is not submitted automatically. Requests are versioned; a competing answer does not overwrite a draft. History and exact issued contexts remain inspectable in full autonomy.

```yaml
automation:
  coordinator:
    decisions:
      enabled: true
      human_question_min_risk: high  # low | medium | high | critical | never_ask
      trust_mode: local_trust
      max_context_bytes: 32768       # includes rendered instructions and all bindings
      max_request_bytes: 65536
      mandatory_human_triggers: [production, irreversible]
      approval_reuse: exact_revision # exact_revision | content_equivalent | never
      approval_manifests: {}         # gate ID -> explicit repository-relative files
```

Raising the threshold grants more autonomy. `never_ask` delegates material choices at all risk levels, records the actual agent/coordinator origin and never creates a human vote. It cannot create missing credentials, evidence or essential facts. Mandatory standalone approval/authorization requests require a person in the other modes. Declared risk is raised by structural task/impact evidence; missing impact evidence is conservative.

## Performer protocol

MACC appends the complete current context to the actual development/phase prompt. The prompt explains investigation, structured intake, recommended options, graph proposals, acknowledgements and review evidence. It supplies the current CLI path and pinned project/plan/task/attempt/phase through inherited environment variables. A context file alone does not count as delivery.

An example proposal file:

```json
{
  "idempotency_key": "A-public-contract-v1",
  "task_id": "A",
  "kind": "decision",
  "scope": "task",
  "question": "Which public contract must both consumers keep?",
  "context": "The existing consumers implement conflicting assumptions.",
  "investigated": ["Read both consumer call sites and the contract specification"],
  "options": [
    {"id": "keep", "label": "Keep the current public contract", "consequences": ["Update the inconsistent consumer"]}
  ],
  "recommended_option_id": "keep",
  "recommendation": "Keep the current contract and update the inconsistent consumer.",
  "affected_tasks": ["A"],
  "risk": "high",
  "impact": {"shared_contract": true, "evidence": ["Two consumers share this contract"]}
}
```

```sh
"$MACC_DECISION_CLI" --cwd "$MACC_DECISION_ROOT" decisions request --file /absolute/proposal.json
```

Pending human input parks only affected work and its consumers. The provider exits with the existing `precondition_unmet` protocol; it does not occupy a live process waiting for a person. Unchanged requests deduplicate across attempts. A new material resolution invalidates affected old contexts; committed/dirty work is preserved and the next invocation receives current instructions. Native SQLite sessions rotate when applicable bindings change. Existing checkpoint drift and dirty-worktree guards remain in force: a graph change does not waive changed specification review.

The wrapper verifies that the actual prompt contains the exact rendered context. It separately checks `MACC_DECISION_ACK: <digest>` from assistant output and current context freshness. For nonadvisory bindings, the independent review phase must produce per-decision `MACC_DECISION_COMPLIANCE` with `pass` and concrete evidence. Receipt, acknowledgement, human authority and compliance are different records. The clean, unchanged reviewed commit is attested after review postchecks. The publication fence repeats context and reviewed-artifact verification immediately before integration. With decision management enabled, merge conflicts return to the retained task’s fix/review pipeline (at most three repair handoffs); detached merge hooks cannot modify artifacts after compliance review.

Initial PRDs may contain a top-level `decision_requests` array using the same proposal schema. Intake occurs once per scoped idempotency key. The planner prompt explains this contract. Its external package is pinned to the locally verified upstream commit `mod-one/skills@93f177b4f71383bcbdf2a376e58beaa6b10e9f62`; no cached/user installation is rewritten. The Rust prompt/intake bridge adds the decision protocol without assuming that upstream scripts already implement it.

## Dynamic graph

```json
{
  "plan_id": "PLAN_ID",
  "base_graph_version": 1,
  "idempotency_key": "add-contract-followup-v1",
  "reason": "Implement the recorded contract decision",
  "decision_refs": [],
  "operations": [
    {"op": "add_task", "task": {"id": "FOLLOWUP", "title": "Update the second consumer", "dependencies": ["A"]}}
  ]
}
```

```sh
macc decisions graph show
macc decisions graph preview patch.json
macc decisions graph apply patch.json
# Explicitly reconcile a hand-edited PRD only after inspecting preview's disk digest:
macc decisions graph reconcile patch.json --source-digest sha256:INSPECTED_DIGEST
```

Operations support add/update, dependency add/remove, disable, supersede and decision bindings. Validation rejects cycles, dangling dependencies, ID reuse, forged execution history and changes to delivered tasks. Consumers of a retired task must be rewired/retired in the same patch. Active contracts require a stopped worker; explicit retirement quarantines incompatible completions without deleting their worktrees. Replaced tasks inherit applicable decisions through the supersession mapping.

A performer may attach `proposed_patch` to a structured request. The same risk policy routes it. Autonomous resolution and graph commit share one SQLite transaction. For human intake, selecting the recommended option explicitly applies the displayed proposal; custom feedback is recorded without implicitly accepting the patch. Agent proposals cannot remove approval dependencies or rewrite governance. The standalone audit produces patch proposals rather than rewriting protected PRDs.

CAS rejects competing graph versions; idempotency prevents duplicate commits. An outbox projects accepted graphs into the PRD and updates queue fingerprints. Recovery recognizes earlier accepted projections, repairs missing exports and refuses to overwrite unknown edits. Explicit reconciliation preserves the inspected external JSON in the journal.

## Persistence and approvals

The authority is `<git-common-dir>/macc/control.sqlite`; linked worktrees share the project identity, while plan/source/reference-branch identities isolate graphs, decisions and votes. Explicit `project` guidance also reaches future PRD plans on the same reference branch; task/path/requirement decisions do not leak into colliding task IDs in other plans. A shared controller lock prevents competing dispatch owners. `macc clear` retains this authority and reports that retention. No new branch/worktree deletion is introduced.

The journal is hash chained. Verification checks SQLite integrity, graph/decision/approval projections and acknowledged receipts against journal evidence. Hashes detect corruption; same-UID filesystem access is not an independent security boundary.

Native votes preserve the existing role/quorum/TTL evaluator and bind the full Git commit, gate/task contract and current policy. `content_equivalent` additionally requires configured explicit files, exact Git blob bytes, dependency contracts and policy context; it is never inferred from a filename/hash alone. `never` binds a gate vote to the current recorded execution identity and does not restore it into another execution.

Historical `gate_decisions` are imported idempotently, with their original actor, role, revision, evidence, reason and timestamp. They remain labelled `legacy_local_trust`, with `binding_verified: false` when the original plan/policy binding cannot be proven. They are retained as evidence and cannot silently acquire current-policy authority. Renew the required votes through existing approval commands when such a binding is missing. Native bound votes recorded after activation remain verifiable after runtime projection loss.

Standalone operation authorization requires `kind: authorization` and a stable `operation_id`. `macc decisions authorize ID --operation OPERATION_ID` reserves only that exact operation. Reservations replay idempotently for the same operation and cannot authorize another one. An unknown external result still requires reconciliation; a reservation does not promise exactly-once external execution.

Save bundles include a SQLite-consistent control snapshot when sessions are included, using manifest version 3. Config-only/no-session restores leave live decision authority intact. Restore checks root/project identity, checksum and history ancestry; it preserves newer live history and rejects divergent/foreign authority. Exported JSON is labelled `local_trust_evidence_only`; unsigned exports are not an approval import channel.

For remote web binding, set a 32–128 character `MACC_WEB_ACCESS_TOKEN` containing letters, digits, `_` or `-`. API bearer authentication and same-origin checks apply in addition to existing ownership checks. A successful bearer request establishes an HttpOnly, SameSite=Strict cookie for same-origin event streams/WebSockets. The token is removed from performer, phase and supervisor tool environments. This authenticates access to the local server; it does not provide the isolated approver guarantees of `secure_approval`.

## Validation scope

Core tests cover thresholds, exact feedback, scoped blocking, deduplication, graph CAS/cycles/delivered history, graph proposals, retirement, reconciliation, context delivery/ACK/review/freshness, backups, foreign identity rejection, approval manifests and projection corruption. Client tests cover explicit submission, deferred drafts, full autonomy, input focus, small TUI sizes and remote authentication. A fake provider exercises the real performer shell/shared adapter/CLI receipt path without external AI calls.

These tests do not certify all AT01–AT68 scenarios or production use of every provider. General availability still requires the independent secure-approval integration if that trust mode is promised, signed transport if enabled, and dedicated crash/concurrency/provider qualification against the complete baseline matrix.

# Decision management: local implementation

The implementation is opt-in. `local_trust` attributes operator actions using Git identity. `secure_approval` verifies externally issued SSH attestations against an isolated, protected role registry; MACC holds no approver signing key. Signed archive/Git Notes transport is explicit; divergent histories use a separate guided reconciliation workflow. Deployment qualification is distinct from implemented capability; see the [acceptance matrix](decision-management-acceptance.md).

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

TUI: `D` opens the decision center; `Tab` selects an option, `Enter` inserts a newline, `F5` submits, `Esc` defers, `F4` inspects issued contexts/receipts, `F6` changes autonomy, `F7` enables/disables, `F8` adds project direction, `F9` selects a structured response field, `F10` explicitly accepts the displayed patch, `F11` chooses answer/reject/request-changes, and `F12` writes the exact isolated-approver challenge. `F1` lists these shortcuts, including in compact mode. Drafts survive closing and restarting the client. The input remains visible on smaller terminals; details scroll separately.

Web: **Decisions** opens the inbox/history. New questions open only when another editor/dialog is not active. The initial answer is custom: no option or graph patch is accepted by default. Requests are versioned; a competing answer does not overwrite a draft. History and exact issued contexts remain inspectable in full autonomy.

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

A performer may attach `proposed_patch` to a structured request. The same risk policy routes it. Autonomous resolution and graph commit share one SQLite transaction. For human intake, selecting an option and applying its patch are separate actions. Patch application additionally requires the explicit checkbox/TUI `F10` or CLI `--apply-patch`, the displayed selected option’s patch, a binding answer and a current graph version. Free text, advisory responses, rejection and request-changes cannot implicitly apply a patch. Agent proposals cannot remove approval dependencies or rewrite governance. The standalone audit produces patch proposals rather than rewriting protected PRDs.

CAS rejects competing graph versions; idempotency prevents duplicate commits. An outbox projects accepted graphs into the PRD and updates queue fingerprints. Recovery recognizes earlier accepted projections, repairs missing exports and refuses to overwrite unknown edits. Explicit reconciliation preserves the inspected external JSON in the journal.

## Persistence and approvals

The authority is `<git-common-dir>/macc/control.sqlite`; linked worktrees share the project identity, while plan/source/reference-branch identities isolate graphs, decisions and votes. Explicit `project` guidance also reaches future PRD plans on the same reference branch; task/path/requirement decisions do not leak into colliding task IDs in other plans. A shared controller lock prevents competing dispatch owners. `macc clear` retains this authority and reports that retention. No new branch/worktree deletion is introduced.

The journal is hash chained. Verification checks SQLite integrity, graph/decision/approval projections and acknowledged receipts against journal evidence. Hashes detect corruption; same-UID filesystem access is not an independent security boundary.

Native votes preserve the existing role/quorum/TTL evaluator and bind the full Git commit, gate/task contract and current policy. `content_equivalent` additionally requires configured explicit files, exact Git blob bytes, dependency contracts and policy context; it is never inferred from a filename/hash alone. `never` binds a gate vote to the current recorded execution identity and does not restore it into another execution.

Historical `gate_decisions` are imported idempotently, with their original actor, role, revision, evidence, reason and timestamp. They remain labelled `legacy_local_trust`, with `binding_verified: false` when the original plan/policy binding cannot be proven. They are retained as evidence and cannot silently acquire current-policy authority. Renew the required votes through existing approval commands when such a binding is missing. Native bound votes recorded after activation remain verifiable after runtime projection loss.

Standalone operation authorization requires `kind: authorization` and a stable `operation_id`. `macc decisions authorize ID --operation OPERATION_ID` reserves only that exact operation. Reservations replay idempotently for the same operation and cannot authorize another one. The native operation runner described below adds durable execution/outcome reconciliation; a reservation alone does not promise exactly-once external execution.

Save bundles include a SQLite-consistent control snapshot when sessions are included, using manifest version 3. Config-only/no-session restores leave live decision authority intact. Restore checks root/project identity, checksum and history ancestry; it preserves newer live history and rejects divergent/foreign authority. Exported JSON is labelled `local_trust_evidence_only`; unsigned exports are not an approval import channel.

For remote web binding, set a 32–128 character `MACC_WEB_ACCESS_TOKEN` containing letters, digits, `_` or `-`. API bearer authentication and same-origin checks apply in addition to existing ownership checks. A successful bearer request establishes an HttpOnly, SameSite=Strict cookie for same-origin event streams/WebSockets. The token is removed from performer, phase and supervisor tool environments. This authenticates access to the local server; it does not provide the isolated approver guarantees of `secure_approval`.

## Governed questions, investigation and arbitration

Trusted rules compose risk and response requirements. A routing rule is not automatically a decision topic: configure `decision_domain` for a shared authority, or have performers identify a stable `topic`, requirement or affected path. This prevents an unrelated question from inheriting the answer to the first question matching a broad risk rule.

```yaml
governance_rules:
  - id: public-contract
    affected_paths: [src/api]
    decision_domain: public-api-contract
    project_wide: true
    minimum_risk: high
    mandatory_human: true
    evidence_paths: [docs/api-contract.md]
    response_fields:
      - {id: rationale, label: Compatibility rationale, required: true, multiline: true, choices: []}
investigation: {max_rounds: 2, max_source_bytes: 32768}
```

These keys belong under `automation.coordinator.decisions`. Rules may also match `task_prefixes` or `requirement_refs`. `standing_answer` and optional `standing_choice` provide an explicitly configured existing answer when required sources are available. An unavailable source or declared `missing_facts` blocks only affected work; full autonomy cannot fabricate it. Native source freshness checks do not spend another performer investigation round.

A performer can supply a fact map through `decisions investigate ID --version N --facts-file facts.json`. Each exact missing fact maps to `{source_path, source_digest, excerpt}`. `source_digest` is `sha256:` plus ordinary SHA-256 of the UTF-8 file bytes, compatible with `sha256sum`. MACC verifies actual bounded source bytes and the exact excerpt before resolving it, retaining attribution and evidence. Exhaustion produces a scoped prerequisite diagnostic rather than repeated AI calls or invented facts.

The first resolved binding for a typed subject governs later proposals. A competing performer receives its recorded outcome and a `reused_from` reference; it cannot privately apply a contradictory proposal. Replacing that outcome requires `supersedes: [{id, version}]`. Native reconciliation also replaces derived aliases; revoking their originating authority revokes those aliases too. Independent human bindings require explicit references. Existing guidance cannot supply a fresh approval/operation authorization. Rejections remain blocking until explicitly addressed/revoked. Changed policy thresholds re-evaluate unanswered requests under the publication fence before configuration returns without rewriting old human choices.

The CLI accepts `--fields-file response.json`, `--outcome answer|reject|request_changes` and `--apply-patch`. Approvals/authorizations need explicit evidence for a positive human answer. Web and TUI preserve multiline text, structured fields and unsent drafts. Web exposes distinct delivery, acknowledgement and independent compliance status; exact context/receipt inspection remains available in both clients.

## Versioned editing and recovery

With decisions enabled, the Web PRD editor requires `graphVersion` and translates edits through the same patch controller. Unknown contract fields and archived `tasks_done` are retained; archived IDs cannot be reused. Worker PRDs are read-only projections. Concurrent/stale edits fail without overwriting accepted history. Plan metadata edits and relevant prerequisite contracts enter the semantic context.

Context protocol v2 records `graph_version` and `decision_epoch` as envelope watermarks. The semantic digest includes the task contract, reachable prerequisite contracts, applicable bindings, policy and plan context; unrelated graph counters alone do not invalidate an attempt. Checkpoints and provider leases retain this binding, and stale lease use is rejected while release remains possible. Existing v1 context checksums remain readable; new invocations use v2 and require fresh acknowledgement. Recovery preserves dirty/committed work and refuses incompatible publication.

## External operation execution

An executable authorization includes a stable `operation_id` and:

```json
{"operation_spec":{"adapter":"deploy-staging","target":"staging","source_revision":"FULL_REVIEWED_COMMIT","arguments":{"artifact":"release-42"}}}
```

Configure the adapter under `operation_adapters` in the trusted policy:

```yaml
operation_adapters:
  deploy-staging:
    command: [/opt/project/bin/deploy, --target, '{target}', --artifact, '{argument:artifact}']
    probe: [/opt/project/bin/deploy-status, --operation, '{operation_id}']
    allowed_targets: [staging]
    environment_names: [DEPLOY_TOKEN]
    timeout_seconds: 60
    max_output_bytes: 65536
    verified_idempotency: 'Deployment service stores operation keys before applying effects; verified by project tests'
    idempotency_key_arg: --operation-key
```

Only whole argument placeholders are expanded; no shell interpolation is added. The reviewed source must remain a clean exact commit. Credentials are explicitly allowlisted from the native process environment; private `MACC_*` authority variables are refused. Unix capture drains both streams with a combined hard output budget and kills the owned process group on timeout/overflow. Other platform behavior requires separate qualification.

```sh
macc decisions operations execute DECISION_ID
macc decisions operations list
macc decisions operations probe OPERATION_ID --version N
macc decisions operations reconcile OPERATION_ID --version N --state failed --evidence 'Verified external outcome and evidence reference'
```

Execution intent is committed before spawning. States distinguish `reserved`, `executing`, `succeeded`, `failed` and `unknown`. Crashes, timeouts and failures can leave partial effects: MACC blocks blind repetition. A trusted probe returns 0 for success, 10 for proven absence eligible for a verified keyed retry, 11 for final failure; other outcomes stay blocked. Manual reconciliation uses version checks and evidence, refuses a live owner/child, and cannot rearm a non-idempotent adapter. Repeated successful execution requests return their recorded result without launching again. Recovery and journal verification include operation state.

## Isolated approval attestations

Secure approval currently requires Unix file ownership semantics and `/usr/bin/ssh-keygen`. An administrator must install the registry and allowed-signers file outside the performer's writable tree, owned by a different OS principal, with no writable/symlink ancestor. A same-UID file, missing registry or unsupported platform is rejected. Running MACC as root cannot supply this separation. Protect the deployed service, configuration and storage too; a local verifier does not turn arbitrary same-UID code/filesystem access into a sandbox.

```yaml
trust_mode: secure_approval
trust_registry: /etc/macc/approvers.json
```

Example external registry (public keys, no private keys):

```json
{
  "allowed_signers": "/etc/macc/allowed_signers",
  "roles": {"owner@example.test": ["OWNER"]},
  "required_roles": {
    "decision.answer": ["OWNER"],
    "gate.vote": ["OWNER"],
    "graph.mutate": ["OWNER"],
    "policy.configure": ["OWNER"],
    "transport.import": ["OWNER"]
  }
}
```

The external human/broker must authenticate the person and display/review the exact action before signing; a service that signs every performer request defeats isolation. MACC exposes challenges, not a signing endpoint:

```sh
macc decisions answer ID --version N --text 'Exact response' --challenge
macc decisions answer ID --version N --text 'Exact response' --attestation-file proof.json
macc decisions gate-attestation --task APP --record-file vote.json
macc decisions gate-attestation --task APP --record-file vote.json --attestation-file proof.json
macc decisions config --file policy.json --challenge
macc decisions config --file policy.json --attestation-file proof.json
```

All other response flags/fields must be identical between challenge and submission. Gate records use the exact full commit, principal and entitled role. Graph previews expose separate `challenge` and `reconcile_challenge`; apply/reconcile accept `--attestation-file`. Web provides the challenge and external-attestation input; TUI `F12` writes a challenge file and `F9` selects the proof field.

An attestation is `{body, signature}`. Its body contains `principal`, `role`, `action`, `action_digest`, `nonce`, `issued_at` and `expires_at`. `action_digest` is `sha256:` plus SHA-256 of the challenge's compact UTF-8 JSON with recursively sorted object keys. Sign the body's same canonical JSON bytes with SSH namespace `macc-approval`; `signature` is the resulting armored SSH signature text. Use a fresh nonce of at least 16 characters and an expiry within one hour of issuance. Role entitlement, action, subject/version/policy binding, expiry and signature are verified before mutation. Recorded human bindings retain and recheck their proof; historic local-trust records cannot silently authorize secure execution. Changing an already secure policy also requires its own isolated proof.

A Python canonicalization equivalent for these string/integer envelopes is `json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode('utf-8')`. The native signer/transport uses the same representation. Never pass a private signing key to the performer.

## Signed transport and Git Notes

```sh
macc decisions transport sign --key /path/to/external-signing-key --principal owner@example.test --file archive.json --git-note
macc decisions transport import --file archive.json --allowed-signers /path/to/trusted-signers
# Initial clone only: explicitly enroll an empty, stopped authority
macc decisions transport import --file archive.json --allowed-signers /path/to/trusted-signers --enroll-project
# After separately fetching the notes ref, select an explicit object:
macc decisions transport import --git-note FULL_OBJECT_ID --allowed-signers /path/to/trusted-signers
```

Signing requires stopped execution and is an explicit operator action. No private key is copied into a worktree. Archives are bounded, scanned for potential secrets and signed under namespace `macc-decisions`. Notes use `refs/notes/macc-decisions`; publication preserves existing conflicting notes. Network fetch/push remains explicit. A signed publisher authenticates transport, not an invented human vote: secure imports also require entitlement from the protected registry, and imported human approvals still need their own valid evidence.

Import requires the same repository ancestry and a verified signature. Existing authority only accepts a history extension; older or divergent history is retained/rejected for explicit reconciliation. Cross-project enrollment requires an empty authority and an explicit flag. All tables and their journal projections verify inside the import transaction before commit; the receiving machine rebuilds projections and reconciles interrupted operations. This is stopped-authority transfer, not simultaneous multi-machine coordinator ownership.

## Validation scope

Executed local suites cover native governance/intake, arbitration with concurrent stores, exact responses, graph CAS/history, editing, context/checkpoint/lease safeguards, receipt/ACK/review, operation output/timeout/unknown outcomes, SSH signature negatives, signed transfer into a Git clone, backup/recovery and client drafts/accessibility. A fake provider exercises the actual performer shell/shared runner/CLI path. See the [AT01–AT68 matrix](decision-management-acceptance.md) for the evidence and its limits.

Live provider qualification, a deployed organization's human-authenticated approver, real browser/device visual checks, and production-scale/cross-platform fault qualification remain deployment work. The matrix deliberately does not label all 68 full scenarios passed from unit tests alone.

## Request lifecycle and answer effects

Options may each carry their own `proposed_patch` and `requires_replan`. A selected alternative uses that alternative's patch. The legacy request-level patch belongs to the recommended choice; it is never silently applied for another answer. Positive human responses accept `effects: auto | no_graph_change | replan` and optional explicit `patch`. The CLI exposes `--effects` and `--patch-file`; the Web form and TUI Home actions expose the same choices.

`awaiting_effects` means the answer is durably recorded but dependent implementation cannot resume yet. The coordinator can dispatch the originating development task in **planning-only** mode, within `investigation.max_rounds`. Its delivered context authorizes a mechanical DAG proposal through:

```sh
"$MACC_DECISION_CLI" --cwd "$MACC_DECISION_ROOT" decisions effects propose ID --version N --file effects.json
```

The native service verifies the issued planning receipt, unchanged retained source/Git state, current decision/version, affected tasks, path boundary and governance. Planning cannot rewrite project policy, remove an approval requirement or change an unrelated task. New tasks inherit the recorded answer even when they are independent of the old dependency chain. Applying effects and resolving the answer share one control transaction. Planning completion cannot count as implementation success: the coordinator must issue a fresh implementation context. Other tasks continue. A derived answer stays `waiting_dependency` until its originating effects are resolved; revocation also invalidates derived answers.

A human may explicitly confirm `no_graph_change` when the current DAG is adequate. Advisory feedback and rejected/request-changes responses never apply a graph patch. Their exact text remains immutable evidence. Separate `answered_version` and `effects_commit` preserve the original signed response after planning changes the decision version.

```sh
macc decisions revise ID --version N --file revised-request.json
macc decisions withdraw ID --version N --reason 'Superseded proposal'
macc decisions revoke ID --version N --reason 'Binding no longer applies'
macc decisions effects apply ID --version N --file reviewed-effects.json
```

A revision has a new idempotency key and `revision_of` lineage. Replacement of an already resolved binding uses explicit supersession. Revision of a rejected request retains the need for a new human answer; `never_ask` cannot turn the earlier rejection into consent. Optional `expires_at` is RFC3339; an expired request remains a scoped prerequisite until reviewed revision/withdrawal. Exact lifecycle retries are idempotent and signed revision/revocation/withdrawal evidence is retained transactionally. Web provides guided revision/reason forms; TUI **Home** opens lifecycle, gates/roles, external operations, independent approvals and diagnostics. Very small terminals use a full-screen fallback with visible send/escape keys.

## Independent approval connector

The protected registry may configure an external command connector:

```json
{
  "allowed_signers": "/etc/macc/allowed_signers",
  "roles": {"owner@example.test": ["OWNER"]},
  "required_roles": {
    "decision.answer": ["OWNER"], "decision.feedback": ["OWNER"],
    "decision.revise": ["OWNER"], "decision.withdraw": ["OWNER"],
    "decision.revoke": ["OWNER"], "decision.effects.apply": ["OWNER"],
    "operation.reconcile": ["OWNER"], "gate.vote": ["OWNER"],
    "graph.mutate": ["OWNER"], "policy.configure": ["OWNER"],
    "transport.import": ["OWNER"]
  },
  "broker": {"command": ["/opt/macc-approver/bin/connector"], "timeout_seconds": 30}
}
```

The executable and its implementation/dependencies must remain outside performer control. The administrator installs the connector under the independent service/principal; MACC does not install a signing key or invent an organization's authentication policy. No network credential is inherited automatically. MACC invokes the protected executable with a clean environment, bounded output and a 1–120 second deadline per exchange. It never holds a control mutation lock while waiting for a human or connector response.

The connector reads one canonical JSON object from stdin:

```json
{"protocol":1,"operation":"begin","request_id":null,"challenge":{"action":"decision.answer"},"action_digest":"sha256:..."}
```

`challenge` is the **complete native action**, not the abbreviated illustration above. Poll uses `operation: "poll"` and the connector's previously returned request ID. The response is exactly one of:

```json
{"status":"pending","request_id":"opaque-request-id","approval_url":"https://approver.example.test/review/42"}
{"status":"approved","attestation":{"body":{},"signature":"..."}}
{"status":"rejected","reason":"Required independent review declined"}
```

An approved response contains the complete signed attestation defined earlier. The connector must independently authenticate the person, display the exact challenge, require deliberate approval and preserve its subject/action digest. It must not sign every request automatically. Approval links require HTTPS, apart from explicit localhost HTTP callbacks. No connector process waits open for the person: it returns `pending`, and later polls are separate bounded invocations.

MACC persists a versioned ticket with `pending`, `approved`, `applying`, `applied`, `rejected`, `expired` or `stale` state. Repeated begin requests deduplicate; concurrent ticket updates use CAS. A verified external approval still requires explicit **Apply**. Changed subjects/policies invalidate the ticket. Crash recovery recognizes an already committed exact action before replaying anything. Ticket state alone cannot grant authority; the native mutation verifies the isolated signature. Revocations and manual operation reconciliation now require their own exact action proofs in secure mode. A trusted configured outcome probe remains distinct from an operator claim.

The Web/TUI normally show the approval page, current status, **Check**, and explicit **Apply**; draft text is retained. The ordinary gate window also routes secure votes through this connector. Gate principal/role must match the independently authenticated identity. A signed gate request may predate the approver's attestation by up to one hour to accommodate review; future timestamps are rejected, and the earlier request time does not extend vote TTL. Manual challenge/attestation entry remains an advanced fallback when no connector is configured.

```sh
macc decisions approval begin --file operator-intent.json
macc decisions approval list
macc decisions approval poll TICKET_ID
macc decisions approval apply TICKET_ID
macc decisions operator --file operator-intent.json --challenge
```

`OperatorIntent` is the shared typed API for answer, feedback, revision, withdrawal, revocation, effects, graph editing, gate vote, policy and operation reconciliation. HTTP mutations retain owner/authentication checks and reject actions/tickets from a different selected plan/reference branch. The native gate route repeats the established readiness, role, revision, evidence/reason and quorum validation.

## Guided signed-history reconciliation and diagnostics

Fast-forward signed import still refuses to replace a diverged local ledger. A separate operator workflow now verifies the full incoming archive in an isolated in-memory database, finds the common event ancestor and constructs a three-way graph review:

```sh
macc decisions transport reconcile-preview --file incoming.json --allowed-signers /trusted/signers
# choices.json: {"/tasks/A/title":"local", "/tasks/C/title":"remote"}
macc decisions transport reconcile-preview --file incoming.json --allowed-signers /trusted/signers --choices-file choices.json
macc decisions transport reconcile-apply --file incoming.json --allowed-signers /trusted/signers --choices-file choices.json --expected-head sha256:REVIEWED_LOCAL_HEAD
```

Every conflicting field needs an explicit local/remote choice. The apply command requires stopped execution, the reviewed local event head, ordinary graph invariants and isolated graph approval when configured (`--challenge` / `--attestation-file`). It records the signed incoming evidence and graph mutation in the same transaction. Incoming votes, decisions, contexts, reservations and process ownership do **not** become local authority. Directions in the incoming evidence can be adopted through the normal local revision/answer workflow. Delivered/archived history cannot be overwritten. No branch or worktree deletion is added. This supports reviewed stopped-history reconciliation, not distributed simultaneous coordinator ownership.

`macc decisions analytics` reports observed state/risk/provenance counts, response latency, reused answers, planning attempts and operation outcomes. Web/TUI expose the same diagnostics. Optional `similarity_hints: true`, `similarity_max_candidates: 128` (maximum 512) and `similarity_min_percent: 75` enable bounded lexical/domain suggestions. They consume the configured source-byte budget, return at most 32 pairs and never merge consent. No additional model call or speculative token/cost saving is inferred. Advanced semantic models, paid AI triage and vendor-specific enterprise connectors remain the explicitly optional post-release work package E; the generic isolated connector is available now.

## Reproducing qualification

The acceptance matrix includes exact tests for alternative patches, rejection/revision, bounded planning, broker signature/staleness/recovery, 20 simultaneous native context workers and actual child-process death after graph commit/external effects. The adapter matrix executes all five shipped shell wrappers and their ToolSpecs twice, replacing only provider executables with controlled processes. This validates MACC's prompt/receipt path; it does not certify changing live provider behavior.

```sh
cargo test -p macc-core --lib
cargo test -p macc-cli --test session_store_runner
cargo test -p macc-tui decision
npm --prefix web test -- src/components/DecisionCenter.test.tsx src/components/ApprovalDialog.test.tsx
npm --prefix web run build
python3 automat/tests/decision_browser.py --browser /path/to/chromium --report /tmp/decision-browser.json
```

The browser harness serves a local component fixture with mocked API responses, checks actual 320/768/1440-pixel viewport layout and deferred drafts, and downloads nothing. Missing Chromium produces `not_run` and exit 77. It does not certify screen-reader behavior or native backend deployment. Existing `automat/tests/o2_real_tools.py` remains an explicitly opt-in, budgeted real-provider transport qualification tool; its default validates the plan without model calls. Deployed independent identity/authentication, screen readers, live providers and non-Unix operation capture still require their actual environment.

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

TUI: `D` opens a focused question/action form. `Tab` and `Shift+Tab` move between fields and the visible **Send / Later / Details / More** buttons. Arrows or Space select a choice; Enter in a multiline input inserts a newline, and Enter on **Send** submits explicitly. Missing required fields are focused before any server call. **More** exposes rejection, further investigation, history, project direction, exact patch acceptance, approval challenges and the lifecycle/evidence menu. Operator action forms also use Tab and Enter. No function key is required or advertised in this popup, including inside VS Code. Escape keeps the version-bound draft. Text editing supports Unicode, cursor movement with Left/Right, Backspace and Delete; resizes preserve the cursor and draft. The popup owns its command area, so the global footer cannot obscure the form.

Web uses the same requested fields, explicit intervention confirmation, inline validation and preserved drafts. Its Send/Later actions remain visible while the body scrolls. Context, diagnostics and administrative actions are expandable; they do not precede the response form. A changed or answered request disables stale submission and offers **Review latest request**, retaining the old version's draft. Polling does not clear submission errors.

Simple choice flow: choose a numbered option with the arrows or its number, press Enter to continue to **Send**, then Enter to submit. `0` selects a custom answer. A declared yes/no response field supplies the response text directly; an additional note is optional. After the native service confirms that this exact request/version has a recorded answer, the popup closes. Errors and pending independent approvals keep it open. Recorded history has no pending-form instructions or required-field counters. The short question appears once; full context and the original unabridged request remain in **Details**. Display compaction, field validation, receipts and formatting are deterministic client code, without additional model calls. Performer/planner instructions require script-first investigation, one short question and up to four short choices; scripts verify accessible facts instead of asking the person for redundant output. Essential intervention observations, signed approvals and explicit graph/evidence consent remain separate requirements.

TUI configuration: **Automation / Coordinator → Decisions** (tab `8`) edits activation, consultation threshold, budgets, trust and advanced rules. `s` saves through native policy checks; `F12` exports an isolated policy-approval challenge. Secure-mode approval tickets remain operational actions in the decision center. Questions, answers and history stay in the center opened by `D`; its old `F6`/`F7` configuration shortcuts are removed.

Web: **Decisions** opens the inbox/history. New questions open only when another editor/dialog is not active. The initial answer is custom: no option or graph patch is accepted by default. Requests are versioned; a competing answer does not overwrite a draft. History and exact issued contexts remain inspectable in full autonomy.

```yaml
automation:
  coordinator:
    decisions:
      enabled: true
      human_question_min_risk: high  # low | medium | high | critical | never_ask
      trust_mode: local_trust
      max_context_bytes: 131072       # includes rendered instructions and all bindings
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

A performer may attach `proposed_patch` to a structured request. The same risk policy routes it. Autonomous resolution and graph commit share one SQLite transaction. For human intake, selecting an option and applying its patch are separate actions. Patch application additionally requires the explicit checkbox/TUI **More → Accept the displayed task-plan patch** or CLI `--apply-patch`, the displayed selected option’s patch, a binding answer and a current graph version. Free text, advisory responses, rejection and request-changes cannot implicitly apply a patch. Agent proposals cannot remove approval dependencies or rewrite governance. The standalone audit produces patch proposals rather than rewriting protected PRDs.

CAS rejects competing graph versions; idempotency prevents duplicate commits. An outbox projects accepted graphs into the PRD and updates queue fingerprints. Recovery recognizes earlier accepted projections, repairs missing exports and refuses to overwrite unknown edits. Explicit reconciliation preserves the inspected external JSON in the journal.

## Persistence and approvals

The authority is `<primary-checkout>/.macc/persistent/control.sqlite`; linked worktrees share the project identity, while plan/source/reference-branch identities isolate graphs, decisions and votes. Explicit `project` guidance also reaches future PRD plans on the same reference branch; task/path/requirement decisions do not leak into colliding task IDs in other plans. A shared controller lock prevents competing dispatch owners. `macc clear` retains this authority and reports that retention. No new branch/worktree deletion is introduced.

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
    "transport.import": ["OWNER"], "evidence.share": ["OWNER"]
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

All other response flags/fields must be identical between challenge and submission. Gate records use the exact full commit, principal and entitled role. Graph previews expose separate `challenge` and `reconcile_challenge`; apply/reconcile accept `--attestation-file`. Web provides the challenge and external-attestation input; TUI **More → Prepare independent approver challenge** writes the challenge file; Tab selects the proof field.

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
    "transport.import": ["OWNER"], "evidence.share": ["OWNER"]
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

`macc decisions analytics` reports observed state/risk/provenance counts, response latency, reused answers, planning attempts and operation outcomes. Web/TUI expose the same diagnostics. Optional `similarity_hints: true`, `similarity_max_candidates: 128` (maximum 512) and `similarity_min_percent: 75` enable bounded lexical/domain suggestions; `similarity_backend: local_lsa` additionally enables local classical TF-IDF/LSA with the budgets described below. They consume the configured source-byte budget, return at most 32 pairs and never merge consent. No additional model call or speculative token/cost saving is inferred. Paid generative triage and vendor-specific enterprise identity integrations remain optional deployment extensions; the generic isolated connector is available now.

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

## Essential questions and manual interventions

Each issued performer/reviewer context teaches the native request protocol. The model does not need to discover a private UI API or keep its provider session waiting for interactive input. It investigates first, records a scoped request, preserves its working files, then returns `precondition_unmet`. MACC holds the affected task and its consumers while independent work continues. Ordinary implementation choices remain autonomous.

Use an ordinary `decision` or `clarification` request for an important unresolved choice. For an indispensable action beyond the worker's actual capabilities, use `kind: intervention`:

```json
{
  "idempotency_key": "connect-test-device-v1",
  "task_id": "DEVICE-TEST",
  "kind": "intervention",
  "question": "Connect the test phone and return its actual status",
  "recommendation": "Connect and unlock the test phone",
  "investigated": ["The worker cannot physically connect USB hardware"],
  "impact": {"evidence": ["Device access is indispensable to the acceptance check"]},
  "intervention": {
    "action": "Connect the phone to this computer by USB and unlock it",
    "inaccessible_reason": "The worker has no physical access to the phone",
    "expected_result": "Return the observed device identifier or exact error",
    "verification": "Run the local device-listing command; omit credentials"
  }
}
```

```sh
macc decisions request --file intervention.json
```

An essential intervention is presented in consultation modes even below the discretionary question threshold. It never acquires an invented result from a standing answer or an autonomous recommendation. In `never_ask`, it remains `blocked_external`, without a popup; inspect it and restore consultation when human assistance is appropriate. This preserves the explicit no-question mode without pretending unavailable hardware or credentials exist.

Web presents the exact requested action, unavailable capability, expected output, an **I performed the requested action** checkbox and an actual-result input. TUI leads with the same `performed` and `result` fields. Arrows/Space change the yes/no choice, Tab moves to the next field, Enter inserts a newline in multiline inputs, and Enter on **Send** submits explicitly. For CLI, supply `--fields-file` with `{"performed":"yes","result":"exact observed output"}`. A positive response requires both confirmation and a nonempty result. A user who cannot help can reject or request changes. An observed error is valid evidence, not proof that the task succeeded. The result is retained verbatim and delivered in fresh scoped contexts with the normal receipts, ACK and compliance checks. It does not authorize unrelated operations.

## Conditions, further investigation and evidence

A conditional response contains up to 16 typed `conditions`. Each has an `id`, human-readable `description`, and either an independently deliverable `task_id` or `source_path` plus exact `source_digest` (`sha256:...`). A verification task cannot belong to the blocked subtree. Conditions are checked by native code, not by treating prose or a model's assertion as approval. A conditional decision stays `conditional` until current checks pass; source drift fences execution again. For native human gates, conditional votes do not enter quorum before verification, and source artifacts are read from the exact reviewed Git commit. Existing role, subject, expiry and secure-signature checks still apply.

Web provides **Add approval condition**; TUI uses **More → Edit approval conditions / advanced fields** and Tab to edit condition fields, and gate actions expose their condition fields too. CLI uses `answer --conditions-file conditions.json`, or the typed `conditional_gate_vote` operator action for a native approval gate. A positive conditional answer cannot implicitly apply a proposed graph patch.

The **Further investigation** outcome (**More → Ask the performer to investigate** in TUI; `answer --outcome investigate` in CLI) stores the exact human request and enters `investigating_requested`. The coordinator issues a bounded, native **read-only investigation** attempt. It cannot count as implementation, apply graph effects or alter retained source/Git state. The performer reports through:

```sh
macc decisions investigation-report REQUEST_ID --version VERSION --file report.json
# report.json: {"summary":"Observed facts and remaining uncertainty","artifacts":[]}
```

The original question returns to the user with a labelled, unverified performer report; the request to investigate never becomes consent. Native attempt limits and byte budgets apply.

Evidence is content-addressed and retained with exact bytes/hash, actor, time and plan. Registration is bounded by `max_request_bytes` and a 256 KiB hard cap; references are limited to 16 per request/answer. Built-in secret detection rejects recognized credentials but is not a guarantee that every sensitive value is detected. Supply a redacted result.

```sh
macc decisions artifacts add --file device-output.txt --label 'Observed USB status' --media-type text/plain
macc decisions artifacts list
macc decisions artifacts show ARTIFACT_ID
```

Attach `{id,digest}` references using request `artifacts` or CLI `answer --artifacts-file`. Uploading/attaching does not submit an answer or share bytes with an AI provider. Evidence is **private by default**. Web offers an explicit review and confirmation per file; TUI's evidence action uses Tab to focus **Review / confirm AI sharing**, Enter to review and a second explicit Enter to confirm; Escape cancels and moving focus resets confirmation. Generic CLI operator input is `{"action":"evidence_permission","id":"ARTIFACT_ID","plan_id":"PLAN_ID","version":0,"share":true}`. The native service binds release to exact contents, plan, permission version and current configured AI tools; changed AI configuration invalidates release. Secure mode requires independently signed action `evidence.share`. Workers cannot grant permission to themselves or read private bytes through the artifact CLI. Only explicitly released bytes enter AI context, within its byte budget; unreleased references remain visibly unread metadata.

## Governance, reuse and analysis budgets

Web provides a governance rule editor with explicit preview, stale-policy detection, minimum risk, human requirements, scoped domains/paths and standing delegated directions. The same native policy is available through CLI configuration. Web and TUI let an operator select a resolved ordinary direction from the same project/reference branch and author fresh guidance for a task in the current PRD, with the exact source ID/version and an applicability justification. This deliberately does not copy conditional approvals, operation authorizations, private artifacts or gate votes. Later revocation of the old direction does not revoke the independently authored new guidance.

Advisory similarity supports `similarity_backend: lexical` (default) or `local_lsa`. The latter uses local classical TF-IDF/LSA, with lexical fallback, bounded source bytes/candidates, a 100 ms fit budget, 200 ms overall comparison budget, at most 8,192 comparisons and 32 suggestions. These are local compute limits, not hard real-time deadlines. It makes no extra paid provider calls and cannot merge authority or create consent. Diagnostics expose actual states, response latency, attempts, operation outcomes and triage limits. Existing investigation round/prompt limits bound AI planning. MACC does not invent provider prices or monetary savings. Vendor-specific identity connectors and production deployment certification remain environment-specific qualification, not implicit properties of the generic isolated connector.

## PRD generation validation and promotion

`macc prd validate` and generated-output validation check `decision_requests` using the native request types before a coordinator imports them. Checks cover the 256-proposal limit, configured request byte budget, existing task references, unique request/option/form identities, recommendations, intervention details, scoped references, expiry and artifact-reference shape. A proposal cannot contain an answer or forged authority. Initial requests cannot contain runtime graph patches or `requires_replan: true`; the planner describes the planning need and waits for a native plan identity before proposing effects. Runtime import shares these structural request checks; actual policy, subject and retained evidence remain native runtime checks.

A valid draft with requests remains valid when decision management is disabled, but validation emits `PRD-GEN-DECISIONS-DISABLED` explaining that those requests are not enforced. Missing project context emits `PRD-GEN-DECISIONS-UNCONFIGURED`; invalid project policy is an error. Promotion validates its exact source snapshot against the destination project's policy before creating a backup or changing the destination. Promotion warnings are returned in its structured result and shown by CLI/Web.

`macc prd promote`, including automatic promotion after generation and the Web endpoint, refuses to replace a PRD already recorded in the durable ledger (`PRD-GEN-PROMOTE-MANAGED`). `--yes` only confirms ordinary file replacement; it does not bypass native graph history. The guard covers retained plans even when decisions are currently disabled, the exported file is missing, or another linked worktree/reference branch accesses the shared authority. It also refuses symlink destinations and fails closed on an unreadable authority.

For an existing native plan, prepare and inspect `macc decisions graph preview patch.json`, then use its versioned apply/reconciliation workflow. For a missing export, use native recovery rather than copying another plan over it. New, unregistered draft destinations remain promotable with the existing confirmation/backup behavior. Promotion and first plan registration share a mutation fence; registration reads the source only after any in-flight promotion completes. The validated bytes are published with atomic file replacement.


## Prompt budgets and pinned prerequisite sources

The default `automation.coordinator.decisions.max_context_bytes` is 131072
bytes (128 KiB). Explicit project limits, including a previously saved 32768,
are retained. The existing valid range remains 2048..262144. The limit measures
rendered decision instructions, not tokens, stored manifest size or the complete
provider prompt.

New schema-v3 contexts retain complete prerequisite contracts in their immutable
manifest and full contract digests in freshness/receipt checks. The prompt
contains compact prerequisite identities, digests, exact acceptance criteria and
gates; complete descriptions, steps and other fields are available through scoped
source reads. Plan metadata remains exact and inline using compact JSON, so no
field is guessed to be historical or non-normative. Applicable decision questions,
resolutions and explicitly released evidence retain their exact inline contents.
Nothing silently truncates a binding or changes consent.

Before relying on a prerequisite, read the relevant complete pinned requirements.
Inside a native task invocation:

```sh
"$MACC_DECISION_CLI" --cwd "$MACC_DECISION_ROOT" decisions context-source --dependency TASK-ID
"$MACC_DECISION_CLI" --cwd "$MACC_DECISION_ROOT" decisions context-source --dependency TASK-ID --pointer /acceptance_criteria
"$MACC_DECISION_CLI" --cwd "$MACC_DECISION_ROOT" decisions context-source --plan --pointer /global_constraints
```

These commands require the inherited plan/attempt/phase, reject unrelated sources
and stale contexts, and return exact JSON-source text pages with `next_offset`,
`total_bytes` and source/selection digests. Repeat with `--offset <next_offset>`
until the offset is null. Concatenate page `content` fields to recover the
selected JSON. Source/selection digests use the decision authority’s canonical
JSON encoding; they are not file-byte hashes for `decisions investigate`.
The default encoded-output limit is 8192 bytes, adjustable through
`--limit` within 1024..65536; UTF-8 characters are never split. A reference or a
partial page is not proof that the whole requirement was read or satisfied.

Operators can preview exact byte use without claiming a worker:

```sh
macc decisions context-budget --task TASK-ID --phase dev
```

The report includes required/configured bytes, section sizes and prerequisite/
binding counts. Native dispatch runs this check before worker acquisition and
Apply. If the prompt cannot fit, `MACC-DECISION-CONTEXT-BUDGET` records a scoped
E903 precondition with the numeric evidence in `task_runtime.decision_context_budget`.
Testing/review/fix paths and the final issuer keep the same guard. The supervisor
receives the diagnostic and must not repair application source or override the
configured cap to bypass it. With repair and automatic restart enabled, it can
resume a native dev preparation blocked solely by this budget when a fresh preview
now fits. It also recognizes the older E901 budget message. Decision-policy changes
wake a waiting supervisor; recovery records `decision-context-budgets.json` and
uses the existing guarded task-requeue/restart operation. Other phases and mixed
incidents retain their normal diagnostic/recovery path. Review the project limit
or split the required scope when the context still cannot fit.

Existing schema-v2 contexts keep their original renderer for inspection and audit.
Fresh v3 contexts have new digests; active attempts issued by an older binary
must be refreshed, rather than reinterpreting an old receipt. No database schema
upgrade is required. Exact context history/delivery diagnostics include the byte
report in CLI, TUI and Web service responses.

### Negative diagnostic delivery

See [Diagnostic delivery and external verification](diagnostic-delivery.md) for
review protocol v4, native report receipts, external-verification waiting and the
strict separation between diagnostic completion, feature acceptance and release.


### Shared persistent project layout

See [Persistent project state](../PERSISTENT_PROJECT_STATE.md) for automatic legacy
migration, shared worktree authority, clear retention, Git visibility and backups.

## Explicit intervention responses and durable waiting

New decision contexts use protocol v5. Historical v2/v3/v4 prompts and optional
option fields retain their serialized representation for receipt verification.
Use `clarification` when asking only to confirm a fact; reserve `intervention`
for an unavailable human action with an observable result.

Intervention options can declare `intervention_response`:

- `completed`: requires `performed=yes`, the actual `result`, and requested facts.
- `confirm_fact`: records an attributed statement, without claiming a physical
  action. Other required facts still apply.
- `defer_observation`: explicitly records that observations remain unverified;
  no fake action/result is required. It cannot establish independent compliance,
  waive mandatory PRD criteria, apply a graph patch, or authorize release.
- `unable`: rejects assistance and leaves dependent work stopped.

Existing options keep their original meaning and completed-action requirements.
Clients never infer a deferral from a label or silently change a prior response.
An explicit response-type change clears incompatible action claims and option
selection. Secure approval still signs the exact submitted response.

In the CLI, a selected option supplies its label when `--text` is omitted:

```bash
macc decisions answer <id> --version <version> --option <typed-option-id>
macc decisions answer <id> --version <version> --response-type defer_observation
macc decisions answer <id> --version <version> --response-type confirm_fact \
  --text "Observed fact" --fields-file facts.json
macc decisions answer <id> --version <version> --response-type unable --outcome reject
```

The TUI shows choices before evidence, current requirements and step progress.
Typed deferrals go directly to Send; legacy choices expose an explicit response
type. `Tab` continues from multiline text; `Enter` inserts a new line. A successful
Send closes only when the same native decision has a newer version and its
`resolution.answered_version` matches the submitted version. Escape hides the
popup and preserves the draft; it does not answer, reject or cancel the request.
Web shares the conditional response requirements and explicit type controls.

A recorded response, delivered context, model acknowledgement and independent
review compliance are separate states. TUI history and Web receipts display these
separately; an old context cannot stand in for the answered decision version.
For `E903` with decision origin, press `d` in the TUI pause dialog to open
the relevant failed decision, or open Decisions in Web. Inspect the named selection and
failed check, provide independently checked evidence or propose an authorized
revision, then retry the retained phase. A generic unblock does not prove it.

For SQLite sessions, heartbeat records a native `decision_wait` receipt under the
current verified lease **before** stopping a provider for a newly pending gate.
The receipt includes task, invocation event source, session generation and the
native decision revision. If an answer arrives before that heartbeat, a native
`DM_STALE` receipt instead requests fresh context for the retained phase.
Development and phase monitors accept only a matching
native receipt; raw/stale logs cannot authorize a wait or delivery. This is an
`E903` precondition, never a quota or generic retry. The coordinator parks the
scoped task, keeps its worktree/branch and phase, and resumes with a fresh context
after resolution. Late-cycle requests are reconciled before termination checks.
Real provider quota handling remains in each adapter.

The supervisor recognizes unresolved native decisions without a paid diagnosis.
It keeps that diagnosis while the relevant gate revisions and decision policy
are unchanged, even if unrelated source or tool availability changes. Explicit
retry, gate revision, quota reset and transient retry triggers are recorded in
`.macc/log/supervisor/<incident>-attempt-N/trigger.json`, including the changed
context keys and revision digests when evidence changes. Independent compliance
failures remain separate from unanswered requests and may need external access.

## Runner output authority and phase evidence

Runner protocol interpretation lives in Rust. `macc runner-output --file <path>
--tool <adapter-id> --exit-code <code>` exposes the same projection and registered
adapter normalization to the shell transports. Prompt echoes, command transcripts,
quoted/fenced examples and assistant discussion cannot supply provider errors.
A fence inside excluded command output cannot suppress the next assistant ACK,
compliance object or review verdict. Actual provider error records keep their
adapter's error class, reset hint and model/quota scope.

Invocation boundaries are anchored records in the prompt/provider channel, not
substrings found with a raw last-occurrence search. A flattened prompt echoed in
an executed command cannot start another invocation or hide a later final result.
Each new assistant response resets terminal task fields; old explanations and
results cannot stand in for a missing final result. Repeated CLI final responses
remain supported. This covers the GTransport Codex transcript failure reproduced
on 2026-11-14 with tool output, inline prompt markers and a repeated final response.

A missing terminal result is a native E103 output-protocol failure, never a
fabricated `error_without_changes` performer report. The phase event preserves
the original diagnostic and raw transcript path. Coordinator classification
retains E103, phase, tool and worktree; it does not turn the failure into E906,
infer success from dirty files or commits, or retry an unchanged parser blindly.

Shell invocations also pass `--project-root`, `--task-id` and `--source` together
when an invocation source exists. A matching native wait receipt overrides
terminal/provider interpretation even if output stops inside an excluded tool
transcript. An unrelated source cannot supply this outcome. This read-only
inspection returns `native_wait` separately and never supplies assistant ACKs.

A native terminal failure or precondition survives a nonzero process exit and
retained commits. A retained commit alone never proves success. There is no
legacy log-tail completion fallback, compatibility environment switch, alternate
old runner-directory discovery, old result aliases, shell quota-regex copy or
stdout-success override. Native invocation-scoped receipts remain mandatory.
Current configured external runners remain supported through ToolSpec.

Each review/fix/test invocation archives stdout, stderr, projected output and
`outcome.json` under `.macc/log/performer/<task>/phases/<phase>-<source>/` before
validation. Evidence includes artifact hashes, context digest, eligible ACKs,
exit code and validation error. Review logs are aggregated even on early
validation failure. ACK diagnostics distinguish absent, excluded and mismatched
ACKs (`DM_ACK_ABSENT`, `DM_ACK_EXCLUDED`, `DM_ACK_MISMATCH`), stale context and
failed independent compliance. A valid review ending in CHANGES_REQUESTED enters
its existing correction/review flow; an acknowledged context does not accept a
feature or authorize a merge.

After a runtime change or explicit `macc supervisor start --daemon --retry`, an
E103 output-protocol failure in a retained review can use one bounded native
restart recovery (`MACC-REVIEW-PROTOCOL-REFRESH`). Repair and automatic restart
must both be enabled. It checks current review context readiness and reuses the
retained worktree/phase through the existing recovery contract. It issues a fresh
review, never rewrites an old acknowledgement/compliance receipt, and does not
bypass E903 prerequisites. An unchanged runtime without an explicit retry cannot
trigger this recovery. Runtime-change evidence is retained in `trigger.json`.

The same update/explicit-retry gate allows bounded recovery of a missing task
result in other retained phases. A historical E906 is eligible only with the
native missing-result diagnostic. If that old runner detached the worker,
native unblock corroborates saved path/branch/tool, managed worker metadata,
repository membership, the original phase and the unchanged scoped PRD before
restoring the blocked binding. A reused or ambiguous worker stays blocked.
Checkpoint and protected reconciliation checks then run normally, preserving
dirty/index/untracked work and recording recovery evidence. Test, fix and review
failures resume their own phases. No historical result is promoted to delivery,
no self-review becomes independent approval, and configured verification and
publication requirements remain in force.

The native unblock operation restores these retained protocol reviews to
`reviewing`, rather than dispatching development again. Reconciliation preserves
the exact worker commit when the reference is already in its ancestry; actual
prerequisite changes still use the existing protected reconciliation. Dependency
summaries group E905 fallout under its root without counting blocked descendants
as pending work or interrupting independent tasks waiting for tool availability.

During SQLite runner shutdown, the provider process group is terminated before
the EXIT trap waits for the heartbeat that signaled it. The native wait receipt
is already durable, and session finalization follows provider termination. This
prevents a nested shell wait from leaving the provider alive on a human gate.

### External prerequisite handoff and supervised source recovery

An idle coordinator waits only for actual `pending_human` decisions and approval
gates. A `blocked_external`, rejected, expired or other non-human prerequisite is
not counted as a pending approval. If it prevents all remaining progress, the run
terminates as blocked, retains its tasks/worktrees and releases engine ownership.
The supervisor can then diagnose the incident without racing running performers.
Both clients display the external block explicitly, including in `never_ask`;
that mode does not fabricate an action or an observed result.

Ordinary missing code should be returned as `precondition_unmet`, with concrete
source paths and `out_of_scope_fix` evidence when appropriate. The supervisor's
existing isolated repair process can implement that prerequisite beyond the
original performer's writable scope, validate it, independently review it and
publish against the exact reference base before requeuing retained work.

If a source prerequisite needs a durable intervention request, its optional
`intervention.source_repair_paths` declares up to 16 concrete application source
paths. These are not physical-action reports. Under explicit `never_ask`
delegation, enabled repair and independent verification, the supervisor may
reconcile that exact unresolved revision after publishing a verified repair that
changes the declared paths. State, secrets, PRDs and generated tool/runtime files
cannot be treated as application source recovery paths. Configured context and
PRD/registry paths are also checked against the current project configuration. Mandatory governance,
approval/authorization, changed contracts and required human response fields still
prevent automatic reconciliation. A superseded policy or changed decision version
invalidates recovery.

The receipt records `origin: supervisor_source_repair`, the published commit,
verification evidence and a native version/decision-epoch increment. It does not
claim a human vote, `performed: yes`, device visibility or release authority.
The event is verified by the same immutable journal and distributed in subsequent
performer contexts. No performer CLI command exposes this supervisor authority.

A `blocked_external` status alone no longer skips supervisor diagnosis. Actual
external access, physical actions and legacy requests without a declared source
recovery scope remain external until explicitly reconciled; neither an unrelated
Git change nor a repeated restart can satisfy them. Ambiguous classification
retains the diagnosis and work and exposes the exact required intervention.

### Native decision-context refresh

A lease-fenced `DM_STALE` receipt caused by a decision binding change is routed
separately from an unavailable application precondition. Completion retains the
worker, branch, tool and phase; it does not enter retry salvage or merge. Native
sync refreshes the checkpoint before redispatch, with at most three automatic
refreshes per decision revision; a new revision has its own bounded budget. Pending human/external decisions remain waits, not refreshes.

Decision-only reconciliation requires the same plan and task contract, PRD and
specification hashes, worker/branch/revision and exact retained Git digest. It
rejects unresolved decisions, active leases and stale preview hashes. It neither
accepts implementation nor weakens independent review. Genuine source or contract
changes still require reviewed reconciliation. Diagnostics distinguish decision
binding drift from source and execution-context drift.

The stopped supervisor recognizes legacy `DM_STALE` incidents as native recovery
candidates. It can reconcile and resume without source repair, including after a
non-repairable/non-human AI diagnosis when native evidence proves a pending safe
context update. Model-provided shell commands are never executed as recovery.

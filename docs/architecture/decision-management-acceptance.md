# Decision management acceptance evidence

Updated 2026-11-03. This tracks the [baseline](../prd/MACC_Decision_Management_Architecture_Baseline.md) without changing its accepted requirements or its historical source audit.

`Component/integration coverage` means the named tests exercise the implemented contract. It is **not** a claim that the full production scenario, all providers, a deployed isolated broker or all operating systems were certified. The source test names below are directly searchable.

| Group | Implemented contract | Automated evidence |
|---|---|---|
| G1 | Governance, revision/policy binding, role validation | `governance_rules_raise_risk_enforce_fields_and_recheck_pending`, `approval_requires_existing_unambiguous_commit_and_valid_timestamp`, `isolated_proof_checks_entitlement_expiry_subject_and_signature` |
| G2 | Intake, bounded facts, subject arbitration, versioned replacement | `concurrent_performers_share_one_subject_authority`, `typed_subject_arbitration_reuses_authority_and_requires_versioned_supersession`, `factual_investigation_requires_exact_source_evidence_then_resolves` |
| G3 | Exact response, explicit patch acceptance, autonomy, rejected outcomes | `free_text_and_rejection_never_implicitly_apply_proposed_patch`, `human_text_is_exact_scoped_and_survives_runtime_removal`, `policy_thresholds_and_never_ask_do_not_fabricate_human_votes`; Web structured-response/draft tests |
| G4 | Dynamic graph, archival IDs, editor CAS and queue delivery | `graph_cas_cycles_historical_ids_and_replay_are_enforced`, `editor_patch_preserves_archive_and_versions_plan_requirements`, `retired_inflight_context_can_never_complete_successfully`; existing queue/approval regressions |
| G5 | Scoped contexts, prerequisite contracts, delivery/ACK/compliance, publication | `prerequisite_contract_changes_invalidate_only_their_consumers`, `real_prompt_receipt_ack_review_and_stale_barriers`, `review_records_negative_evidence_and_publication_is_bound_to_the_reviewed_commit`; `session_store_runner` decision lifecycle cases |
| G6 | Persistence, projection replay, backup, migration and approval reuse | `backup_is_consistent_restores_identity_and_never_rolls_back_newer_history`, `projection_tampering_and_event_corruption_fail_closed`, `content_equivalent_approval_requires_manifest_and_current_matching_bytes`, existing recovery/session suites |
| G7 | One-shot execution, unknown outcomes, missing authority | `operation_runner_persists_success_and_never_blindly_repeats_unknown`, `journal_detects_forged_operation_state_and_recovery_preserves_ambiguity`, `bounded_investigation_blocks_without_fabricating_missing_sources` |
| G8 | Isolated approval, signed import and remote API access | `secure_mode_rejects_missing_or_same_principal_registry`, `signature_verifier_checks_exact_payload_principal_and_namespace`, `signed_transport_enrolls_empty_clone_and_rejects_tampering_or_rollback`; Web remote-auth tests |
| G9 | Responsive editing, no focus theft, multiline input and accessibility | Web `DecisionCenter.test.tsx` (10 tests including axe); TUI `decision_center` tests (6 cases, multiple terminal dimensions) |
| G10 | Parallel scheduling and subtree fencing | Concurrent two-store arbitration plus independent-scope/context tests; 20-worker production-scale qualification remains outstanding |

Sources: [core decision tests](../../core/src/coordinator/decisions/tests.rs), [isolated verification tests](../../core/src/coordinator/decisions/secure.rs), [runner integration](../../cli/tests/support/performer_lifecycle.rs), [Web](../../web/src/components/DecisionCenter.test.tsx), [TUI](../../tui/src/decision_center.rs).

| Acceptance ID | Baseline scenario | Evidence / qualification |
|---|---|---|
| AT01 | Planner creates a known sensitive decision gate | G1 — component/integration coverage; real planner/provider behavior must be qualified. |
| AT02 | Performer encounters an unplanned architecture decision | G2 — component/integration coverage. |
| AT03 | Reviewer discovers policy conflict during review | G2 — component/integration coverage; real planner/provider behavior must be qualified. |
| AT04 | Human chooses option B instead of agent recommendation A, with extra remark | G3 — component/integration coverage. |
| AT05 | New task and dependency are inserted during active run | G4 — component/integration coverage. |
| AT06 | A patch introduces a dependency cycle | G4 — component/integration coverage. |
| AT07 | Two agents propose patches against the same graph version | G4 — component/integration coverage. |
| AT08 | Patch affects an active worktree/task | G4 — component/integration coverage. |
| AT09 | Patch changes an already merged task | G4 — component/integration coverage. |
| AT10 | Coordinator process crashes after graph DB commit but before PRD projection | G6 — component/integration coverage. |
| AT11 | `macc clear` runs after an approval and dynamic patch | G6 — component/integration coverage. |
| AT12 | Coordinator restarts after clear on unchanged subject | G6 — component/integration coverage. |
| AT13 | Same artifact, different SHA under `content_equivalent` | G6 — component/integration coverage. |
| AT14 | Same SHA but policy/required quorum changes | G1 — component/integration coverage. |
| AT15 | `never` approval for one-shot destructive action exists | G7 — component/integration coverage. |
| AT16 | Expired/revoked approval is restored from disk | G6 — component/integration coverage. |
| AT17 | Unsigned forged Git note claims human approval | G8 — component/integration coverage; isolated deployment and human authentication require external qualification. |
| AT18 | Same-UID agent calls a privileged approval CLI in local-trust mode | G8 — component/integration coverage; isolated deployment and human authentication require external qualification. |
| AT19 | PRD manually edited while coordinator runs | G4 — component/integration coverage. |
| AT20 | `audit-prd` suggests changing an approved ADR | G2 — component/integration coverage; real planner/provider behavior must be qualified. |
| AT21 | Restart with missing/corrupt graph event segment | G6 — component/integration coverage. |
| AT22 | 20 parallel workers and a decision affects only one feature subtree | G10 — component/integration coverage; full-scale/cross-process crash exercise remains a release qualification. |
| AT23 | Approval quorum contains multiple required roles | G1 — component/integration coverage; isolated deployment and human authentication require external qualification. |
| AT24 | Web server exposed beyond localhost attempts anonymous mutation | G8 — component/integration coverage. |
| AT25 | User selects B and adds a negative constraint while performer expected A | G3 — component/integration coverage. |
| AT26 | User adds an applicable remark after a task is already running; DAG topology unchanged | G5 — component/integration coverage. |
| AT27 | Graph patch dynamically inserts a new task | G4 — component/integration coverage. |
| AT28 | Reviewer receives the same affected task as the performer | G5 — component/integration coverage. |
| AT29 | Long PRD conversation exceeds model prompt budget | G5 — component/integration coverage. |
| AT30 | User guidance contradicts prior binding design requirement | G2 — component/integration coverage. |
| AT31 | Runner acknowledges obsolete context digest | G5 — component/integration coverage. |
| AT32 | `macc clear` after user answered a mid-run question and graph changed | G6 — component/integration coverage. |
| AT33 | Human provides advisory feedback that is not a hard constraint | G3 — component/integration coverage. |
| AT34 | Agent claims to have followed an approved instruction but violates it | G5 — component/integration coverage. |
| AT35 | Human posts a new project direction with no prior gate or request | G3 — component/integration coverage. |
| AT36 | An approval is supplied for an unresolved/ambiguous revision or has an invalid expiry timestamp | G1 — component/integration coverage. |
| AT37 | Two plans/worktrees reuse the same task ID in a shared Git-common-dir DB | G4 — component/integration coverage. |
| AT38 | Accepted control mutation commits but coordinator DB/context projection is unavailable | G5 — component/integration coverage. |
| AT39 | An unrelated graph branch or advisory note changes while another task runs | G5 — component/integration coverage. |
| AT40 | Context is regenerated twice with unchanged pinned content but different issuance timestamps/rendering | G5 — component/integration coverage. |
| AT41 | A runner echoes the right digest but never receives a mandatory constraint in its effective prompt | G5 — component/integration coverage. |
| AT42 | Migration or clear races an active SQLite writer, including uncheckpointed WAL data | G6 — component/integration coverage; full-scale/cross-process crash exercise remains a release qualification. |
| AT43 | Crash occurs after reserving a one-shot authorization but before observing the external operation result | G7 — component/integration coverage; full-scale/cross-process crash exercise remains a release qualification. |
| AT44 | An already approved human gate is the last remaining PRD entry | G4 — component/integration coverage. |
| AT45 | A performer encounters routine naming/local implementation choices, even with threshold `low` | G3 — component/integration coverage; real planner/provider behavior must be qualified. |
| AT46 | A material unresolved choice has effective risk exactly equal to the configured threshold | G1 — component/integration coverage; real planner/provider behavior must be qualified. |
| AT47 | User raises threshold from `high` to `critical`, then a new high-risk choice within explicit delegated scope arises | G3 — component/integration coverage. |
| AT48 | Threshold is `critical` but an explicit human gate, binding-policy conflict or fresh operation authorization requires intervention | G1 — component/integration coverage. |
| AT49 | High-risk task encounters a choice already answered by an applicable verified decision/policy | G2 — component/integration coverage. |
| AT50 | Model reports low confidence, or proposes an unsupported lower risk class | G1 — component/integration coverage; real planner/provider behavior must be qualified. |
| AT51 | User changes the threshold while human requests are pending | G3 — component/integration coverage. |
| AT52 | One performer asks an important question while independent tasks remain ready; response changes instructions but no dependencies | G5 — component/integration coverage. |
| AT53 | Invalid autonomy value is entered or the same policy is edited through CLI/TUI/web | G1 — component/integration coverage. |
| AT54 | A below-threshold task lacks essential factual data and has no authorized safe default | G2 — component/integration coverage. |
| AT55 | Parallel/reused tool sessions discover the same unresolved decision, or a tool attempts a direct interactive question | G2 — component/integration coverage. |
| AT56 | A Web question with long context/options is opened on a narrow/mobile viewport and then resized | G9 — component/integration coverage; real browser/device/terminal visual qualification remains. |
| AT57 | TUI receives a long question in a small terminal, then terminal dimensions change | G9 — component/integration coverage; real browser/device/terminal visual qualification remains. |
| AT58 | Performer submits a clarification, option choice, approval or specific structured request | G3 — component/integration coverage. |
| AT59 | A new request arrives while the operator is typing, another popup is open or the user defers a request | G9 — component/integration coverage. |
| AT60 | Double-submit, transient disconnection, stale subject or two clients answering the same request | G3 — component/integration coverage. |
| AT61 | `never_ask` encounters low through critical questions, including a MACC human-approval declaration | G3 — component/integration coverage; real planner/provider behavior must be qualified. |
| AT62 | Human or autonomous decision occurs under each of the five selector values, then a downstream/reused/new task starts | G3 — component/integration coverage. |
| AT63 | Operator enters `never_ask` with pending dialogs and unsent human text, then later returns to a threshold | G3 — component/integration coverage. |
| AT64 | User answers by keyboard/screen reader, with color unavailable and long multiline input | G9 — component/integration coverage; real browser/device/terminal visual qualification remains. |
| AT65 | `never_ask` lacks necessary credentials, independent external approval evidence or indispensable factual input | G7 — component/integration coverage; real planner/provider behavior must be qualified. |
| AT66 | Parallel performers recommend conflicting autonomous changes to a shared contract | G2 — component/integration coverage. |
| AT67 | An autonomous choice cannot be durably recorded or required context projection is unavailable | G5 — component/integration coverage. |
| AT68 | User or another authorized agent inspects an autonomous decision from a different task/client | G5 — component/integration coverage. |

Reproduction commands:

```sh
cargo test -p macc-core --lib
cargo test -p macc-cli -p macc-tui decision
cargo test -p macc-cli --test session_store_runner
npm --prefix web test -- src/components/DecisionCenter.test.tsx
npm --prefix web run build
```

The complete core suite passed 1,209 tests (2 deliberately opt-in external installation tests ignored), the runner suite passed 22, and the Web decision suite passed 10. The final coordinator suite, including scoped-plan and wildcard-policy regressions, passed 471 tests. The workspace check also passed. Client and runner checks are reported separately. No live provider calls, installed organization approver, production deployment or Git network publication is inferred from those results.

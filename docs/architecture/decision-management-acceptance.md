# Decision management acceptance evidence

Updated 2026-11-04, extended implementation. This tracks the [baseline](../prd/MACC_Decision_Management_Architecture_Baseline.md) without changing its accepted requirements or its historical source audit.

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
| G9 | Responsive editing, no focus theft, multiline input and accessibility | Web `DecisionCenter.test.tsx` and `ApprovalDialog.test.tsx` (including axe, exact alternative/replan effects, broker flow, deferred drafts and `never_ask` ordering); TUI lifecycle/structured tests, [guided form tests](../../tui/src/decision_center/form_tests.rs), actual buffer assertions for visible choices/Enter/Esc at small dimensions, ordinary-key advanced actions, missing-field focus and separate scrollable patch acceptance. `automat/tests/decision_browser.py` provides opt-in real Chromium layout checks. |
| G10 | Parallel scheduling and subtree fencing | `twenty_parallel_workers_share_authority_and_only_affected_subtree_waits` executes 20 simultaneous native context workers; full production scheduler/load qualification remains distinct. |
| G11 | Alternative effects, explicit replanning, lifecycle and renewal | `alternative_option_b_applies_its_own_patch_and_exact_negative_constraint`, `custom_answer_waits_for_effects_new_tasks_inherit_and_planning_never_completes_implementation`, `planning_rejects_retained_source_edits_and_unrelated_graph_mutations`, `rejection_revision_withdrawal_and_revocation_are_versioned_and_idempotent`, `expired_requests_and_bounded_planning_remain_scoped_prerequisites`, `delegation_waits_for_replan_and_shared_alias_waits_for_parent_effects` |
| G12 | Persistent independent approval connector and privileged lifecycle | `broker_workflow_persists_deduplicates_checks_signatures_and_recovers_committed_action`, `broker_marks_changed_subject_stale_and_secure_revocation_requires_exact_proof`, `secure_feedback_remains_bound_after_risk_threshold_change_but_not_forged`, `independent_gate_review_can_take_minutes_without_forging_later_vote_time`. Fixtures inject the registry source only; SSH signature/action/role checks execute normally. This does not certify OS isolation or real human authentication. |
| G13 | Guided signed divergence and bounded diagnostics | `signed_divergence_has_explicit_three_way_choices_and_preserves_local_authority`, `analytics_and_related_candidates_never_create_consent_or_unbounded_work` |
| G14 | Actual process death, committed WAL/outbox and ambiguous external effects | `process_crash_replays_committed_wal_outbox_and_clear_retains_decisions_and_source`, `actual_owner_death_after_external_effect_is_unknown_and_never_reexecutes` |
| G15 | Every shipped runner/provider contract | `all_five_shipped_adapters_deliver_exact_decisions_on_initial_and_reused_invocations`: actual Claude/Codex/Gemini/AGY/Vibe shell wrappers and ToolSpecs, two calls per adapter, controlled provider processes, native receipts/ACKs, passive and structured capture where declared. Live provider behavior is not inferred. |


| G16 | Essential manual interventions and exact user results | `performer_can_request_essential_physical_action_and_receives_exact_human_result`, `intervention_missing_external_facts_still_routes_to_the_person_not_an_ai_retry_loop`, `never_ask_does_not_fabricate_a_physical_action_and_can_return_to_consultation`, `human_rejection_of_unavailable_intervention_never_becomes_a_result`; real shell/CLI `real_performer_can_request_user_hardware_intervention_without_losing_work`; Web manual-result and TUI intervention tests. No physical hardware or paid AI is invoked. |
| G17 | Conditional authority, complementary investigation, explicit evidence disclosure | `conditional_answer_blocks_until_checked_evidence_and_changed_evidence_fences_again`, `conditional_gate_never_counts_as_a_vote_until_a_real_verification_task_is_delivered`, `conditional_gate_enters_quorum_only_after_current_verified_delivery`, `investigation_is_read_only_bounded_and_returns_report_without_inferred_consent`, `evidence_is_exact_retained_and_never_disclosed_without_explicit_operator_consent`, `forged_or_foreign_evidence_and_oversized_files_are_rejected`, `revoked_evidence_disclosure_cannot_be_rolled_back_to_an_earlier_valid_permission`; Web conditional gate, explicit evidence-sharing and investigation regressions. |
| G18 | Bounded optional local semantics, governance and fresh scoped reuse | `local_semantic_suggestions_have_explicit_compute_budgets_and_zero_remote_calls`; `DecisionGovernance.test.tsx` tests preserved stale drafts and fresh scoped adoption without copying authority. Native policy remains the validation authority. |


| G19 | Generated PRD intake validation and promotion fences | `initial_decisions_use_native_shape_references_and_effect_boundaries`, `intervention_requires_real_action_details_and_never_fabricates_a_result`, `request_budget_uses_destination_policy_and_preserves_input`, `registered_prd_cannot_be_promoted_even_when_disabled_or_missing`, `linked_worktree_and_other_reference_cannot_bypass_retained_authority`, `first_registration_waits_for_promotion_before_reading_source_bytes`, `existing_plan_verification_does_not_reacquire_the_publication_lock`, `corrupt_native_authority_is_not_treated_as_an_unmanaged_destination`; actual CLI `native_validation_rejects_malformed_decisions_and_reports_disabled_intake`, `cli_promotion_cannot_overwrite_a_native_plan_with_yes`. |

Sources: [core decision tests](../../core/src/coordinator/decisions/tests.rs), [new lifecycle/fault/approval scenarios](../../core/src/coordinator/decisions/tests_workflows.rs), [manual interventions, conditions and evidence](../../core/src/coordinator/decisions/tests_interaction.rs), [isolated verification tests](../../core/src/coordinator/decisions/secure.rs), [runner integration](../../cli/tests/support/performer_lifecycle.rs), [five-adapter matrix](../../cli/tests/support/decision_adapter_matrix.rs), [Web](../../web/src/components/DecisionCenter.test.tsx), [TUI](../../tui/src/decision_center.rs), [browser qualification harness](../../automat/tests/decision_browser.py).

| Acceptance ID | Baseline scenario | Evidence / qualification |
|---|---|---|
| AT01 | Planner creates a known sensitive decision gate | G1 — component/integration coverage; G15 verifies native transport for all five declared adapters; real planner/provider behavior still needs qualification. |
| AT02 | Performer encounters an unplanned architecture decision | G2 — component/integration coverage. |
| AT03 | Reviewer discovers policy conflict during review | G2 — component/integration coverage; G15 verifies native transport for all five declared adapters; real planner/provider behavior still needs qualification. |
| AT04 | Human chooses option B instead of agent recommendation A, with extra remark | G3/G11/G12 — exact native and client workflow coverage; independent deployment remains separately qualified. |
| AT05 | New task and dependency are inserted during active run | G4 — component/integration coverage. |
| AT06 | A patch introduces a dependency cycle | G4 — component/integration coverage. |
| AT07 | Two agents propose patches against the same graph version | G4 — component/integration coverage. |
| AT08 | Patch affects an active worktree/task | G4 — component/integration coverage. |
| AT09 | Patch changes an already merged task | G4 — component/integration coverage. |
| AT10 | Coordinator process crashes after graph DB commit but before PRD projection | G6/G14 — a separate process exits after native graph commit without dropping the SQLite connection; committed WAL/outbox is recovered. |
| AT11 | `macc clear` runs after an approval and dynamic patch | G6 — component/integration coverage. |
| AT12 | Coordinator restarts after clear on unchanged subject | G6 — component/integration coverage. |
| AT13 | Same artifact, different SHA under `content_equivalent` | G6 — component/integration coverage. |
| AT14 | Same SHA but policy/required quorum changes | G1 — component/integration coverage. |
| AT15 | `never` approval for one-shot destructive action exists | G7 — component/integration coverage. |
| AT16 | Expired/revoked approval is restored from disk | G6 — component/integration coverage. |
| AT17 | Unsigned forged Git note claims human approval | G8 — component/integration coverage; isolated deployment and human authentication require external qualification. |
| AT18 | Same-UID agent calls a privileged approval CLI in local-trust mode | G8 — component/integration coverage; isolated deployment and human authentication require external qualification. |
| AT19 | PRD manually edited while coordinator runs | G4 — component/integration coverage. |
| AT20 | `audit-prd` suggests changing an approved ADR | G2 — component/integration coverage; G15 verifies native transport for all five declared adapters; real planner/provider behavior still needs qualification. |
| AT21 | Restart with missing/corrupt graph event segment | G6 — component/integration coverage. |
| AT22 | 20 parallel workers and a decision affects only one feature subtree | G10/G16 — `twenty_real_processes_use_native_scheduler_while_only_hardware_subtree_is_blocked` uses the native selector/runtime sync, twenty actual controlled child processes and native contexts/receipts/ACKs while a hardware subtree is blocked. Production provider scheduling/throughput still needs load qualification. |
| AT23 | Approval quorum contains multiple required roles | G1 — component/integration coverage; isolated deployment and human authentication require external qualification. |
| AT24 | Web server exposed beyond localhost attempts anonymous mutation | G8 — component/integration coverage. |
| AT25 | User selects B and adds a negative constraint while performer expected A | G3/G11/G12 — exact native and client workflow coverage; independent deployment remains separately qualified. |
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
| AT42 | Migration or clear races an active SQLite writer, including uncheckpointed WAL data | G6/G14/G17 — `actual_clear_and_consistent_backup_survive_a_concurrent_process_wal_writer` runs real clear and consistent backup against a separate active SQLite WAL writer, checks retained dirty source and native recovery. Migration/restore on every production platform remains separate qualification. |
| AT43 | Crash occurs after reserving a one-shot authorization but before observing the external operation result | G7/G14 — actual execution owner is killed after a controlled external effect; recovery records unknown and refuses a second execution. Trusted service-specific outcome/idempotency still requires deployment evidence. |
| AT44 | An already approved human gate is the last remaining PRD entry | G4 — component/integration coverage. |
| AT45 | A performer encounters routine naming/local implementation choices, even with threshold `low` | G3 — component/integration coverage; G15 verifies native transport for all five declared adapters; real planner/provider behavior still needs qualification. |
| AT46 | A material unresolved choice has effective risk exactly equal to the configured threshold | G1 — component/integration coverage; G15 verifies native transport for all five declared adapters; real planner/provider behavior still needs qualification. |
| AT47 | User raises threshold from `high` to `critical`, then a new high-risk choice within explicit delegated scope arises | G3 — component/integration coverage. |
| AT48 | Threshold is `critical` but an explicit human gate, binding-policy conflict or fresh operation authorization requires intervention | G1 — component/integration coverage. |
| AT49 | High-risk task encounters a choice already answered by an applicable verified decision/policy | G2 — component/integration coverage. |
| AT50 | Model reports low confidence, or proposes an unsupported lower risk class | G1 — component/integration coverage; G15 verifies native transport for all five declared adapters; real planner/provider behavior still needs qualification. |
| AT51 | User changes the threshold while human requests are pending | G3 — component/integration coverage. |
| AT52 | One performer asks an important question while independent tasks remain ready; response changes instructions but no dependencies | G5 — component/integration coverage. |
| AT53 | Invalid autonomy value is entered or the same policy is edited through CLI/TUI/web | G1 — component/integration coverage. |
| AT54 | A below-threshold task lacks essential factual data and has no authorized safe default | G2 — component/integration coverage. |
| AT55 | Parallel/reused tool sessions discover the same unresolved decision, or a tool attempts a direct interactive question | G2 — component/integration coverage. |
| AT56 | A Web question with long context/options is opened on a narrow/mobile viewport and then resized | G9 — component/integration coverage; real browser/device/terminal visual qualification remains. |
| AT57 | TUI receives a long question in a small terminal, then terminal dimensions change | G9 — component/integration coverage; real browser/device/terminal visual qualification remains. |
| AT58 | Performer submits a clarification, option choice, approval or specific structured request | G3/G11/G12 — exact native and client workflow coverage; independent deployment remains separately qualified. |
| AT59 | A new request arrives while the operator is typing, another popup is open or the user defers a request | G9 — component/integration coverage. |
| AT60 | Double-submit, transient disconnection, stale subject or two clients answering the same request | G3/G11/G12 — exact native and client workflow coverage; independent deployment remains separately qualified. |
| AT61 | `never_ask` encounters low through critical questions, including a MACC human-approval declaration | G3 — component/integration coverage; G15 verifies native transport for all five declared adapters; real planner/provider behavior still needs qualification. |
| AT62 | Human or autonomous decision occurs under each of the five selector values, then a downstream/reused/new task starts | G3 — component/integration coverage. |
| AT63 | Operator enters `never_ask` with pending dialogs and unsent human text, then later returns to a threshold | G3/G11/G12 — exact native and client workflow coverage; independent deployment remains separately qualified. |
| AT64 | User answers by keyboard/screen reader, with color unavailable and long multiline input | G9 — component/integration coverage; real browser/device/terminal visual qualification remains. |
| AT65 | `never_ask` lacks necessary credentials, independent external approval evidence or indispensable factual input | G7 — component/integration coverage; G15 verifies native transport for all five declared adapters; real planner/provider behavior still needs qualification. |
| AT66 | Parallel performers recommend conflicting autonomous changes to a shared contract | G2 — component/integration coverage. |
| AT67 | An autonomous choice cannot be durably recorded or required context projection is unavailable | G5 — component/integration coverage. |
| AT68 | User or another authorized agent inspects an autonomous decision from a different task/client | G5 — component/integration coverage. |


The following additional scenarios specialize the baseline's question/response contract:

| Extension ID | Scenario | Evidence |
|---|---|---|
| UI69 | Performer needs a person to connect inaccessible hardware and return actual output | G16; native service, shell/CLI, Web and TUI tests; no fabricated result or implementation commit. |
| UI70 | Full autonomy encounters an indispensable manual action | G16; scoped `blocked_external`, no popup/inferred success, can return to consultation. |
| UI71 | Approval is conditional on independent verification | G17; no execution/quorum before checks, current evidence rechecked, invalid source/subject rejected. |
| UI72 | User requests more investigation before answering | G17; native bounded read-only attempt, report preserves original request, human answer required anew. |
| UI73 | User attaches result containing information unsuitable for external AI | G17; retained hash/bytes, default-private refs, separate explicit operator consent bound to configuration; no implicit sharing. |
| UI74 | User adopts prior PRD direction for a new task | G18; exact prior text/source/version plus fresh applicability justification, no imported vote/operation authority. |

Reproduction commands:

```sh
cargo test -p macc-core --lib
cargo test -p macc-cli -p macc-tui decision
cargo test -p macc-cli --test session_store_runner
npm --prefix web test -- src/components/DecisionCenter.test.tsx src/components/ApprovalDialog.test.tsx src/components/DecisionGovernance.test.tsx
npm --prefix web run build
```

Validation results are recorded only for the executed checkout below. A passing component or harness is not blanket certification of all 68 production acceptance scenarios.

Before the PRD entry-path hardening, the complete core run passed **1,243 tests**, with **2 opt-in external-installation tests ignored**; the focused native decision suite passed **78 tests**, the full TUI suite passed **150**, runner integration passed **24** and the Web decision/approval/governance suites passed **30**. `cargo check --workspace`, the Web production build, shared-runner Bash syntax, browser-harness Python syntax and `git diff --check` passed. Native authority tests include actual process concurrency/WAL recovery and rejection of rolled-back evidence disclosure permissions. No paid model call, physical device action, independently deployed human authentication, Git network publication or screen-reader certification is inferred.

At that earlier validation, the real Chromium harness returned `not_run` (exit 77): no compatible browser executable was available. That run claimed no visual pass. Existing JSDOM/axe and native terminal-buffer tests remain separate evidence.

Optional work package E now includes stopped signed-history reconciliation, observed analytics, bounded local lexical/TF-IDF/LSA hints, explicit Web governance editing, Web/TUI fresh scoped inter-PRD guidance and the generic isolated connector. Triage diagnostics expose actual configured rounds/context limits and zero extra paid sidecar calls. Paid generative triage, vendor-specific enterprise identity adapters and their deployment certification remain optional extensions, not first-release acceptance claims.

The full TUI pass also caught and fixed an existing settings-list drift: Supervisor navigation now derives all field indices from the shared `settings::FIELDS` contract, so adding a field cannot hide the last setting. The five-adapter run caught and fixed the config-driven model path under `set -e`: a missing CLI model flag now reaches the declared configuration-file fallback instead of stopping Vibe. No user branch or worktree cleanup was performed.


PRD entry-path hardening validation (2026-11-04): the complete core suite passed **1,256 tests**, with **2 opt-in external-installation cases ignored**. All **3 PRD CLI integration tests**, `cargo check --workspace`, the Web production build and `git diff --check` passed. The added tests protect retained graphs, missing exports, linked worktrees, invalid proposal shapes, disabled-policy warnings, backup/file retention and the promotion/registration race. No live AI or existing user repository was exercised. Sources: [intake tests](../../core/src/prd_generation/tests_intake.rs), [promotion tests](../../core/src/prd_generation/tests_promotion.rs), [CLI tests](../../cli/tests/prd_validate_paths.rs).


Question/intervention UX validation (2026-11-06): **164 TUI tests** and the full **280-test Web suite** pass, along with the Web production build, `cargo check --workspace` and `git diff --check`. The TUI tests exercise actual application routing and terminal buffers, including 40-column layouts, field-focused errors, Unicode editing, refresh races and an explicit review/accept/send sequence for long task-plan patches. New Web cases reproduce a general “reconnected !” note with missing structured output, preserve server errors across polling, support an honest inability response, and retain drafts when a new version arrives. Chromium checks use the local mocked-network fixture, not a running coordinator or physical device; this does not certify assistive-technology behavior, real hardware or enterprise authentication. Direct Chromium/CDP checks passed for questions at 360×720 and 1280×900, and interventions at 390×844 and 1280×900: no horizontal overflow, visible Send controls, retained deferred drafts, focused required-field validation and no mutations from an incomplete response. The fixture is `web/tests/decision-browser.html`; `?scenario=intervention` selects the intervention variant. The legacy dump-DOM runner did not produce a usable DOM result in this environment; its failed invocation is not counted as a browser pass.


Simple response flow (2026-11-07): clients close only after an answer receipt and a matching, recorded request/version. Pending approval tickets and failed saves do not count as recorded answers. TUI numeric choices lead straight to Send; a declared yes/no field supplies its exact selected value as the response without demanding an additional written answer. Native intervention observations and independent signatures remain mandatory where applicable. History renders as recorded/read-only rather than showing an unfinished form. New coverage: TUI `numbered_choice_goes_straight_to_send_without_an_extra_note`, `a_single_typed_yes_no_field_is_the_answer_not_a_second_free_text_question`, `recorded_answer_closes_the_popup_but_failure_and_pending_approval_do_not`, and `recorded_history_does_not_show_pending_form_instructions_or_duplicate_action`; Web tests cover acknowledged closure, yes/no buttons, save failure and pending approval versus a concurrent resolution. The local browser fixture has `simple-choice` and `simple-boolean` variants for real-browser submission/closure checks with a mocked native receipt. The protocol and planner guidance specify short questions, up to four concise choices and script-first checks; display, formatting and validation add no model calls. No measured token-reduction percentage is asserted.

### November 8 ask-user regression checks

The native response/wait suite covers actual answer version increments, explicit
fact/deferral semantics, unchanged completed-action requirements, rejection of
contradictory modes, mandatory independent compliance, lease fencing, invocation
isolation, retained worktree/phase and late answers. TUI regressions exercise
choice-first navigation, contextual multiline help, exact-version auto-close and
receipt presentation. Web tests cover equivalent conditional requirements.
`cli/tests/support/decision_wait_runner.rs` stops a fake provider through the real
heartbeat shell after a native human request; it verifies the durable scoped
receipt without invoking a model. Supervisor cache tests assert that unrelated
changes do not spend another diagnosis while a native gate is unanswered.

### 2026-11-10 — deterministic context and external prerequisite recovery

Native tool contexts are assembled together from available assets. Regression
fixtures cover empty selections, French conventions, shared Gemini/Antigravity
output, custom Gemini filenames, missing selected skills, provenance-aware context
migration, preservation of edited root/worktree files and conflicting writes.
See `registry/tests/context_generation.rs` and `docs/architecture/context-generation.md`.

External waits no longer keep an idle coordinator running as if an approval were
pending. Source-recovery fixtures use real local Git commits and native decisions
to verify exact revisions, published path evidence, journal consistency and
machine attribution; physical or legacy interventions cannot be cleared by source
edits. The web fixture checks explicit blocked status without an automatic
question popup in `never_ask`. No fixture requires a paid model or a user project.

Validation for this change: the core library passes 1,398 tests (two explicit
provider smoke tests remain ignored), the native adapter/registry libraries pass,
and the context-generation integration suite passes eight tests. The TUI suite
passes 174 tests, the decision-center web suite passes 28 tests, and the web
production build passes. Local Git coordinator and plan/apply integration tests
also pass without a provider call.

The CLI unit suite passes 272 tests with `RUST_MIN_STACK=8388608`. Its root-parser
test overflows the default debug test-thread stack; this run does not establish
that default-stack parser issue is fixed. No production stack setting was changed.
No running user coordinator or installed binary was replaced by these checks.

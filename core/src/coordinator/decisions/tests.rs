use super::*;
use crate::coordinator::model::TaskRegistry;
use serde_json::json;
use std::{collections::BTreeMap, path::Path, process::Command};
fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}
fn fixture() -> (tempfile::TempDir, Store, Plan, TaskRegistry, Policy) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Operator"]);
    git(root, &["config", "user.email", "operator@example.test"]);
    std::fs::create_dir_all(root.join(".macc")).unwrap();
    let mut cfg = crate::config::CanonicalConfig::default();
    let p = Policy {
        enabled: true,
        ..Default::default()
    };
    let c = cfg
        .automation
        .coordinator
        .get_or_insert_with(Default::default);
    c.prd_file = Some("prd.json".into());
    c.reference_branch = Some("main".into());
    c.decisions = Some(p.clone());
    std::fs::write(root.join(".macc/macc.yaml"), cfg.to_yaml().unwrap()).unwrap();
    let graph = json!({"tasks":[{"id":"A","title":"Implementation","dependencies":[]},{"id":"B","dependencies":["A"]},{"id":"C","dependencies":[]}]});
    std::fs::write(root.join("prd.json"), serde_json::to_vec(&graph).unwrap()).unwrap();
    let reg = TaskRegistry::from_value(&json!({"tasks":graph["tasks"]})).unwrap();
    let mut store = Store::open(root).unwrap();
    let plan = store
        .ensure_plan(root, Path::new("prd.json"), "main", &reg)
        .unwrap();
    (tmp, store, plan, reg, p)
}
fn question(id: &str) -> Request {
    serde_json::from_value(json!({"idempotency_key":id,"topic":id,"task_id":"A","question":"Which storage contract?","context":"Two implementations conflict","investigated":["Read both call sites"],"recommendation":"Use the versioned contract","risk":"high","impact":{"shared_contract":true,"evidence":["Both clients consume it"]}})).unwrap()
}
fn patch(plan: &Plan, key: &str, operations: Vec<Operation>) -> Patch {
    Patch {
        plan_id: plan.id.clone(),
        base_graph_version: plan.graph_version,
        idempotency_key: key.into(),
        reason: "Reviewed architecture adjustment".into(),
        decision_refs: vec![],
        operations,
        metadata_changes: BTreeMap::new(),
    }
}
#[test]
fn policy_thresholds_and_never_ask_do_not_fabricate_human_votes() {
    let (_tmp, mut store, plan, _, mut p) = fixture();
    for mode in [
        Consultation::Low,
        Consultation::Medium,
        Consultation::High,
        Consultation::Critical,
        Consultation::NeverAsk,
    ] {
        p.human_question_min_risk = mode;
        let mut r = question(&format!("{mode:?}"));
        r.question = format!("Question {mode:?}");
        let d = store.submit(&plan, r, "agent:A", "agent", &p).unwrap();
        assert_eq!(d.status == "pending_human", mode.asks(Risk::High));
        if let Some(r) = d.resolution {
            assert_eq!(r.origin, "delegated_autonomous");
        }
    }
}
#[test]
fn human_text_is_exact_scoped_and_survives_runtime_removal() {
    let (tmp, mut store, plan, _, p) = fixture();
    let d = store
        .submit(&plan, question("q"), "agent:A", "agent", &p)
        .unwrap();
    assert!(store.context(&plan, "B", "b", "dev", &p).is_err());
    assert!(store.context(&plan, "C", "c", "dev", &p).is_ok());
    let text = "Conserver SQLite.\nNe PAS modifier le contrat public; conserver les accents éà.";
    let resolved = store
        .answer(&d.id, d.version, text, None, "Human", false, &p)
        .unwrap();
    assert_eq!(resolved.resolution.as_ref().unwrap().text, text);
    assert!(store
        .answer(&d.id, d.version, "stale", None, "Other", false, &p)
        .is_err());
    drop(store);
    std::fs::remove_dir_all(tmp.path().join(".macc")).unwrap();
    let store = Store::open(tmp.path()).unwrap();
    let ctx = store
        .context(&store.plan(&plan.id).unwrap(), "B", "new", "review", &p)
        .unwrap();
    assert_eq!(ctx.bindings[0].resolution.as_ref().unwrap().text, text);
    assert!(ctx.render().contains("Ne PAS modifier"));
}
#[test]
fn requests_are_idempotent_across_attempts_but_changed_questions_are_rejected() {
    let (_tmp, mut store, plan, _, p) = fixture();
    let r = question("same");
    let first = store
        .submit(&plan, r.clone(), "agent:A", "agent", &p)
        .unwrap();
    let mut retry = r;
    retry.attempt_id = "next-attempt".into();
    assert_eq!(
        store
            .submit(&plan, retry.clone(), "agent:A", "agent", &p)
            .unwrap()
            .id,
        first.id
    );
    retry.question = "Changed question".into();
    assert!(store.submit(&plan, retry, "agent:A", "agent", &p).is_err());
}
#[test]
fn graph_cas_cycles_historical_ids_and_replay_are_enforced() {
    let (tmp, mut store, plan, reg, _) = fixture();
    let add = patch(
        &plan,
        "add",
        vec![Operation::AddTask {
            task: json!({"id":"D","dependencies":["B"]}),
        }],
    );
    let next = store.apply_patch(&plan.id, &add, &reg, "Human").unwrap();
    assert_eq!(next.graph_version, 2);
    assert_eq!(
        store
            .apply_patch(&plan.id, &add, &reg, "Human")
            .unwrap()
            .graph_version,
        2
    );
    assert!(store
        .apply_patch(
            &plan.id,
            &patch(
                &plan,
                "stale",
                vec![Operation::DisableTask {
                    task_id: "C".into()
                }]
            ),
            &reg,
            "Human"
        )
        .is_err());
    assert!(store
        .preview_patch(
            &next,
            &patch(
                &next,
                "cycle",
                vec![Operation::AddDependency {
                    task_id: "A".into(),
                    depends_on: "D".into()
                }]
            ),
            &reg
        )
        .is_err());
    assert!(store
        .preview_patch(
            &next,
            &patch(
                &next,
                "orphan",
                vec![Operation::DisableTask {
                    task_id: "A".into()
                }]
            ),
            &reg
        )
        .is_err());
    store.project(tmp.path()).unwrap();
    let disk: serde_json::Value =
        serde_json::from_slice(&std::fs::read(tmp.path().join("prd.json")).unwrap()).unwrap();
    assert_eq!(disk, next.graph);
    store.verify().unwrap();
    drop(store);
    let mut store = Store::open(tmp.path()).unwrap();
    std::fs::remove_file(tmp.path().join("prd.json")).unwrap();
    store.project(tmp.path()).unwrap();
    assert!(tmp.path().join("prd.json").exists());
}
#[test]
fn delivered_tasks_cannot_be_rewritten_and_new_tasks_inherit_decisions() {
    let (_tmp, mut store, plan, mut reg, mut p) = fixture();
    reg.find_task_mut("A").unwrap().state = "merged".into();
    assert!(store
        .preview_patch(
            &plan,
            &patch(
                &plan,
                "rewrite",
                vec![Operation::UpdateTask {
                    task_id: "A".into(),
                    changes: BTreeMap::from([("title".into(), json!("changed"))])
                }]
            ),
            &reg
        )
        .is_err());
    p.human_question_min_risk = Consultation::NeverAsk;
    let d = store
        .submit(&plan, question("q"), "agent:A", "agent", &p)
        .unwrap();
    let next = store
        .apply_patch(
            &plan.id,
            &patch(
                &plan,
                "new",
                vec![Operation::AddTask {
                    task: json!({"id":"D","dependencies":["B"]}),
                }],
            ),
            &reg,
            "Human",
        )
        .unwrap();
    assert_eq!(
        store.context(&next, "D", "d", "dev", &p).unwrap().bindings[0].id,
        d.id
    );
}
#[test]
fn real_prompt_receipt_ack_review_and_stale_barriers() {
    let (tmp, mut store, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::NeverAsk;
    let d = store
        .submit(&plan, question("q"), "agent:A", "agent", &p)
        .unwrap();
    let plan = store.plan(&plan.id).unwrap();
    let (ctx, _) = store
        .issue(&plan, "A", "attempt", "review", tmp.path(), &p)
        .unwrap();
    assert!(store
        .issued(&plan.id, "attempt", "review", "only a file path", &p)
        .is_err());
    store
        .issued(&plan.id, "attempt", "review", &ctx.render(), &p)
        .unwrap();
    assert!(store
        .complete(&plan.id, "attempt", "review", "done", &p)
        .is_err());
    let ack = format!("MACC_DECISION_ACK: {}", ctx.context_digest);
    assert!(store
        .complete(&plan.id, "attempt", "review", &ack, &p)
        .is_err());
    let output = format!(
        "{ack}\nMACC_DECISION_COMPLIANCE: {}",
        json!({d.id.clone():{"status":"pass","evidence":"Compared both consumer call sites and ran contract tests"}})
    );
    assert!(
        store
            .complete(&plan.id, "attempt", "review", &output, &p)
            .unwrap()
            .acknowledged
    );
    store
        .revoke(&d.id, d.version, "Human", "Contract invalid")
        .unwrap();
    assert!(store
        .complete(&plan.id, "attempt", "review", &output, &p)
        .unwrap_err()
        .to_string()
        .contains("DM_STALE"));
}
#[test]
fn policy_changes_are_checked_before_actual_invocation() {
    let (tmp, mut store, plan, _, mut p) = fixture();
    let (ctx, _) = store
        .issue(&plan, "A", "attempt", "dev", tmp.path(), &p)
        .unwrap();
    p.human_question_min_risk = Consultation::NeverAsk;
    assert!(store
        .issued(&plan.id, "attempt", "dev", &ctx.render(), &p)
        .is_err());
}
#[test]
fn mandatory_context_is_never_silently_truncated() {
    let (_tmp, mut store, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::NeverAsk;
    let mut r = question("big");
    r.recommendation = "Exact human constraints. ".repeat(300);
    store.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    p.max_context_bytes = 2048;
    assert!(store
        .context(&plan, "A", "a", "dev", &p)
        .unwrap_err()
        .to_string()
        .contains("no binding was truncated"));
}
#[test]
fn projection_tampering_and_event_corruption_fail_closed() {
    let (_tmp, store, plan, _, _) = fixture();
    store.db.execute("UPDATE plans SET payload=json_set(payload,'$.graph.tasks[0].title','tampered') WHERE id=?1",[&plan.id]).unwrap();
    assert!(store.verify().is_err());
}
#[test]
fn authorization_consumption_is_atomic_and_operation_scoped() {
    let (_tmp, mut store, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::NeverAsk;
    let mut r = question("auth");
    r.kind = "authorization".into();
    r.operation_id = Some("operation-1".into());
    configure(_tmp.path(), p.clone()).unwrap();
    let d = store.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    store.consume_authorization(&d.id, "operation-1").unwrap();
    store.consume_authorization(&d.id, "operation-1").unwrap();
    assert!(store.consume_authorization(&d.id, "operation-2").is_err());
    store.verify().unwrap();
}
#[test]
fn out_of_band_prd_is_not_overwritten_by_recovery() {
    let (tmp, mut store, plan, reg, _) = fixture();
    store
        .apply_patch(
            &plan.id,
            &patch(
                &plan,
                "add",
                vec![Operation::AddTask {
                    task: json!({"id":"D"}),
                }],
            ),
            &reg,
            "Human",
        )
        .unwrap();
    std::fs::write(
        tmp.path().join("prd.json"),
        r#"{"tasks":[{"id":"EXTERNAL"}]}"#,
    )
    .unwrap();
    assert!(store.project(tmp.path()).is_err());
    assert!(std::fs::read_to_string(tmp.path().join("prd.json"))
        .unwrap()
        .contains("EXTERNAL"));
}
#[test]
fn linked_worktrees_share_authority_but_distinct_plan_scopes() {
    let (tmp, mut store, plan, reg, _) = fixture();
    git(tmp.path(), &["add", "prd.json"]);
    git(tmp.path(), &["commit", "-m", "baseline"]);
    let other = tmp.path().join("linked");
    git(
        tmp.path(),
        &["worktree", "add", "-b", "other", other.to_str().unwrap()],
    );
    assert_eq!(Store::location(&other).unwrap(), store.path);
    let next = store
        .ensure_plan(tmp.path(), Path::new("prd.json"), "other", &reg)
        .unwrap();
    assert_ne!(next.id, plan.id);
    assert_eq!(next.project_id, plan.project_id);
}

#[test]
fn autonomous_graph_proposal_is_committed_atomically_with_its_decision() {
    let (tmp, mut store, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::NeverAsk;
    let mut request = question("plan-change");
    request.proposed_patch = Some(patch(
        &plan,
        "proposal-patch",
        vec![Operation::AddTask {
            task: json!({"id":"FOLLOWUP","dependencies":["A"],"title":"Implement the selected contract"}),
        }],
    ));
    let d = store
        .submit(&plan, request, "agent:A", "agent", &p)
        .unwrap();
    assert_eq!(d.status, "resolved");
    let latest = store.plan(&plan.id).unwrap();
    assert_eq!(latest.graph_version, 2);
    assert!(latest.graph["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["id"] == "FOLLOWUP"));
    store.project(tmp.path()).unwrap();
    assert_eq!(
        store
            .context(&latest, "FOLLOWUP", "next", "dev", &p)
            .unwrap()
            .bindings[0]
            .id,
        d.id
    );
    store.verify().unwrap();
}
#[test]
fn human_can_apply_recommended_graph_proposal_explicitly() {
    let (_tmp, mut store, plan, _, p) = fixture();
    let mut r = question("plan-change");
    r.options = vec![OptionChoice {proposed_patch:None,requires_replan:false,
        id: "apply".into(),
        label: "Apply the shown patch".into(),
        consequences: vec!["Adds a follow-up task".into()],
    }];
    r.recommended_option_id = Some("apply".into());
    r.proposed_patch = Some(patch(
        &plan,
        "approved-patch",
        vec![Operation::AddTask {
            task: json!({"id":"D","dependencies":["A"]}),
        }],
    ));
    let d = store.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    assert_eq!(store.plan(&plan.id).unwrap().graph_version, 1);
    store
        .answer_details(
            &d.id,
            d.version,
            &AnswerInput { conditions:Vec::new(), artifacts:Vec::new(),
                text: "Oui, conserver le contrat".into(),
                option: Some("apply".into()),
                apply_patch: true,
                ..Default::default()
            },
            "Human",
            &p,
        )
        .unwrap();
    assert_eq!(store.plan(&plan.id).unwrap().graph_version, 2);
    store.verify().unwrap();
}
#[test]
fn retired_inflight_context_can_never_complete_successfully() {
    let (tmp, mut store, plan, reg, p) = fixture();
    let (ctx, _) = store
        .issue(&plan, "C", "inflight", "dev", tmp.path(), &p)
        .unwrap();
    store
        .issued(&plan.id, "inflight", "dev", &ctx.render(), &p)
        .unwrap();
    store
        .apply_patch(
            &plan.id,
            &patch(
                &plan,
                "disable",
                vec![Operation::DisableTask {
                    task_id: "C".into(),
                }],
            ),
            &reg,
            "Human",
        )
        .unwrap();
    let ack = format!("MACC_DECISION_ACK: {}", ctx.context_digest);
    assert!(store
        .complete(&plan.id, "inflight", "dev", &ack, &p)
        .unwrap_err()
        .to_string()
        .contains("DM_STALE"));
}
#[test]
fn backup_is_consistent_restores_identity_and_never_rolls_back_newer_history() {
    let (tmp, mut store, plan, _, p) = fixture();
    let backup = tempfile::tempdir().unwrap();
    let d = store
        .submit(&plan, question("q"), "agent:A", "agent", &p)
        .unwrap();
    let meta = capture_backup(tmp.path(), backup.path()).unwrap().unwrap();
    let source = backup.path().join(&meta.database_path);
    store
        .answer(
            &d.id,
            d.version,
            "Keep the public contract",
            None,
            "Human",
            false,
            &p,
        )
        .unwrap();
    let epoch = store.plan(&plan.id).unwrap().decision_epoch;
    restore_backup(tmp.path(), &source, &meta).unwrap();
    assert_eq!(
        Store::open(tmp.path())
            .unwrap()
            .plan(&plan.id)
            .unwrap()
            .decision_epoch,
        epoch
    );
    let path = store.path.clone();
    drop(store);
    std::fs::remove_file(&path).unwrap();
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    restore_backup(tmp.path(), &source, &meta).unwrap();
    let recovered = Store::open(tmp.path()).unwrap();
    assert_eq!(recovered.project_id, meta.project_id);
    assert_eq!(recovered.decision(&d.id).unwrap().status, "pending_human");
}
#[test]
fn foreign_backup_cannot_become_local_approval_authority() {
    let (tmp, _store, _, _, _) = fixture();
    let backup = tempfile::tempdir().unwrap();
    let meta = capture_backup(tmp.path(), backup.path()).unwrap().unwrap();
    let (other, _, _, _, _) = fixture();
    assert!(validate_backup(
        other.path(),
        &backup.path().join(&meta.database_path),
        &meta
    )
    .is_err());
}
fn gate_fixture() -> (tempfile::TempDir, Store, Plan, Policy, String) {
    let (tmp, mut store, plan, reg, p) = fixture();
    std::fs::write(tmp.path().join("contract.txt"), "public bytes").unwrap();
    git(tmp.path(), &["add", "contract.txt"]);
    git(
        tmp.path(),
        &["commit", "-m", "Implement A\n\n[macc:task A]"],
    );
    let sha = git(tmp.path(), &["rev-parse", "HEAD"]);
    let gate = json!({"id":"APP","dependencies":["A"],"gate":{"kind":"human_approval","subject_task":"A","required_approvers":[{"role":"OWNER","count":1}],"quorum":"all","bind_to":"commit_sha"}});
    let plan = store
        .apply_patch(
            &plan.id,
            &patch(&plan, "add-gate", vec![Operation::AddTask { task: gate }]),
            &reg,
            "Human",
        )
        .unwrap();
    store.project(tmp.path()).unwrap();
    (tmp, store, plan, p, sha)
}
fn vote(revision: &str) -> crate::coordinator::model::ApprovalRecord {
    crate::coordinator::model::ApprovalRecord {
        decision: crate::coordinator::model::ApprovalDecision::Approved,
        actor: "Human".into(),
        role: "OWNER".into(),
        revision: Some(revision.into()),
        evidence: Some("Reviewed contract.txt".into()),
        reason: None,
        recorded_at: now(),
    }
}
#[test]
fn exact_revision_approval_survives_projection_loss_but_policy_change_invalidates_it() {
    let (tmp, mut store, plan, mut p, sha) = gate_fixture();
    store
        .record_approval(tmp.path(), &plan, "APP", &vote(&sha))
        .unwrap();
    assert_eq!(
        store.verified_approvals(tmp.path(), &plan).unwrap()["APP"].len(),
        1
    );
    drop(store);
    let mut store = Store::open(tmp.path()).unwrap();
    assert_eq!(
        store.verified_approvals(tmp.path(), &plan).unwrap()["APP"].len(),
        1
    );
    p.human_question_min_risk = Consultation::Critical;
    configure(tmp.path(), p).unwrap();
    assert!(store
        .verified_approvals(tmp.path(), &plan)
        .unwrap()
        .is_empty());
    store.import_legacy(tmp.path(), &plan).unwrap();
    assert!(store
        .verified_approvals(tmp.path(), &plan)
        .unwrap()
        .is_empty());
}
#[test]
fn content_equivalent_approval_requires_manifest_and_current_matching_bytes() {
    let (tmp, mut store, plan, mut p, sha) = gate_fixture();
    p.approval_reuse = ApprovalReuse::ContentEquivalent;
    p.approval_manifests
        .insert("APP".into(), vec!["contract.txt".into()]);
    configure(tmp.path(), p.clone()).unwrap();
    store
        .record_approval(tmp.path(), &plan, "APP", &vote(&sha))
        .unwrap();
    git(
        tmp.path(),
        &[
            "commit",
            "--allow-empty",
            "-m",
            "Recreate A\n\n[macc:task A]",
        ],
    );
    assert!(!store
        .verified_approvals(tmp.path(), &plan)
        .unwrap()
        .is_empty());
    std::fs::write(tmp.path().join("contract.txt"), "changed bytes").unwrap();
    git(tmp.path(), &["add", "contract.txt"]);
    git(tmp.path(), &["commit", "-m", "Change A\n\n[macc:task A]"]);
    assert!(store
        .verified_approvals(tmp.path(), &plan)
        .unwrap()
        .is_empty());
}
#[test]
fn approval_requires_existing_unambiguous_commit_and_valid_timestamp() {
    let (tmp, mut store, plan, _, _) = gate_fixture();
    assert!(store
        .record_approval(tmp.path(), &plan, "APP", &vote("deadbeef"))
        .is_err());
}
#[test]
fn operator_reconciliation_requires_the_inspected_disk_digest_and_preserves_original() {
    let (tmp, mut store, plan, _, _) = fixture();
    let mut disk = plan.graph.clone();
    disk["tasks"][0]["title"] = json!("External proposal");
    std::fs::write(
        tmp.path().join("prd.json"),
        serde_json::to_vec(&disk).unwrap(),
    )
    .unwrap();
    let p = patch(
        &plan,
        "reconcile",
        vec![Operation::UpdateTask {
            task_id: "A".into(),
            changes: BTreeMap::from([("title".into(), json!("Accepted title"))]),
        }],
    );
    assert!(store
        .reconcile_projection(tmp.path(), &p, "wrong", "Human")
        .is_err());
    let next = store
        .reconcile_projection(tmp.path(), &p, &digest(&disk), "Human")
        .unwrap();
    store.project(tmp.path()).unwrap();
    assert_eq!(next.graph["tasks"][0]["title"], "Accepted title");
    assert!(store
        .events(&plan.id)
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "graph.source.reconciled" && e["payload"]["graph"] == disk));
    store.verify().unwrap();
}
#[test]
fn approval_projection_cannot_be_forged_without_journal_evidence() {
    let (_tmp, store, plan, _, _) = fixture();
    store
        .db
        .execute(
            "INSERT INTO legacy_approvals VALUES(?1,'forged','{}')",
            [plan.id],
        )
        .unwrap();
    assert!(store.verify().is_err());
}
#[test]
fn project_feedback_reaches_future_prd_plans_but_task_consent_and_other_branches_do_not() {
    let (tmp, mut store, plan, reg, p) = fixture();
    let mut global = question("project");
    global.task_id = "*".into();
    global.scope = "project".into();
    let d = store.submit(&plan, global, "Human", "human", &p).unwrap();
    store
        .answer(
            &d.id,
            d.version,
            "Preserve historical relations in every future feature",
            None,
            "Human",
            false,
            &p,
        )
        .unwrap();
    let local = store
        .submit(&plan, question("task-only"), "agent:A", "agent", &p)
        .unwrap();
    store
        .answer(
            &local.id,
            local.version,
            "Applies only to original task A",
            None,
            "Human",
            false,
            &p,
        )
        .unwrap();
    std::fs::write(
        tmp.path().join("next.json"),
        serde_json::to_vec(&json!({"tasks":[{"id":"A"}]})).unwrap(),
    )
    .unwrap();
    let next = store
        .ensure_plan(tmp.path(), Path::new("next.json"), "main", &reg)
        .unwrap();
    let ctx = store.context(&next, "A", "next", "dev", &p).unwrap();
    assert_eq!(ctx.bindings.len(), 1);
    assert_eq!(ctx.bindings[0].id, d.id);
    assert!(store
        .scoped_history(&next)
        .unwrap()
        .iter()
        .any(|e| e["payload"].get("id") == Some(&json!(d.id))));
    let other = store
        .ensure_plan(tmp.path(), Path::new("next.json"), "other", &reg)
        .unwrap();
    assert!(store
        .context(&other, "A", "other", "dev", &p)
        .unwrap()
        .bindings
        .is_empty());
}
#[test]
fn review_records_negative_evidence_and_publication_is_bound_to_the_reviewed_commit() {
    let (tmp, mut store, plan, mut reg, p) = fixture();
    std::fs::write(tmp.path().join(".gitignore"), ".macc/\n").unwrap();
    git(tmp.path(), &["add", ".gitignore", "prd.json"]);
    git(tmp.path(), &["commit", "-m", "baseline"]);
    let original = git(tmp.path(), &["rev-parse", "HEAD"]);
    let d = store
        .submit(&plan, question("binding"), "agent:A", "agent", &p)
        .unwrap();
    store
        .answer(
            &d.id,
            d.version,
            "Keep the public contract",
            None,
            "Human",
            false,
            &p,
        )
        .unwrap();
    let current = store.plan(&plan.id).unwrap();
    let (ctx, _) = store
        .issue(&current, "A", "reviewed", "review", tmp.path(), &p)
        .unwrap();
    store
        .issued(&plan.id, "reviewed", "review", &ctx.render(), &p)
        .unwrap();
    let ack = format!("MACC_DECISION_ACK: {}", ctx.context_digest);
    let failed = format!(
        "{ack}\nMACC_DECISION_COMPLIANCE: {}",
        json!({d.id.clone():{"status":"fail","evidence":"The consumer changed the public payload"}})
    );
    assert!(store
        .complete(&plan.id, "reviewed", "review", &failed, &p)
        .is_err());
    let history = store.context_history(&plan.id, "A").unwrap();
    assert_eq!(history[0]["receipt"]["acknowledged"], true);
    assert_eq!(
        history[0]["receipt"]["completion_status"],
        "compliance_failed"
    );
    assert_eq!(history[0]["receipt"]["compliance"][&d.id]["status"], "fail");
    let passed = format!(
        "{ack}\nMACC_DECISION_COMPLIANCE: {}",
        json!({d.id.clone():{"status":"pass","evidence":"Checked exact public payload against both consumers"}})
    );
    store
        .complete(&plan.id, "reviewed", "review", &passed, &p)
        .unwrap();
    store
        .attest_review_artifact(&plan.id, "reviewed", tmp.path(), &original)
        .unwrap();
    let task = reg.find_task_mut("A").unwrap();
    task.worktree = Some(
        serde_json::from_value(
            json!({"worktree_path":tmp.path(),"branch":"main","base_branch":"main"}),
        )
        .unwrap(),
    );
    task.task_runtime.extra.insert(
        "decision_context".into(),
        json!({"attempt_id":"reviewed","phase":"review"}),
    );
    crate::coordinator::state::coordinator_state_registry_save(
        tmp.path(),
        &BTreeMap::new(),
        &reg.to_value().unwrap(),
    )
    .unwrap();
    merge_ready(tmp.path(), "A").unwrap();
    std::fs::write(tmp.path().join("unreviewed.txt"), "unreviewed change").unwrap();
    git(tmp.path(), &["add", "unreviewed.txt"]);
    git(tmp.path(), &["commit", "-m", "unreviewed edit"]);
    assert!(merge_ready(tmp.path(), "A")
        .unwrap_err()
        .to_string()
        .contains("DM_STALE"));
    store.verify().unwrap();
}
#[test]
fn repeated_operator_feedback_is_idempotent_and_does_not_silently_demote_a_binding() {
    let (_tmp, mut store, plan, _, p) = fixture();
    let first = store
        .feedback(
            &plan,
            "*",
            "Conserver les relations historiques.\nGarder le contrat public.",
            "Human",
            false,
            &p,
        )
        .unwrap();
    let retry = store
        .feedback(
            &plan,
            "*",
            "Conserver les relations historiques.\nGarder le contrat public.",
            "Human",
            false,
            &p,
        )
        .unwrap();
    assert_eq!(first.id, retry.id);
    assert_eq!(retry.status, "resolved");
    assert_eq!(store.decisions(&plan.id).unwrap().len(), 1);
    assert!(store
        .feedback(
            &plan,
            "*",
            "Conserver les relations historiques.\nGarder le contrat public.",
            "Human",
            true,
            &p
        )
        .is_err());
    assert!(store
        .feedback(&plan, "*", "  ", "Human", false, &p)
        .is_err());
    store.verify().unwrap();
}

#[test]
fn typed_subject_arbitration_reuses_authority_and_requires_versioned_supersession() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::NeverAsk;
    let mut a = question("subject-a");
    a.topic = Some("storage-contract".into());
    let first = s.submit(&plan, a, "agent:A", "agent", &p).unwrap();
    let mut b = question("subject-b");
    b.task_id = "C".into();
    b.topic = Some("storage-contract".into());
    b.recommendation = "Contradictory private recommendation".into();
    let second = s.submit(&plan, b.clone(), "agent:C", "agent", &p).unwrap();
    assert_eq!(second.reused_from.as_ref().unwrap().id, first.id);
    assert_eq!(
        second.resolution.as_ref().unwrap().text,
        first.resolution.as_ref().unwrap().text
    );
    assert!(s
        .context(&s.plan(&plan.id).unwrap(), "A", "fresh", "dev", &p)
        .unwrap()
        .bindings
        .iter()
        .any(|d| d.id == second.id));
    // Explicit supersession remains possible under delegated autonomy and invalidates old contexts.
    b.idempotency_key = "replacement".into();
    b.supersedes = vec![
        DecisionRef {
            id: first.id.clone(),
            version: first.version,
        },
        DecisionRef {
            id: second.id.clone(),
            version: second.version,
        },
    ];
    let replacement = s.submit(&plan, b, "agent:C", "agent", &p).unwrap();
    assert_eq!(s.decision(&first.id).unwrap().status, "superseded");
    assert_eq!(
        replacement.resolution.as_ref().unwrap().text,
        "Contradictory private recommendation"
    );
    s.verify().unwrap();
}
#[test]
fn concurrent_human_answers_cannot_silently_override_subject_authority() {
    let (_tmp, mut s, plan, _, p) = fixture();
    let mut r = question("concurrent-a");
    r.topic = Some("api-contract".into());
    let a = s.submit(&plan, r.clone(), "agent:A", "agent", &p).unwrap();
    r.idempotency_key = "concurrent-b".into();
    r.question = "Other proposal".into();
    let b = s.submit(&plan, r, "agent:C", "agent", &p).unwrap();
    s.answer(&a.id, a.version, "SQLite", None, "Owner", false, &p)
        .unwrap();
    assert!(s
        .answer(&b.id, b.version, "Postgres", None, "Other", false, &p)
        .unwrap_err()
        .to_string()
        .contains("supersession"));
    s.reevaluate_pending(&plan, &p).unwrap();
    assert_eq!(
        s.decision(&b.id).unwrap().resolution.unwrap().text,
        "SQLite"
    );
}
#[test]
fn free_text_and_rejection_never_implicitly_apply_proposed_patch() {
    let (_tmp, mut s, plan, _, p) = fixture();
    let mut r = question("custom-answer");
    r.options = vec![OptionChoice {proposed_patch:None,requires_replan:false,
        id: "yes".into(),
        label: "Apply".into(),
        consequences: vec![],
    }];
    r.recommended_option_id = Some("yes".into());
    r.proposed_patch = Some(patch(
        &plan,
        "custom-patch",
        vec![Operation::AddTask {
            task: json!({"id":"D","dependencies":[]}),
        }],
    ));
    let d = s.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    s.answer(
        &d.id,
        d.version,
        "Different human conditions",
        Some("yes".into()),
        "Owner",
        false,
        &p,
    )
    .unwrap();
    assert_eq!(s.plan(&plan.id).unwrap().graph_version, 1);
    assert!(!s.decision(&d.id).unwrap().resolution.unwrap().patch_applied);
}
#[test]
fn governance_rules_raise_risk_enforce_fields_and_recheck_pending() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    p.governance_rules.push(GovernanceRule {
        id: "security-review".into(),
        task_prefixes: vec!["A".into()],
        minimum_risk: Risk::Critical,
        mandatory_human: true,
        response_fields: vec![ResponseField {
            id: "reason".into(),
            label: "Security rationale".into(),
            required: true,
            multiline: true,
            choices: vec![],
        }],
        ..Default::default()
    });
    let r = question("rule");
    let d = s.submit(&plan, r.clone(), "agent:A", "agent", &p).unwrap();
    assert_eq!(d.effective_risk, Risk::Critical);
    assert_eq!(s.submit(&plan, r, "agent:A", "agent", &p).unwrap().id, d.id);
    assert!(s
        .answer(&d.id, d.version, "yes", None, "Owner", false, &p)
        .is_err());
    let input = AnswerInput { conditions:Vec::new(), artifacts:Vec::new(),
        text: "Reviewed".into(),
        fields: std::collections::BTreeMap::from([(
            "reason".into(),
            "Tested permission boundaries".into(),
        )]),
        ..Default::default()
    };
    s.answer_details(&d.id, d.version, &input, "Owner", &p)
        .unwrap();
    let mut other = question("threshold-change");
    other.task_id = "C".into();
    let pending = s.submit(&plan, other, "agent:C", "agent", &p).unwrap();
    p.human_question_min_risk = Consultation::Critical;
    s.reevaluate_pending(&plan, &p).unwrap();
    assert_eq!(s.decision(&pending.id).unwrap().status, "resolved");
    s.verify().unwrap();
}
#[test]
fn bounded_investigation_blocks_without_fabricating_missing_sources() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::NeverAsk;
    p.governance_rules.push(GovernanceRule {
        id: "source".into(),
        evidence_paths: vec!["missing-spec.md".into()],
        ..Default::default()
    });
    let d = s
        .submit(&plan, question("facts"), "agent:A", "agent", &p)
        .unwrap();
    assert_eq!(d.status, "investigating");
    for _ in 0..5 {
        s.reevaluate_pending(&plan, &p).unwrap();
    }
    let d = s.decision(&d.id).unwrap();
    assert_eq!(d.investigation.unwrap().rounds, 1);
    assert!(d.resolution.is_none());
    assert!(s
        .context(&s.plan(&plan.id).unwrap(), "C", "unrelated", "dev", &p)
        .is_ok());
    s.verify().unwrap();
}
#[test]
fn editor_patch_preserves_archive_and_versions_plan_requirements() {
    let (_tmp, mut s, plan, reg, p) = fixture();
    let original = s.context(&plan, "A", "before", "dev", &p).unwrap();
    let mut edited = plan.graph.clone();
    edited["product_requirements"] = json!(["Preserve compatibility"]);
    let patch = s
        .edit_patch(&plan, &edited, plan.graph_version, "Owner")
        .unwrap();
    let next = s.apply_patch(&plan.id, &patch, &reg, "Owner").unwrap();
    let fresh = s.context(&next, "A", "before", "dev", &p).unwrap();
    assert_ne!(original.context_digest, fresh.context_digest);
    assert!(fresh.render().contains("Preserve compatibility"));
    assert!(s.edit_patch(&next, &edited, 1, "Owner").is_err());
    edited["tasks_done"] = json!([{"id":"ARCHIVED"}]);
    assert!(s
        .edit_patch(&next, &edited, next.graph_version, "Owner")
        .is_err());
    s.verify().unwrap();
}
#[test]
fn unrelated_graph_versions_do_not_invalidate_semantic_context() {
    let (_tmp, mut s, plan, reg, p) = fixture();
    let a = s.context(&plan, "A", "same", "dev", &p).unwrap();
    let patch = patch(
        &plan,
        "unrelated",
        vec![Operation::UpdateTask {
            task_id: "C".into(),
            changes: BTreeMap::from([("title".into(), json!("Independent title"))]),
        }],
    );
    let next = s.apply_patch(&plan.id, &patch, &reg, "Owner").unwrap();
    let b = s.context(&next, "A", "same", "dev", &p).unwrap();
    assert_eq!(a.context_digest, b.context_digest);
    assert_ne!(a.graph_version, b.graph_version);
}
#[test]
fn operation_runner_persists_success_and_never_blindly_repeats_unknown() {
    let (tmp, mut s, plan, _, mut p) = fixture();
    // Keep runtime artifacts outside the reviewed source manifest.
    std::fs::write(tmp.path().join(".gitignore"), ".macc/\n").unwrap();
    git(tmp.path(), &["add", "prd.json", ".gitignore"]);
    git(tmp.path(), &["commit", "-m", "subject"]);
    let sha = git(tmp.path(), &["rev-parse", "HEAD"]);
    p.human_question_min_risk = Consultation::NeverAsk;
    p.operation_adapters.insert(
        "success".into(),
        OperationAdapter {
            command: vec!["/usr/bin/true".into()],
            allowed_targets: vec!["test".into()],
            ..Default::default()
        },
    );
    p.operation_adapters.insert(
        "ambiguous".into(),
        OperationAdapter {
            command: vec!["/usr/bin/false".into()],
            allowed_targets: vec!["test".into()],
            ..Default::default()
        },
    );
    configure(tmp.path(), p.clone()).unwrap();
    p.operation_adapters.insert(
        "bounded".into(),
        OperationAdapter {
            command: vec!["/usr/bin/yes".into()],
            allowed_targets: vec!["test".into()],
            max_output_bytes: 1024,
            ..Default::default()
        },
    );
    p.operation_adapters.insert(
        "timeout".into(),
        OperationAdapter {
            command: vec!["/usr/bin/sleep".into(), "2".into()],
            allowed_targets: vec!["test".into()],
            timeout_seconds: 1,
            ..Default::default()
        },
    );
    configure(tmp.path(), p.clone()).unwrap();
    for (id, adapter) in [
        ("once", "success"),
        ("uncertain", "ambiguous"),
        ("output-cap", "bounded"),
        ("time-cap", "timeout"),
    ] {
        let mut r = question(id);
        r.kind = "authorization".into();
        r.operation_id = Some(id.into());
        r.operation_spec = Some(OperationSpec {
            adapter: adapter.into(),
            target: "test".into(),
            source_revision: sha.clone(),
            arguments: Default::default(),
        });
        let d = s.submit(&plan, r, "agent:A", "agent", &p).unwrap();
        let op = s.execute_operation(&d.id).unwrap();
        if id == "output-cap" {
            assert!(
                std::fs::metadata(op.output_path.as_ref().unwrap())
                    .unwrap()
                    .len()
                    <= 1024
            );
        }
        if id == "once" {
            assert_eq!(op.state, OperationState::Succeeded);
            assert_eq!(s.execute_operation(&d.id).unwrap().version, op.version);
        } else {
            assert_eq!(op.state, OperationState::Unknown);
            assert!(s.execute_operation(&d.id).is_err());
            assert!(s
                .reconcile_operation(
                    &plan.id,
                    id,
                    op.version,
                    OperationState::Reserved,
                    "retry",
                    "Owner"
                )
                .is_err());
            assert!(s
                .reconcile_operation(
                    &plan.id,
                    id,
                    op.version + 1,
                    OperationState::Failed,
                    "Checked no effect",
                    "Owner"
                )
                .is_err());
            s.reconcile_operation(
                &plan.id,
                id,
                op.version,
                OperationState::Failed,
                "Checked target evidence: no effect",
                "Owner",
            )
            .unwrap();
        }
    }
    s.verify().unwrap();
    drop(s);
    let s = Store::open(tmp.path()).unwrap();
    assert_eq!(s.operations(&plan.id).unwrap().len(), 4);
}

fn signing_key(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let key = root.join("signing-key");
    let status = Command::new("/usr/bin/ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&key)
        .status()
        .unwrap();
    assert!(status.success());
    let public = std::fs::read_to_string(key.with_extension("pub")).unwrap();
    let signers = root.join("allowed_signers");
    std::fs::write(&signers, format!("operator@example.test {}", public)).unwrap();
    (key, signers)
}
#[test]
fn signed_transport_enrolls_empty_clone_and_rejects_tampering_or_rollback() {
    let (tmp, mut s, plan, reg, p) = fixture();
    std::fs::write(tmp.path().join(".gitignore"), ".macc/\n").unwrap();
    git(tmp.path(), &["add", "prd.json", ".gitignore"]);
    git(tmp.path(), &["commit", "-m", "subject"]);
    let keys = tempfile::tempdir().unwrap();
    let (key, signers) = signing_key(keys.path());
    let d = s
        .submit(&plan, question("transport"), "agent:A", "agent", &p)
        .unwrap();
    s.answer(
        &d.id,
        d.version,
        "Preserve compatibility",
        None,
        "Owner",
        false,
        &p,
    )
    .unwrap();
    let interrupted = OperationExecution {
        operation_id: "interrupted-transfer".into(),
        decision_id: d.id.clone(),
        plan_id: plan.id.clone(),
        version: 1,
        state: OperationState::Executing,
        spec_digest: "spec".into(),
        policy_digest: digest(&p),
        owner_identity: Some("2147483647:dead:1".into()),
        child_identity: None,
        evidence: "Crash injection before outcome".into(),
        output_digest: None,
        output_path: None,
        updated_at: now(),
    };
    let tx = s.db.transaction().unwrap();
    tx.execute(
        "INSERT INTO metadata(key,value) VALUES(?1,?2)",
        rusqlite::params![
            format!(
                "operation:{}",
                digest(&json!([plan.id, interrupted.operation_id]))
            ),
            serde_json::to_string(&interrupted).unwrap()
        ],
    )
    .unwrap();
    super::store::event(
        &tx,
        &plan.id,
        "operation.transition",
        &json!({"actor":"test fault injection","operation":interrupted}),
    )
    .unwrap();
    tx.commit().unwrap();
    let bundle = s.sign_archive(&key, "operator@example.test").unwrap();
    let object = s.publish_note(&bundle).unwrap();
    assert_eq!(s.publish_note(&bundle).unwrap(), object);
    assert_eq!(
        s.read_note(&object).unwrap().archive.project_id,
        s.project_id
    );
    let clone = tempfile::tempdir().unwrap();
    git(clone.path(), &["clone", tmp.path().to_str().unwrap(), "."]);
    std::fs::create_dir_all(clone.path().join(".macc")).unwrap();
    std::fs::copy(
        tmp.path().join(".macc/macc.yaml"),
        clone.path().join(".macc/macc.yaml"),
    )
    .unwrap();
    let mut imported = Store::open(clone.path()).unwrap();
    assert!(imported.import_archive(&bundle, &signers, false).is_err());
    imported.import_archive(&bundle, &signers, true).unwrap();
    assert_eq!(imported.project_id, s.project_id);
    assert_eq!(
        imported
            .operation(&plan.id, "interrupted-transfer")
            .unwrap()
            .unwrap()
            .state,
        OperationState::Unknown
    );
    assert_eq!(
        imported.decision(&d.id).unwrap().resolution.unwrap().text,
        "Preserve compatibility"
    );
    imported.verify().unwrap();
    let mut forged = bundle.clone();
    forged.archive.project_id = "forged".into();
    assert!(imported.import_archive(&forged, &signers, true).is_err());
    imported.verify().unwrap();
    let current = imported.plan(&plan.id).unwrap();
    let patch = patch(
        &current,
        "local-change",
        vec![Operation::AddTask {
            task: json!({"id":"LOCAL","dependencies":[]}),
        }],
    );
    imported
        .apply_patch(&current.id, &patch, &reg, "Owner")
        .unwrap();
    assert!(imported.import_archive(&bundle, &signers, false).is_err());
    assert!(super::graph::task(&imported.plan(&current.id).unwrap().graph, "LOCAL").is_ok());
    imported.verify().unwrap();
}
#[test]
fn signature_verifier_checks_exact_payload_principal_and_namespace() {
    let (tmp, s, _, _, _) = fixture();
    git(tmp.path(), &["add", "prd.json"]);
    git(tmp.path(), &["commit", "-m", "subject"]);
    let keys = tempfile::tempdir().unwrap();
    let (key, signers) = signing_key(keys.path());
    let bundle = s.sign_archive(&key, "operator@example.test").unwrap();
    let payload = super::secure::canonical_bytes(&bundle.archive).unwrap();
    assert!(super::secure::verify_signature(
        tmp.path(),
        &signers,
        "operator@example.test",
        "macc-decisions",
        &payload,
        &bundle.signature
    )
    .is_ok());
    assert!(super::secure::verify_signature(
        tmp.path(),
        &signers,
        "forged",
        "macc-decisions",
        &payload,
        &bundle.signature
    )
    .is_err());
    assert!(super::secure::verify_signature(
        tmp.path(),
        &signers,
        "operator@example.test",
        "macc-approval",
        &payload,
        &bundle.signature
    )
    .is_err());
    assert!(super::secure::verify_signature(
        tmp.path(),
        &signers,
        "operator@example.test",
        "macc-decisions",
        b"altered",
        &bundle.signature
    )
    .is_err());
}
#[test]
fn secure_mode_rejects_missing_or_same_principal_registry() {
    let (tmp, _, _, _, mut p) = fixture();
    p.trust_mode = "secure_approval".into();
    assert!(p.validate().is_err());
    p.trust_registry = Some(tmp.path().join("untrusted-registry.json"));
    std::fs::write(p.trust_registry.as_ref().unwrap(), "{}").unwrap();
    assert!(p.validate().unwrap_err().to_string().contains("isolated"));
}
#[test]
fn factual_investigation_requires_exact_source_evidence_then_resolves() {
    let (tmp, mut s, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::NeverAsk;
    let mut r = question("missing-fact");
    r.missing_facts = vec!["supported schema".into()];
    let d = s.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    assert_eq!(d.status, "investigating");
    let data = b"The supported schema is version 2.";
    std::fs::write(tmp.path().join("spec.md"), data).unwrap();
    let mut facts = BTreeMap::from([(
        "supported schema".into(),
        FactEvidence {
            source_path: "spec.md".into(),
            source_digest: evidence_digest(data),
            excerpt: "version 3".into(),
        },
    )]);
    assert!(s
        .investigate(&d.id, d.version, &facts, "agent:A", &p)
        .is_err());
    facts.get_mut("supported schema").unwrap().excerpt = "version 2".into();
    let resolved = s
        .investigate(&d.id, d.version, &facts, "agent:A", &p)
        .unwrap();
    assert_eq!(resolved.status, "resolved");
    assert!(resolved.request.missing_facts.is_empty());
    s.verify().unwrap();
}
#[test]
fn journal_detects_forged_operation_state_and_recovery_preserves_ambiguity() {
    let (_tmp, mut s, plan, _, _) = fixture();
    let op = OperationExecution {
        operation_id: "orphan".into(),
        decision_id: "unknown".into(),
        plan_id: plan.id.clone(),
        version: 1,
        state: OperationState::Executing,
        spec_digest: "spec".into(),
        policy_digest: "policy".into(),
        owner_identity: Some("2147483647:dead:1".into()),
        child_identity: None,
        evidence: String::new(),
        output_digest: None,
        output_path: None,
        updated_at: now(),
    };
    let tx = s.db.transaction().unwrap();
    tx.execute(
        "INSERT INTO metadata(key,value) VALUES(?1,?2)",
        rusqlite::params![
            format!("operation:{}", digest(&json!([plan.id, op.operation_id]))),
            serde_json::to_string(&op).unwrap()
        ],
    )
    .unwrap();
    super::store::event(
        &tx,
        &plan.id,
        "operation.transition",
        &json!({"actor":"test fault injection","operation":op}),
    )
    .unwrap();
    tx.commit().unwrap();
    s.verify().unwrap();
    assert_eq!(s.recover_operations().unwrap(), 1);
    assert_eq!(
        s.operation(&plan.id, "orphan").unwrap().unwrap().state,
        OperationState::Unknown
    );
    s.verify().unwrap();
    s.db.execute("UPDATE metadata SET value=replace(value,'unknown','succeeded') WHERE key LIKE 'operation:%'",[]).unwrap();
    assert!(s.verify().is_err());
}
#[test]
fn semantic_diff_reports_dependency_and_contract_changes_readably() {
    let before = json!({"requirements":["old"],"tasks":[{"id":"A","dependencies":[]}]});
    let after = json!({"requirements":["new"],"tasks":[{"id":"A","dependencies":["B"]},{"id":"B","dependencies":[]}]});
    let diff = graph_diff(&before, &after);
    assert_eq!(diff["tasks"].as_array().unwrap().len(), 2);
    assert_eq!(diff["tasks"][0]["dependencies_after"], json!(["B"]));
    assert_eq!(diff["plan_fields"], json!(["requirements"]));
}

#[test]
fn concurrent_performers_share_one_subject_authority() {
    let (tmp, mut s, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::NeverAsk;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut threads = Vec::new();
    for (task, choice) in [("A", "SQLite"), ("C", "Postgres")] {
        let root = tmp.path().to_path_buf();
        let plan = plan.clone();
        let p = p.clone();
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            let mut store = Store::open(&root).unwrap();
            let mut request = question(task);
            request.task_id = task.into();
            request.topic = Some("shared-contract".into());
            request.recommendation = choice.into();
            barrier.wait();
            store
                .submit(&plan, request, &format!("agent:{task}"), "agent", &p)
                .unwrap()
        }));
    }
    let a = threads.remove(0).join().unwrap();
    let b = threads.remove(0).join().unwrap();
    assert_eq!(
        a.resolution.as_ref().unwrap().text,
        b.resolution.as_ref().unwrap().text
    );
    assert!(a.reused_from.is_some() || b.reused_from.is_some());
    let mut replacement = question("replace-root");
    replacement.topic = Some("shared-contract".into());
    replacement.recommendation = "New versioned contract".into();
    let root = if a.reused_from.is_none() { &a } else { &b };
    replacement.supersedes = vec![DecisionRef {
        id: root.id.clone(),
        version: root.version,
    }];
    let new = s
        .submit(&plan, replacement.clone(), "agent:A", "agent", &p)
        .unwrap();
    assert_eq!(
        s.submit(&plan, replacement, "agent:A", "agent", &p)
            .unwrap()
            .id,
        new.id
    );
    assert_eq!(s.decision(&a.id).unwrap().status, "superseded");
    assert_eq!(s.decision(&b.id).unwrap().status, "superseded");
    s.verify().unwrap();
}
#[test]
fn prerequisite_contract_changes_invalidate_only_their_consumers() {
    let (_tmp, mut s, plan, reg, p) = fixture();
    let before = s.context(&plan, "B", "consumer", "review", &p).unwrap();
    let other = s.context(&plan, "C", "unrelated", "dev", &p).unwrap();
    let patch = patch(
        &plan,
        "dependency-change",
        vec![Operation::UpdateTask {
            task_id: "A".into(),
            changes: BTreeMap::from([(
                "acceptance_criteria".into(),
                json!(["Preserve public version 2"]),
            )]),
        }],
    );
    let next = s.apply_patch(&plan.id, &patch, &reg, "Owner").unwrap();
    assert_ne!(
        before.context_digest,
        s.context(&next, "B", "consumer", "review", &p)
            .unwrap()
            .context_digest
    );
    assert_eq!(
        other.context_digest,
        s.context(&next, "C", "unrelated", "dev", &p)
            .unwrap()
            .context_digest
    );
}

#[test]
fn revoking_authority_also_revokes_derived_decision_aliases() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::NeverAsk;
    let mut first = question("revocation-source");
    first.topic = Some("contract".into());
    let root = s.submit(&plan, first, "agent:A", "agent", &p).unwrap();
    let mut second = question("revocation-alias");
    second.topic = Some("contract".into());
    second.task_id = "C".into();
    let alias = s.submit(&plan, second, "agent:C", "agent", &p).unwrap();
    assert_eq!(alias.reused_from.as_ref().unwrap().id, root.id);
    s.revoke(
        &root.id,
        root.version,
        "Owner",
        "Withdraw the contract direction",
    )
    .unwrap();
    assert_eq!(s.decision(&alias.id).unwrap().status, "revoked");
    let context = s
        .context(&s.plan(&plan.id).unwrap(), "C", "after-revoke", "dev", &p)
        .unwrap();
    assert!(context
        .bindings
        .iter()
        .all(|d| d.id != root.id && d.id != alias.id));
    s.verify().unwrap();
}
#[test]
fn ordinary_guidance_cannot_supply_a_fresh_operation_authorization() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::NeverAsk;
    let mut r = question("choice");
    r.topic = Some("release".into());
    s.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    p.human_question_min_risk = Consultation::High;
    let mut auth = question("new-external-operation");
    auth.topic = Some("release".into());
    auth.kind = "authorization".into();
    auth.operation_id = Some("fresh-effect".into());
    let d = s.submit(&plan, auth, "agent:A", "agent", &p).unwrap();
    assert_eq!(d.status, "pending_human");
    assert!(d.reused_from.is_none());
    assert!(s.consume_authorization(&d.id, "fresh-effect").is_err());
}

#[test]
fn editing_another_plan_does_not_inherit_colliding_task_runtime_history() {
    let (tmp, mut s, plan, mut registry, _p) = fixture();
    registry
        .extra
        .insert("decision_plan_id".into(), json!(plan.id));
    registry
        .find_task_mut("A")
        .unwrap()
        .set_workflow_state(crate::coordinator::WorkflowState::Merged);
    std::fs::write(
        tmp.path().join("other.json"),
        serde_json::to_vec(&plan.graph).unwrap(),
    )
    .unwrap();
    let other = s
        .ensure_plan(tmp.path(), Path::new("other.json"), "main", &registry)
        .unwrap();
    let change = vec![Operation::UpdateTask {
        task_id: "A".into(),
        changes: BTreeMap::from([("title".into(), json!("Independent plan contract"))]),
    }];
    let updated = s
        .apply_patch(
            &other.id,
            &patch(&other, "other-contract", change.clone()),
            &registry,
            "Owner",
        )
        .unwrap();
    assert_eq!(
        updated.graph["tasks"][0]["title"],
        "Independent plan contract"
    );
    assert!(s
        .apply_patch(
            &plan.id,
            &patch(&plan, "delivered-contract", change),
            &registry,
            "Owner"
        )
        .is_err());
    s.verify().unwrap();
}

#[test]
fn wildcard_governance_cannot_be_bypassed_by_low_reported_risk() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::Critical;
    p.governance_rules.push(GovernanceRule {
        id: "sensitive-files".into(),
        affected_paths: vec!["**".into()],
        minimum_risk: Risk::Critical,
        mandatory_human: true,
        ..Default::default()
    });
    let mut request = question("wildcard-risk");
    request.risk = Risk::Low;
    request.impact.shared_contract = false;
    request.affected_paths = vec!["src/security/session.rs".into()];
    let d = s.submit(&plan, request, "agent:A", "agent", &p).unwrap();
    assert_eq!(d.effective_risk, Risk::Critical);
    assert_eq!(d.status, "pending_human");
}

#[path = "tests_workflows.rs"]
mod workflows;

#[path = "tests_interaction.rs"]
mod interaction_tests;

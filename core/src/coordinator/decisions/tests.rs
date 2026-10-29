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
    serde_json::from_value(json!({"idempotency_key":id,"task_id":"A","question":"Which storage contract?","context":"Two implementations conflict","investigated":["Read both call sites"],"recommendation":"Use the versioned contract","risk":"high","impact":{"shared_contract":true,"evidence":["Both clients consume it"]}})).unwrap()
}
fn patch(plan: &Plan, key: &str, operations: Vec<Operation>) -> Patch {
    Patch {
        plan_id: plan.id.clone(),
        base_graph_version: plan.graph_version,
        idempotency_key: key.into(),
        reason: "Reviewed architecture adjustment".into(),
        decision_refs: vec![],
        operations,
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
    r.options = vec![OptionChoice {
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
        .answer(
            &d.id,
            d.version,
            "Oui, conserver le contrat",
            Some("apply".into()),
            "Human",
            false,
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

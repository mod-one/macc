#![cfg(test)]
use super::*;
fn physical_request(key: &str) -> Request {
    let mut r = question(key);
    r.kind = "intervention".into();
    r.impact.shared_contract = false;
    r.impact.evidence = vec!["USB hardware is not accessible in this worker".into()];
    r.risk = Risk::Low;
    r.intervention = Some(UserIntervention {
        action: "Connect the phone by USB and unlock its screen".into(),
        inaccessible_reason: "Worker cannot connect physical hardware".into(),
        expected_result: "Paste the observed device identifier or exact error".into(),
        verification: "Run adb devices locally; omit credentials".into(),
    });
    r
}
#[test]
fn performer_can_request_essential_physical_action_and_receives_exact_human_result() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::Critical;
    let d = s
        .submit(&plan, physical_request("phone"), "agent:A", "agent", &p)
        .unwrap();
    assert_eq!(d.status, "pending_human");
    assert!(d.resolution.is_none());
    assert!(s.context(&plan, "A", "wait", "dev", &p).is_err());
    assert!(s.context(&plan, "B", "dependent", "dev", &p).is_err());
    assert!(s.context(&plan, "C", "independent", "dev", &p).is_ok());
    let mut input = AnswerInput {
        text: "Connected and unlocked. Output follows.".into(),
        ..Default::default()
    };
    assert!(s
        .answer_details(&d.id, d.version, &input, "Human", &p)
        .is_err());
    input.fields.insert("performed".into(), "yes".into());
    input.fields.insert(
        "result".into(),
        "List of devices attached\nABC-é\tunauthorized".into(),
    );
    let answered = s
        .answer_details(&d.id, d.version, &input, "Human", &p)
        .unwrap();
    assert_eq!(answered.status, "resolved");
    let context = s
        .context(&s.plan(&plan.id).unwrap(), "A", "resume", "dev", &p)
        .unwrap();
    assert!(context.render().contains("ABC-é"));
    assert!(context.render().contains("unauthorized"));
    assert!(context.render().contains("kind intervention"));
    s.verify().unwrap();
}
#[test]
fn never_ask_does_not_fabricate_a_physical_action_and_can_return_to_consultation() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::NeverAsk;
    let d = s
        .submit(
            &plan,
            physical_request("capability"),
            "agent:A",
            "agent",
            &p,
        )
        .unwrap();
    assert_eq!(d.status, "blocked_external");
    assert!(s.resolve_delegated(&d.id, &p).is_err());
    s.reevaluate_pending(&plan, &p).unwrap();
    assert_eq!(s.decision(&d.id).unwrap().status, "blocked_external");
    assert!(s.context(&plan, "C", "free", "dev", &p).is_ok());
    p.human_question_min_risk = Consultation::Critical;
    s.reevaluate_pending(&plan, &p).unwrap();
    assert_eq!(s.decision(&d.id).unwrap().status, "pending_human");
    s.verify().unwrap();
}
#[test]
fn human_rejection_of_unavailable_intervention_never_becomes_a_result() {
    let (_tmp, mut s, plan, _, p) = fixture();
    let d = s
        .submit(
            &plan,
            physical_request("cannot-connect"),
            "agent:A",
            "agent",
            &p,
        )
        .unwrap();
    let input = AnswerInput {
        text: "I do not have the phone today".into(),
        outcome: AnswerOutcome::Reject,
        ..Default::default()
    };
    let d = s
        .answer_details(&d.id, d.version, &input, "Human", &p)
        .unwrap();
    assert_eq!(d.status, "rejected");
    assert!(s.context(&plan, "A", "blocked", "dev", &p).is_err());
    s.verify().unwrap();
}
#[test]
fn conditional_answer_blocks_until_checked_evidence_and_changed_evidence_fences_again() {
    let (tmp, mut s, plan, _, p) = fixture();
    let d = s
        .submit(&plan, question("condition"), "agent:A", "agent", &p)
        .unwrap();
    let data = b"rollback verified\n";
    let c = ApprovalCondition {
        id: "rollback".into(),
        description: "Rollback test result must match".into(),
        task_id: None,
        source_path: Some("rollback-result.txt".into()),
        source_digest: Some(evidence_digest(data)),
    };
    let input = AnswerInput {
        text: "Approve only after rollback evidence".into(),
        conditions: vec![c],
        ..Default::default()
    };
    let d = s
        .answer_details(&d.id, d.version, &input, "Human", &p)
        .unwrap();
    assert_eq!(d.status, "conditional");
    assert!(s.context(&plan, "B", "dependent", "dev", &p).is_err());
    assert!(s.context(&plan, "C", "free", "dev", &p).is_ok());
    assert_eq!(s.refresh_conditions(&plan, &p).unwrap(), 0);
    std::fs::write(tmp.path().join("rollback-result.txt"), data).unwrap();
    assert_eq!(s.refresh_conditions(&plan, &p).unwrap(), 1);
    assert_eq!(s.decision(&d.id).unwrap().status, "resolved");
    assert!(s.context(&plan, "A", "ready", "dev", &p).is_ok());
    std::fs::write(tmp.path().join("rollback-result.txt"), "changed").unwrap();
    assert!(s.context(&plan, "A", "stale", "dev", &p).is_err());
    s.verify().unwrap();
}
#[test]
fn evidence_is_exact_retained_and_never_disclosed_without_explicit_operator_consent() {
    let (tmp, mut s, plan, _, p) = fixture();
    let bytes = b"device id ABC; no secret";
    let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let a = s
        .add_artifact(&plan.id, "device-output.txt", "text/plain", &hex, "Human")
        .unwrap();
    assert_eq!(a.digest, evidence_digest(bytes));
    assert!(!s.can_deliver_evidence(&a.id).unwrap());
    let d = s
        .submit(&plan, question("evidence"), "agent:A", "agent", &p)
        .unwrap();
    let input = AnswerInput {
        text: "Keep the original device contract".into(),
        artifacts: vec![ArtifactRef {
            id: a.id.clone(),
            digest: a.digest.clone(),
        }],
        ..Default::default()
    };
    s.answer_details(&d.id, d.version, &input, "Human", &p)
        .unwrap();
    let private = s.context(&plan, "A", "private", "dev", &p).unwrap();
    assert!(private.artifacts.is_empty());
    assert!(!private.render().contains("device id ABC"));
    s.share_evidence(&a.id, 0, true, "Human", None).unwrap();
    let released = s.context(&plan, "A", "shared", "dev", &p).unwrap();
    assert!(released.render().contains("device id ABC"));
    assert_ne!(private.context_digest, released.context_digest);
    s.share_evidence(&a.id, 1, false, "Human", None).unwrap();
    let revoked = s.context(&plan, "A", "revoked", "dev", &p).unwrap();
    assert!(revoked.artifacts.is_empty());
    assert_ne!(released.context_digest, revoked.context_digest);
    let location = s.path.clone();
    drop(s);
    crate::clear(&crate::ProjectPaths::from_root(tmp.path())).unwrap();
    let s = Store::open(tmp.path()).unwrap();
    assert_eq!(s.path, location);
    assert_eq!(s.artifact(&a.id).unwrap().data_hex, hex);
    s.verify().unwrap();
}
#[test]
fn forged_or_foreign_evidence_and_oversized_files_are_rejected() {
    let (_tmp, mut s, plan, _, p) = fixture();
    assert!(s
        .add_artifact(
            &plan.id,
            "large",
            "text/plain",
            &"ab".repeat(p.max_request_bytes + 1),
            "Human"
        )
        .is_err());
    let a = s
        .add_artifact(&plan.id, "small", "text/plain", "6869", "Human")
        .unwrap();
    let d = s
        .submit(&plan, question("forged-ref"), "agent:A", "agent", &p)
        .unwrap();
    let input = AnswerInput {
        text: "Answer".into(),
        artifacts: vec![ArtifactRef {
            id: a.id,
            digest: "sha256:forged".into(),
        }],
        ..Default::default()
    };
    assert!(s
        .answer_details(&d.id, d.version, &input, "Human", &p)
        .is_err());
    assert!(s.share_evidence("unknown", 0, true, "Human", None).is_err());
    s.verify().unwrap();
}
#[test]
fn investigation_is_read_only_bounded_and_returns_report_without_inferred_consent() {
    let (tmp, mut s, plan, _, p) = fixture();
    std::fs::write(tmp.path().join(".gitignore"), ".macc/\n").unwrap();
    git(tmp.path(), &["add", "prd.json", ".gitignore"]);
    git(tmp.path(), &["commit", "-m", "baseline"]);
    let d = s
        .submit(&plan, question("investigate-first"), "agent:A", "agent", &p)
        .unwrap();
    let input = AnswerInput {
        text: "Compare both client implementations before I decide".into(),
        outcome: AnswerOutcome::Investigate,
        ..Default::default()
    };
    let d = s
        .answer_details(&d.id, d.version, &input, "Human", &p)
        .unwrap();
    assert_eq!(d.status, "investigating_requested");
    let (c, _) = s
        .issue(&plan, "A", "investigation", "dev", tmp.path(), &p)
        .unwrap();
    assert_eq!(c.planning.as_ref().unwrap().purpose, "investigation");
    assert!(c.render().contains("INVESTIGATION ONLY"));
    assert!(s
        .complete_investigation(
            &d.id,
            d.version,
            "Read both clients; both support SQLite",
            &[],
            "investigation",
            "dev"
        )
        .is_err());
    s.issued(&plan.id, "investigation", "dev", &c.render(), &p)
        .unwrap();
    let reported = s
        .complete_investigation(
            &d.id,
            d.version,
            "Read both clients; both support SQLite",
            &[],
            "investigation",
            "dev",
        )
        .unwrap();
    assert_eq!(reported.status, "pending_human");
    assert!(reported.resolution.is_none());
    assert!(reported
        .request
        .context
        .contains("Compare both client implementations"));
    assert!(reported.request.context.contains("both support SQLite"));
    assert!(s
        .complete(
            &plan.id,
            "investigation",
            "dev",
            &format!("MACC_DECISION_ACK: {}", c.context_digest),
            &p
        )
        .is_err());
    s.verify().unwrap();
}
#[test]
fn conditional_gate_never_counts_as_a_vote_until_a_real_verification_task_is_delivered() {
    let (tmp, mut s, plan, p, sha) = gate_fixture();
    let conditions = vec![ApprovalCondition {
        id: "verify-c".into(),
        description: "Independent task C must be delivered".into(),
        task_id: Some("C".into()),
        source_path: None,
        source_digest: None,
    }];
    let record = vote(&sha);
    s.record_gate_conditions_attested(tmp.path(), &plan, "APP", &record, &conditions, None)
        .unwrap();
    assert!(!s
        .verified_approvals(tmp.path(), &plan)
        .unwrap()
        .contains_key("APP"));
    assert_ne!(
        approval_challenge(&plan, "APP", &record, &p).unwrap(),
        super::super::approvals::approval_conditions_challenge(
            &plan,
            "APP",
            &record,
            &conditions,
            &p
        )
        .unwrap()
    );
    s.verify().unwrap();
}

#[test]
fn intervention_missing_external_facts_still_routes_to_the_person_not_an_ai_retry_loop() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    p.human_question_min_risk = Consultation::Critical;
    let mut r = physical_request("missing-device-id");
    r.missing_facts = vec!["Actual connected device identifier".into()];
    let d = s.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    assert_eq!(d.status, "pending_human");
    s.reevaluate_pending(&plan, &p).unwrap();
    assert_eq!(s.decision(&d.id).unwrap().status, "pending_human");
}
#[test]
fn local_semantic_suggestions_have_explicit_compute_budgets_and_zero_remote_calls() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    p.similarity_hints = true;
    p.similarity_backend = "local_lsa".into();
    p.similarity_min_percent = 1;
    for (i, text) in [
        "Public API compatibility for both storage clients",
        "Storage client compatibility contract for API consumers",
        "Independent display colours and accessible contrast",
    ]
    .iter()
    .enumerate()
    {
        let mut r = question(&format!("semantic-{i}"));
        r.question = (*text).into();
        s.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    }
    let hints = s.similar_requests(&plan, &p).unwrap();
    assert_eq!(hints["authority"], false);
    assert_eq!(hints["paid_model_calls"], 0);
    assert!(hints["comparisons"].as_u64().unwrap() <= 8192);
    assert!(hints["suggestions"].as_array().unwrap().len() <= 32);
    assert!(s
        .decisions(&plan.id)
        .unwrap()
        .iter()
        .all(|d| d.resolution.is_none()));
    p.similarity_backend = "remote_untrusted".into();
    assert!(p.validate().is_err());
}
#[test]
fn conditional_gate_enters_quorum_only_after_current_verified_delivery() {
    let (tmp, mut s, plan, _, sha) = gate_fixture();
    let conditions = vec![ApprovalCondition {
        id: "verification".into(),
        description: "Deliver independent verification C".into(),
        task_id: Some("C".into()),
        source_path: None,
        source_digest: None,
    }];
    s.record_gate_conditions_attested(tmp.path(), &plan, "APP", &vote(&sha), &conditions, None)
        .unwrap();
    assert!(!s
        .verified_approvals(tmp.path(), &plan)
        .unwrap()
        .contains_key("APP"));
    let mut registry = TaskRegistry::from_value(&json!({"tasks":plan.graph["tasks"]})).unwrap();
    registry.find_task_mut("C").unwrap().state = "merged".into();
    registry
        .extra
        .insert("decision_plan_id".into(), json!(plan.id));
    crate::coordinator::state::coordinator_state_registry_save(
        tmp.path(),
        &Default::default(),
        &registry.to_value().unwrap(),
    )
    .unwrap();
    assert_eq!(
        s.verified_approvals(tmp.path(), &plan).unwrap()["APP"].len(),
        1
    );
    s.verify().unwrap();
}

#[test]
fn twenty_real_processes_use_native_scheduler_while_only_hardware_subtree_is_blocked() {
    use crate::coordinator::{
        task_selector::{select_next_ready_task_typed, TaskSelectorConfig},
        WorkflowState,
    };
    use std::io::Write;
    let (tmp, mut s, plan, _, p) = fixture();
    let mut graph = plan.graph.clone();
    for task in graph["tasks"].as_array_mut().unwrap() {
        task["priority"] = json!("1");
        task["state"] = json!("todo");
    }
    for i in 0..20 {
        graph["tasks"].as_array_mut().unwrap().push(json!({"id":format!("W{i}"),"priority":"1","dependencies":if i==1{vec!["W0"]}else{vec![]}}));
    }
    let edit = s
        .edit_patch(&plan, &graph, plan.graph_version, "Human")
        .unwrap();
    let mut registry = TaskRegistry::from_value(&json!({"tasks":graph["tasks"]})).unwrap();
    let plan = s.apply_patch(&plan.id, &edit, &registry, "Human").unwrap();
    s.project(tmp.path()).unwrap();
    let mut r = physical_request("hardware-subtree");
    r.task_id = "W0".into();
    let pending = s.submit(&plan, r, "agent:W0", "agent", &p).unwrap();
    super::super::sync(tmp.path(), Path::new("prd.json"), &mut registry).unwrap();
    let config = TaskSelectorConfig {
        enabled_tools: vec!["fake".into()],
        default_tool: "fake".into(),
        default_base_branch: "main".into(),
        max_parallel: 20,
        max_parallel_per_tool: std::collections::HashMap::from([("fake".into(), 20)]),
        ..Default::default()
    };
    let mut children = Vec::new();
    while let Some(selected) = select_next_ready_task_typed(&registry, &config) {
        assert_ne!(selected.id, "W0");
        assert_ne!(selected.id, "W1");
        let attempt = format!("native-{}", selected.id);
        let (context, _) = s
            .issue(&plan, &selected.id, &attempt, "dev", tmp.path(), &p)
            .unwrap();
        s.issued(&plan.id, &attempt, "dev", &context.render(), &p)
            .unwrap();
        let child = Command::new("/usr/bin/dash")
            .args([
                "-c",
                "cat >/dev/null; printf 'MACC_DECISION_ACK: %s\\n' \"$FIXTURE_ACK\"",
            ])
            .env("FIXTURE_ACK", &context.context_digest)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let task = registry.find_task_mut(&selected.id).unwrap();
        task.set_workflow_state(WorkflowState::Claimed);
        task.tool = Some("fake".into());
        task.task_runtime.pid = Some(child.id() as i64);
        children.push((child, context));
    }
    assert_eq!(
        children.len(),
        20,
        "Unrelated native scheduling should fill all twenty slots"
    );
    assert!(select_next_ready_task_typed(&registry, &config).is_none());
    for (mut child, context) in children {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(context.render().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        s.complete(
            &plan.id,
            &context.attempt_id,
            "dev",
            &String::from_utf8_lossy(&output.stdout),
            &p,
        )
        .unwrap();
    }
    assert_eq!(s.decision(&pending.id).unwrap().status, "pending_human");
    assert_eq!(
        registry.find_task("W0").unwrap().workflow_state(),
        Some(WorkflowState::Blocked)
    );
    assert_eq!(
        registry.find_task("W1").unwrap().workflow_state(),
        Some(WorkflowState::Blocked)
    );
    s.verify().unwrap();
}
#[test]
fn sqlite_writer_process_fixture() {
    use std::io::Read;
    let Some(root) = std::env::var_os("MACC_DM_WRITER_FIXTURE") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let mut s = Store::open(&root).unwrap();
    s.db.execute_batch("PRAGMA wal_autocheckpoint=0").unwrap();
    let plan = s.plans().unwrap().remove(0);
    let change = patch(
        &plan,
        "writer-wal-graph",
        vec![Operation::AddTask {
            task: json!({"id":"WAL_TASK","dependencies":["A"]}),
        }],
    );
    s.apply_patch(
        &plan.id,
        &change,
        &TaskRegistry::default(),
        "Controlled writer",
    )
    .unwrap();
    let _lock = s.mutation_lock().unwrap();
    let tx =
        s.db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
    super::super::store::event(
        &tx,
        &plan.id,
        "fixture.writer.active",
        &json!({"phase":"clear/backup race"}),
    )
    .unwrap();
    std::fs::write(root.join("writer-ready"), "ready").unwrap();
    let mut byte = [0];
    std::io::stdin().read_exact(&mut byte).unwrap();
    tx.commit().unwrap();
    std::process::exit(86);
}
#[test]
fn actual_clear_refuses_a_concurrent_authority_writer_and_preserves_its_commit() {
    use std::io::Write;
    let (tmp, s, plan, _, _) = fixture();
    let paths = crate::ProjectPaths::from_root(tmp.path());
    crate::write_if_changed(
        &paths,
        "generated.md",
        &tmp.path().join("generated.md"),
        b"owned generated output",
        |_| Ok(()),
    )
    .unwrap();
    std::fs::write(tmp.path().join("dirty-user-work.txt"), "must survive").unwrap();
    crate::save_managed_paths(
        &paths,
        &std::collections::BTreeSet::from(["generated.md".into()]),
    )
    .unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "coordinator::decisions::tests::interaction_tests::sqlite_writer_process_fixture",
        ])
        .env("MACC_DM_WRITER_FIXTURE", tmp.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..200 {
        if tmp.path().join("writer-ready").exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(tmp.path().join("writer-ready").exists());
    let backup = tempfile::tempdir().unwrap();
    let root = tmp.path().to_owned();
    let destination = backup.path().to_owned();
    let capture = std::thread::spawn(move || capture_backup(&root, &destination));
    assert!(crate::clear(&paths).is_err());
    assert!(tmp.path().join("generated.md").exists());
    assert!(s.path.exists());
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("dirty-user-work.txt")).unwrap(),
        "must survive"
    );
    child.stdin.take().unwrap().write_all(b"c").unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(86));
    let meta = capture.join().unwrap().unwrap().unwrap();
    validate_backup(tmp.path(), &backup.path().join(&meta.database_path), &meta).unwrap();
    let cleared = crate::clear(&paths).unwrap();
    assert!(cleared.removed > 0);
    assert!(!tmp.path().join("generated.md").exists());
    drop(s);
    let mut recovered = Store::open(tmp.path()).unwrap();
    assert!(
        super::super::graph::task(&recovered.plan(&plan.id).unwrap().graph, "WAL_TASK").is_ok()
    );
    recovered.project(tmp.path()).unwrap();
    recovered.verify().unwrap();
}

#[test]
fn revoked_evidence_disclosure_cannot_be_rolled_back_to_an_earlier_valid_permission() {
    let (tmp, mut s, plan, _, _) = fixture();
    let a = s
        .add_artifact(&plan.id, "redacted result", "text/plain", "6869", "Human")
        .unwrap();
    s.share_evidence(&a.id, 0, true, "Human", None).unwrap();
    let old: String =
        s.db.query_row(
            "SELECT value FROM metadata WHERE key=?1",
            [format!("evidence-permission:{}", a.id)],
            |r| r.get(0),
        )
        .unwrap();
    s.share_evidence(&a.id, 1, false, "Human", None).unwrap();
    s.db.execute(
        "UPDATE metadata SET value=?2 WHERE key=?1",
        rusqlite::params![format!("evidence-permission:{}", a.id), old],
    )
    .unwrap();
    assert!(s.can_deliver_evidence(&a.id).is_err());
    assert!(s.verify().is_err());
    drop(s);
    assert!(Store::open(tmp.path()).is_err());
}

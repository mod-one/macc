//! Lifecycle, effect planning and independent approval integration contracts.
use super::*;

fn answer_input(text: &str) -> AnswerInput {
    serde_json::from_value(json!({"text":text})).unwrap()
}
fn commit_fixture(root: &Path) {
    std::fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
    git(root, &["add", ".gitignore", "prd.json"]);
    git(root, &["commit", "-m", "initial fixture"]);
}
#[test]
fn alternative_option_b_applies_its_own_patch_and_exact_negative_constraint() {
    let (_tmp, mut s, plan, _, p) = fixture();
    let mut r = question("alternative");
    let a = patch(
        &plan,
        "choice-a",
        vec![Operation::AddTask {
            task: json!({"id":"OPTION_A","dependencies":["A"]}),
        }],
    );
    let b = patch(
        &plan,
        "choice-b",
        vec![Operation::AddTask {
            task: json!({"id":"OPTION_B","dependencies":["A"]}),
        }],
    );
    r.options=serde_json::from_value(json!([{"id":"a","label":"Recommended","proposed_patch":a},{"id":"b","label":"Alternative","proposed_patch":b}])).unwrap();
    r.recommended_option_id = Some("a".into());
    let d = s.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    let mut input = answer_input("Choisir B. Ne pas modifier le contrat public.");
    input.option = Some("b".into());
    input.apply_patch = true;
    let d = s
        .answer_details(&d.id, d.version, &input, "Human", &p)
        .unwrap();
    let current = s.plan(&plan.id).unwrap();
    assert_eq!(d.status, "resolved");
    assert!(super::super::graph::task(&current.graph, "OPTION_B").is_ok());
    assert!(super::super::graph::task(&current.graph, "OPTION_A").is_err());
    let c = s
        .context(&current, "OPTION_B", "attempt", "dev", &p)
        .unwrap();
    assert!(c.render().contains("Ne pas modifier le contrat public."));
    s.verify().unwrap();
}
#[test]
fn custom_answer_waits_for_effects_new_tasks_inherit_and_planning_never_completes_implementation() {
    let (tmp, mut s, plan, _, p) = fixture();
    commit_fixture(tmp.path());
    let d = s
        .submit(&plan, question("custom-replan"), "agent:A", "agent", &p)
        .unwrap();
    let mut input = answer_input("Adopter B, conserver les anciennes données.");
    input.effects = AnswerEffects::Replan;
    let waiting = s
        .answer_details(&d.id, d.version, &input, "Human", &p)
        .unwrap();
    assert_eq!(waiting.status, "awaiting_effects");
    assert!(s.context(&plan, "B", "dependent", "dev", &p).is_err());
    assert!(s.context(&plan, "C", "independent", "dev", &p).is_ok());
    let (context, _) = s
        .issue(&plan, "A", "planning", "dev", tmp.path(), &p)
        .unwrap();
    assert!(context.planning.is_some());
    assert!(context.render().contains("PLANNING ONLY"));
    let effects = patch(
        &plan,
        "planned-effects",
        vec![Operation::AddTask {
            task: json!({"id":"NEW","dependencies":[]}),
        }],
    );
    assert!(s
        .propose_effects(&d.id, waiting.version, &effects, "planning", "dev")
        .is_err());
    s.issued(&plan.id, "planning", "dev", &context.render(), &p)
        .unwrap();
    let applied = s
        .propose_effects(&d.id, waiting.version, &effects, "planning", "dev")
        .unwrap();
    assert_eq!(applied.status, "resolved");
    assert_eq!(
        applied.resolution.as_ref().unwrap().answered_version,
        Some(d.version)
    );
    assert_eq!(
        s.propose_effects(&d.id, waiting.version, &effects, "planning", "dev")
            .unwrap(),
        applied
    );
    assert!(s
        .complete(
            &plan.id,
            "planning",
            "dev",
            &format!("MACC_DECISION_ACK: {}", context.context_digest),
            &p
        )
        .is_err());
    let new_plan = s.plan(&plan.id).unwrap();
    let c = s
        .context(&new_plan, "NEW", "implementation", "dev", &p)
        .unwrap();
    assert!(c.render().contains("conserver les anciennes données"));
    assert!(c.planning.is_none());
    s.verify().unwrap();
}
#[test]
fn planning_rejects_retained_source_edits_and_unrelated_graph_mutations() {
    let (tmp, mut s, plan, _, p) = fixture();
    commit_fixture(tmp.path());
    let d = s
        .submit(&plan, question("read-only-plan"), "agent:A", "agent", &p)
        .unwrap();
    let mut input = answer_input("Replanifier");
    input.effects = AnswerEffects::Replan;
    let d = s
        .answer_details(&d.id, d.version, &input, "Human", &p)
        .unwrap();
    let (c, _) = s
        .issue(&plan, "A", "planning", "dev", tmp.path(), &p)
        .unwrap();
    s.issued(&plan.id, "planning", "dev", &c.render(), &p)
        .unwrap();
    let unrelated = patch(
        &plan,
        "unrelated",
        vec![Operation::UpdateTask {
            task_id: "C".into(),
            changes: BTreeMap::from([("title".into(), json!("silently changed"))]),
        }],
    );
    assert!(s
        .propose_effects(&d.id, d.version, &unrelated, "planning", "dev")
        .unwrap_err()
        .to_string()
        .contains("unrelated"));
    std::fs::write(tmp.path().join("retained.txt"), "must survive").unwrap();
    let effects = patch(
        &plan,
        "new-task",
        vec![Operation::AddTask {
            task: json!({"id":"D","dependencies":[]}),
        }],
    );
    assert!(s
        .propose_effects(&d.id, d.version, &effects, "planning", "dev")
        .unwrap_err()
        .to_string()
        .contains("source or Git"));
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("retained.txt")).unwrap(),
        "must survive"
    );
    assert_eq!(s.decision(&d.id).unwrap().status, "awaiting_effects");
}
#[test]
fn explicit_no_graph_change_resolves_without_inferred_patch_acceptance() {
    let (_tmp, mut s, plan, _, p) = fixture();
    let mut r = question("no-change");
    r.proposed_patch = Some(patch(
        &plan,
        "not-accepted",
        vec![Operation::AddTask {
            task: json!({"id":"D","dependencies":[]}),
        }],
    ));
    let d = s.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    let mut input = answer_input("Seulement une contrainte de conception");
    input.effects = AnswerEffects::NoGraphChange;
    let d = s
        .answer_details(&d.id, d.version, &input, "Human", &p)
        .unwrap();
    assert_eq!(d.status, "resolved");
    assert_eq!(s.plan(&plan.id).unwrap().graph_version, 1);
    s.verify().unwrap();
}
#[test]
fn rejection_revision_withdrawal_and_revocation_are_versioned_and_idempotent() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    let d = s
        .submit(&plan, question("first"), "agent:A", "agent", &p)
        .unwrap();
    let mut input = answer_input("Revoir le contrat");
    input.outcome = AnswerOutcome::RequestChanges;
    let rejected = s
        .answer_details(&d.id, d.version, &input, "Human", &p)
        .unwrap();
    p.human_question_min_risk = Consultation::NeverAsk;
    let mut next = question("revision");
    next.question = "Le contrat révisé convient-il ?".into();
    let revision = s
        .revise_request(
            &d.id,
            rejected.version,
            next.clone(),
            "agent:A",
            "agent",
            &p,
            None,
        )
        .unwrap();
    assert_eq!(revision.status, "investigating");
    assert!(revision.requires_human_renewal);
    assert!(revision.resolution.is_none());
    assert_eq!(
        s.revise_request(&d.id, rejected.version, next, "agent:A", "agent", &p, None)
            .unwrap(),
        revision
    );
    assert_eq!(s.decision(&d.id).unwrap().status, "superseded");
    let withdrawn = s
        .withdraw_request(&revision.id, revision.version, "Redondant", "Human", None)
        .unwrap();
    assert_eq!(withdrawn.status, "withdrawn");
    assert_eq!(
        s.withdraw_request(&revision.id, revision.version, "Redondant", "Human", None)
            .unwrap(),
        withdrawn
    );
    assert!(s
        .withdraw_request(
            &revision.id,
            revision.version,
            "Different reason",
            "Human",
            None
        )
        .is_err());
    let revoked = s
        .revoke(&d.id, rejected.version + 1, "Human", "Obsolete")
        .unwrap();
    assert_eq!(
        s.revoke(&d.id, rejected.version + 1, "Human", "Obsolete")
            .unwrap(),
        revoked
    );
    s.verify().unwrap();
}
#[test]
fn expired_requests_and_bounded_planning_remain_scoped_prerequisites() {
    let (tmp, mut s, plan, _, mut p) = fixture();
    commit_fixture(tmp.path());
    let mut r = question("expired");
    r.expires_at = Some("2000-01-01T00:00:00Z".into());
    let expired = s.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    assert_eq!(expired.status, "expired");
    assert!(s
        .answer(
            &expired.id,
            expired.version,
            "too late",
            None,
            "Human",
            false,
            &p
        )
        .is_err());
    s.withdraw_request(&expired.id, expired.version, "Replace", "Human", None)
        .unwrap();
    let d = s
        .submit(&plan, question("planning-budget"), "agent:A", "agent", &p)
        .unwrap();
    let mut input = answer_input("Planifier seulement");
    input.effects = AnswerEffects::Replan;
    s.answer_details(&d.id, d.version, &input, "Human", &p)
        .unwrap();
    p.investigation.max_rounds = 1;
    s.issue(&plan, "A", "once", "dev", tmp.path(), &p).unwrap();
    s.issue(&plan, "A", "once", "dev", tmp.path(), &p).unwrap();
    assert!(s
        .issue(&plan, "A", "twice", "dev", tmp.path(), &p)
        .unwrap_err()
        .to_string()
        .contains("budget exhausted"));
    assert!(s.context(&plan, "C", "free", "dev", &p).is_ok());
    s.verify().unwrap();
}
#[test]
fn delegation_waits_for_replan_and_shared_alias_waits_for_parent_effects() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    let mut r = question("parent");
    r.options =
        serde_json::from_value(json!([{"id":"chosen","label":"Replan","requires_replan":true}]))
            .unwrap();
    r.recommended_option_id = Some("chosen".into());
    let d = s.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    p.human_question_min_risk = Consultation::NeverAsk;
    let d = s.resolve_delegated(&d.id, &p).unwrap();
    assert_eq!(d.status, "awaiting_effects");
    let mut r = question("alias");
    r.topic = Some("parent".into());
    r.task_id = "B".into();
    r.question = "Can the dependent consumer use a competing contract?".into();
    let alias = s.submit(&plan, r, "agent:B", "agent", &p).unwrap();
    assert_eq!(alias.status, "waiting_dependency");
    assert!(s.context(&plan, "B", "blocked", "dev", &p).is_err());
    let effect = patch(
        &plan,
        "reconcile-parent",
        vec![Operation::AddTask {
            task: json!({"id":"D","dependencies":["A"]}),
        }],
    );
    s.apply_effects_attested(&d.id, d.version, &effect, "Human", None)
        .unwrap();
    assert_eq!(s.reconcile_waiting_decisions(&plan, &p).unwrap(), 1);
    let alias = s.decision(&alias.id).unwrap();
    assert_eq!(alias.status, "resolved");
    assert!(s
        .context(&s.plan(&plan.id).unwrap(), "B", "fresh", "dev", &p)
        .is_ok());
    s.verify().unwrap();
}
#[test]
fn twenty_parallel_workers_share_authority_and_only_affected_subtree_waits() {
    let (tmp, mut s, plan, _, p) = fixture();
    let mut graph = plan.graph.clone();
    graph["tasks"] = json!((0..20)
        .map(|i| json!({"id":format!("W{i}"),"dependencies":[]}))
        .collect::<Vec<_>>());
    let patch = s
        .edit_patch(&plan, &graph, plan.graph_version, "Human")
        .unwrap();
    let plan = s
        .apply_patch(
            &plan.id,
            &patch,
            &TaskRegistry::from_value(&json!({"tasks":graph["tasks"]})).unwrap(),
            "Human",
        )
        .unwrap();
    let mut r = question("subtree");
    r.task_id = "W0".into();
    r.affected_tasks = vec!["W0".into()];
    s.submit(&plan, r, "agent:W0", "agent", &p).unwrap();
    let root = tmp.path().to_owned();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(20));
    let joins = (0..20)
        .map(|i| {
            let root = root.clone();
            let barrier = barrier.clone();
            let plan = plan.clone();
            let p = p.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let mut s = Store::open(&root).unwrap();
                let task = format!("W{i}");
                if i == 0 {
                    assert!(s.context(&plan, &task, "blocked", "dev", &p).is_err());
                    return;
                }
                let (c, _) = s
                    .issue(&plan, &task, &format!("worker{i}"), "dev", &root, &p)
                    .unwrap();
                s.issued(&plan.id, &c.attempt_id, "dev", &c.render(), &p)
                    .unwrap();
                s.complete(
                    &plan.id,
                    &c.attempt_id,
                    "dev",
                    &format!("MACC_DECISION_ACK: {}", c.context_digest),
                    &p,
                )
                .unwrap();
            })
        })
        .collect::<Vec<_>>();
    for join in joins {
        join.join().unwrap();
    }
    s.verify().unwrap();
}
#[test]
fn analytics_and_related_candidates_never_create_consent_or_unbounded_work() {
    let (_tmp, mut s, plan, _, mut p) = fixture();
    s.submit(&plan, question("analytics-a"), "agent:A", "agent", &p)
        .unwrap();
    let mut r = question("analytics-b");
    r.context = "Two implementations conflict with the same public contract".into();
    s.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    assert_eq!(s.similar_requests(&plan, &p).unwrap()["enabled"], false);
    p.similarity_hints = true;
    p.similarity_min_percent = 1;
    p.similarity_max_candidates = 1;
    let hints = s.similar_requests(&plan, &p).unwrap();
    assert_eq!(hints["authority"], false);
    assert_eq!(hints["candidate_count"], 1);
    assert_eq!(hints["budget_limited"], true);
    assert_eq!(
        s.decision_analytics(&plan).unwrap()["by_status"]["pending_human"],
        2
    );
    assert!(s
        .decisions(&plan.id)
        .unwrap()
        .iter()
        .all(|d| d.resolution.is_none()));
}

fn save_policy(root: &Path, p: &Policy) {
    let paths = crate::ProjectPaths::from_root(root);
    let mut cfg = crate::load_canonical_config(&paths.config_path).unwrap();
    cfg.automation.coordinator.as_mut().unwrap().decisions = Some(p.clone());
    std::fs::write(paths.config_path, cfg.to_yaml().unwrap()).unwrap();
}
fn isolated_fixture(
    root: &Path,
    p: &mut Policy,
) -> (std::path::PathBuf, super::super::secure::TestRegistryGuard) {
    let key = root.join("approver-key");
    assert!(Command::new("/usr/bin/ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&key)
        .status()
        .unwrap()
        .success());
    let signers = root.join("allowed-signers");
    std::fs::write(
        &signers,
        format!(
            "independent {}",
            std::fs::read_to_string(key.with_extension("pub")).unwrap()
        ),
    )
    .unwrap();
    p.trust_mode = "secure_approval".into();
    p.trust_registry = Some(root.join("fixture-registry"));
    let reply = root.join("broker-reply.json");
    let registry = TrustRegistry {
        broker: Some(BrokerConfig {
            command: vec![
                "/usr/bin/dash".into(),
                "-c".into(),
                format!("cat '{}'", reply.display()),
            ],
            timeout_seconds: 2,
        }),
        allowed_signers: signers,
        roles: BTreeMap::from([("independent".into(), vec!["OWNER".into()])]),
        required_roles: [
            "decision.answer",
            "decision.revoke",
            "decision.withdraw",
            "decision.revise",
            "decision.effects.apply",
            "operation.reconcile",
            "decision.feedback",
        ]
        .into_iter()
        .map(|s| (s.into(), vec!["OWNER".into()]))
        .collect(),
    };
    let guard = super::super::secure::test_registry(p.trust_registry.clone().unwrap(), registry);
    save_policy(root, p);
    (key, guard)
}
fn sign_action(root: &Path, key: &Path, challenge: &serde_json::Value) -> Attestation {
    let now = chrono::Utc::now().timestamp();
    let body = AttestationBody {
        principal: "independent".into(),
        role: "OWNER".into(),
        action: challenge["action"].as_str().unwrap().into(),
        action_digest: digest(challenge),
        nonce: uuid::Uuid::new_v4().to_string(),
        issued_at: now,
        expires_at: now + 300,
    };
    let file = root.join(format!("proof-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&file, super::super::secure::canonical_bytes(&body).unwrap()).unwrap();
    assert!(Command::new("/usr/bin/ssh-keygen")
        .args(["-Y", "sign", "-n", "macc-approval", "-f"])
        .arg(key)
        .arg(&file)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success());
    let signature = std::fs::read_to_string(file.with_extension("json.sig")).unwrap();
    Attestation { body, signature }
}
#[test]
fn broker_workflow_persists_deduplicates_checks_signatures_and_recovers_committed_action() {
    let (tmp, mut s, plan, _, mut p) = fixture();
    let (key, _guard) = isolated_fixture(tmp.path(), &mut p);
    let d = s
        .submit(&plan, question("broker"), "agent:A", "agent", &p)
        .unwrap();
    let intent = OperatorIntent::Answer {
        id: d.id.clone(),
        version: d.version,
        input: answer_input("Conserver exactement SQLite."),
    };
    let reply = tmp.path().join("broker-reply.json");
    std::fs::write(&reply,r#"{"status":"pending","request_id":"external-42","approval_url":"https://approver.example.test/42"}"#).unwrap();
    let ticket = s.begin_approval(intent.clone(), "Operator").unwrap();
    assert_eq!(ticket.status, "pending");
    assert!(s.apply_approval(&ticket.id, "Operator").is_err());
    assert_eq!(
        s.begin_approval(intent.clone(), "Operator").unwrap().id,
        ticket.id
    );
    assert_eq!(
        s.poll_approval(&ticket.id, "Operator").unwrap().version,
        ticket.version
    );
    let proof = sign_action(tmp.path(), &key, &s.operator_challenge(&intent).unwrap());
    let mut forged = proof.clone();
    forged.body.action_digest = "wrong".into();
    std::fs::write(
        &reply,
        serde_json::to_vec(&json!({"status":"approved","attestation":forged})).unwrap(),
    )
    .unwrap();
    assert!(s.poll_approval(&ticket.id, "Operator").is_err());
    assert_eq!(s.approval_ticket(&ticket.id).unwrap().status, "pending");
    std::fs::write(
        &reply,
        serde_json::to_vec(&json!({"status":"approved","attestation":proof})).unwrap(),
    )
    .unwrap();
    let approved = s.poll_approval(&ticket.id, "Operator").unwrap();
    assert_eq!(approved.status, "approved");
    assert_eq!(s.decision(&d.id).unwrap().status, "pending_human");
    // Simulate interruption after the native action commit and before ticket completion.
    s.apply_operator(&intent, Some(&proof), "Operator").unwrap();
    drop(s);
    let mut s = Store::open(tmp.path()).unwrap();
    let recovered = s.apply_approval(&ticket.id, "Operator").unwrap();
    assert_eq!(recovered.status, "applied");
    assert_eq!(
        s.apply_approval(&ticket.id, "Operator").unwrap().version,
        recovered.version
    );
    assert_eq!(s.decision(&d.id).unwrap().version, 2);
    s.verify().unwrap();
}
#[test]
fn broker_marks_changed_subject_stale_and_secure_revocation_requires_exact_proof() {
    let (tmp, mut s, plan, _, mut p) = fixture();
    let (key, _guard) = isolated_fixture(tmp.path(), &mut p);
    let d = s
        .submit(&plan, question("stale-broker"), "agent:A", "agent", &p)
        .unwrap();
    let intent = OperatorIntent::Answer {
        id: d.id.clone(),
        version: d.version,
        input: answer_input("SQLite"),
    };
    let reply = tmp.path().join("broker-reply.json");
    std::fs::write(
        &reply,
        r#"{"status":"pending","request_id":"remote","approval_url":null}"#,
    )
    .unwrap();
    let ticket = s.begin_approval(intent, "Operator").unwrap();
    assert!(s
        .revoke(&d.id, d.version, "forged actor", "skip approvals")
        .is_err());
    let proof = sign_action(
        tmp.path(),
        &key,
        &revoke_challenge(&d, "Withdraw unsafe proposal", &p),
    );
    let revoked = s
        .revoke_attested(
            &d.id,
            d.version,
            "ignored actor",
            "Withdraw unsafe proposal",
            Some(&proof),
        )
        .unwrap();
    assert_eq!(
        s.revoke_attested(
            &d.id,
            d.version,
            "ignored actor",
            "Withdraw unsafe proposal",
            Some(&proof)
        )
        .unwrap(),
        revoked
    );
    assert_eq!(
        s.poll_approval(&ticket.id, "Operator").unwrap().status,
        "stale"
    );
    assert!(s.apply_approval(&ticket.id, "Operator").is_err());
    s.verify().unwrap();
}
#[test]
fn secure_feedback_remains_bound_after_risk_threshold_change_but_not_forged() {
    let (tmp, mut s, plan, _, mut p) = fixture();
    let (key, _guard) = isolated_fixture(tmp.path(), &mut p);
    let text = "Ne jamais supprimer les données existantes.";
    let intent = OperatorIntent::Feedback {
        plan_id: plan.id.clone(),
        task: "A".into(),
        text: text.into(),
        advisory: false,
    };
    assert!(s.apply_operator(&intent, None, "forged actor").is_err());
    let proof = sign_action(tmp.path(), &key, &s.operator_challenge(&intent).unwrap());
    let d = s.apply_operator(&intent, Some(&proof), "Operator").unwrap();
    let d = s.decision(d["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        d.resolution.as_ref().unwrap().origin,
        "human_feedback_secure"
    );
    p.human_question_min_risk = Consultation::Critical;
    save_policy(tmp.path(), &p);
    assert!(s
        .context(&plan, "B", "after-policy", "review", &p)
        .unwrap()
        .render()
        .contains(text));
    s.verify().unwrap();
}

#[test]
fn signed_divergence_has_explicit_three_way_choices_and_preserves_local_authority() {
    let (tmp, mut local, plan, reg, p) = fixture();
    commit_fixture(tmp.path());
    let key = tmp.path().join("transport-key");
    assert!(Command::new("/usr/bin/ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(&key)
        .status()
        .unwrap()
        .success());
    let signers = tmp.path().join("transport-signers");
    std::fs::write(
        &signers,
        format!(
            "publisher {}",
            std::fs::read_to_string(key.with_extension("pub")).unwrap()
        ),
    )
    .unwrap();
    let initial = local.sign_archive(&key, "publisher").unwrap();
    let clone = tempfile::tempdir().unwrap();
    git(clone.path(), &["clone", tmp.path().to_str().unwrap(), "."]);
    std::fs::create_dir_all(clone.path().join(".macc")).unwrap();
    std::fs::copy(
        tmp.path().join(".macc/macc.yaml"),
        clone.path().join(".macc/macc.yaml"),
    )
    .unwrap();
    let mut remote = Store::open(clone.path()).unwrap();
    remote.import_archive(&initial, &signers, true).unwrap();
    let local_change = patch(
        &plan,
        "local-title",
        vec![Operation::UpdateTask {
            task_id: "A".into(),
            changes: BTreeMap::from([("title".into(), json!("Local reviewed contract"))]),
        }],
    );
    local
        .apply_patch(&plan.id, &local_change, &reg, "Local owner")
        .unwrap();
    let remote_plan = remote.plan(&plan.id).unwrap();
    let remote_change = patch(
        &remote_plan,
        "remote-titles",
        vec![
            Operation::UpdateTask {
                task_id: "A".into(),
                changes: BTreeMap::from([("title".into(), json!("Remote proposal"))]),
            },
            Operation::UpdateTask {
                task_id: "C".into(),
                changes: BTreeMap::from([("title".into(), json!("Independent remote work"))]),
            },
        ],
    );
    remote
        .apply_patch(&plan.id, &remote_change, &reg, "Remote owner")
        .unwrap();
    let remote_plan = remote.plan(&plan.id).unwrap();
    let mut q = question("foreign-direction");
    q.task_id = "C".into();
    let d = remote
        .submit(&remote_plan, q, "agent:C", "agent", &p)
        .unwrap();
    remote
        .answer(
            &d.id,
            d.version,
            "Remote human guidance",
            None,
            "Remote human",
            false,
            &p,
        )
        .unwrap();
    let incoming = remote.sign_archive(&key, "publisher").unwrap();
    assert!(local.import_archive(&incoming, &signers, false).is_err());
    let review = local
        .archive_review(&incoming, &signers, &plan.id, &BTreeMap::new())
        .unwrap();
    assert_eq!(review.conflicts.len(), 1);
    assert_eq!(review.conflicts[0].path, "/tasks/A/title");
    assert!(review.patch.is_none());
    let choices = BTreeMap::from([("/tasks/A/title".into(), MergeSide::Local)]);
    let review = local
        .archive_review(&incoming, &signers, &plan.id, &choices)
        .unwrap();
    assert!(review.patch.is_some());
    assert!(local
        .apply_archive_review(
            &incoming,
            &signers,
            &plan.id,
            &choices,
            "stale-head",
            "Local owner",
            None
        )
        .is_err());
    let merged = local
        .apply_archive_review(
            &incoming,
            &signers,
            &plan.id,
            &choices,
            &review.local_head,
            "Local owner",
            None,
        )
        .unwrap();
    assert_eq!(
        super::super::graph::task(&merged.graph, "A").unwrap()["title"],
        "Local reviewed contract"
    );
    assert_eq!(
        super::super::graph::task(&merged.graph, "C").unwrap()["title"],
        "Independent remote work"
    );
    assert!(local.decision(&d.id).is_err());
    assert!(local
        .events(&plan.id)
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "archive.reconciled"));
    local.verify().unwrap();
}

// A separate test process exits without dropping its connection. This exercises
// actual OS/WAL recovery rather than merely reopening a cleanly closed handle.
#[test]
fn crash_worker() {
    let Some(root) = std::env::var_os("MACC_DM_TEST_CRASH_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let mut s = Store::open(&root).unwrap();
    let plan = s.plans().unwrap().remove(0);
    match std::env::var("MACC_DM_TEST_CRASH_SCENARIO")
        .unwrap()
        .as_str()
    {
        "graph" => {
            s.db.execute_batch("PRAGMA wal_autocheckpoint=0").unwrap();
            let effects = patch(
                &plan,
                "crash-graph",
                vec![Operation::AddTask {
                    task: json!({"id":"AFTER_CRASH","dependencies":["A"]}),
                }],
            );
            s.apply_patch(
                &plan.id,
                &effects,
                &TaskRegistry::default(),
                "Crash fixture",
            )
            .unwrap();
            std::process::exit(86);
        }
        "operation" => {
            let id = std::env::var("MACC_DM_TEST_CRASH_DECISION").unwrap();
            let _ = s.execute_operation(&id);
            panic!("External fault fixture should have terminated this worker");
        }
        _ => panic!("Unknown controlled crash scenario"),
    }
}
fn crash_process(root: &Path, scenario: &str, decision: Option<&str>) -> std::process::ExitStatus {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "coordinator::decisions::tests::workflows::crash_worker",
            "--nocapture",
        ])
        .env("MACC_DM_TEST_CRASH_ROOT", root)
        .env("MACC_DM_TEST_CRASH_SCENARIO", scenario)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(id) = decision {
        command.env("MACC_DM_TEST_CRASH_DECISION", id);
    }
    command.status().unwrap()
}
#[test]
fn process_crash_replays_committed_wal_outbox_and_clear_retains_decisions_and_source() {
    let (tmp, mut s, plan, _, p) = fixture();
    commit_fixture(tmp.path());
    let d = s
        .submit(
            &plan,
            question("durable-before-crash"),
            "agent:A",
            "agent",
            &p,
        )
        .unwrap();
    s.answer(
        &d.id,
        d.version,
        "Conserver exactement ces données",
        None,
        "Human",
        false,
        &p,
    )
    .unwrap();
    let config = std::fs::read(tmp.path().join(".macc/macc.yaml")).unwrap();
    std::fs::write(tmp.path().join("untracked-user-work.txt"), "retained").unwrap();
    assert_eq!(crash_process(tmp.path(), "graph", None).code(), Some(86));
    assert!(!std::fs::read_to_string(tmp.path().join("prd.json"))
        .unwrap()
        .contains("AFTER_CRASH"));
    let backup = tempfile::tempdir().unwrap();
    assert!(capture_backup(tmp.path(), backup.path()).unwrap().is_some());
    crate::clear(&crate::ProjectPaths::from_root(tmp.path())).unwrap();
    std::fs::create_dir_all(tmp.path().join(".macc")).unwrap();
    std::fs::write(tmp.path().join(".macc/macc.yaml"), config).unwrap();
    drop(s);
    let mut s = Store::open(tmp.path()).unwrap();
    s.project(tmp.path()).unwrap();
    assert!(std::fs::read_to_string(tmp.path().join("prd.json"))
        .unwrap()
        .contains("AFTER_CRASH"));
    assert_eq!(
        s.decision(&d.id).unwrap().resolution.unwrap().text,
        "Conserver exactement ces données"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("untracked-user-work.txt")).unwrap(),
        "retained"
    );
    s.verify().unwrap();
}
#[cfg(unix)]
#[test]
fn actual_owner_death_after_external_effect_is_unknown_and_never_reexecutes() {
    let (tmp, mut s, plan, _, mut p) = fixture();
    commit_fixture(tmp.path());
    p.human_question_min_risk = Consultation::NeverAsk;
    p.operation_adapters.insert("crash-fixture".into(),OperationAdapter{command:vec!["/usr/bin/python3.12".into(),"-c".into(),"import os,signal;open('.macc/external-effect-count','a').write('once\\n');os.kill(os.getppid(),signal.SIGKILL)".into()],allowed_targets:vec!["fixture".into()],..Default::default()});
    save_policy(tmp.path(), &p);
    let mut r = question("crash-once");
    r.kind = "authorization".into();
    r.operation_id = Some("operation-crash".into());
    r.operation_spec = Some(OperationSpec {
        adapter: "crash-fixture".into(),
        target: "fixture".into(),
        source_revision: git(tmp.path(), &["rev-parse", "HEAD"]),
        arguments: Default::default(),
    });
    let d = s.submit(&plan, r, "agent:A", "agent", &p).unwrap();
    assert!(!crash_process(tmp.path(), "operation", Some(&d.id)).success());
    // The killed owner can exit before its external child finishes shutting down.
    // Recovery must retain Executing until both identities are actually gone.
    for _ in 0..200 {
        s.recover_operations().unwrap();
        if s.operation(&plan.id, "operation-crash")
            .unwrap()
            .unwrap()
            .state
            == OperationState::Unknown
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let op = s.operation(&plan.id, "operation-crash").unwrap().unwrap();
    assert_eq!(op.state, OperationState::Unknown);
    assert!(s.execute_operation(&d.id).is_err());
    assert_eq!(
        std::fs::read_to_string(tmp.path().join(".macc/external-effect-count")).unwrap(),
        "once\n"
    );
    // Manual reconciliation is a privileged mutation even after a real crash.
    let (key, _guard) = isolated_fixture(tmp.path(), &mut p);
    assert!(s
        .reconcile_operation(
            &plan.id,
            &op.operation_id,
            op.version,
            OperationState::Succeeded,
            "Verified by external system",
            "forged operator"
        )
        .is_err());
    let proof = sign_action(
        tmp.path(),
        &key,
        &operation_challenge(
            &op,
            OperationState::Succeeded,
            "Verified by external system",
            &p,
        ),
    );
    let result = s
        .reconcile_operation_attested(
            &plan.id,
            &op.operation_id,
            op.version,
            OperationState::Succeeded,
            "Verified by external system",
            "ignored",
            Some(&proof),
        )
        .unwrap();
    assert_eq!(result.state, OperationState::Succeeded);
    s.verify().unwrap();
}

#[test]
fn independent_gate_review_can_take_minutes_without_forging_later_vote_time() {
    let (tmp, mut s, plan, reg, mut p) = fixture();
    commit_fixture(tmp.path());
    let gate = patch(
        &plan,
        "approval-gate",
        vec![Operation::AddTask {
            task: json!({"id":"APP","dependencies":["A"],"gate":{"kind":"human_approval","subject_task":"A","required_approvers":[{"role":"OWNER","count":1}]}}),
        }],
    );
    let plan = s.apply_patch(&plan.id, &gate, &reg, "Human").unwrap();
    let (key, _guard) = isolated_fixture(tmp.path(), &mut p);
    // This fixture only needs the gate.vote verification role, not an OS admin.
    let signers = tmp.path().join("allowed-signers");
    let _gate_registry = super::super::secure::test_registry(
        p.trust_registry.clone().unwrap(),
        TrustRegistry {
            broker: None,
            allowed_signers: signers,
            roles: BTreeMap::from([("independent".into(), vec!["OWNER".into()])]),
            required_roles: BTreeMap::from([("gate.vote".into(), vec!["OWNER".into()])]),
        },
    );
    let record = crate::coordinator::model::ApprovalRecord {
        decision: crate::coordinator::model::ApprovalDecision::Approved,
        role: "OWNER".into(),
        actor: "independent".into(),
        revision: Some(git(tmp.path(), &["rev-parse", "HEAD"])),
        evidence: Some("Independent review of exact artifact".into()),
        reason: None,
        recorded_at: (chrono::Utc::now() - chrono::Duration::minutes(5)).to_rfc3339(),
    };
    let proof = sign_action(
        tmp.path(),
        &key,
        &approval_challenge(&plan, "APP", &record, &p).unwrap(),
    );
    s.record_approval_attested(tmp.path(), &plan, "APP", &record, Some(&proof))
        .unwrap();
    s.verify().unwrap();
    let mut future = record.clone();
    future.recorded_at = (chrono::Utc::now() + chrono::Duration::minutes(2)).to_rfc3339();
    let proof = sign_action(
        tmp.path(),
        &key,
        &approval_challenge(&plan, "APP", &future, &p).unwrap(),
    );
    assert!(s
        .record_approval_attested(tmp.path(), &plan, "APP", &future, Some(&proof))
        .is_err());
}

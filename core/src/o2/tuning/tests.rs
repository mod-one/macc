use super::*;
fn fixture() -> tempfile::TempDir {
    let t = super::super::tests::root();
    let view = policy::inspect(t.path()).unwrap();
    let mut p = view.policy;
    p.context_engine.tuning_enabled = true;
    policy::update(t.path(), &view.revision, Some(p)).unwrap();
    t
}
fn plan(root: &Path) -> Plan {
    Plan {
        field: "context_engine.push_memory_limit_tokens".into(),
        candidate: 800.,
        cases: (0..20)
            .map(|i| Case {
                task_id: format!("TASK-{i}"),
                revision: crate::git::head_commit(root).unwrap(),
            })
            .collect(),
        protocol_path: "contract.rs".into(),
        protocol_sha256: digest(&std::fs::read(root.join("contract.rs")).unwrap()),
    }
}
fn quality() -> Quality {
    Quality {
        accepted: true,
        tests_passed: true,
        spec_fidelity: true,
        max_finding_severity: 0,
        retries: 0,
    }
}
fn observation(root: &Path, task: &str) -> Observation {
    Observation {
        task_id: task.into(),
        baseline: quality(),
        candidate: quality(),
        evidence_path: "contract.rs".into(),
        evidence_sha256: digest(&std::fs::read(root.join("contract.rs")).unwrap()),
    }
}
fn trial(root: &Path) -> Trial {
    serde_json::from_value(create(root, plan(root), true).unwrap()["report"].clone()).unwrap()
}
fn set(root: &Path, p: &policy::Policy) {
    let view = policy::inspect(root).unwrap();
    policy::update(root, &view.revision, Some(p.clone())).unwrap();
}
fn run(root: &Path, task: &str, cost: f64) -> String {
    let spec = json!({"id":"future-local-meter","performer":{"command":"fake","usage":{"schema":"trial-meter-v1","model":"/model","session_id":"/session","token_scope":"invocation","cost_scope":"invocation","cost_quality":"exact","currency":"USD","fields":{"cost":"/cost"}},"session":{"enabled":true,"scope":"task_role_tool","create":{"command":"fake","args":["new","{session_id}"]}}}});
    let identity = crate::coordinator::session_identity::identity(
        root,
        root,
        task,
        "performer",
        "future-local-meter",
        &spec,
        &crate::coordinator::session_identity::SessionRouting {
            tier: "".into(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let grant = store::acquire(
        root,
        &store::AcquireRequest {
            identity,
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: false,
            checkpoint_id: None,
        },
    )
    .unwrap();
    store::bind(root, &grant.key, &grant.key.session_id).unwrap();
    store::usage::begin(
        root,
        &store::usage::BeginRequest {
            key: grant.key.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    let id = format!("{}:{}", grant.key.session_id, grant.key.generation);
    let mut measured = store::usage::end(
        root,
        &store::usage::EndRequest {
            key: grant.key.clone(),
            outcome: "success".into(),
            raw: json!({"model":"future-model","session":grant.key.session_id,"cost":cost})
                .to_string(),
            omitted: false,
        },
    )
    .unwrap();
    // Synthetic provider fixture: fix elapsed time rather than racing wall-clock seconds.
    measured.elapsed_ms = Some(1000);
    {
        let (db, _) = database(root).unwrap();
        db.execute(
            "UPDATE invocation_usage SET record_json=?2 WHERE invocation_id=?1",
            rusqlite::params![id, encoded(&measured).unwrap()],
        )
        .unwrap();
    }
    store::release(root, &grant.key, true).unwrap();
    store::close(root, &grant.key.session_id).unwrap();
    id
}
fn pair_run(root: &Path, t: &Trial, index: usize) {
    let task = &t.plan.cases[index].task_id;
    for arm in if index % 2 == 0 {
        [false, true]
    } else {
        [true, false]
    } {
        set(root, if arm { &t.candidate } else { &t.baseline });
        run(
            root,
            &format!("{task}--{}", if arm { "candidate" } else { "baseline" }),
            if arm { 0.7 } else { 1.0 },
        );
    }
}
#[test]
fn exact_sign_test_matches_binomial_tails_and_ties() {
    assert_eq!(statistics::sign_p(0, 0), 1.);
    assert!((statistics::sign_p(20, 0) - 2f64.powi(-20)).abs() < 1e-15);
    assert!((statistics::sign_p(3, 1) - 0.3125).abs() < 1e-15);
    assert!(statistics::sign_p(10, 10) > 0.5);
    assert!(statistics::sign_p(0, 20) > 0.999);
}
#[test]
fn registration_is_bounded_immutable_and_never_changes_policy() {
    let t = fixture();
    let root = t.path();
    let before = policy::inspect(root).unwrap();
    assert!(create(root, plan(root), false).is_err());
    let mut p = plan(root);
    p.cases.pop();
    assert!(create(root, p, true).is_err());
    let mut p = plan(root);
    p.cases[1].task_id = p.cases[0].task_id.clone();
    assert!(create(root, p, true).is_err());
    let mut p = plan(root);
    p.field = "memory.min_confidence".into();
    assert!(create(root, p, true).is_err());
    let mut p = plan(root);
    p.protocol_sha256 = "changed".into();
    assert!(create(root, p, true).is_err());
    let a = trial(root);
    assert!(create(root, plan(root), true).is_err());
    assert_eq!(policy::inspect(root).unwrap().revision, before.revision);
    assert!(close(root, &a.id, false, true).is_err());
    assert_eq!(
        close(root, &a.id, true, true).unwrap()["report"]["passed"],
        false
    );
    let b = trial(root);
    assert!(b.alpha <= a.alpha);
    assert_eq!(b.ordinal, 2);
    assert_eq!(
        inspect(root, None).unwrap()["fields"]
            .as_array()
            .unwrap()
            .len(),
        FIELDS.len()
    );
}
#[test]
fn complete_measured_trial_requires_separate_preview_cas_and_confirmation() {
    let f = fixture();
    let root = f.path();
    let t = trial(root);
    for index in 0..20 {
        pair_run(root, &t, index);
        let o = observation(root, &t.plan.cases[index].task_id);
        assert_eq!(
            record(root, &t.id, o.clone(), true).unwrap()["pair"]["quality_passed"],
            true
        );
        if index == 0 {
            assert!(record(root, &t.id, o, true).is_err());
        }
    }
    set(root, &t.baseline);
    let envelope = close(root, &t.id, false, true).unwrap();
    let r: Report = serde_json::from_value(envelope["report"].clone()).unwrap();
    assert!(r.passed, "{:?}", r.reasons);
    assert_eq!(r.wins, 20);
    assert!(r.cost_reduction > 0.29);
    assert!(preview(root, &r.id, "foreign-sha").is_err());
    let sha = envelope["checksum"].as_str().unwrap();
    let p = preview(root, &r.id, sha).unwrap();
    assert_eq!(policy::load(root).unwrap(), t.baseline);
    let revision = p["revision"].as_str().unwrap();
    assert!(apply(root, &r.id, sha, revision, false).is_err());
    assert!(apply(root, &r.id, sha, "stale", true).is_err());
    apply(root, &r.id, sha, revision, true).unwrap();
    assert_eq!(policy::load(root).unwrap(), t.candidate);
    assert!(apply(root, &r.id, sha, revision, true).is_err());
    let (db, _) = readonly_database(root).unwrap();
    assert_eq!(
        events::<Value>(&db, "tuning_adoption_intent", &{
            db.query_row(
                "SELECT record_id FROM o2_audit WHERE kind='tuning_adopted' LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
        })
        .unwrap()
        .len(),
        1
    );
}
#[test]
fn estimated_cost_changed_policy_and_missing_receipts_cannot_be_used() {
    let f = fixture();
    let root = f.path();
    let t = trial(root);
    pair_run(root, &t, 0);
    let o = observation(root, "TASK-0");
    let (db, _) = database(root).unwrap();
    let raw: String = db
        .query_row(
            "SELECT record_json FROM invocation_usage ORDER BY rowid LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let mut changed: Value = serde_json::from_str(&raw).unwrap();
    changed["cost_quality"] = json!("estimated");
    let id = changed["invocation_id"].as_str().unwrap();
    db.execute(
        "UPDATE invocation_usage SET record_json=?2 WHERE invocation_id=?1",
        rusqlite::params![id, changed.to_string()],
    )
    .unwrap();
    assert!(record(root, &t.id, o.clone(), true).is_err());
    db.execute(
        "UPDATE invocation_usage SET record_json=?2 WHERE invocation_id=?1",
        rusqlite::params![id, raw],
    )
    .unwrap();
    db.execute("UPDATE o2_audit SET payload_json=json_set(payload_json,'$.policy','changed') WHERE kind='tuning_finish' AND record_id=?1",[id]).unwrap();
    assert!(record(root, &t.id, o, true).is_err());
}
#[test]
fn trial_quality_latency_effect_size_and_non_tied_sample_are_independent_gates() {
    let f = fixture();
    let t = trial(f.path());
    let rows = || {
        t.plan
            .cases
            .iter()
            .map(|c| PairResult {
                task_id: c.task_id.clone(),
                baseline_cost: 100,
                candidate_cost: 70,
                baseline_ms: 100,
                candidate_ms: 100,
                currency: "USD".into(),
                quality_passed: true,
                telemetry_sha256: "fixture".into(),
            })
            .collect::<Vec<_>>()
    };
    assert!(statistics::evaluate(&t, rows()).passed);
    let mut r = rows();
    r[0].quality_passed = false;
    assert!(!statistics::evaluate(&t, r).passed);
    let mut r = rows();
    r[0].candidate_ms = 10000;
    assert!(!statistics::evaluate(&t, r).passed);
    let mut r = rows();
    for p in &mut r {
        p.candidate_cost = 99;
    }
    assert!(!statistics::evaluate(&t, r).passed);
    let mut r = rows();
    r[0].candidate_cost = 100;
    assert!(!statistics::evaluate(&t, r).passed);
    let mut r = rows();
    r[0].currency = "EUR".into();
    assert!(!statistics::evaluate(&t, r).passed);
}
#[test]
fn source_revision_order_dirty_inputs_and_scope_are_enforced() {
    let f = fixture();
    let root = f.path();
    let t = trial(root);
    set(root, &t.candidate);
    run(root, "TASK-0--candidate", 0.7);
    set(root, &t.baseline);
    run(root, "TASK-0--baseline", 1.);
    assert!(record(root, &t.id, observation(root, "TASK-0"), true).is_err());
    assert!(record(root, &t.id, observation(root, "OTHER"), true).is_err());
    pair_run(root, &t, 2);
    let (db, _) = database(root).unwrap();
    db.execute("UPDATE o2_audit SET payload_json=json_set(payload_json,'$.clean',json('false')) WHERE kind='tuning_invocation' AND json_extract(payload_json,'$.task') IN ('TASK-2--baseline','TASK-2--candidate')",[]).unwrap();
    assert!(record(root, &t.id, observation(root, "TASK-2"), true).is_err());
    assert!(record(root, &t.id, observation(root, "TASK-4"), true).is_err());
}
#[test]
fn catalog_covers_every_tuning_control_and_rejects_unsafe_candidates() {
    let mut p = policy::Policy::default();
    for key in [
        "tuning_enabled",
        "tuning_min_pairs",
        "tuning_alpha",
        "tuning_min_cost_reduction",
        "tuning_max_latency_regression",
    ] {
        assert!(policy::fields()
            .iter()
            .any(|f| f.path == format!("context_engine.{key}")));
    }
    p.context_engine.tuning_min_pairs = 1;
    assert!(p.validate().is_err());
    p.context_engine.tuning_min_pairs = 20;
    p.context_engine.tuning_alpha = 0.9;
    assert!(p.validate().is_err());
    assert!(trials::candidate(&policy::Policy::default(), "memory.enabled", 1.).is_err());
    assert!(trials::candidate(
        &policy::Policy::default(),
        "memory.ranking_task_weight",
        1.5
    )
    .is_err());
    assert!(trials::candidate(
        &policy::Policy::default(),
        "adaptive_compaction.warning_ratio",
        0.95
    )
    .is_err());
}
#[test]
fn preexisting_execution_identities_and_changed_memory_corpus_cannot_bias_trials() {
    let f = fixture();
    let root = f.path();
    // Even without an open trial, ordinary usage remains functional and unstamped.
    run(root, "TASK-0--baseline", 1.);
    assert!(create(root, plan(root), true).is_err());
    let mut fresh = plan(root);
    for c in &mut fresh.cases {
        c.task_id = format!("FRESH-{}", c.task_id);
    }
    let t: Trial =
        serde_json::from_value(create(root, fresh, true).unwrap()["report"].clone()).unwrap();
    run(root, "FRESH-TASK-0--baseline", 1.);
    let proposal = super::super::tests::proposal(root, None);
    let memory = memory::propose(root, &proposal).unwrap();
    super::super::tests::activate(root, &memory, None);
    set(root, &t.candidate);
    run(root, "FRESH-TASK-0--candidate", 0.7);
    assert!(record(root, &t.id, observation(root, "FRESH-TASK-0"), true)
        .unwrap_err()
        .to_string()
        .contains("corpus changed"));
    let stamps = inspect(root, Some(&t.id)).unwrap();
    assert_eq!(stamps["invocations"].as_array().unwrap().len(), 2);
    assert_eq!(
        stamps["execution_tasks"][0]["baseline"],
        "FRESH-TASK-0--baseline"
    );
}

#[test]
fn temporarily_disabled_capture_cannot_hide_a_whole_expensive_session() {
    let f = fixture();
    let root = f.path();
    let t = trial(root);
    let mut off = t.baseline.clone();
    off.context_engine.tuning_enabled = false;
    set(root, &off);
    run(root, "TASK-0--baseline", 10.);
    pair_run(root, &t, 0);
    let error = record(root, &t.id, observation(root, "TASK-0"), true)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unstamped invocations"), "{error}");
}

#[test]
fn decimal_reports_and_policy_fingerprints_survive_persistence_exactly() {
    let f = fixture();
    let t = trial(f.path());
    let pairs = t
        .plan
        .cases
        .iter()
        .map(|c| PairResult {
            task_id: c.task_id.clone(),
            baseline_cost: 100,
            candidate_cost: 70,
            baseline_ms: 21,
            candidate_ms: 22,
            currency: "USD".into(),
            quality_passed: true,
            telemetry_sha256: "fixture".into(),
        })
        .collect();
    let r = statistics::evaluate(&t, pairs);
    assert!(r.passed);
    let persisted = serde_json::to_value(&r).unwrap().to_string();
    let restored: Report = store::json(persisted.as_bytes()).unwrap();
    assert_eq!(hash(&r).unwrap(), hash(&restored).unwrap());
    // Also protect long decimal policy values used to bind invocation stamps.
    for i in 1..100 {
        let v = 1. + 1. / i as f64;
        let back: f64 = store::json(encoded(&v).unwrap().as_bytes()).unwrap();
        assert_eq!(v.to_bits(), back.to_bits());
    }
}

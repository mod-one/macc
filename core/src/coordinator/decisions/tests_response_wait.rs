//! Real native versions, leases and late answers; no provider or network.
use super::tests::fixture as base_fixture;
use super::*;
use serde_json::json;
use std::collections::BTreeMap;
fn fixture() -> (
    tempfile::TempDir,
    Store,
    Plan,
    crate::coordinator::model::TaskRegistry,
    Policy,
) {
    let (tmp, store, plan, registry, p) = base_fixture();
    std::fs::write(
        tmp.path().join("prd.json"),
        serde_json::to_vec(&plan.graph).unwrap(),
    )
    .unwrap();
    std::fs::write(tmp.path().join(".gitignore"), ".macc/\n").unwrap();
    for args in [
        vec!["add", "prd.json", ".gitignore"],
        vec!["commit", "-m", "Fixture requirements"],
    ] {
        assert!(std::process::Command::new("git")
            .current_dir(tmp.path())
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
    crate::coordinator::state::coordinator_state_registry_save(
        tmp.path(),
        &BTreeMap::new(),
        &registry.to_value().unwrap(),
    )
    .unwrap();
    (tmp, store, plan, registry, p)
}
fn request(key: &str) -> Request {
    serde_json::from_value(json!({"idempotency_key":key,"task_id":"A","kind":"intervention","question":"Provide isolated device access or defer observations?","context":"Hardware unavailable","recommendation":"Use isolated access when available","investigated":["Checked local access"],"risk":"high","impact":{"evidence":["No USB access"]},"intervention":{"action":"Connect isolated device","inaccessible_reason":"No USB access","expected_result":"adb get-state output","verification":"Independent adb probe"},"options":[{"id":"provide","label":"Provide device access","intervention_response":"completed"},{"id":"defer","label":"Keep observations unverified","intervention_response":"defer_observation"}],"response_fields":[{"id":"device","label":"Device output","required":true}]})).unwrap()
}
#[test]
fn explicit_deferral_has_no_fabricated_action_and_does_not_establish_compliance() {
    let (tmp, mut store, plan, _, p) = fixture();
    let d = store
        .submit(&plan, request("defer"), "agent:A", "agent", &p)
        .unwrap();
    let mut answer = AnswerInput {
        text: "Keep observations unverified".into(),
        option: Some("defer".into()),
        ..Default::default()
    };
    answer.fields.insert("performed".into(), "yes".into());
    assert!(store
        .answer_details(&d.id, d.version, &answer, "Human", &p)
        .is_err());
    answer.fields.clear();
    let resolved = store
        .answer_details(&d.id, d.version, &answer, "Human", &p)
        .unwrap();
    assert_eq!(resolved.version, 2);
    assert_eq!(
        resolved.resolution.as_ref().unwrap().answered_version,
        Some(1)
    );
    assert!(!resolved
        .resolution
        .as_ref()
        .unwrap()
        .fields
        .contains_key("performed"));
    let (context, _) = store
        .issue(
            &store.plan(&plan.id).unwrap(),
            "A",
            "review-defer",
            "review",
            tmp.path(),
            &p,
        )
        .unwrap();
    store
        .issued(&plan.id, "review-defer", "review", &context.render(), &p)
        .unwrap();
    let output = format!("MACC_DECISION_ACK: {}\n", context.context_digest);
    assert!(store
        .complete(&plan.id, "review-defer", "review", &output, &p)
        .unwrap_err()
        .to_string()
        .contains("DM_COMPLIANCE"));
    assert!(pending_for_task(tmp.path(), "A").unwrap().is_none());
    store.verify().unwrap();
}
#[test]
fn legacy_completion_still_requires_real_result_and_required_device_field() {
    let (_tmp, mut store, plan, _, p) = fixture();
    let d = store
        .submit(&plan, request("complete"), "agent:A", "agent", &p)
        .unwrap();
    let mut input = AnswerInput {
        text: "yes".into(),
        option: Some("provide".into()),
        ..Default::default()
    };
    assert!(store.answer_details(&d.id, 1, &input, "Human", &p).is_err());
    input.fields.insert("performed".into(), "yes".into());
    input
        .fields
        .insert("result".into(), "adb: connected".into());
    assert!(store.answer_details(&d.id, 1, &input, "Human", &p).is_err());
    input.fields.insert("device".into(), "ABC-42".into());
    assert!(store.answer_details(&d.id, 1, &input, "Human", &p).is_ok());
}
#[test]
fn explicit_fact_mode_and_unknown_or_contradictory_modes_are_checked_natively() {
    let (_tmp, mut store, plan, _, p) = fixture();
    let d = store
        .submit(&plan, request("fact"), "agent:A", "agent", &p)
        .unwrap();
    let mut input = AnswerInput {
        text: "Device exists; not connected".into(),
        fields: BTreeMap::from([
            (RESPONSE_MODE.into(), "confirm_fact".into()),
            ("device".into(), "Device exists".into()),
        ]),
        ..Default::default()
    };
    input.option = Some("provide".into());
    assert!(store.answer_details(&d.id, 1, &input, "Human", &p).is_err());
    input.option = None;
    input.fields.insert(RESPONSE_MODE.into(), "guess".into());
    assert!(store.answer_details(&d.id, 1, &input, "Human", &p).is_err());
    input
        .fields
        .insert(RESPONSE_MODE.into(), "confirm_fact".into());
    let answered = store.answer_details(&d.id, 1, &input, "Human", &p).unwrap();
    assert_eq!(
        answered.resolution.unwrap().fields[RESPONSE_MODE],
        "confirm_fact"
    );
}
#[test]
fn native_wait_receipt_is_lease_fenced_invocation_scoped_and_survives_late_answer() {
    use crate::session_store as ss;
    let (tmp, mut store, plan, mut registry, p) = fixture();
    let root = tmp.path();
    let preview = ss::migrate(root, false, None).unwrap();
    ss::migrate(root, true, Some(&preview.source_sha256)).unwrap();
    let identity = crate::coordinator::session_identity::identity(
        root,
        root,
        "A",
        "performer",
        "fake",
        &json!({"id":"fake","performer":{"command":"fake","session":{"enabled":true}}}),
        &crate::coordinator::session_identity::SessionRouting {
            tier: String::new(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let grant = ss::acquire(
        root,
        &ss::AcquireRequest {
            identity,
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: false,
            checkpoint_id: None,
        },
    )
    .unwrap();
    let d = store
        .submit(&plan, request("late"), "agent:A", "agent", &p)
        .unwrap();
    let mut stale = grant.key.clone();
    stale.generation += 1;
    assert!(ss::decision_wait::heartbeat_state(root, &stale, "current").is_err());
    assert!(
        ss::decision_wait::waiting_for_invocation(root, "A", "current")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        ss::decision_wait::heartbeat_state(root, &grant.key, "current").unwrap()["status"],
        "waiting_decision"
    );
    let receipt = ss::decision_wait::waiting_for_invocation(root, "A", "current")
        .unwrap()
        .unwrap();
    assert_eq!(receipt.decision.unwrap().id, d.id);
    assert!(
        ss::decision_wait::waiting_for_invocation(root, "A", "stale")
            .unwrap()
            .is_none()
    );
    assert!(
        ss::decision_wait::waiting_for_invocation(root, "B", "current")
            .unwrap()
            .is_none()
    );
    let kept = root.join(".macc/worktree/retained");
    assert!(std::process::Command::new("git")
        .current_dir(root)
        .args(["worktree", "add", "-b", "retained-branch"])
        .arg(&kept)
        .arg("main")
        .output()
        .unwrap()
        .status
        .success());
    std::fs::write(kept.join("unfinished.txt"), "Preserve unfinished work").unwrap();
    let task = registry.find_task_mut("A").unwrap();
    task.state = "blocked".into();
    task.task_runtime.current_phase = Some("review".into());
    task.worktree = Some(
        serde_json::from_value(
            json!({"worktree_path":kept,"branch":"retained-branch","base_branch":"main"}),
        )
        .unwrap(),
    );
    assert!(refresh_waits(root, &mut registry).unwrap());
    assert_eq!(
        registry.find_task("A").unwrap().task_runtime.extra["decision_wait"],
        d.id
    );
    let answer = AnswerInput {
        text: "Explicitly defer".into(),
        option: Some("defer".into()),
        ..Default::default()
    };
    store
        .answer_details(&d.id, 1, &answer, "Human", &p)
        .unwrap();
    assert!(
        ss::decision_wait::waiting_for_invocation(root, "A", "current")
            .unwrap()
            .is_some()
    );
    assert!(pending_for_task(root, "A").unwrap().is_none());
    sync(root, &root.join("prd.json"), &mut registry).unwrap();
    let task = registry.find_task("A").unwrap();
    assert_eq!(
        task.worktree
            .as_ref()
            .and_then(|w| w.worktree_path.as_deref()),
        kept.to_str()
    );
    assert_eq!(task.task_runtime.current_phase.as_deref(), Some("review"));
    assert!(!task.task_runtime.extra.contains_key("decision_wait"));
    assert_eq!(
        std::fs::read_to_string(kept.join("unfinished.txt")).unwrap(),
        "Preserve unfinished work"
    );
}
#[test]
fn optional_option_semantics_preserve_legacy_serialization() {
    let value = json!({"id":"a","label":"A","consequences":[],"proposed_patch":null,"requires_replan":false});
    let option: OptionChoice = serde_json::from_value(value).unwrap();
    assert!(serde_json::to_value(option)
        .unwrap()
        .get("intervention_response")
        .is_none());
}

#[test]
fn an_answer_before_the_next_heartbeat_records_a_native_context_refresh() {
    use crate::session_store as ss;
    let (tmp, mut store, plan, _, p) = fixture();
    let root = tmp.path();
    let preview = ss::migrate(root, false, None).unwrap();
    ss::migrate(root, true, Some(&preview.source_sha256)).unwrap();
    let identity = crate::coordinator::session_identity::identity(
        root,
        root,
        "A",
        "performer",
        "fake",
        &json!({"id":"fake","performer":{"command":"fake","session":{"enabled":true}}}),
        &crate::coordinator::session_identity::SessionRouting {
            tier: String::new(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    let grant = ss::acquire(
        root,
        &ss::AcquireRequest {
            identity,
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: false,
            checkpoint_id: None,
        },
    )
    .unwrap();
    let d = store
        .submit(&plan, request("quick-answer"), "agent:A", "agent", &p)
        .unwrap();
    store
        .answer_details(
            &d.id,
            1,
            &AnswerInput {
                text: "Explicitly defer observations".into(),
                option: Some("defer".into()),
                ..Default::default()
            },
            "Human",
            &p,
        )
        .unwrap();
    let state = ss::decision_wait::heartbeat_state(root, &grant.key, "quick").unwrap();
    assert_eq!(state["status"], "waiting_decision");
    let receipt = ss::decision_wait::waiting_for_invocation(root, "A", "quick")
        .unwrap()
        .unwrap();
    assert!(receipt.decision.is_none());
    assert!(receipt.reason.starts_with("DM_STALE:"));
}

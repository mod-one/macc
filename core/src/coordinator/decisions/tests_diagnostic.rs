use super::*;
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, process::Command};
fn git(root: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().into()
}
fn fixture(
    intervention: bool,
) -> (
    tempfile::TempDir,
    Store,
    ContextPackage,
    Policy,
    Value,
    String,
    String,
) {
    let (tmp, mut store, plan, registry, p) = super::tests::fixture();
    std::fs::write(tmp.path().join(".gitignore"), ".macc/\n").unwrap();
    let patch = Patch {
        plan_id: plan.id.clone(),
        base_graph_version: plan.graph_version,
        idempotency_key: "diagnostic-contract".into(),
        reason: "Declare diagnostic delivery".into(),
        decision_refs: vec![],
        metadata_changes: BTreeMap::new(),
        operations: vec![Operation::UpdateTask {
            task_id: "A".into(),
            changes: BTreeMap::from([
                ("category".into(), json!("verification")),
                (
                    "execution_contract".into(),
                    json!({"diagnostic_completion":true}),
                ),
                (
                    "change_scope".into(),
                    json!({"allowed_paths":["reports/**"]}),
                ),
            ]),
        }],
    };
    let plan = store
        .apply_patch(&plan.id, &patch, &registry, "Owner")
        .unwrap();
    let mut request = json!({"idempotency_key":"verify-external","task_id":"A","question":"Prepare the validation capability?","recommendation":"Supply and verify access","investigated":["Inspected the runtime"],"options":[{"id":"assist","label":"Prepare access","consequences":[]},{"id":"defer","label":"Explicit deferral","consequences":[]}],"recommended_option_id":"assist","impact":{"evidence":["External validation only"]}});
    if intervention {
        request["kind"] = json!("intervention");
        request["intervention"] = json!({"action":"Prepare test access","inaccessible_reason":"Operator access required","expected_result":"Independent access observation","verification":"Probe the test system"});
    }
    let d = store
        .submit(
            &plan,
            serde_json::from_value(request).unwrap(),
            "Operator",
            "human",
            &p,
        )
        .unwrap();
    if intervention {
        store
            .answer_details(
                &d.id,
                d.version,
                &AnswerInput {
                    text: "ok".into(),
                    option: Some("assist".into()),
                    fields: BTreeMap::from([
                        ("performed".into(), "yes".into()),
                        ("result".into(), "ok".into()),
                    ]),
                    ..Default::default()
                },
                "Operator",
                &p,
            )
            .unwrap();
    } else {
        store
            .answer(
                &d.id,
                d.version,
                "ok",
                Some("assist".into()),
                "Operator",
                false,
                &p,
            )
            .unwrap();
    }
    let plan = store.plan(&plan.id).unwrap();
    store.project(tmp.path()).unwrap();
    std::fs::create_dir_all(tmp.path().join("reports")).unwrap();
    let bytes=serde_json::to_vec(&json!({"task":"A","verdict":"changes_requested","featureAccepted":false,"releaseAuthorized":false,"observations":[{"status":"not_verified"}]})).unwrap();
    std::fs::write(tmp.path().join("reports/A.json"), &bytes).unwrap();
    git(tmp.path(), &["add", "."]);
    git(tmp.path(), &["commit", "-qm", "Diagnostic evidence"]);
    let (ctx, _) = store
        .issue(&plan, "A", "review-diagnostic", "review", tmp.path(), &p)
        .unwrap();
    store
        .issued(&plan.id, &ctx.attempt_id, "review", &ctx.render(), &p)
        .unwrap();
    use sha2::{Digest, Sha256};
    let marker = json!({"report_path":"reports/A.json","report_digest":format!("sha256:{:x}",Sha256::digest(&bytes)),"completed":true,"feature_accepted":false,"release_authorized":false});
    let output=format!("MACC_DECISION_ACK: {}\nMACC_DECISION_COMPLIANCE: {}\nMACC_DIAGNOSTIC_RESULT: {}\nREVIEW_VERDICT: OK\n",ctx.context_digest,json!({d.id:{"status":"needs_human_review","evidence":"The independent test does not establish access"}}),marker);
    let task = plan.graph["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "A")
        .unwrap()
        .clone();
    let head = git(tmp.path(), &["rev-parse", "HEAD"]);
    (tmp, store, ctx, p, task, head, output)
}
#[test]
fn diagnostic_completion_preserves_failed_intervention_and_does_not_approve_delivery() {
    let (tmp, mut store, ctx, p, task, head, out) = fixture(true);
    assert!(ctx.render().contains("MACC_DIAGNOSTIC_RESULT:"));
    let mut legacy = ctx.clone();
    legacy.schema_version = 3;
    assert!(!legacy.render().contains("MACC_DIAGNOSTIC_RESULT:"));
    assert!(store
        .complete(&ctx.plan_id, &ctx.attempt_id, "review", &out, &p)
        .unwrap_err()
        .to_string()
        .contains("DM_COMPLIANCE"));
    let delivery = store
        .diagnostic_delivery(&ctx, &task, tmp.path(), &head, &out)
        .unwrap()
        .unwrap();
    assert!(delivery.completed);
    assert!(!delivery.feature_accepted);
    assert!(!delivery.release_authorized);
    assert_eq!(delivery.unverified_decisions.len(), 1);
    let history = store.context_history(&ctx.plan_id, "A").unwrap();
    assert_eq!(
        history[0]["receipt"]["completion_status"],
        "diagnostic_complete_external_pending"
    );
    assert_eq!(
        history[0]["receipt"]["compliance"][&delivery.unverified_decisions[0]]["status"],
        "needs_human_review"
    );
    let d = store.decision(&delivery.unverified_decisions[0]).unwrap();
    assert_eq!(
        d.resolution.unwrap().selected_option_id.as_deref(),
        Some("assist".into())
    );
    assert_eq!(
        store
            .pending_diagnostic(&ctx.plan_id, &ctx.attempt_id, tmp.path())
            .unwrap(),
        Some(delivery)
    );
    assert!(store
        .attest_review_artifact(&ctx.plan_id, &ctx.attempt_id, tmp.path(), &head)
        .unwrap_err()
        .to_string()
        .contains("not accepted"));
    // A new failed completion invalidates the prior native diagnostic receipt.
    assert!(store
        .complete(&ctx.plan_id, &ctx.attempt_id, "review", "", &p)
        .is_err());
    assert!(store
        .pending_diagnostic(&ctx.plan_id, &ctx.attempt_id, tmp.path())
        .unwrap()
        .is_none());
}
#[test]
fn ordinary_bindings_and_missing_contract_cannot_gain_diagnostic_exception() {
    let (tmp, mut store, ctx, p, mut task, head, out) = fixture(false);
    assert!(store
        .complete(&ctx.plan_id, &ctx.attempt_id, "review", &out, &p)
        .is_err());
    assert!(store
        .diagnostic_delivery(&ctx, &task, tmp.path(), &head, &out)
        .unwrap()
        .is_none());
    task["execution_contract"] = json!({});
    assert!(store
        .diagnostic_delivery(&ctx, &task, tmp.path(), &head, &out)
        .unwrap()
        .is_none());
}
#[test]
fn changed_or_falsified_report_and_review_commit_are_rejected() {
    let (tmp, mut store, ctx, p, task, head, out) = fixture(true);
    assert!(store
        .complete(&ctx.plan_id, &ctx.attempt_id, "review", &out, &p)
        .is_err());
    let forged = out.replace("\"feature_accepted\":false", "\"feature_accepted\":true");
    assert!(store
        .diagnostic_delivery(&ctx, &task, tmp.path(), &head, &forged)
        .is_err());
    let forged = out.replace("reports/A.json", "../reports/A.json");
    assert!(store
        .diagnostic_delivery(&ctx, &task, tmp.path(), &head, &forged)
        .is_err());
    std::fs::write(tmp.path().join("reports/A.json"), "changed").unwrap();
    assert!(store
        .diagnostic_delivery(&ctx, &task, tmp.path(), &head, &out)
        .is_err());
    git(tmp.path(), &["add", "."]);
    git(tmp.path(), &["commit", "-qm", "Review must not commit"]);
    assert!(store
        .diagnostic_delivery(&ctx, &task, tmp.path(), &head, &out)
        .is_err());
}

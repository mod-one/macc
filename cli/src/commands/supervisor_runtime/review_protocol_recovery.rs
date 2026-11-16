//! A runtime update may retry a retained review; it cannot manufacture acceptance.
use macc_core::{
    coordinator::{decisions, model::Task},
    supervisor::incident::{self, Incident},
    ProjectPaths, Result,
};
use serde_json::{json, Value};
use std::path::Path;
fn eligible(task: &Task) -> bool {
    incident::repair_candidate(task)
        && macc_core::coordinator::phase_diagnostics::retained_review_protocol(task)
}
fn changed_or_requested(trigger: &Value) -> bool {
    trigger["runtime_updated"] == true || trigger["reason"] == "explicit_retry"
}
/// Legacy MACC classified complete negative compliance as E903. A runtime
/// update may issue a fresh review, never turn its old objection into a pass.
fn negative_receipt(paths: &ProjectPaths, task: &Task) -> Result<bool> {
    if !incident::repair_candidate(task)
        || task.task_runtime.current_phase.as_deref() != Some("review")
        || task.task_runtime.last_error_code.as_deref() != Some("E903")
        || task.task_runtime.last_error_origin.as_deref() != Some("decisions")
        || task.worktree_path().is_none()
        || task.task_runtime.pid.is_some()
    {
        return Ok(false);
    }
    let Some(pin) = task.task_runtime.extra.get("decision_context") else {
        return Ok(false);
    };
    let (store, plan) = decisions::active_plan(&paths.root)?;
    if pin["plan_id"] != plan.id || pin["phase"] != "review" {
        return Ok(false);
    }
    let Some(attempt) = pin["attempt_id"].as_str() else {
        return Ok(false);
    };
    let context = store.read_context(&plan.id, attempt, "review")?;
    if context.task_id != task.id || pin["digest"] != context.context_digest {
        return Ok(false);
    }
    let history = store.context_history(&plan.id, &task.id)?;
    let Some(entry) = history
        .iter()
        .find(|e| e["context"]["attempt_id"] == attempt && e["context"]["phase"] == "review")
    else {
        return Ok(false);
    };
    let receipt = &entry["receipt"];
    Ok(receipt["acknowledged"] == true
        && receipt["context_digest"] == context.context_digest
        && receipt["completion_status"] == "compliance_failed"
        && context
            .bindings
            .iter()
            .any(|b| receipt["compliance"][&b.id]["status"] == "fail")
        && context.bindings.iter().all(|b| {
            b.resolution.as_ref().is_some_and(|r| r.advisory)
                || receipt["compliance"][&b.id]["status"]
                    .as_str()
                    .is_some_and(|s| matches!(s, "pass" | "fail"))
        }))
}
pub(super) fn recover(
    paths: &ProjectPaths,
    incident: &Incident,
    dir: &Path,
) -> Result<Option<Vec<String>>> {
    let trigger_path = dir.join("trigger.json");
    let trigger: Value = match std::fs::read(&trigger_path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            macc_core::MaccError::Validation(format!("Invalid supervisor retry trigger: {e}"))
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(macc_core::MaccError::Io {
                path: trigger_path.display().to_string(),
                action: "read review recovery trigger".into(),
                source,
            })
        }
    };
    if !changed_or_requested(&trigger) {
        return Ok(None);
    }
    let roots: Vec<_> = incident
        .tasks
        .iter()
        .filter(|t| incident::repair_candidate(t))
        .collect();
    if roots.is_empty() {
        return Ok(None);
    }
    for task in &roots {
        if !eligible(task) && !negative_receipt(paths, task)? {
            return Ok(None);
        }
    }
    let mut ids = Vec::new();
    let mut budgets = Vec::new();
    for task in roots {
        let Some(budget) = decisions::context_budget(&paths.root, &task.id, "review")? else {
            return Ok(None);
        };
        if !budget.fits {
            return Ok(None);
        }
        ids.push(task.id.clone());
        budgets.push(budget);
    }
    super::report::write_json(
        &dir.join("review-protocol-recovery.json"),
        &json!({"code":"MACC-REVIEW-PROTOCOL-REFRESH","tasks":ids,"budgets":budgets,"action":"Retry the retained review with a newly issued context; no prior receipt, feature acceptance or compliance is synthesized","trigger":trigger}),
    )?;
    Ok(Some(ids))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_negative_review_can_only_request_a_fresh_review_from_an_exact_native_receipt() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let paths = ProjectPaths::from_root(root);
        std::fs::create_dir_all(&paths.macc_dir).unwrap();
        macc_core::git::run_git_output_mapped(
            root,
            &["init", "-q", "-b", "main"],
            "review migration fixture",
        )
        .unwrap();
        std::fs::write(
            root.join("prd.json"),
            json!({"tasks":[{"id":"A","title":"Retained delivery"}]}).to_string(),
        )
        .unwrap();
        let p = decisions::Policy {
            enabled: true,
            human_question_min_risk: decisions::Consultation::NeverAsk,
            ..Default::default()
        };
        let mut cfg = macc_core::config::CanonicalConfig::default();
        let c = cfg
            .automation
            .coordinator
            .get_or_insert_with(Default::default);
        c.reference_branch = Some("main".into());
        c.prd_file = Some("prd.json".into());
        c.decisions = Some(p.clone());
        macc_core::save_canonical_config(&paths, &cfg).unwrap();
        let (mut store, plan) = decisions::active_plan(root).unwrap();
        let request=serde_json::from_value(json!({"idempotency_key":"legacy-negative","task_id":"A","kind":"clarification","question":"Which internal label?","context":"Existing conventions","investigated":["Read source"],"recommendation":"Existing label","risk":"low","impact":{"evidence":["Local convention"]}})).unwrap();
        let d = store
            .submit(&plan, request, "agent:A", "agent", &p)
            .unwrap();
        assert_eq!(d.status, "resolved");
        let plan = store.plan(&plan.id).unwrap();
        let (context, _) = store
            .issue(&plan, "A", "legacy-review", "review", root, &p)
            .unwrap();
        store
            .issued(&plan.id, "legacy-review", "review", &context.render(), &p)
            .unwrap();
        // Older releases persisted this exact receipt even when a verdict was
        // absent. Recovery only reissues a review; it never assumes its verdict.
        let output = format!(
            "MACC_DECISION_ACK: {}\nMACC_DECISION_COMPLIANCE: {}",
            context.context_digest,
            json!({d.id:{"status":"fail","evidence":"Independent reproduction violated the selected invariant"}})
        );
        assert!(store
            .complete(&plan.id, "legacy-review", "review", &output, &p)
            .is_err());
        let mut incident:Incident=serde_json::from_value(json!({"id":"legacy","run_id":"old","epoch":1,"reason":"DM_COMPLIANCE","recent_events":[],"tasks":[{"id":"A","state":"blocked","worktree":{"worktree_path":"/fixture/retained","branch":"A"},"task_runtime":{"current_phase":"review","last_error_code":"E903","last_error_origin":"decisions","decision_context":{"plan_id":plan.id,"attempt_id":"legacy-review","phase":"review","digest":context.context_digest}}}]})).unwrap();
        let dir = paths.macc_dir.join("log/migration");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("trigger.json"),
            json!({"runtime_updated":false}).to_string(),
        )
        .unwrap();
        assert!(recover(&paths, &incident, &dir).unwrap().is_none());
        std::fs::write(
            dir.join("trigger.json"),
            json!({"runtime_updated":true}).to_string(),
        )
        .unwrap();
        assert_eq!(
            recover(&paths, &incident, &dir).unwrap(),
            Some(vec!["A".into()])
        );
        assert_eq!(
            store.context_history(&plan.id, "A").unwrap()[0]["receipt"]["completion_status"],
            "compliance_failed"
        );
        incident.tasks[0]
            .task_runtime
            .extra
            .get_mut("decision_context")
            .unwrap()["digest"] = json!("forged");
        assert!(recover(&paths, &incident, &dir).unwrap().is_none());
    }
    #[test]
    fn only_retained_protocol_reviews_after_update_or_explicit_retry_can_resume() {
        let mut task:Task=serde_json::from_value(json!({"id":"A","state":"blocked","worktree":{"worktree_path":"/fixture/retained","branch":"task-A"},"task_runtime":{"current_phase":"review","last_error_code":"E103","last_error_origin":"output_protocol"}})).unwrap();
        assert!(eligible(&task));
        assert!(!changed_or_requested(&json!({"runtime_updated":false})));
        assert!(changed_or_requested(&json!({"runtime_updated":true})));
        assert!(changed_or_requested(&json!({"reason":"explicit_retry"})));
        task.task_runtime.current_phase = Some("dev".into());
        assert!(!eligible(&task));
        task.task_runtime.current_phase = Some("review".into());
        task.task_runtime.last_error_code = Some("E903".into());
        assert!(!eligible(&task));
    }
    #[test]
    fn readiness_preview_records_the_trigger_without_issuing_or_accepting_a_context() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        std::fs::create_dir_all(tmp.path().join(".macc")).unwrap();
        let out = macc_core::git::run_git_output_mapped(
            tmp.path(),
            &["init", "-q", "-b", "main"],
            "initialize recovery fixture",
        )
        .unwrap();
        assert!(out.status.success());
        let mut config = macc_core::config::CanonicalConfig::default();
        let c = config
            .automation
            .coordinator
            .get_or_insert_with(Default::default);
        c.reference_branch = Some("main".into());
        c.prd_file = Some("prd.json".into());
        c.decisions = Some(decisions::Policy {
            enabled: true,
            ..Default::default()
        });
        std::fs::write(&paths.config_path, config.to_yaml().unwrap()).unwrap();
        std::fs::write(
            tmp.path().join("prd.json"),
            json!({"tasks":[{"id":"A","title":"Retained implementation"}]}).to_string(),
        )
        .unwrap();
        let incident: Incident = serde_json::from_value(json!({"id":"incident","run_id":"run","epoch":1,"reason":"Missing ACK","recent_events":[],"tasks":[{"id":"A","state":"blocked","worktree":{"worktree_path":"/fixture/retained","branch":"task-A"},"task_runtime":{"current_phase":"review","last_error_code":"E103","last_error_origin":"output_protocol"}}]})).unwrap();
        let dir = tmp.path().join(".macc/log/recovery");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("trigger.json"),
            json!({"runtime_updated":false}).to_string(),
        )
        .unwrap();
        assert!(recover(&paths, &incident, &dir).unwrap().is_none());
        std::fs::write(
            dir.join("trigger.json"),
            json!({"runtime_updated":true}).to_string(),
        )
        .unwrap();
        assert_eq!(
            recover(&paths, &incident, &dir).unwrap(),
            Some(vec!["A".into()])
        );
        let (store, plan) = decisions::active_plan(tmp.path()).unwrap();
        assert!(store.context_history(&plan.id, "A").unwrap().is_empty());
        let proof: Value = serde_json::from_slice(
            &std::fs::read(dir.join("review-protocol-recovery.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(proof["code"], "MACC-REVIEW-PROTOCOL-REFRESH");
        assert_eq!(proof["budgets"][0]["fits"], true);
    }
}

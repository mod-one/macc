//! A native review requiring governance is not a missing device or source repair.
use super::repair::Diagnosis;
use macc_core::{
    coordinator::decisions,
    supervisor::incident::{self, Incident},
    Result,
};
pub(super) fn diagnosis(root: &std::path::Path, incident: &Incident) -> Result<Option<Diagnosis>> {
    let tasks = incident
        .tasks
        .iter()
        .filter(|t| incident::repair_candidate(t))
        .collect::<Vec<_>>();
    if tasks.is_empty() {
        return Ok(None);
    }
    let mut blocks = vec![];
    for task in &tasks {
        let Some(block) = decisions::review_compliance_for_task(root, task)? else {
            return Ok(None);
        };
        if !block.needs_human_review()
            || block
                .findings
                .iter()
                .any(|f| matches!(f.decision_kind.as_str(), "intervention" | "authorization"))
        {
            return Ok(None);
        }
        blocks.push(block);
    }
    Ok(Some(Diagnosis {
        summary: format!(
            "DM_COMPLIANCE: {}",
            blocks
                .iter()
                .map(|b| format!("{}: {}. {}", b.task_id, b.describe(), b.next_action()))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        repairable: false,
        requires_human: true,
        restart_only: false,
        task_ids: tasks.iter().map(|t| t.id.clone()).collect(),
        // These are governed decision objections, not external hardware/access.
        external_prerequisites: vec![],
        retry_at: None,
        unavailable_tool: None,
        related_task_ids: vec![],
        macc_findings: vec![],
        macc_repair_required: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::{coordinator::model::Task, ProjectPaths};
    use serde_json::json;
    #[test]
    fn exact_native_human_review_is_diagnosed_without_source_repair_or_fabricated_hardware() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let paths = ProjectPaths::from_root(root);
        std::fs::create_dir_all(&paths.macc_dir).unwrap();
        macc_core::git::run_git_output_mapped(
            root,
            &["init", "-q", "-b", "main"],
            "compliance fixture",
        )
        .unwrap();
        std::fs::write(
            root.join("prd.json"),
            json!({"tasks":[{"id":"A","title":"Receipt task"}]}).to_string(),
        )
        .unwrap();
        let mut config = macc_core::config::CanonicalConfig::default();
        let p = decisions::Policy {
            enabled: true,
            human_question_min_risk: decisions::Consultation::NeverAsk,
            ..Default::default()
        };
        let c = config
            .automation
            .coordinator
            .get_or_insert_with(Default::default);
        c.prd_file = Some("prd.json".into());
        c.reference_branch = Some("main".into());
        c.decisions = Some(p.clone());
        macc_core::save_canonical_config(&paths, &config).unwrap();
        let (mut store, plan) = decisions::active_plan(root).unwrap();
        let request=serde_json::from_value(json!({"idempotency_key":"receipt-choice","task_id":"A","kind":"clarification","question":"Which implementation preserves the contract?","context":"Normative contract","investigated":["Read contract"],"recommendation":"Preserve the contract","risk":"high","impact":{"evidence":["Canonical invariant"]}})).unwrap();
        let d = store
            .submit(&plan, request, "agent:A", "agent", &p)
            .unwrap();
        let plan = store.plan(&plan.id).unwrap();
        let (context, _) = store
            .issue(&plan, "A", "review", "review", root, &p)
            .unwrap();
        store
            .issued(&plan.id, "review", "review", &context.render(), &p)
            .unwrap();
        let evidence="src/issuer.ts:26-41: the selected decision conflicts with the normative lock invariant; a governed revision is needed";
        let output=format!("MACC_DECISION_ACK: {}\nMACC_DECISION_COMPLIANCE: {}\nREVIEW_VERDICT: CHANGES_REQUESTED",context.context_digest,json!({d.id.clone():{"status":"needs_human_review","evidence":evidence}}));
        let error = store
            .complete(&plan.id, "review", "review", &output, &p)
            .unwrap_err();
        let task:Task=serde_json::from_value(json!({"id":"A","state":"blocked","task_runtime":{"current_phase":"review","last_error_code":"E903","last_error_origin":"decisions","last_error_message":error.to_string(),"decision_context":{"plan_id":plan.id,"attempt_id":"review","phase":"review","digest":context.context_digest}}})).unwrap();
        let mut incident = Incident {
            id: "incident".into(),
            run_id: "run".into(),
            epoch: 1,
            reason: error.to_string(),
            tasks: vec![task],
            recent_events: vec![],
            run_status: Some("blocked".into()),
        };
        let before = store.context_history(&plan.id, "A").unwrap();
        let native = diagnosis(root, &incident).unwrap().unwrap();
        assert!(native.requires_human && !native.repairable && !native.restart_only);
        assert!(native.external_prerequisites.is_empty());
        assert!(
            native.summary.contains(evidence) && native.summary.contains(&format!("{} v1", d.id))
        );
        let report:super::super::report::Intervention=serde_json::from_value(json!({"incident":incident,"attempt":1,"status":"escalated","tool":null,"diagnosis":native,"worktree":null,"validation":[],"commit":null,"restart_pid":null,"detail":error.to_string()})).unwrap();
        let action = super::super::policy::next_action(
            &super::super::policy::Pending::default(),
            &report,
            3,
        )
        .unwrap();
        assert!(action.contains("Open Decisions") && action.contains("governed revision"));
        assert!(!action.contains("Supply the recorded external devices"));
        assert_eq!(store.context_history(&plan.id, "A").unwrap(), before);
        incident.tasks[0]
            .task_runtime
            .extra
            .get_mut("decision_context")
            .unwrap()["digest"] = json!("wrong");
        assert!(
            diagnosis(root, &incident).unwrap().is_none(),
            "cached or model-provided text cannot trigger the native diagnosis"
        );
        // Actual inaccessible actions keep their separate external-capability
        // path even if a review also requests independent human verification.
        let external=serde_json::from_value(json!({"idempotency_key":"device","task_id":"A","kind":"intervention","question":"Connect the test device","context":"No physical access","investigated":["Checked device visibility"],"recommendation":"Connect the device","risk":"high","impact":{"evidence":["No device available"]},"intervention":{"action":"Connect test device","inaccessible_reason":"No physical access","expected_result":"Device visible to the independent test"}})).unwrap();
        let mut consultation = p.clone();
        consultation.human_question_min_risk = decisions::Consultation::High;
        config.automation.coordinator.as_mut().unwrap().decisions = Some(consultation.clone());
        macc_core::save_canonical_config(&paths, &config).unwrap();
        let external = store
            .submit(
                &store.plan(&plan.id).unwrap(),
                external,
                "agent:A",
                "agent",
                &consultation,
            )
            .unwrap();
        assert_eq!(external.status, "pending_human");
        store
            .answer_details(
                &external.id,
                external.version,
                &decisions::AnswerInput {
                    text: "Device connected".into(),
                    fields: std::collections::BTreeMap::from([
                        ("performed".into(), "yes".into()),
                        ("result".into(), "The test device is visible".into()),
                    ]),
                    ..Default::default()
                },
                "Human",
                &consultation,
            )
            .unwrap();
        config.automation.coordinator.as_mut().unwrap().decisions = Some(p.clone());
        macc_core::save_canonical_config(&paths, &config).unwrap();
        let plan = store.plan(&plan.id).unwrap();
        let (context, _) = store
            .issue(&plan, "A", "physical-review", "review", root, &p)
            .unwrap();
        store
            .issued(&plan.id, "physical-review", "review", &context.render(), &p)
            .unwrap();
        let output=format!("MACC_DECISION_ACK: {}\nMACC_DECISION_COMPLIANCE: {}\nREVIEW_VERDICT: CHANGES_REQUESTED",context.context_digest,json!({d.id:{"status":"pass","evidence":"Independent check verified the invariant"},external.id:{"status":"needs_human_review","evidence":"The physical connection still needs independent observation"}}));
        assert!(store
            .complete(&plan.id, "physical-review", "review", &output, &p)
            .is_err());
        incident.tasks[0].task_runtime.extra.insert("decision_context".into(),json!({"plan_id":plan.id,"attempt_id":"physical-review","phase":"review","digest":context.context_digest}));
        let physical = decisions::review_compliance_for_task(root, &incident.tasks[0])
            .unwrap()
            .unwrap();
        assert!(physical
            .findings
            .iter()
            .any(|f| f.decision_kind == "intervention"));
        assert!(
            diagnosis(root, &incident).unwrap().is_none(),
            "a real physical prerequisite is not reclassified as an implementation decision"
        );
    }
}

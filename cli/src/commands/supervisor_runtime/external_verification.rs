//! Native receipt evidence needs no paid diagnosis or speculative source repair.
use super::repair::Diagnosis;
use macc_core::{
    coordinator::decisions::DiagnosticDelivery,
    supervisor::incident::{self, Incident},
};
pub(super) fn diagnosis(incident: &Incident) -> Option<Diagnosis> {
    let tasks: Vec<_> = incident
        .tasks
        .iter()
        .filter(|t| incident::repair_candidate(t))
        .collect();
    if tasks.is_empty() {
        return None;
    }
    let mut prerequisites = Vec::new();
    let mut ids = Vec::new();
    for task in tasks {
        let delivery: DiagnosticDelivery =
            serde_json::from_value(task.task_runtime.extra.get("diagnostic_delivery")?.clone())
                .ok()?;
        if !delivery.completed
            || delivery.feature_accepted
            || delivery.release_authorized
            || delivery.unverified_decisions.is_empty()
            || task.task_runtime.last_error_origin.as_deref() != Some("decisions")
        {
            return None;
        }
        prerequisites.push(format!("Independently verify the selected assistance for {} (decisions {}), or explicitly reconcile that selection. Diagnostic report {} is complete; feature acceptance and release remain withheld.",task.id,delivery.unverified_decisions.join(", "),delivery.report_path));
        ids.push(task.id.clone());
    }
    Some(Diagnosis {summary:"Diagnostic reports delivered; external intervention verification remains pending. No application repair or blind restart is appropriate.".into(),repairable:false,requires_human:true,restart_only:false,task_ids:ids,external_prerequisites:prerequisites,retry_at:None,unavailable_tool:None,related_task_ids:Vec::new(),macc_findings:Vec::new(),macc_repair_required:false})
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn native_external_pending_receipt_avoids_repair_and_restart_but_not_unrelated_incidents() {
        let mut task = macc_core::coordinator::model::Task {
            id: "A".into(),
            state: "blocked".into(),
            ..Default::default()
        };
        task.task_runtime
            .set_last_error_details("E903", "decisions", "DM_EXTERNAL_VERIFICATION");
        task.task_runtime.extra.insert("diagnostic_delivery".into(),json!({"report_path":"reports/A.json","report_digest":"sha256:test","completed":true,"feature_accepted":false,"release_authorized":false,"unverified_decisions":["assist-1"]}));
        let mut incident = Incident {
            id: "incident".into(),
            run_id: "run".into(),
            epoch: 1,
            reason: "external pending".into(),
            tasks: vec![task],
            recent_events: vec![],
            run_status: Some("blocked".into()),
        };
        let d = diagnosis(&incident).unwrap();
        assert!(d.requires_human);
        assert!(!d.repairable);
        assert!(!d.restart_only);
        assert!(d.external_prerequisites[0].contains("assist-1"));
        incident.tasks[0].task_runtime.last_error_origin = Some("runner".into());
        assert!(diagnosis(&incident).is_none());
        incident.tasks[0].task_runtime.last_error_origin = Some("decisions".into());
        incident.tasks[0]
            .task_runtime
            .extra
            .get_mut("diagnostic_delivery")
            .unwrap()["feature_accepted"] = json!(true);
        assert!(diagnosis(&incident).is_none());
    }
}

/// Resolve known native waits without a tool call, inferred consent or a repair.
pub(super) fn pending_diagnosis(
    root: &std::path::Path,
    incident: &Incident,
) -> macc_core::Result<Option<Diagnosis>> {
    let tasks: Vec<_> = incident
        .tasks
        .iter()
        .filter(|t| incident::repair_candidate(t))
        .collect();
    if tasks.is_empty() {
        return Ok(None);
    }
    let mut prerequisites = vec![];
    let mut ids = vec![];
    for task in tasks {
        let Some(wait) = macc_core::coordinator::decisions::pending_for_task(root, &task.id)?
        else {
            return Ok(None);
        };
        // A stopped external prerequisite gets a diagnosis: it may be a
        // project defect. A real human decision still cannot be repaired away.
        if wait.status == "blocked_external" {
            return Ok(None);
        }
        prerequisites.push(format!("{} v{} ({}): {}. Open Decisions; answer or revise this exact request. Retained task work is preserved.",wait.id,wait.version,wait.status,wait.question));
        ids.push(task.id.clone());
    }
    Ok(Some(Diagnosis{summary:"DM_PENDING: native human decision required; source repair and unchanged retries cannot answer it.".into(),repairable:false,requires_human:true,restart_only:false,task_ids:ids,external_prerequisites:prerequisites,retry_at:None,unavailable_tool:None,related_task_ids:vec![],macc_findings:vec![],macc_repair_required:false}))
}

/// Capture exact decision revisions before any source mutation; never clear an
/// actual external capability or human gate through an unrelated Git repair.
pub(super) fn source_repair_refs(
    root: &std::path::Path,
    ids: &[String],
) -> macc_core::Result<Vec<macc_core::coordinator::decisions::DecisionRef>> {
    use macc_core::coordinator::decisions as dm;
    let mut refs = std::collections::BTreeMap::new();
    for id in ids {
        let Some(wait) = dm::pending_for_task(root, id)? else {
            continue;
        };
        let store = dm::Store::open(root)?;
        let d = store.decision(&wait.id)?;
        if dm::policy(root)?.human_question_min_risk != dm::Consultation::NeverAsk {
            return Err(macc_core::MaccError::Validation(
                "Source prerequisite recovery requires the current native delegation policy".into(),
            ));
        }
        if d.status != "blocked_external"
            || d.request.kind != "intervention"
            || d.request
                .intervention
                .as_ref()
                .is_none_or(|i| i.source_repair_paths.is_empty())
        {
            return Err(macc_core::MaccError::Validation(format!("Blocked: external intervention required. Decision {} has no explicit application source recovery scope; diagnosis retained, work preserved. Resolve or revise this exact request; source edits cannot substitute for an external action",d.id)));
        }
        refs.insert(
            d.id.clone(),
            dm::DecisionRef {
                id: d.id,
                version: d.version,
            },
        );
    }
    Ok(refs.into_values().collect())
}

#[cfg(test)]
mod source_routing_tests {
    use super::*;
    use macc_core::{coordinator::decisions as dm, coordinator::model::TaskRegistry, ProjectPaths};
    fn native_wait(source: bool) -> (tempfile::TempDir, Incident) {
        let root = tempfile::tempdir().unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "Test"],
            vec!["config", "user.email", "test@example.test"],
        ] {
            assert!(macc_core::git::run_git_output_mapped(
                root.path(),
                &args,
                "initialize native prerequisite fixture",
            )
            .unwrap()
            .status
            .success());
        }
        let paths = ProjectPaths::from_root(root.path());
        std::fs::create_dir_all(&paths.macc_dir).unwrap();
        let mut config = macc_core::config::CanonicalConfig::default();
        let mut p = dm::Policy {
            enabled: true,
            human_question_min_risk: dm::Consultation::NeverAsk,
            ..Default::default()
        };
        p.investigation.max_rounds = 1;
        let c = config
            .automation
            .coordinator
            .get_or_insert_with(Default::default);
        c.prd_file = Some("prd.json".into());
        c.reference_branch = Some("main".into());
        c.decisions = Some(p.clone());
        std::fs::write(&paths.config_path, config.to_yaml().unwrap()).unwrap();
        let graph = serde_json::json!({"tasks":[{"id":"A","state":"blocked"}]});
        std::fs::write(root.path().join("prd.json"), graph.to_string()).unwrap();
        let mut registry = TaskRegistry::from_value(&graph).unwrap();
        macc_core::coordinator::state::coordinator_state_registry_save(
            root.path(),
            &Default::default(),
            &graph,
        )
        .unwrap();
        let mut store = dm::Store::open(root.path()).unwrap();
        let plan = store
            .ensure_plan(
                root.path(),
                std::path::Path::new("prd.json"),
                "main",
                &registry,
            )
            .unwrap();
        let request=serde_json::from_value(serde_json::json!({"idempotency_key":"request","task_id":"A","kind":"intervention","question":"Missing prerequisite","context":"Checked source and access","recommendation":"Verify before resuming","investigated":["Read requirements"],"impact":{"evidence":["Missing capability"]},"intervention":{"action":"Restore prerequisite","inaccessible_reason":"Unavailable to this worker","expected_result":"Verified capability","source_repair_paths":if source {vec!["src/api.rs"]} else {vec![]}}})).unwrap();
        let d = store
            .submit(&plan, request, "agent:A", "agent", &p)
            .unwrap();
        let wait = dm::pending_for_task(root.path(), "A").unwrap().unwrap();
        assert_eq!(wait.id, d.id);
        wait.apply(&mut registry.tasks[0].task_runtime);
        (
            root,
            Incident {
                id: "incident".into(),
                run_id: "run".into(),
                epoch: 1,
                reason: "Blocked prerequisite".into(),
                tasks: registry.tasks,
                recent_events: vec![],
                run_status: Some("blocked".into()),
            },
        )
    }
    #[test]
    fn blocked_external_is_diagnosed_but_only_an_explicit_source_scope_can_enter_repair() {
        let (root, incident) = native_wait(false);
        assert!(
            pending_diagnosis(root.path(), &incident).unwrap().is_none(),
            "do not label every external block as an answered human question"
        );
        assert!(
            source_repair_refs(root.path(), &["A".into()]).is_err(),
            "legacy/physical capabilities cannot be repaired by arbitrary source edits"
        );
        let (root, incident) = native_wait(true);
        assert!(pending_diagnosis(root.path(), &incident).unwrap().is_none());
        assert_eq!(
            source_repair_refs(root.path(), &["A".into()])
                .unwrap()
                .len(),
            1
        );
    }
}

pub(super) fn exact_requirements(
    root: &std::path::Path,
    ids: &[String],
) -> macc_core::Result<Vec<macc_core::coordinator::decisions::Decision>> {
    use macc_core::coordinator::decisions as dm;
    let mut requests = std::collections::BTreeMap::new();
    for id in ids {
        if let Some(wait) = dm::pending_for_task(root, id)? {
            let store = dm::Store::open(root)?;
            requests.insert(wait.id.clone(), store.decision(&wait.id)?);
        }
    }
    Ok(requests.into_values().collect())
}

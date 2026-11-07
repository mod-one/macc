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

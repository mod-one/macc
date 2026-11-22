//! Bind supervisor-authored proposals to MACC's pre-diagnosis snapshot.
use super::repair::{scope_diagnosis, Diagnosis};
use macc_core::{
    config::CoordinatorConfig,
    coordinator::resume_check::{Category, DiagnosisSnapshot, Proposal},
    supervisor::incident::Incident,
    MaccError, Result,
};
use serde::Deserialize;
use std::path::Path;

pub(super) fn recorded_wait(
    root: &Path,
    config: Option<&CoordinatorConfig>,
    incident: &Incident,
) -> Option<Diagnosis> {
    if incident.tasks.is_empty()
        || incident
            .tasks
            .iter()
            .any(|t| t.task_runtime.resume_check.is_none())
    {
        return None;
    }
    let prd = macc_core::prd_queue::active_path(root, config, None).ok()?;
    let mut ids = std::collections::BTreeMap::new();
    let mut conditions = Vec::new();
    for task in &incident.tasks {
        let check = macc_core::coordinator::resume_check::waiting_check(root, &prd, task)?;
        ids.insert(task.id.clone(), check.id.clone());
        let detail = check
            .last
            .as_ref()
            .map_or(check.criterion.as_str(), |r| r.detail.as_str());
        conditions.push(format!("{} ({}): {detail}", task.id, check.phase));
    }
    Some(Diagnosis {
        summary: format!(
            "{}. Restart the coordinator after restoring the condition.",
            conditions.join("; ")
        ),
        task_ids: ids.keys().cloned().collect(),
        resume_check_ids: ids,
        repairable: false,
        requires_human: false,
        restart_only: false,
        retry_at: None,
        unavailable_tool: None,
        related_task_ids: vec![],
        macc_findings: vec![],
        macc_repair_required: false,
        external_prerequisites: vec![],
    })
}

#[derive(Deserialize)]
pub(super) struct Response {
    #[serde(flatten)]
    diagnosis: Diagnosis,
    #[serde(default)]
    resume_checks: Vec<Proposal>,
}

impl Response {
    pub(super) fn bind(
        self,
        bindings: &DiagnosisSnapshot,
        root: &Path,
        config: Option<&CoordinatorConfig>,
        incident: &Incident,
    ) -> Result<Diagnosis> {
        let Self {
            mut diagnosis,
            resume_checks,
        } = self;
        diagnosis.resume_check_ids.clear();
        scope_diagnosis(&mut diagnosis, incident);
        if resume_checks.is_empty() {
            return Ok(diagnosis);
        }

        if resume_checks
            .iter()
            .any(|p| !diagnosis.task_ids.contains(&p.task_id))
        {
            return Err(MaccError::Validation(
                "Resume check must name a diagnosed incident root".into(),
            ));
        }
        let ids = bindings.register(root, config, &resume_checks)?;
        route_registered(&mut diagnosis, incident, &resume_checks, ids);
        Ok(diagnosis)
    }
}

fn route_registered(
    diagnosis: &mut Diagnosis,
    incident: &Incident,
    resume_checks: &[Proposal],
    ids: Vec<String>,
) {
    let mut corrections = Vec::new();
    for (proposal, id) in resume_checks.iter().zip(ids) {
        let note = match proposal.category {
            Category::TechnicalCondition => {
                let note = format!(
                    "{}: {}. Restart the coordinator to check {} and resume {} when ready ({}).",
                    proposal.task_id, proposal.criterion, proposal.target, proposal.phase, id
                );
                diagnosis
                    .resume_check_ids
                    .insert(proposal.task_id.clone(), id);
                note
            }
            Category::ReviewCorrection => {
                if incident
                    .tasks
                    .iter()
                    .any(|t| t.id == proposal.task_id && t.state == "changes_requested")
                {
                    corrections.push(proposal.task_id.clone());
                }
                format!(
                    "{}: address the review findings in the retained correction workflow: {}",
                    proposal.task_id, proposal.criterion
                )
            }
            category => {
                let note = format!(
                    "{}: {} — {}",
                    proposal.task_id,
                    category.label(),
                    proposal.criterion
                );
                diagnosis.external_prerequisites.push(note.clone());
                note
            }
        };
        diagnosis.summary.push_str(&format!(" {note}"));
    }
    // A restart or source repair for another incident root must never requeue a checked task.
    if diagnosis
        .task_ids
        .iter()
        .all(|id| resume_checks.iter().any(|p| p.task_id == *id))
    {
        diagnosis.repairable = false;
        diagnosis.restart_only =
            !corrections.is_empty() && diagnosis.external_prerequisites.is_empty();
        if diagnosis.restart_only {
            diagnosis.task_ids = corrections;
        }
        diagnosis.requires_human = !diagnosis.restart_only && !diagnosis.waiting_for_checks();
    } else {
        diagnosis
            .task_ids
            .retain(|id| !resume_checks.iter().any(|p| p.task_id == *id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_a_pending_native_correction_can_restart_alongside_a_technical_wait() {
        for (state, authorized) in [("changes_requested", true), ("blocked", false)] {
            let incident: Incident = serde_json::from_value(json!({
                "id":"run-1","epoch":1,"run_id":"run","reason":"Stopped",
                "tasks":[{"id":"FIX","state":state},{"id":"DEVICE","state":"blocked"}],
                "recent_events":[]
            }))
            .unwrap();
            let mut diagnosis: Diagnosis = serde_json::from_value(json!({
                "summary":"Two prerequisites","repairable":true,"task_ids":["FIX","DEVICE"]
            }))
            .unwrap();
            let checks: Vec<Proposal> = serde_json::from_value(json!([
                {"task_id":"FIX","phase":"fix","category":"review_correction","criterion":"Address review","target":"worker-01"},
                {"task_id":"DEVICE","phase":"dev","category":"technical_condition","criterion":"Reserved device connected","target":"device-02"}
            ])).unwrap();
            route_registered(
                &mut diagnosis,
                &incident,
                &checks,
                vec!["fix-note".into(), "device-check".into()],
            );
            assert_eq!(diagnosis.restart_only, authorized);
            assert!(!diagnosis.repairable);
            assert_eq!(diagnosis.requires_human, !authorized);
            assert!(diagnosis.external_prerequisites.is_empty());
            if authorized {
                assert_eq!(diagnosis.task_ids, ["FIX"]);
            }
        }
    }
}

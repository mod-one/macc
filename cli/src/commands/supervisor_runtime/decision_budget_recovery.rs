//! A configuration/projection recovery must not create an application source repair.
use macc_core::{
    coordinator::decisions,
    supervisor::incident::{self, Incident},
    ProjectPaths, Result,
};
use std::path::Path;

fn budget_root(task: &macc_core::coordinator::model::Task) -> bool {
    if !incident::repair_candidate(task)
        || task.task_runtime.current_phase.as_deref() != Some("dev")
        || task.task_runtime.pid.is_some()
    {
        return false;
    }
    let runtime = &task.task_runtime;
    let recognized_origin = matches!(
        runtime.last_error_origin.as_deref(),
        Some("decision_context_budget" | "dispatch_preparation")
    );
    let recognized_code = matches!(runtime.last_error_code.as_deref(), Some("E901" | "E903"));
    recognized_origin
        && recognized_code
        && runtime.last_error_message.as_deref().is_some_and(|text| {
            text.contains(decisions::CONTEXT_BUDGET_CODE)
                || text.contains("Decision control: Mandatory decision context exceeds budget;")
        })
}

pub(super) fn recover(
    paths: &ProjectPaths,
    incident: &Incident,
    dir: &Path,
) -> Result<Option<Vec<String>>> {
    let roots: Vec<_> = incident
        .tasks
        .iter()
        .filter(|t| incident::repair_candidate(t))
        .collect();
    if roots.is_empty() || !roots.iter().all(|t| budget_root(t)) {
        return Ok(None);
    }
    let mut reports = Vec::new();
    let mut ids = Vec::new();
    let mut fits = true;
    for task in roots {
        match decisions::context_budget(&paths.root, &task.id, "dev") {
            Ok(Some(report)) => {
                fits &= report.fits;
                reports.push(serde_json::json!(report));
            }
            other => {
                fits = false;
                reports.push(serde_json::json!({"task_id":task.id,"error":match other {Err(e)=>e.to_string(),_=>"Decision management is disabled; no recovery inferred".into()}}));
            }
        }
        ids.push(task.id.clone());
    }
    super::report::write_json(
        &dir.join("decision-context-budgets.json"),
        &serde_json::json!(reports),
    )?;
    Ok(if fits { Some(ids) } else { None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::coordinator::model::Task;
    #[test]
    fn only_native_dev_budget_failures_can_use_deterministic_recovery() {
        let mut task = Task {
            id: "A".into(),
            state: "blocked".into(),
            ..Default::default()
        };
        task.task_runtime.current_phase = Some("dev".into());
        task.task_runtime.set_last_error_details(
            "E903",
            "decision_context_budget",
            decisions::CONTEXT_BUDGET_CODE,
        );
        assert!(budget_root(&task));
        task.task_runtime.set_last_error_details("E901","dispatch_preparation","Validation error: Decision control: Mandatory decision context exceeds budget; split scope");
        assert!(budget_root(&task));
        task.task_runtime.pid = Some(1);
        assert!(!budget_root(&task));
        task.task_runtime.pid = None;
        task.task_runtime.current_phase = Some("review".into());
        assert!(!budget_root(&task));
        task.task_runtime.current_phase = Some("dev".into());
        task.task_runtime
            .set_last_error_details("E901", "runner", decisions::CONTEXT_BUDGET_CODE);
        assert!(!budget_root(&task));
        task.task_runtime.set_last_error_details(
            "E903",
            "dispatch_preparation",
            "Application acceptance criteria not satisfied",
        );
        assert!(!budget_root(&task));
    }
}

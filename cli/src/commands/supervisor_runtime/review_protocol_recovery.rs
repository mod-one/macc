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
    if roots.is_empty() || !roots.iter().all(|t| eligible(t)) {
        return Ok(None);
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

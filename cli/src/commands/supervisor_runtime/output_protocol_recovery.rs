//! An updated parser may retry retained work; transcript claims never grant delivery.
use macc_core::{
    coordinator::model::Task,
    supervisor::incident::{self, Incident},
    ProjectPaths, Result,
};
use serde_json::{json, Value};
use std::path::Path;
fn eligible(task: &Task) -> bool {
    let runtime = &task.task_runtime;
    macc_core::coordinator::phase_diagnostics::missing_result_protocol(task)
        && !macc_core::coordinator::phase_diagnostics::retained_review_protocol(task)
        && incident::repair_candidate(task)
        && runtime.pid.is_none()
}
pub(super) fn recover(
    paths: &ProjectPaths,
    incident: &Incident,
    dir: &Path,
) -> Result<Option<Vec<String>>> {
    let trigger: Value = match std::fs::read(dir.join("trigger.json")) {
        Ok(raw) => serde_json::from_slice(&raw).map_err(|e| {
            macc_core::MaccError::Validation(format!("Invalid protocol recovery trigger: {e}"))
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(macc_core::MaccError::Validation(e.to_string())),
    };
    if trigger["runtime_updated"] != true && trigger["reason"] != "explicit_retry" {
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
    let pool = paths
        .macc_dir
        .join("worktree")
        .canonicalize()
        .map_err(|e| macc_core::MaccError::Validation(e.to_string()))?;
    let mut ids = Vec::new();
    for task in roots {
        let path = task
            .worktree_path()
            .or_else(|| task.task_runtime.last_worktree_path.as_deref());
        let Some(path) = path else {
            return Ok(None);
        };
        let path = Path::new(path)
            .canonicalize()
            .map_err(|e| macc_core::MaccError::Validation(e.to_string()))?;
        if path.parent() != Some(pool.as_path())
            || macc_core::git::git_common_dir(&path)?
                != macc_core::git::git_common_dir(&paths.root)?
        {
            return Ok(None);
        }
        ids.push(task.id.clone());
    }
    super::report::write_json(
        &dir.join("output-protocol-recovery.json"),
        &json!({"tasks":ids,"trigger":trigger,"action":"Resume retained work through native unblock/checkpoint verification; re-run validation and delivery. No prior success or acceptance is inferred."}),
    )?;
    Ok(Some(ids))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_fault_is_distinct_from_deliberate_failure_or_live_work() {
        let mut task:Task=serde_json::from_value(json!({"id":"A","state":"blocked","task_runtime":{"last_error_code":"E103","last_error_origin":"output_protocol","last_error_message":"Missing explicit MACC_TASK_RESULT; inspect raw transcript"}})).unwrap();
        assert!(eligible(&task));
        task.task_runtime.last_error_code = Some("E906".into());
        task.task_runtime.last_error_message = Some("Deliberate failure".into());
        assert!(!eligible(&task));
        task.task_runtime.last_error =
            Some("Missing explicit MACC_TASK_RESULT; retained work".into());
        assert!(eligible(&task));
        task.task_runtime.pid = Some(1);
        assert!(!eligible(&task));
    }
    #[test]
    fn recovery_requires_an_update_or_explicit_retry_and_retains_work_without_acceptance() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let paths = ProjectPaths::from_root(root);
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "base",
            ],
        ] {
            assert!(macc_core::git::run_git_output_mapped(
                root,
                &args,
                "protocol recovery fixture"
            )
            .unwrap()
            .status
            .success());
        }
        let worker = root.join(".macc/worktree/worker-01");
        std::fs::create_dir_all(worker.parent().unwrap()).unwrap();
        assert!(macc_core::git::run_git_output_mapped(
            root,
            &[
                "worktree",
                "add",
                "-b",
                "worker-01",
                worker.to_str().unwrap()
            ],
            "protocol recovery fixture"
        )
        .unwrap()
        .status
        .success());
        std::fs::write(worker.join("retained.txt"), "unfinished implementation").unwrap();
        let incident:Incident=serde_json::from_value(json!({"id":"incident","run_id":"run","epoch":1,"reason":"protocol failure","recent_events":[],"tasks":[{"id":"A","state":"blocked","task_runtime":{"last_worktree_path":worker,"last_error_code":"E906","last_error_message":"Missing explicit MACC_TASK_RESULT; legacy parser failure"}}]})).unwrap();
        let dir = root.join(".macc/log/protocol-recovery");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(recover(&paths, &incident, &dir).unwrap().is_none());
        std::fs::write(
            dir.join("trigger.json"),
            json!({"runtime_updated":false}).to_string(),
        )
        .unwrap();
        assert!(recover(&paths, &incident, &dir).unwrap().is_none());
        assert!(!dir.join("output-protocol-recovery.json").exists());
        for trigger in [
            json!({"runtime_updated":true}),
            json!({"reason":"explicit_retry"}),
        ] {
            std::fs::write(dir.join("trigger.json"), trigger.to_string()).unwrap();
            assert_eq!(
                recover(&paths, &incident, &dir).unwrap(),
                Some(vec!["A".into()])
            );
            let receipt: Value = serde_json::from_slice(
                &std::fs::read(dir.join("output-protocol-recovery.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(receipt["trigger"], trigger);
        }
        assert_eq!(
            std::fs::read_to_string(worker.join("retained.txt")).unwrap(),
            "unfinished implementation"
        );
        assert_eq!(incident.tasks[0].state, "blocked");
        assert!(incident.tasks[0].task_runtime.completion_kind.is_none());
    }
}

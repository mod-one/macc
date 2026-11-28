//! Inspect the retained phase against the current decision and checkpoint before launching.
use super::invalid;
use crate::coordinator::{decisions, model::TaskRegistry};
use crate::{MaccError, ProjectPaths, Result};
use std::collections::BTreeMap;

pub(super) fn inspect(
    paths: &ProjectPaths,
    registry: &TaskRegistry,
    args: &BTreeMap<String, String>,
    position: &str,
) -> Result<Vec<String>> {
    let mut decisions_seen = Vec::new();
    for task in registry.tasks.iter().filter(|t| {
        t.state == "blocked"
            && t.has_worktree_attached()
            && !t.is_human_approval_gate()
            && t.task_runtime.last_error_code.as_deref() == Some("E903")
    }) {
        let wait = crate::coordinator::phase_diagnostics::decision_wait_id(task);
        if wait.is_none() && !crate::coordinator::phase_diagnostics::context_refresh_blocked(task) {
            continue;
        }
        if decisions::pending_for_task(&paths.root, &task.id)?.is_some() {
            continue;
        }
        let history = task
            .task_runtime
            .extra
            .get("checkpoint_previous_decision")
            .and_then(|v| v.as_str());
        if let Some(id) = wait.or(history) {
            if let Some(store) = decisions::Store::inspect_registered(&paths.root)? {
                let decision = store.decision(id)?;
                decisions_seen.push(format!(
                    "{} · decision {} v{} · {}{}",
                    task.id,
                    id,
                    decision.version,
                    decision.status,
                    decision
                        .resolution
                        .as_ref()
                        .map(|r| format!(" · {}", r.text))
                        .unwrap_or_default()
                ));
            }
        }
        if let Err(error) =
            crate::session_store::checkpoints::recovery::ensure_decision_context_ready(
                &paths.root,
                &task.id,
            )
        {
            let mut current = task.clone();
            decisions::record_refresh_failure(&mut current, &error);
            let error = if &current != task {
                crate::coordinator::state::replace_phase_task(paths, args, task, &current)?;
                // Registry ownership is part of the confirmation fingerprint.
                // Obtain the operator preview against the just-persisted state.
                crate::session_store::checkpoints::recovery::preview_decision_context(
                    &paths.root,
                    &task.id,
                )
                .err()
                .unwrap_or_else(|| {
                    invalid(
                    "Checkpoint state changed during inspection. Run macc coordinator resume again."
                )
                })
            } else {
                error
            };
            return Err(match error {
                MaccError::CheckpointWorktreeReview { id, sha, message } => {
                    MaccError::CheckpointWorktreeReview {
                        id,
                        sha,
                        message: format!("{position}\n{}\n{message}", decisions_seen.join("\n")),
                    }
                }
                other => other,
            });
        }
        decisions_seen.push(format!(
            "{} · retained {} phase ready for resume",
            task.id,
            task.task_runtime
                .current_phase
                .as_deref()
                .unwrap_or("execution")
        ));
    }
    Ok(decisions_seen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator_storage::{CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage};
    use crate::session_store::checkpoints::{reconcile, recovery};
    #[test]
    fn confirmed_resume_uses_digest_after_publishing_the_current_blocker() {
        let (temp, id, _) = recovery::tests::setup();
        let paths = ProjectPaths::from_root(temp.path());
        let storage = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        let mut snapshot = storage.load_snapshot().unwrap();
        let task = snapshot.registry.find_task_mut("A").unwrap();
        task.task_runtime.current_phase = Some("dev".into());
        task.task_runtime.set_last_error_details(
            "E903",
            "decision_context_refresh",
            "DM_STALE: old reason",
        );
        task.task_runtime
            .extra
            .insert("decision_wait".into(), "context_refresh".into());
        storage.save_snapshot(&snapshot).unwrap();
        let preview = reconcile(temp.path(), &id, None).unwrap();
        reconcile(temp.path(), &id, preview["current_sha256"].as_str()).unwrap();
        let file = temp.path().join(".macc/worktree/worker-01/new.txt");
        std::fs::write(&file, "retained").unwrap();
        let args = BTreeMap::from([("storage-mode".into(), "sqlite".into())]);
        let error = inspect(&paths, &snapshot.registry, &args, "PRD 1/1").unwrap_err();
        let MaccError::CheckpointWorktreeReview { id, sha, message } = error else {
            panic!("{error}")
        };
        assert!(message.contains("PRD 1/1") && message.contains("new.txt"));
        let registry = storage.load_snapshot().unwrap().registry;
        let task = registry.find_task("A").unwrap();
        assert!(task.task_runtime.unmet_preconditions[0].contains(recovery::WORKTREE_DRIFT));
        reconcile(temp.path(), &id, Some(&sha)).unwrap();
        assert!(!inspect(&paths, &registry, &args, "PRD 1/1")
            .unwrap()
            .is_empty());
        assert_eq!(std::fs::read_to_string(file).unwrap(), "retained");
    }
}

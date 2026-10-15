//! One drain definition for requests and completion checks, across all phases.
use super::WorkflowState;
use crate::{MaccError, Result};
use serde_json::Value;
use std::str::FromStr;
pub fn is_pending(state: &str) -> bool {
    matches!(
        WorkflowState::from_str(state),
        Ok(WorkflowState::Claimed
            | WorkflowState::InProgress
            | WorkflowState::Testing
            | WorkflowState::Reviewing
            | WorkflowState::ChangesRequested
            | WorkflowState::PrOpen
            | WorkflowState::Queued)
    )
}
pub fn has_pending(snapshot: Option<&str>, registry: &Value, jobs_active: bool) -> Result<bool> {
    if jobs_active {
        return Ok(true);
    }
    let ids: Vec<String> = serde_json::from_str(snapshot.ok_or_else(|| {
        MaccError::Validation("Drain snapshot missing; refusing premature shutdown".into())
    })?)
    .map_err(|e| MaccError::Validation(format!("Invalid drain snapshot: {e}")))?;
    let tasks = registry["tasks"]
        .as_array()
        .ok_or_else(|| MaccError::Validation("Drain task registry missing".into()))?;
    for id in ids {
        let task = tasks.iter().find(|t| t["id"] == id).ok_or_else(|| {
            MaccError::Validation(format!("Drain task {id} missing; reconciliation required"))
        })?;
        let state = task["state"]
            .as_str()
            .ok_or_else(|| MaccError::Validation(format!("Drain task {id} has no state")))?;
        WorkflowState::from_str(state).map_err(|e| MaccError::Validation(e.to_string()))?;
        if is_pending(state) || state == "todo" {
            return Ok(true);
        }
    }
    Ok(false)
}
/// Selection-only projection: preserve dependency evidence, hide unrelated work.
pub fn selection_registry(root: &std::path::Path, registry: &Value) -> Result<Value> {
    let storage = crate::coordinator_storage::SqliteStorage::new(
        crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(
            &crate::ProjectPaths::from_root(root),
        ),
    );
    let mut projected = registry.clone();
    if let Some(control) = storage.get_coordinator_control()? {
        if control.mode == "draining" {
            let ids: Vec<String> = serde_json::from_str(
                control
                    .drain_snapshot_json
                    .as_deref()
                    .ok_or_else(|| MaccError::Validation("Drain snapshot missing".into()))?,
            )
            .map_err(|e| MaccError::Validation(e.to_string()))?;
            if let Some(tasks) = projected["tasks"].as_array_mut() {
                for task in tasks {
                    if task["state"] == "todo" && !ids.iter().any(|id| task["id"] == *id) {
                        task["state"] = Value::String("blocked".into());
                    }
                }
            }
        }
    }
    Ok(projected)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn every_work_phase_holds_drain_until_terminal_and_jobs_finish() {
        for state in [
            "claimed",
            "in_progress",
            "testing",
            "reviewing",
            "changes_requested",
            "pr_open",
            "queued",
        ] {
            let r = json!({"tasks":[{"id":"A","state":state}]});
            assert!(is_pending(state));
            assert!(has_pending(Some("[\"A\"]"), &r, false).unwrap());
        }
        for state in [
            "merged",
            "blocked",
            "abandoned",
            "approved",
            "rejected",
            "expired",
            "waiting_approval",
        ] {
            let r = json!({"tasks":[{"id":"A","state":state}]});
            assert!(!has_pending(Some("[\"A\"]"), &r, false).unwrap());
            assert!(has_pending(Some("[\"A\"]"), &r, true).unwrap());
        }
    }
    #[test]
    fn corrupt_or_missing_drain_evidence_never_means_complete() {
        let r = json!({"tasks":[]});
        assert!(has_pending(None, &r, false).is_err());
        assert!(has_pending(Some("invalid"), &r, false).is_err());
        assert!(has_pending(Some("[\"A\"]"), &r, false).is_err());
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn drain_dispatch_only_resumes_captured_tasks_and_keeps_dependency_evidence() {
        let t = tempfile::tempdir().unwrap();
        let paths = crate::ProjectPaths::from_root(t.path());
        std::fs::create_dir_all(&paths.macc_dir).unwrap();
        let storage = crate::coordinator_storage::SqliteStorage::new(
            crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&paths),
        );
        storage
            .set_coordinator_control(&crate::coordinator_storage::CoordinatorControl {
                mode: "draining".into(),
                requested_at: None,
                requested_by: None,
                drain_snapshot_json: Some("[\"A\"]".into()),
                force_grace_seconds: None,
                cleanup_after_force: None,
                reason: None,
            })
            .unwrap();
        let original = json!({"tasks":[{"id":"D","state":"merged"},{"id":"A","state":"todo","dependencies":["D"]},{"id":"B","state":"todo"}]});
        let projected = selection_registry(t.path(), &original).unwrap();
        assert_eq!(projected["tasks"][0]["state"], "merged");
        assert_eq!(projected["tasks"][1]["state"], "todo");
        assert_eq!(projected["tasks"][2]["state"], "blocked");
        assert_eq!(original["tasks"][2]["state"], "todo");
        assert!(has_pending(Some("[\"A\"]"), &original, false).unwrap());
    }
}

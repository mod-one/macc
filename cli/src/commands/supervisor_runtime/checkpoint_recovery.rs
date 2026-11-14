//! Deterministic infrastructure recovery before spending a provider invocation.
use macc_core::{supervisor::incident::Incident, ProjectPaths, Result};
use std::path::Path;

pub(super) fn recover(
    paths: &ProjectPaths,
    incident: &Incident,
    dir: &Path,
) -> Result<Option<Vec<String>>> {
    let roots: Vec<_> = incident
        .tasks
        .iter()
        .filter(|t| macc_core::supervisor::incident::repair_candidate(t))
        .collect();
    if roots.is_empty()
        || !roots.iter().all(|t| {
            t.task_runtime.last_error_code.as_deref() == Some("E903")
                && t.task_runtime
                    .last_error_message
                    .as_deref()
                    .is_some_and(|m| {
                        m.contains(macc_core::session_store::checkpoints::recovery::CODE)
                            || (m.contains("Checkpoint") && m.contains("reconciliation"))
                    })
        })
    {
        return Ok(None);
    }
    for task in &roots {
        if !macc_core::session_store::checkpoints::recovery::has_task_checkpoint(
            &paths.root,
            &task.id,
        )? {
            return Ok(None);
        }
    }
    let diagnostics: Vec<_> =
        macc_core::session_store::checkpoints::recovery::diagnostics(&paths.root)?
            .into_iter()
            .filter(|d| roots.iter().any(|t| d["task_id"] == t.id))
            .collect();
    super::report::write_json(
        &dir.join("checkpoint-recovery.json"),
        &serde_json::json!(diagnostics),
    )?;
    let mut ids = Vec::new();
    for task in roots {
        // Transfer missing retained files before moving checkpoint ownership.
        // Conflicts and contract changes remain explicit infrastructure incidents;
        // never ask a source repair agent to bypass them.
        let result = (|| {
            let transfer = macc_core::session_store::checkpoints::recovery::preserve_task_work(
                &paths.root,
                &task.id,
            )?;
            super::report::write_json(
                &dir.join(format!("checkpoint-transfer-{}.json", ids.len())),
                &transfer,
            )?;
            macc_core::session_store::checkpoints::recovery::ensure_task_ready(
                &paths.root,
                &task.id,
            )
        })();
        if let Err(error) = result {
            super::report::write_json(
                &dir.join("checkpoint-recovery-blocked.json"),
                &serde_json::json!({"code":macc_core::session_store::checkpoints::recovery::CODE,"task_id":task.id,"error":error.to_string(),"work_preserved":true,"next_action":"Inspect the retained source and assigned worker; reconcile conflicting files or changed requirements before retrying. No application repair or blind restart can resolve this ownership incident."}),
            )?;
            return Err(error);
        }
        ids.push(task.id.clone());
    }
    Ok(Some(ids))
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::coordinator_storage::{
        CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage,
    };
    fn fixture(conflict: bool) -> (tempfile::TempDir, ProjectPaths, Incident) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.name", "Test"],
            vec!["config", "user.email", "test@example.invalid"],
        ] {
            assert!(
                macc_core::git::run_git_output_mapped(root, &args, "checkpoint fixture")
                    .unwrap()
                    .status
                    .success()
            );
        }
        let paths = ProjectPaths::from_root(root);
        std::fs::create_dir_all(paths.macc_dir.join("state")).unwrap();
        std::fs::write(root.join(".gitignore"), ".macc/\nworktree.prd.json\n").unwrap();
        let prd = serde_json::json!({"tasks":[{"id":"A","acceptance_criteria":["Preserve requirements"]}]});
        std::fs::write(root.join("prd.json"), prd.to_string()).unwrap();
        for args in [vec!["add", "."], vec!["commit", "-m", "base"]] {
            assert!(
                macc_core::git::run_git_output_mapped(root, &args, "checkpoint fixture")
                    .unwrap()
                    .status
                    .success()
            );
        }
        macc_core::save_canonical_config(&paths, &Default::default()).unwrap();
        let migration = macc_core::session_store::migrate(root, false, None).unwrap();
        macc_core::session_store::migrate(root, true, Some(&migration.source_sha256)).unwrap();
        for worker in ["worker-01", "worker-02"] {
            let path = paths.macc_dir.join("worktree").join(worker);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            assert!(macc_core::git::run_git_output_mapped(
                root,
                &[
                    "worktree",
                    "add",
                    "-b",
                    worker,
                    path.to_str().unwrap(),
                    "main"
                ],
                "checkpoint fixture"
            )
            .unwrap()
            .status
            .success());
            macc_core::coordinator::helpers::write_worktree_prd_for_task(
                &root.join("prd.json"),
                "A",
                &path,
            )
            .unwrap();
        }
        let old = paths.macc_dir.join("worktree/worker-01");
        let target = paths.macc_dir.join("worktree/worker-02");
        let identity = macc_core::coordinator::session_identity::identity(
            root,
            &old,
            "A",
            "performer",
            "fake",
            &serde_json::json!({"id":"fake","performer":{"command":"fake","session":{"enabled":true,"scope":"task_role_tool"}}}),
            &macc_core::coordinator::session_identity::SessionRouting {
                tier: "".into(),
                mode: "auto".into(),
            },
        )
        .unwrap();
        macc_core::session_store::checkpoints::prepare(
            root,
            &macc_core::session_store::checkpoints::ContextRequest {
                identity,
                phase: "dev".into(),
            },
        )
        .unwrap();
        std::fs::write(old.join("retained.txt"), "original evidence").unwrap();
        if conflict {
            std::fs::write(target.join("retained.txt"), "different evidence").unwrap();
        }
        let mut task = macc_core::coordinator::model::Task {
            id: "A".into(),
            state: "blocked".into(),
            ..Default::default()
        };
        task.ensure_worktree().worktree_path = Some(target.display().to_string());
        task.ensure_worktree().branch = Some("worker-02".into());
        task.task_runtime.set_last_error_details(
            "E903",
            "checkpoint",
            macc_core::session_store::checkpoints::recovery::CODE,
        );
        let mut snapshot = CoordinatorSnapshot::empty();
        snapshot.registry.tasks.push(task.clone());
        SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths))
            .save_snapshot(&snapshot)
            .unwrap();
        let incident = Incident {
            id: "incident".into(),
            run_id: "run".into(),
            epoch: 1,
            reason: "checkpoint drift".into(),
            tasks: vec![task],
            recent_events: vec![],
            run_status: Some("blocked".into()),
        };
        (temp, paths, incident)
    }
    #[test]
    fn stopped_supervisor_recovers_retained_evidence_without_an_ai_call() {
        let (_temp, paths, incident) = fixture(false);
        let dir = paths.macc_dir.join("log/recovery-test");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            recover(&paths, &incident, &dir).unwrap(),
            Some(vec!["A".into()])
        );
        assert_eq!(
            std::fs::read(paths.macc_dir.join("worktree/worker-01/retained.txt")).unwrap(),
            std::fs::read(paths.macc_dir.join("worktree/worker-02/retained.txt")).unwrap()
        );
        assert!(dir.join("checkpoint-transfer-0.json").is_file());
    }
    #[test]
    fn conflicting_retained_evidence_produces_a_specific_durable_diagnostic() {
        let (_temp, paths, incident) = fixture(true);
        let dir = paths.macc_dir.join("log/recovery-test");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(recover(&paths, &incident, &dir).is_err());
        let report: serde_json::Value = serde_json::from_slice(
            &std::fs::read(dir.join("checkpoint-recovery-blocked.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(report["work_preserved"], true);
        assert!(report["error"].as_str().unwrap().contains("conflicts"));
    }
}

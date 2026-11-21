//! Deterministic infrastructure recovery before spending a provider invocation.
use macc_core::{supervisor::incident::Incident, ProjectPaths, Result};
use std::path::Path;

pub(super) const RESTART_LIMIT: &str = "MACC-CONTEXT-REFRESH-RESTART-LIMIT";

pub(super) fn recover(
    paths: &ProjectPaths,
    incident: &Incident,
    dir: &Path,
) -> Result<Option<Vec<String>>> {
    run(paths, incident, dir, true)
}
pub(super) fn preview(
    paths: &ProjectPaths,
    incident: &Incident,
    dir: &Path,
) -> Result<Option<Vec<String>>> {
    run(paths, incident, dir, false)
}
fn run(
    paths: &ProjectPaths,
    incident: &Incident,
    dir: &Path,
    apply: bool,
) -> Result<Option<Vec<String>>> {
    let mut roots = Vec::new();
    for task in incident
        .tasks
        .iter()
        .filter(|t| macc_core::supervisor::incident::repair_candidate(t))
    {
        if task.task_runtime.last_error_code.as_deref() == Some("E903")
            && task
                .task_runtime
                .last_error_message
                .as_deref()
                .is_some_and(|message| {
                    !message.contains("DM_PENDING:")
                        && (message.contains(macc_core::session_store::checkpoints::recovery::CODE)
                            || message.contains("DM_STALE:")
                            || (message.contains("Checkpoint")
                                && message.contains("reconciliation")))
                })
            && (apply || macc_core::coordinator::phase_diagnostics::context_refresh_blocked(task))
            && macc_core::session_store::checkpoints::recovery::has_task_checkpoint(
                &paths.root,
                &task.id,
            )?
        {
            roots.push(task);
        }
    }
    if roots.is_empty() {
        return Ok(None);
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
            if !apply
                || macc_core::coordinator::phase_diagnostics::context_refresh_blocked(task)
                || task
                    .task_runtime
                    .last_error_message
                    .as_deref()
                    .is_some_and(|m| m.contains("DM_STALE:"))
            {
                return if apply {
                    macc_core::session_store::checkpoints::recovery::ensure_decision_context_ready(
                        &paths.root,
                        &task.id,
                    )
                } else {
                    macc_core::session_store::checkpoints::recovery::preview_decision_context(
                        &paths.root,
                        &task.id,
                    )
                };
            }
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
                &serde_json::json!({"code":macc_core::session_store::checkpoints::recovery::CODE,"task_id":task.id,"error":error.to_string(),"work_preserved":true,"next_action":"Inspect the exact changed field. For retained content/index drift, confirm all invocation tools have stopped, inspect late or external writes, then reconcile reviewed changes. For changed requirements or ownership, inspect the contract and assigned worker. Do not bypass this guard with application edits or an unchanged restart."}),
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
    fn stale_decision_context_enters_native_recovery_but_cannot_move_a_checkpoint() {
        let (_temp, paths, mut incident) = fixture(false);
        incident.tasks[0].task_runtime.set_last_error_details(
            "E903",
            "precondition",
            "DM_STALE: native decision binding changed",
        );
        let dir = paths.macc_dir.join("log/stale-test");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(recover(&paths, &incident, &dir).is_err());
        assert!(dir.join("checkpoint-recovery-blocked.json").exists());
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
    fn unrelated_human_decision_stays_blocked_during_checkpoint_recovery() {
        let (_temp, paths, mut incident) = fixture(false);
        let mut pending = incident.tasks[0].clone();
        pending.id = "EXTERNAL".into();
        pending.task_runtime.set_last_error_details(
            "E903",
            "decisions",
            "DM_PENDING: requires device approval",
        );
        incident.tasks.push(pending);
        let dir = paths.macc_dir.join("checkpoint-test");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(recover(&paths, &incident, &dir).unwrap().unwrap(), ["A"]);
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

    #[test]
    fn context_refresh_budget_survives_new_run_and_checkpoint_ids_and_previews_without_transfer() {
        use super::super::policy;
        use macc_core::session_store::checkpoints::recovery as native;
        let (_temp, paths, mut incident) = fixture(false);
        let dir = paths.macc_dir.join("log/budget-test");
        std::fs::create_dir_all(&dir).unwrap();
        recover(&paths, &incident, &dir).unwrap().unwrap();
        let task = &mut incident.tasks[0];
        task.task_runtime
            .set_last_error_details("E903", "decision_context_refresh", native::CODE);
        task.task_runtime.current_phase = Some("dev".into());
        task.task_runtime
            .extra
            .insert("decision_wait".into(), serde_json::json!("context_refresh"));
        let storage = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        let mut snapshot = storage.load_snapshot().unwrap();
        snapshot.registry.tasks[0] = task.clone();
        storage.save_snapshot(&snapshot).unwrap();
        let fingerprint = policy::recovery_fingerprint(&paths, &incident).unwrap();
        assert!(fingerprint.starts_with("context-refresh-v2:"));
        let mut ledger = policy::Ledger::default();
        ledger
            .restarts
            .insert(policy::crash_fingerprint(&incident), 1);
        assert_eq!(
            ledger.restarts.get(&fingerprint),
            None,
            "legacy budgets allow one native migration attempt"
        );
        ledger.restarts.insert(fingerprint.clone(), 1);
        incident.id = "another-incident".into();
        incident.run_id = "another-run".into();
        incident.epoch += 1;
        incident.reason = "same native fault with a different outer message".into();
        assert_eq!(
            policy::recovery_fingerprint(&paths, &incident).unwrap(),
            fingerprint
        );
        // A code-only native context refresh must use the strict preview branch,
        // never transfer historical files when a restart is no longer permitted.
        std::fs::write(
            paths.macc_dir.join("worktree/worker-01/retained.txt"),
            "changed historical evidence",
        )
        .unwrap();
        let before = macc_core::session_store::inspect(&paths.root).unwrap();
        assert_eq!(
            preview(&paths, &incident, &dir).unwrap(),
            Some(vec!["A".into()])
        );
        let after = macc_core::session_store::inspect(&paths.root).unwrap();
        assert_eq!(
            serde_json::to_value(before).unwrap(),
            serde_json::to_value(after).unwrap(),
            "budget inspection must not alter checkpoint or session state"
        );
        assert_eq!(
            std::fs::read_to_string(paths.macc_dir.join("worktree/worker-02/retained.txt"))
                .unwrap(),
            "original evidence"
        );
        let before_revision = native::revision(&paths.root, &["A".into()]).unwrap();
        let id = before_revision[0]["id"].as_str().unwrap();
        let reconciliation = native::reconcile(&paths.root, id, None).unwrap();
        native::reconcile(&paths.root, id, reconciliation["current_sha256"].as_str()).unwrap();
        assert_ne!(
            native::revision(&paths.root, &["A".into()]).unwrap(),
            before_revision
        );
        let same = policy::recovery_fingerprint(&paths, &incident).unwrap();
        assert_eq!(
            same, fingerprint,
            "new checkpoint UUID and validation metadata cannot grant another restart"
        );
        assert!(ledger.restarts[&same] >= policy::MAX_RESTARTS_PER_FINGERPRINT);
        incident.tasks[0].task_runtime.set_last_error_details(
            "E103",
            "output_protocol",
            "missing result",
        );
        assert_eq!(
            policy::recovery_fingerprint(&paths, &incident).unwrap(),
            policy::crash_fingerprint(&incident),
            "other failures retain their existing restart policy"
        );
    }
}

/// A diagnosis may identify native context recovery without authorizing source changes.
/// Native evidence, not a model-provided shell command, decides whether to act.
pub(super) fn after_diagnosis(
    paths: &ProjectPaths,
    incident: &Incident,
    dir: &Path,
) -> Result<Option<Vec<String>>> {
    let diagnostics = macc_core::session_store::checkpoints::recovery::diagnostics(&paths.root)?;
    let roots: Vec<_> = incident
        .tasks
        .iter()
        .filter(|t| macc_core::supervisor::incident::repair_candidate(t))
        .collect();
    if roots.is_empty()
        || !roots.iter().all(|t| {
            diagnostics.iter().any(|d| {
                d["task_id"] == t.id
                    && d["diagnostic"]["needs_context_update"] == true
                    && d["diagnostic"]["requires_contract_review"] == false
            })
        })
    {
        return Ok(None);
    }
    let mut ids = Vec::new();
    for task in roots {
        macc_core::session_store::checkpoints::recovery::ensure_decision_context_ready(
            &paths.root,
            &task.id,
        )?;
        ids.push(task.id.clone());
    }
    super::report::write_json(
        &dir.join("checkpoint-context-recovered.json"),
        &serde_json::json!({"tasks":ids,"acceptance_inferred":false}),
    )?;
    Ok(Some(ids))
}

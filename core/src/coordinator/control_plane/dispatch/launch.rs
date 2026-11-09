//! Prepare and launch a performer; pre-spawn failures retain task ownership evidence.
use super::*;

pub(super) async fn launch_performer(
    ctx: LaunchPerformerContext<'_>,
    claim: &DispatchClaim,
) -> Result<Option<i64>> {
    let LaunchPerformerContext {
        repo_root,
        prd_file,
        canonical,
        coordinator,
        env_cfg,
        state,
        logger,
    } = ctx;
    let prepared: Result<_> = async {
        write_worktree_prd_for_task(prd_file, &claim.task_id, &claim.worktree_path)?;
        ensure_tool_json_for_tool(repo_root, &claim.worktree_path, &claim.tool)?;
        let worktree_paths = crate::ProjectPaths::from_root(&claim.worktree_path);
        crate::init(&worktree_paths, false)?;
        let canonical_yaml = canonical.to_yaml().map_err(|e| {
            MaccError::Validation(format!(
                "Failed to serialize canonical config for worktree dispatch apply: {}",
                e
            ))
        })?;
        crate::atomic_write(
            &worktree_paths,
            &worktree_paths.config_path,
            canonical_yaml.as_bytes(),
        )?;
        let mut apply_cmd = tokio::process::Command::new(std::env::current_exe().map_err(|e| {
            MaccError::Validation(format!("Failed to resolve current executable path: {}", e))
        })?);
        apply_cmd
            .current_dir(repo_root)
            .env("MACC_INTERNAL_INVOCATION", "1")
            .arg("--cwd")
            .arg(repo_root)
            .arg("worktree")
            .arg("apply")
            .arg(claim.worktree_path.to_string_lossy().to_string())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let apply_output = apply_cmd.output().await.map_err(|e| MaccError::Io {
            path: claim.worktree_path.to_string_lossy().into(),
            action: "run worktree apply for coordinator dispatch".into(),
            source: e,
        })?;
        if !apply_output.status.success() {
            let error = worktree_apply_error(repo_root, claim, &apply_output);
            let _ = append_coordinator_event_with_severity(
                repo_root,
                "worktree_apply_failed",
                &claim.task_id,
                "dispatch",
                "failed",
                &error.to_string(),
                "blocking",
            );
            if let Some(log) = logger {
                let _ = log.note(format!("- {}", error));
            }
            return Err(error);
        }
        // Use stale_in_progress_seconds as the per-phase hard kill timeout, matching
        // the FSM path (fsm.rs). Default is 0 (disabled) — consistent with
        // CoordinatorConfigResolved::resolve returning 0 when not configured.
        let phase_timeout_seconds = env_cfg.stale_in_progress_seconds.unwrap_or_else(|| {
            let cfg = CoordinatorConfigResolved::resolve(coordinator);
            cfg.stale_in_progress_seconds
        });
        if !ensure_expected_worktree_branch(&claim.worktree_path, &claim.branch)? {
            return Err(MaccError::Coordinator {
                code: "worktree_head_mismatch",
                message: claim.branch.clone(),
            });
        }
        let current_exe = std::env::current_exe().map_err(|e| {
            MaccError::Validation(format!("Failed to resolve current executable path: {}", e))
        })?;
        let epoch = std::env::var("COORDINATOR_EPOCH")
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        // Compute model routing decision for this task (spec §8–§11).
        // Reads routing_hints from the task's extra fields; defaults to Standard if absent.
        let mut routing_env = compute_routing_env(repo_root, &claim.task_id, canonical);
        routing_env.push(("MACC_BASE_REF", claim.base_branch.clone()));
        if claim.resume_attempt > 0 {
            routing_env.push(("MACC_RESUME_ATTEMPT", claim.resume_attempt.to_string()));
        }
        routing_env.extend(decision_environment(repo_root, claim, &current_exe)?);

        Ok((current_exe, epoch, phase_timeout_seconds, routing_env))
    }
    .await;
    let (current_exe, epoch, phase_timeout_seconds, routing_env) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            if let Some(wait) =
                crate::coordinator::decisions::pending_for_task(repo_root, &claim.task_id)?
                    .filter(|_| error.to_string().contains("DM_PENDING:"))
            {
                let value = crate::coordinator::state::coordinator_state_registry_load(
                    repo_root,
                    &BTreeMap::new(),
                )?;
                let mut registry = crate::coordinator::model::TaskRegistry::from_value(&value)?;
                if let Some(task) = registry.find_task_mut(&claim.task_id) {
                    if task.task_runtime.claim_id.as_deref() != Some(&claim.claim_id)
                        || task.workflow_state() != Some(crate::coordinator::WorkflowState::Claimed)
                        || task.task_runtime.pid.is_some()
                    {
                        return Err(error);
                    }
                    task.set_workflow_state(crate::coordinator::WorkflowState::Blocked);
                    task.task_runtime.pid = None;
                    wait.apply(&mut task.task_runtime);
                }
                let mut value = registry.to_value()?;
                crate::coordinator::helpers::recompute_resource_locks_from_tasks(&mut value);
                crate::coordinator::state::coordinator_state_registry_save(
                    repo_root,
                    &BTreeMap::new(),
                    &value,
                )?;
                return Ok(None);
            }
            return Err(super::failure::record(repo_root, claim, error, logger)?);
        }
    };

    let pid = coordinator_runtime::spawn_performer_job(
        &current_exe,
        repo_root,
        &claim.task_id,
        &claim.base_branch,
        &claim.worktree_path,
        &state.event_tx,
        &mut state.join_set,
        phase_timeout_seconds,
        state.performer_ipc_addr.as_deref(),
        &claim.claim_id,
        epoch,
        &routing_env,
        state.activity_wake.clone(),
    )
    .map_err(|error| {
        super::failure::record(repo_root, claim, error, logger).unwrap_or_else(|error| error)
    })?;
    let mut registry =
        crate::coordinator::state::coordinator_state_registry_load(repo_root, &BTreeMap::new())?;
    coordinator_engine::apply_dispatch_pid_in_registry(&mut registry, &claim.task_id, pid)?;
    let keys = crate::coordinator::quota_scope::keys(
        repo_root,
        &registry,
        canonical,
        &state.normalizer_registry,
    );
    if let Some(key) = keys
        .get(&claim.task_id)
        .and_then(|keys| keys.get(&claim.tool))
    {
        let mut typed = crate::coordinator::model::TaskRegistry::from_value(&registry)?;
        if let Some(task) = typed.find_task_mut(&claim.task_id) {
            task.task_runtime
                .extra
                .insert("quota_scope_key".into(), key.clone().into());
        }
        registry = typed.to_value()?;
    }

    set_registry_updated_at(&mut registry);
    crate::coordinator::state::coordinator_state_registry_save(
        repo_root,
        &BTreeMap::new(),
        &registry,
    )?;
    if let Some(log) = logger {
        let _ = log.note(format!("- Lifecycle task={} stage=run", claim.task_id));
    }
    Ok(pid)
}

/// The same immutable attempt is used by the context, receipt and performer.
pub(super) fn decision_environment(
    repo_root: &Path,
    claim: &DispatchClaim,
    current_exe: &Path,
) -> Result<Vec<(&'static str, String)>> {
    let mut env = Vec::new();
    if let Some((context, file)) = crate::coordinator::decisions::prepare_context(
        repo_root,
        &claim.task_id,
        "dev",
        &claim.worktree_path,
        &claim.claim_id,
    )? {
        env.extend([
            ("MACC_DECISION_ROOT", repo_root.display().to_string()),
            ("MACC_DECISION_PLAN", context.plan_id),
            ("MACC_DECISION_ATTEMPT", claim.claim_id.clone()),
            ("MACC_DECISION_PHASE", "dev".into()),
            ("MACC_DECISION_CONTEXT_FILE", file.display().to_string()),
            ("MACC_DECISION_CLI", current_exe.display().to_string()),
        ]);
    }
    Ok(env)
}

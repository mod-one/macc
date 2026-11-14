//! Restore a detached legacy protocol failure only from corroborated native ownership.
use crate::coordinator::{model::Task, worktree_roles as roles};
use crate::{config::CoordinatorConfig, fs_lock::AdvisoryLock, ProjectPaths, Result};
use std::path::Path;

/// The caller keeps the returned role lock until the blocked binding is saved.
pub(super) fn restore(
    paths: &ProjectPaths,
    config: Option<&CoordinatorConfig>,
    task: &mut Task,
    occupied: &[String],
) -> Result<Option<AdvisoryLock>> {
    if task.has_worktree_attached()
        || !crate::coordinator::phase_diagnostics::missing_result_protocol(task)
    {
        return Ok(None);
    }
    let guard = roles::lock(&paths.root)?;
    let runtime = &task.task_runtime;
    let path = runtime
        .last_worktree_path
        .as_deref()
        .map(Path::new)
        .ok_or_else(|| {
            roles::conflict("Protocol recovery has no retained worker; task stays blocked")
        })?;
    if runtime.pid.is_some()
        || !roles::performer(&paths.root, path)
        || occupied
            .iter()
            .any(|p| roles::same_path(Path::new(p), path))
        || crate::git::git_common_dir(path)?
            .canonicalize()
            .map_err(crate::persistent::error)?
            != crate::git::git_common_dir(&paths.root)?
                .canonicalize()
                .map_err(crate::persistent::error)?
    {
        return Err(roles::conflict(
            "Protocol recovery worker is live, foreign or assigned elsewhere; work preserved",
        ));
    }
    let metadata = crate::read_worktree_metadata(path)?
        .ok_or_else(|| roles::conflict("Protocol recovery worker metadata missing"))?;
    let phase = runtime
        .current_phase
        .as_deref()
        .or_else(|| {
            runtime
                .extra
                .get("decision_context")
                .and_then(|v| v["phase"].as_str())
        })
        .filter(|p| matches!(*p, "dev" | "fix" | "test" | "review"))
        .ok_or_else(|| {
            roles::conflict("Protocol recovery phase is ambiguous; task stays blocked")
        })?;
    if runtime.last_branch.as_deref() != Some(metadata.branch.as_str())
        || runtime.last_session_tool.as_deref() != Some(metadata.tool.as_str())
    {
        return Err(roles::conflict(
            "Protocol recovery branch/tool changed; task stays blocked",
        ));
    }
    let prd = crate::prd_queue::active_path(
        &paths.root,
        config,
        runtime
            .extra
            .get("checkpoint_prd_file")
            .and_then(serde_json::Value::as_str),
    )?;
    let expected = crate::coordinator::helpers::scoped_prd_for_task(&prd, &task.id)?;
    let local: serde_json::Value = serde_json::from_slice(
        &std::fs::read(path.join("worktree.prd.json")).map_err(crate::persistent::error)?,
    )
    .map_err(crate::persistent::error)?;
    if local != expected {
        return Err(roles::conflict(
            "Protocol recovery task contract changed; inspect retained PRD before retrying",
        ));
    }
    let path = path.to_string_lossy().into_owned();
    let phase = phase.to_string();
    let worktree = task.ensure_worktree();
    worktree.worktree_path = Some(path);
    worktree.branch = Some(metadata.branch);
    worktree.base_branch = Some(metadata.base);
    task.tool = Some(metadata.tool);
    task.task_runtime.current_phase = Some(phase);
    task.task_runtime.completion_kind = None;
    task.task_runtime.set_last_error_details("E103", "output_protocol",
        "Missing explicit MACC_TASK_RESULT; legacy protocol failure restored to retained worker; delivery requires a fresh result");
    task.task_runtime.extra.insert(
        "legacy_protocol_binding_restored".into(),
        serde_json::json!(true),
    );
    Ok(Some(guard))
}

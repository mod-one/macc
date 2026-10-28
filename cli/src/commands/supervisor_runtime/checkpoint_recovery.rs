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
    if diagnostics
        .iter()
        .any(|d| d["error"].is_string() || d["diagnostic"]["requires_contract_review"] == true)
    {
        return Ok(None);
    }
    let mut ids = Vec::new();
    for task in roots {
        macc_core::session_store::checkpoints::recovery::ensure_task_ready(&paths.root, &task.id)?;
        ids.push(task.id.clone());
    }
    Ok(Some(ids))
}

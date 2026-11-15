//! Select execution evidence by the retained phase, never by a historical review.
use super::*;

pub(super) fn ids(
    root: &Path,
    db: &rusqlite::Connection,
    project: &str,
    task: &str,
    strict: bool,
) -> Result<Vec<String>> {
    let ids = task_checkpoint_ids(db, project, task)?;
    let registry = maintenance::registry(root)?;
    let retained = registry.find_task(task);
    let phase = retained.and_then(|t| t.task_runtime.current_phase.as_deref());
    if strict {
        if let (Some(phase), Some(pinned)) = (
            phase,
            retained
                .and_then(|t| t.task_runtime.extra.get("decision_context"))
                .and_then(|v| v["phase"].as_str()),
        ) {
            if phase != pinned {
                return Err(error(format!("{CODE}: task {task}: retained phase={phase} disagrees with native invocation phase={pinned}; inspect before context recovery")));
            }
        }
    }
    if let Some(phase @ ("dev" | "fix" | "review" | "test" | "testing")) = phase {
        let role = crate::coordinator::session_identity::role_for_phase(phase);
        let mut selected = Vec::new();
        for id in &ids {
            let c = load(db, id)?;
            if c.role == role {
                if strict && c.phase != phase {
                    return Err(error(format!("{CODE}: task {task} phase={phase} role={role}: latest checkpoint {} belongs to phase={}; inspect the invocation before recovery",c.id,c.phase)));
                }
                selected.push(id.clone());
            }
        }
        if strict && selected.is_empty() {
            return Err(error(format!("{CODE}: task {task} phase={phase} role={role}: no checkpoint for the interrupted invocation; work preserved")));
        }
        return Ok(selected);
    }
    if strict && (ids.len() > 1 || phase.is_some()) {
        return Err(error(format!("{CODE}: task {task}: retained phase/role is ambiguous (phase={}, checkpoints={}); inspect before context recovery",phase.unwrap_or("unknown"),ids.join(", "))));
    }
    // Single-role legacy projects remain recoverable under the same strict
    // source/ownership checks. Multiple roles require an explicit native phase.
    Ok(ids)
}

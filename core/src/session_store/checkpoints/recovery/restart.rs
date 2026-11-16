//! Stable recovery evidence. IDs, timestamps and leases never renew restart budgets.
use super::*;

/// Semantic restart budget evidence, scoped to the interrupted role and phase.
/// The live binding is used even before reconciliation. Pairing it with the old
/// checkpoint binding would accidentally grant another restart after recovery.
pub fn restart_revision(root: &Path, tasks: &[String]) -> Result<Value> {
    if !root.join(".macc/memory.db").exists()
        || !authority(root)?.is_some_and(|a| a.mode == "sqlite")
    {
        return Ok(Value::Null);
    }
    let mut current = std::collections::BTreeMap::new();
    for task in tasks {
        if crate::coordinator::decisions::pending_for_task(root, task)?.is_some() {
            return Ok(Value::Null);
        }
        current.insert(
            task,
            crate::coordinator::decisions::checkpoint_binding(root, task)?,
        );
    }
    let _guard = lock(root)?;
    let (db, a) = db::active(root)?;
    if !ready(&db)? {
        return Ok(Value::Null);
    }
    let mut rows = Vec::new();
    for task in tasks {
        let ids = active::ids(root, &db, &a.project_id, task, false)?;
        if ids.is_empty() {
            return Ok(Value::Null);
        }
        for id in ids {
            let c = load(&db, &id)?;
            let live = current.get(task).and_then(Option::as_ref);
            rows.push(json!({
                "task_id":task, "role":c.role, "phase":c.phase,
                "worktree":c.worktree, "branch":c.branch, "head":c.source_revision,
                "dirty_digest":c.worktree_dirty_digest,
                "content_digest":c.worktree_content_digest, "sources":c.source_hashes,
                "current_plan":live.map(|b|&b["plan_id"]),
                "current_contract":live.map(|b|&b["contract_digest"]),
                "current_decisions":live.map(|b|&b["context_digest"])
            }));
        }
    }
    rows.sort_by_key(Value::to_string);
    Ok(json!(rows))
}

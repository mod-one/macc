//! Reviewed recovery of checkpoint/dispatch context drift. No provider-specific behavior.
mod context;
#[cfg(test)]
pub(crate) mod tests;
use super::*;
use rusqlite::TransactionBehavior;

/// Stable diagnostic identifier under the existing E903 precondition contract.
pub const CODE: &str = "MACC-CHECKPOINT-CONTEXT-DRIFT";

/// Preview and apply resolve the same actual worker and authoritative PRD as dispatch.
pub fn reconcile(root: &Path, id: &str, expected: Option<&str>) -> Result<Value> {
    reconcile_mode(root, id, expected, false)
}

fn reconcile_mode(root: &Path, id: &str, expected: Option<&str>, automatic: bool) -> Result<Value> {
    let _guard = lock(root)?;
    let (mut db, a) = db::active(root)?;
    let previous = load(&db, id)?;
    if previous.project_id != a.project_id {
        return Err(error("Foreign checkpoint"));
    }
    let latest = latest(&db, &a.project_id, &previous.task_id, &previous.role)?;
    if latest.as_ref().is_some_and(|c| c.id != id) {
        return Err(error(format!(
            "{CODE}: checkpoint superseded; preview `macc session-store reconcile {}`",
            latest.unwrap().id
        )));
    }
    super::lifecycle::idle(&db, &previous.worktree)?;
    let context::Context {
        mut current,
        payload,
        binding,
        materialization_required,
    } = context::resolve(root, &previous)?;
    super::lifecycle::idle(&db, &current.worktree)?;
    current.id = uuid::Uuid::new_v4().to_string();
    current.reason = if automatic {
        "automatic_context_reconciliation"
    } else {
        "operator_reconciliation"
    }
    .into();
    current.blocking_issues.push(if automatic { "Execution context aligned without changing requirements; all criteria and existing objections require revalidation" } else { "Operator accepted current sources; all criteria and existing objections require revalidation" }.into());
    current.blocking_issues.sort();
    current.blocking_issues.dedup();
    current.acceptance_criteria_ids = current.mandatory_task["tasks"][0]["acceptance_criteria"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .enumerate()
                .map(|(i, c)| {
                    c["id"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("AC-{:02}", i + 1))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut value = serde_json::to_value(&current).map_err(|e| error(e.to_string()))?;
    redact_evidence(&mut value);
    current = serde_json::from_value(value).map_err(|e| error(e.to_string()))?;
    let policy = policy::load(root)?;
    schema::check_mandatory_size(&current, policy.checkpoints.max_content_bytes)?;
    current.validation["max_content_bytes"] = json!(policy.checkpoints.max_finish_content_bytes);
    current.validation["max_finish_content_bytes"] =
        json!(policy.checkpoints.max_finish_content_bytes);
    current.validation["content_budget_setting"] = json!("max_finish_content_bytes");
    schema::check_size(
        &current,
        policy.checkpoints.max_finish_content_bytes,
        "max_finish_content_bytes",
    )?;
    let sha = digest(
        json!([id, fingerprint(&current), binding])
            .to_string()
            .as_bytes(),
    );
    let validation_state: String = sql(db.query_row(
        "SELECT validation_state FROM task_checkpoints WHERE id=?1",
        [id],
        |r| r.get(0),
    ))?;
    let needs_context_update = validation_state == "needs_reconciliation"
        || fingerprint(&previous) != fingerprint(&current)
        || materialization_required;
    let requires_review = previous.source_hashes != current.source_hashes;
    if automatic && requires_review {
        return Err(error(format!(
            "{CODE}: automatic recovery cannot accept changed task requirements"
        )));
    }
    let diagnostic = json!({"code":CODE,"task_id":previous.task_id,"checkpoint_id":id,"previous_worktree":previous.worktree,"execution_worktree":current.worktree,"previous_source_hashes":previous.source_hashes,"current_source_hashes":current.source_hashes,"requires_contract_review":requires_review,"needs_context_update":needs_context_update,"changed_task_fields":changed_fields(&previous.mandatory_task,&current.mandatory_task)});
    if expected.is_none() {
        return Ok(
            json!({"previous":previous,"current":current,"current_sha256":sha,"applied":false,"diagnostic":diagnostic}),
        );
    }
    if expected != Some(sha.as_str()) {
        return Err(error("Reconciliation source changed; preview again"));
    }
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    super::lifecycle::idle(&tx, &previous.worktree)?;
    super::lifecycle::idle(&tx, &current.worktree)?;
    if let Some(payload) = payload {
        let paths = crate::ProjectPaths::from_root(Path::new(&current.worktree));
        crate::atomic_write(
            &paths,
            &paths.root.join("worktree.prd.json"),
            &serde_json::to_vec_pretty(&payload).map_err(|e| error(e.to_string()))?,
        )?;
    }
    schema::insert(&tx, &current, policy.checkpoints.max_finish_content_bytes)?;
    sql(tx.execute(
        "UPDATE task_checkpoints SET validation_state='reconciled' WHERE id=?1",
        [id],
    ))?;
    sql(tx.execute("UPDATE logical_sessions SET status='rotated' WHERE project_id=?1 AND task_id=?2 AND role=?3 AND status='available'",params![a.project_id,current.task_id,current.role]))?;
    db::event(
        &tx,
        id,
        "checkpoint_reconciled",
        json!({"code":CODE,"replacement_checkpoint":current.id,"reviewed_sha256":sha,"automatic":automatic,"diagnostic":diagnostic}),
    )?;
    sql(tx.commit())?;
    tracing::info!(code=CODE,task_id=%current.task_id,checkpoint=%current.id,worktree=%current.worktree,"Checkpoint execution context reconciled");
    Ok(json!({"current_sha256":sha,"checkpoint":current.id,"applied":true,"diagnostic":diagnostic}))
}

fn changed_fields(previous: &Value, current: &Value) -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for task in [&previous["tasks"][0], &current["tasks"][0]] {
        if let Some(task) = task.as_object() {
            names.extend(task.keys().cloned());
        }
    }
    names
        .into_iter()
        .filter(|k| previous["tasks"][0][k] != current["tasks"][0][k])
        .collect()
}

/// Resolve only the current task's checkpoints. Automatic recovery never accepts changed contracts.
pub fn ensure_task_ready(root: &Path, task_id: &str) -> Result<()> {
    if !root.join(".macc/memory.db").exists()
        || !authority(root)?.is_some_and(|a| a.mode == "sqlite")
    {
        return Ok(());
    }
    let ids = {
        let _guard = lock(root)?;
        let (db, a) = db::active(root)?;
        if !ready(&db)? {
            return Ok(());
        }
        let mut q = sql(db.prepare("SELECT id FROM task_checkpoints c WHERE project_id=?1 AND task_id=?2 AND role!='merge_fix' AND rowid=(SELECT rowid FROM task_checkpoints n WHERE n.project_id=c.project_id AND n.task_id=c.task_id AND n.role=c.role ORDER BY created_at DESC,rowid DESC LIMIT 1)"))?;
        let rows = sql(q.query_map(params![a.project_id, task_id], |r| r.get::<_, String>(0)))?;
        rows.map(sql).collect::<Result<Vec<_>>>()?
    };
    for id in ids {
        let preview = reconcile(root, &id, None)?;
        if preview["diagnostic"]["requires_contract_review"] == true {
            return Err(error(format!("{CODE}: task {task_id} checkpoint {id} differs from the dispatch contract. Review `macc session-store reconcile {id}` then apply with --expected-sha256 {}; unblock was not applied",preview["current_sha256"].as_str().unwrap_or_default())));
        }
        if preview["diagnostic"]["needs_context_update"] == true {
            reconcile_mode(root, &id, preview["current_sha256"].as_str(), true)?;
        }
    }
    Ok(())
}

/// A supervisor must establish checkpoint ownership before automatic recovery.
pub fn has_task_checkpoint(root: &Path, task: &str) -> Result<bool> {
    if !root.join(".macc/memory.db").exists()
        || !authority(root)?.is_some_and(|a| a.mode == "sqlite")
    {
        return Ok(false);
    }
    let _guard = lock(root)?;
    let (db, a) = db::active(root)?;
    if !ready(&db)? {
        return Ok(false);
    }
    let count:i64 = sql(db.query_row("SELECT count(*) FROM task_checkpoints WHERE project_id=?1 AND task_id=?2 AND role!='merge_fix'",params![a.project_id,task],|r|r.get(0)))?;
    Ok(count > 0)
}

/// Stable evidence revision for supervisor wakeups; no Git scan or provider call.
pub fn revision(root: &Path, task_ids: &[String]) -> Result<Value> {
    if !root.join(".macc/memory.db").exists()
        || !authority(root)?.is_some_and(|a| a.mode == "sqlite")
    {
        return Ok(Value::Null);
    }
    let _guard = lock(root)?;
    let (db, a) = db::active(root)?;
    if !ready(&db)? {
        return Ok(Value::Null);
    }
    let mut rows = Vec::new();
    for task in task_ids {
        let mut q = sql(db.prepare("SELECT id,role,content_sha256,validation_state FROM task_checkpoints c WHERE project_id=?1 AND task_id=?2 AND rowid=(SELECT rowid FROM task_checkpoints n WHERE n.project_id=c.project_id AND n.task_id=c.task_id AND n.role=c.role ORDER BY created_at DESC,rowid DESC LIMIT 1) ORDER BY role"))?;
        let records = sql(q.query_map(params![a.project_id, task], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        }))?;
        for record in records {
            let (id, role, sha, state) = sql(record)?;
            rows.push(
                json!({"task_id":task,"id":id,"role":role,"sha256":sha,"validation_state":state}),
            );
        }
    }
    Ok(json!(rows))
}

/// Structured, non-mutating drift evidence for sync and supervisor clients.
pub fn diagnostics(root: &Path) -> Result<Vec<Value>> {
    if !root.join(".macc/memory.db").exists()
        || !authority(root)?.is_some_and(|a| a.mode == "sqlite")
    {
        return Ok(vec![]);
    }
    let ids = {
        let _guard = lock(root)?;
        let (db, _) = db::active(root)?;
        if !ready(&db)? {
            return Ok(vec![]);
        }
        let mut seen = std::collections::BTreeSet::new();
        super::list(&db)?
            .into_iter()
            .filter(|c| seen.insert((c.task_id.clone(), c.role.clone())))
            .collect::<Vec<_>>()
    };
    let mut diagnostics = Vec::new();
    for checkpoint in ids {
        let id = checkpoint.id;
        match reconcile(root,&id,None) {
            Ok(v) if checkpoint.validation_state=="needs_reconciliation" || v["diagnostic"]["requires_contract_review"]==true || v["diagnostic"]["previous_worktree"]!=v["diagnostic"]["execution_worktree"] => diagnostics.push(json!({"code":CODE,"task_id":checkpoint.task_id,"checkpoint_id":id,"diagnostic":v["diagnostic"],"next_command":format!("macc session-store reconcile {id} --expected-sha256 {}",v["current_sha256"].as_str().unwrap_or_default())})),
            Err(e) if checkpoint.validation_state=="needs_reconciliation" => diagnostics.push(json!({"code":CODE,"task_id":checkpoint.task_id,"checkpoint_id":id,"error":e.to_string()})),
            _ => {},
        }
    }
    Ok(diagnostics)
}

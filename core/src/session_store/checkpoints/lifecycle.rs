use super::*;
use rusqlite::{params, TransactionBehavior};

pub(super) fn idle(db: &rusqlite::Connection, worktree: &str) -> Result<()> {
    let active: i64 = sql(db.query_row(
        "SELECT count(*) FROM session_leases WHERE owner_worktree_id=?1",
        [worktree],
        |r| r.get(0),
    ))?;
    if active != 0 {
        return Err(error(
            "Checkpoint requires an idle retained worktree; active lease exists",
        ));
    }
    Ok(())
}
pub fn prepare(root: &Path, request: &ContextRequest) -> Result<PreparedContext> {
    let freshness = super::memory_context::preflight(root, request)?;
    let _guard = lock(root)?;
    let (mut db, a) = db::active(root)?;
    if !ready(&db)? {
        return Ok(PreparedContext {
            enabled: false,
            checkpoint_id: None,
            prompt: String::new(),
            acknowledgement: None,
        });
    }
    let policy = policy::load(root)?;
    let mut c = make(root, &a, request, "invocation_started")?;
    c.validation["max_content_bytes"] = json!(policy.checkpoints.max_content_bytes);
    c.validation["max_finish_content_bytes"] = json!(policy.checkpoints.max_finish_content_bytes);
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    idle(&tx, &c.worktree)?;
    let mut content_limit = policy.checkpoints.max_content_bytes;
    if let Some(previous) = latest(&tx, &a.project_id, &c.task_id, &c.role)? {
        let prior_validation: String = sql(tx.query_row(
            "SELECT validation_state FROM task_checkpoints WHERE id=?1",
            [&previous.id],
            |r| r.get(0),
        ))?;
        let mut reasons = Vec::new();
        if prior_validation == "needs_reconciliation" {
            reasons.push("a prior checkpoint still requires reviewed reconciliation");
        }
        if previous.project_id != c.project_id || previous.task_id != c.task_id {
            reasons.push("project or task changed");
        }
        if previous.worktree != c.worktree
            && !super::slot_reassignment::allowed(root, &previous.worktree, &c.worktree, &c.task_id)
        {
            reasons.push("execution worktree changed");
        }
        if previous.worktree != c.worktree {
            idle(&tx, &previous.worktree)?;
        }
        if previous.source_hashes != c.source_hashes {
            reasons.push("mandatory PRD or specification sources changed");
        }
        if !git_state::descends_from(Path::new(&c.worktree), &previous.source_revision) {
            reasons.push("current Git HEAD does not descend from the checkpoint revision");
        }
        if !reasons.is_empty() {
            sql(tx.execute(
                "UPDATE task_checkpoints SET validation_state='needs_reconciliation' WHERE id=?1",
                [&previous.id],
            ))?;
            db::event(
                &tx,
                &previous.id,
                "checkpoint_context_drift",
                json!({"code": super::recovery::CODE, "task_id": c.task_id, "previous_worktree": previous.worktree, "execution_worktree": c.worktree, "previous_source_hashes": previous.source_hashes, "current_source_hashes": c.source_hashes, "reasons": reasons}),
            )?;
            tracing::warn!(code=super::recovery::CODE,task_id=%c.task_id,checkpoint=%previous.id,"Checkpoint context drift prevents dispatch");
            sql(tx.commit())?;
            return Err(error(format!("{}: Checkpoint {} needs reconciliation: {}. Inspect with `macc session-store reconcile {}` before dispatch",super::recovery::CODE, previous.id, reasons.join("; "), previous.id)));
        }
        schema::check_mandatory_size(&c, policy.checkpoints.max_content_bytes)?;
        content_limit = policy.checkpoints.max_finish_content_bytes;
        c.validation["max_content_bytes"] = json!(content_limit);
        c.validation["content_budget_setting"] = json!("max_finish_content_bytes");
        let branch_changed = previous.branch != c.branch;
        let worktree_changed = previous.worktree != c.worktree;
        if branch_changed
            || worktree_changed
            || previous.source_revision != c.source_revision
            || previous.worktree_dirty_digest != c.worktree_dirty_digest
        {
            // No active provider owns the slot. Reconcile current Git evidence,
            // preserve objections and require all criteria to be revalidated.
            sql(tx.execute(
                "UPDATE task_checkpoints SET validation_state='needs_reconciliation' WHERE id=?1",
                [&previous.id],
            ))?;
            db::event(
                &tx,
                &previous.id,
                "checkpoint_reconciled",
                json!({"task":c.task_id,"reason":"retained worktree changed; current Git evidence recaptured; completion not inferred","previous_branch":previous.branch,"current_branch":c.branch,"branch_changed":branch_changed,"worktree_changed":worktree_changed}),
            )?;
            if branch_changed
                || worktree_changed
                || previous.worktree_content_digest != c.worktree_content_digest
            {
                sql(tx.execute("UPDATE logical_sessions SET status='rotated' WHERE project_id=?1 AND task_id=?2 AND role=?3 AND status='available'",params![a.project_id,c.task_id,c.role]))?;
            }
            c.blocking_issues.push("Retained Git state changed since prior checkpoint; revalidate all acceptance criteria and outstanding review findings".into());
        }
        if c.review_objections.is_null() {
            c.review_objections = previous.review_objections;
        }
        c.blocking_issues.extend(previous.blocking_issues);
    }
    c.blocking_issues.sort();
    c.blocking_issues.dedup();
    let optional_context = crate::o2::context::for_checkpoint(&tx, root, &c, &freshness)?;
    schema::insert(&tx, &c, content_limit)?;
    sql(tx.commit())?;
    let acknowledgement = format!(
        "MACC_CHECKPOINT_ACK: {} task={} phase={}",
        c.id, c.task_id, c.phase
    );
    let prompt=format!("\n## MACC task checkpoint (evidence; PRD remains authoritative)\n{}\nVerify task and phase first. On a fresh session, emit exactly this acknowledgement before work:\n{}\nDo not claim tests passed without running them. Checkpoints never authorize task completion or scope changes.\n",serde_json::to_string(&c).map_err(|e|error(e.to_string()))?,acknowledgement);
    Ok(PreparedContext {
        enabled: true,
        checkpoint_id: Some(c.id),
        prompt: format!("{prompt}{optional_context}"),
        acknowledgement: Some(acknowledgement),
    })
}
/// Invoked within the acquisition transaction. A context handle cannot be
/// transplanted to another identity or reused after the working tree changes.
pub fn rotation_guard(
    db: &rusqlite::Connection,
    a: &Authority,
    request: &AcquireRequest,
    policy: &SessionPolicy,
) -> Result<()> {
    let Some(id) = request.checkpoint_id.as_deref() else {
        return Ok(());
    };
    if !ready(db)? {
        return Err(error("Checkpoint schema upgrade required"));
    }
    let c = load(db, id)?;
    let phase = c.phase.clone();
    let current = make(
        Path::new(&a.project_root),
        a,
        &ContextRequest {
            identity: request.identity.clone(),
            phase,
        },
        "acquire",
    )?;
    if c.role != current.role
        || c.tool_id != current.tool_id
        || fingerprint(&c) != fingerprint(&current)
    {
        return Err(error(
            "Checkpoint changed before acquisition; reconcile and prepare again",
        ));
    }
    let valid: String = sql(db.query_row(
        "SELECT validation_state FROM task_checkpoints WHERE id=?1",
        [id],
        |r| r.get(0),
    ))?;
    if valid != "verified" {
        return Err(error("Checkpoint requires reconciliation"));
    }
    idle(db, &c.worktree)?;
    let last:Option<(String,String,i64,String)>=sql(db.query_row("SELECT tool_id,identity_json,use_count,id FROM logical_sessions WHERE project_id=?1 AND task_id=?2 AND role=?3 ORDER BY updated_at DESC,rowid DESC LIMIT 1",params![a.project_id,c.task_id,c.role],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional())?;
    let expected_identity = request.identity.to_string();
    if let Some((tool, identity, uses, session_id)) = last {
        let (adaptive, _) = crate::o2::compaction::evaluate_pressure(
            db,
            Path::new(&a.project_root),
            &session_id,
            uses.max(0) as u64,
            policy.checkpoints.rotate_after_uses,
        )?;
        let adaptive_rotate = adaptive.action == "checkpoint_rotate";
        if crate::o2::ready(db)?
            && crate::o2::policy::load(Path::new(&a.project_root))?
                .adaptive_compaction
                .enabled
        {
            db::event(
                db,
                &session_id,
                "compaction_decision",
                serde_json::to_value(&adaptive).map_err(|e| error(e.to_string()))?,
            )?;
        }
        if adaptive_rotate
            || tool != c.tool_id
            || identity != expected_identity
            || uses >= policy.checkpoints.rotate_after_uses as i64
        {
            sql(db.execute("UPDATE logical_sessions SET status='rotated' WHERE project_id=?1 AND task_id=?2 AND role=?3 AND status='available'",params![a.project_id,c.task_id,c.role]))?;
            db::event(
                db,
                id,
                "session_rotated",
                json!({"task":c.task_id,"checkpoint":id,"reason":if tool!=c.tool_id {"tool_switch"} else if identity!=expected_identity{"incompatible_configuration"}else if adaptive_rotate{"adaptive_checkpoint_rotation"}else{"invocation_budget"}}),
            )?;
        }
    }
    Ok(())
}
pub fn finish(root: &Path, request: &FinishRequest) -> Result<Checkpoint> {
    let _guard = lock(root)?;
    let (mut db, a) = db::active(root)?;
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    super::super::leases::verify(&tx, &request.key)?;
    let raw: String = sql(tx.query_row(
        "SELECT identity_json FROM logical_sessions WHERE id=?1",
        [&request.key.session_id],
        |r| r.get(0),
    ))?;
    let result = match request.result.as_str() {
        "success" | "failure" | "interrupted" | "quota" | "precondition_unmet"
        | "changes_requested" => request.result.as_str(),
        _ => return Err(error("Unknown checkpoint result")),
    };
    let mut c = make(
        root,
        &a,
        &ContextRequest {
            identity: json(raw.as_bytes())?,
            phase: request.phase.clone(),
        },
        "invocation_finished",
    )?;
    c.blocking_issues.push(format!(
        "Provider outcome: {result}; acceptance tests and delivery remain coordinator-owned"
    ));
    if !request.acknowledged {
        c.blocking_issues
            .push("Fresh-session checkpoint acknowledgement was not verified".into());
    }
    let mut sources_changed = false;
    let mut content_limit = policy::load(root)?.checkpoints.max_finish_content_bytes;
    // Preserve all unresolved evidence, including failed attempts across retries.
    if let Some(previous) = latest(&tx, &a.project_id, &c.task_id, &c.role)? {
        sources_changed = !same_source(&previous, &c);
        if let Some(limit) = previous.validation["max_finish_content_bytes"].as_u64() {
            content_limit = limit.clamp(2048, 262144);
        }
        c.validation["max_content_bytes"] = json!(content_limit);
        c.blocking_issues.extend(previous.blocking_issues);
        if c.review_objections.is_null() {
            c.review_objections = previous.review_objections;
        }
    }
    c.blocking_issues.sort();
    c.blocking_issues.dedup();
    schema::insert(&tx, &c, content_limit)?;
    db::event(
        &tx,
        &request.key.session_id,
        "checkpoint_saved",
        json!({"checkpoint":c.id,"task":c.task_id,"phase":c.phase,"outcome":result,"acknowledged":request.acknowledged}),
    )?;
    if sources_changed {
        sql(tx.execute(
            "UPDATE task_checkpoints SET validation_state='needs_reconciliation' WHERE id=?1",
            [&c.id],
        ))?;
    }
    if !sources_changed {
        crate::o2::extraction::phase_in(&tx, root, &c, result)?;
    }
    sql(tx.commit())?;
    if sources_changed {
        return Err(error(format!("Checkpoint {} requires reconciliation: mandatory task sources changed during invocation",c.id)));
    }
    Ok(c)
}

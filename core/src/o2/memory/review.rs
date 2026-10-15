use super::*;
pub fn review(root: &Path, r: &Review) -> Result<Memory> {
    if !r.confirmed
        || r.reason.trim().is_empty()
        || r.reason.len() > 500
        || !["active", "rejected", "stale", "invalidated"].contains(&r.status.as_str())
    {
        return Err(error(
            "Memory review requires explicit confirmation, reason and valid status",
        ));
    }
    super::super::paths::safe_text(&r.reason)?;
    let policy = super::super::policy::load(root)?;
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    let mut m = load(&tx, &r.id, &a.project_id)?;
    if hit(m.clone(), 0, "")?.checksum != r.expected_checksum {
        return Err(error("Memory changed since inspection; review again"));
    }
    let allowed = match m.status.as_str() {
        "proposed" => ["active", "rejected"].contains(&r.status.as_str()),
        "active" => ["stale", "invalidated"].contains(&r.status.as_str()),
        "stale" => ["active", "invalidated"].contains(&r.status.as_str()),
        _ => false,
    };
    if !allowed {
        return Err(error("Invalid memory lifecycle transition"));
    }
    if let Some(evidence) = &r.evidence {
        if r.status != "active" {
            return Err(error(
                "Replacement evidence is only accepted during activation/revalidation",
            ));
        }
        m.proposal.evidence = evidence.clone();
        validate(&m.proposal, policy.memory.max_body_chars)?;
    }
    if r.status == "active" {
        if m.proposal.evidence.is_empty()
            || !m.proposal.evidence.iter().any(|e| {
                ["file", "prd", "test", "human_approval", "review"].contains(&e.kind.as_str())
            })
        {
            return Err(error("Activation requires validated source or explicit human/review evidence; a commit alone is insufficient"));
        }
        let target = if m.proposal.scope == "worktree" {
            super::super::providers::native_worktree(root, m.proposal.scope_ref.as_deref())?
        } else {
            root.to_path_buf()
        };
        freshness(&target, &m, policy.memory.provider_note_ttl_days)?;
        m.verified_revision = Some(crate::git::head_commit(&target)?);
        m.reviewed_by = Some("project_owner".into());
    }
    m.status = r.status.clone();
    m.updated_at = now();
    m.staleness_reason = if r.status == "stale" {
        Some(r.reason.clone())
    } else {
        None
    };
    save(&tx, &m)?;
    super::super::schema::audit(
        &tx,
        "memory_reviewed",
        &m.id,
        serde_json::json!({"status":m.status,"reason":r.reason,"reviewer":"project_owner"}),
    )?;
    sql(tx.commit())?;
    Ok(m)
}
pub fn conflict(root: &Path, left: &str, right: &str, reason: &str) -> Result<String> {
    if left == right || reason.trim().is_empty() || reason.len() > 500 {
        return Err(error("Conflict requires two claims and a bounded reason"));
    }
    super::super::paths::safe_text(reason)?;
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    load(&tx, left, &a.project_id)?;
    load(&tx, right, &a.project_id)?;
    let id = uuid::Uuid::new_v4().to_string();
    sql(tx.execute(
        "INSERT INTO memory_conflicts VALUES(?1,?2,?3,?4,'open')",
        params![id, left, right, reason],
    ))?;
    super::super::schema::audit(
        &tx,
        "memory_conflict",
        &id,
        serde_json::json!({"left":left,"right":right,"reason":reason}),
    )?;
    sql(tx.commit())?;
    Ok(id)
}

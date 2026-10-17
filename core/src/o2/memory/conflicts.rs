//! Explicit, atomic owner resolution; retaining an audit trail never activates a proposal.
use super::*;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
fn checksum(
    db: &rusqlite::Connection,
    project: &str,
    id: &str,
) -> Result<(String, Memory, Memory)> {
    let (left, right, reason, status): (String, String, String, String) = sql(db.query_row(
        "SELECT left_id,right_id,reason,status FROM memory_conflicts WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ))?;
    let a = load(db, &left, project)?;
    let b = load(db, &right, project)?;
    let hash = digest(
        &serde_json::to_vec(&(
            id,
            &reason,
            &status,
            hit(a.clone(), 0, "")?.checksum,
            hit(b.clone(), 0, "")?.checksum,
        ))
        .map_err(|e| error(e.to_string()))?,
    );
    Ok((hash, a, b))
}
pub fn conflicts(root: &Path) -> Result<Vec<Value>> {
    let _guard = store::lock(root)?;
    let (db, a) = database(root)?;
    let mut q=sql(db.prepare("SELECT c.id,c.reason,c.status FROM memory_conflicts c JOIN memories l ON c.left_id=l.id JOIN memories r ON c.right_id=r.id WHERE l.project_id=?1 AND r.project_id=?1 ORDER BY c.rowid DESC LIMIT 100"))?;
    let rows = sql(q.query_map([&a.project_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    }))?
    .map(sql)
    .collect::<Result<Vec<_>>>()?;
    rows.into_iter().map(|(id,reason,status)|{let (sha,l,r)=checksum(&db,&a.project_id,&id)?;Ok(json!({"id":id,"left":l.id,"right":r.id,"left_status":l.status,"right_status":r.status,"left_access":access(&l),"right_access":access(&r),"reason":reason,"status":status,"checksum":sha}))}).collect()
}
pub fn resolve_conflict(
    root: &Path,
    id: &str,
    reason: &str,
    confirmed: bool,
    resolution: Resolution,
    expected: Option<&str>,
) -> Result<Value> {
    if !confirmed || reason.trim().is_empty() || reason.len() > 500 {
        return Err(error(
            "Conflict resolution requires confirmed review and reason",
        ));
    }
    super::super::paths::safe_text(reason)?;
    if std::env::var_os("MACC_SESSION_ROLE").is_some() {
        return Err(error(
            "Conflict resolution requires a separate operator review",
        ));
    }
    let p = super::super::policy::load(root)?;
    let prepared = if matches!(resolution, Resolution::BothStale) {
        PreparedFreshness::default()
    } else {
        let (db, a) = super::super::readonly_database(root)?;
        let (_, left, right) = checksum(&db, &a.project_id, id)?;
        drop(db);
        let ids = match resolution {
            Resolution::KeepLeft => vec![left.id],
            Resolution::KeepRight => vec![right.id],
            _ => vec![left.id, right.id],
        };
        super::prepare_records(root, &ids)?
    };
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    let (sha, mut left, mut right) = checksum(&tx, &a.project_id, id)?;
    if expected.is_some_and(|v| v != sha)
        || (expected.is_none() && !matches!(resolution, Resolution::BothStale))
    {
        return Err(error(
            "Conflict/claims changed or checksum missing; inspect both claims again",
        ));
    }
    let prior = (left.status.clone(), right.status.clone());
    for (m, winner) in [
        (&mut left, matches!(resolution, Resolution::KeepLeft)),
        (&mut right, matches!(resolution, Resolution::KeepRight)),
    ] {
        if winner || (matches!(resolution, Resolution::Dismiss) && m.status == "active") {
            if m.status != "active" {
                return Err(error(
                    "A retained claim must already be reviewed active; revalidate it separately",
                ));
            }
            let target = if m.proposal.scope == "worktree" {
                super::super::providers::native_worktree(root, m.proposal.scope_ref.as_deref())?
            } else {
                root.to_path_buf()
            };
            super::preflight::Verifier::new(root, &target, &tx, &p.memory, &prepared).check(m)?;
        } else if !matches!(resolution, Resolution::Dismiss) {
            if matches!(resolution, Resolution::BothStale) {
                if m.status == "active" {
                    m.status = "stale".into();
                    m.staleness_reason = Some(reason.into());
                }
            } else if !["invalidated", "rejected"].contains(&m.status.as_str()) {
                m.status = if m.status == "proposed" {
                    "rejected"
                } else {
                    "invalidated"
                }
                .into();
            }
        }
        m.updated_at = now();
        save(&tx, m)?;
    }
    if sql(tx.execute(
        "UPDATE memory_conflicts SET status='resolved' WHERE id=?1 AND status='open'",
        [id],
    ))? != 1
    {
        return Err(error("Open conflict not found"));
    }
    super::super::schema::audit(
        &tx,
        "memory_conflict_resolved",
        id,
        json!({"reason":reason,"resolution":resolution,"before":prior,"after":[left.status,right.status],"checksum":sha}),
    )?;
    sql(tx.commit())?;
    Ok(json!({"resolved":id,"resolution":resolution}))
}
pub(super) fn existing(
    db: &rusqlite::Connection,
    left: &str,
    right: &str,
) -> Result<Option<String>> {
    sql(db.query_row("SELECT id FROM memory_conflicts WHERE status='open' AND ((left_id=?1 AND right_id=?2) OR (left_id=?2 AND right_id=?1)) ORDER BY id LIMIT 1",params![left,right],|r|r.get(0)).optional())
}

fn access(m: &Memory) -> Access {
    let mut a = Access::default();
    match m.proposal.scope.as_str() {
        "task" => a.task_id = m.proposal.scope_ref.clone(),
        "tool" => a.tool_id = m.proposal.scope_ref.clone(),
        "worktree" => a.worktree = m.proposal.scope_ref.clone(),
        "feature" => a.feature = m.proposal.scope_ref.clone(),
        "user" => a.user_id = m.proposal.scope_ref.clone(),
        _ => {}
    }
    a
}

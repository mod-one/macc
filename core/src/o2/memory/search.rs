use super::*;
pub(crate) const CANDIDATE_LIMIT: usize = 500;
pub fn search(root: &Path, r: &Search) -> Result<Vec<Hit>> {
    let prepared = super::prepare_search(root, r)?;
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    let target = super::super::providers::native_worktree(root, r.access.worktree.as_deref())?;
    let results = search_in(&tx, &a.project_id, root, &target, r, false, &prepared)?;
    sql(tx.commit())?;
    Ok(results.hits)
}
pub(crate) fn search_in(
    db: &rusqlite::Connection,
    project: &str,
    root: &Path,
    worktree: &Path,
    r: &Search,
    persist_staleness: bool,
    prepared: &super::PreparedFreshness,
) -> Result<Retrieval> {
    if r.query.len() > 512 {
        return Err(error("Memory query exceeds 512 bytes"));
    }
    let policy = super::super::policy::load(root)?;
    if !policy.memory.enabled && !r.include_inactive {
        return Ok(Retrieval {
            hits: vec![],
            exclusions: vec!["Memory Ledger is disabled; automatic retrieval excluded".into()],
        });
    }
    let mut hits = Vec::new();
    let mut exclusions = Vec::new();
    let semantic = match crate::o2::semantic::current(
        db,
        project,
        root,
        worktree,
        r,
        prepared,
        &policy.memory,
    ) {
        Ok(valid) => valid,
        Err(e) => {
            exclusions.push(format!("Semantic proof changed: {e}; FTS retained"));
            false
        }
    };
    if !semantic && !prepared.semantic.key.is_empty() {
        exclusions.push("Semantic proof/gate changed; FTS retained".into());
    }
    if !prepared.semantic.diagnostic.is_empty() {
        exclusions.push(prepared.semantic.diagnostic.clone());
    }
    let mut records = candidates(db, project, r)?;
    if semantic {
        for id in prepared.semantic.scores.keys() {
            if !records.iter().any(|m| m.id == *id) {
                records.push(load(db, id, project)?);
            }
        }
    }

    let mut verifier =
        super::preflight::Verifier::new(root, worktree, db, &policy.memory, prepared);
    for mut m in records {
        let id = m.id.clone();
        if !r.include_inactive && m.status != "active" {
            continue;
        }
        if !in_scope(&m.proposal, &r.access) {
            continue;
        }
        if m.status == "active" {
            if let Err(e) = verifier.check(&m) {
                exclusions.push(format!("{id}: snapshot_mismatch: {e}"));
                m.status = "stale".into();
                m.staleness_reason = Some(e.to_string());
                m.updated_at = now();
                // A branch-local mismatch is not evidence that the canonical
                // project fact became invalid for every other worktree.
                let authoritative_target = m.proposal.scope == "worktree"
                    && m.proposal.scope_ref.as_deref() == worktree.to_str()
                    && m.proposal.evidence.iter().all(|e| e.binding.is_none());
                if persist_staleness && authoritative_target {
                    save(db, &m)?;
                    super::super::schema::audit(
                        db,
                        "memory_stale",
                        &id,
                        serde_json::json!({"reason":m.staleness_reason}),
                    )?;
                }
                if !r.include_inactive {
                    continue;
                }
            }
        }
        if !r.include_inactive {
            let conflict:bool=sql(db.query_row("SELECT EXISTS(SELECT 1 FROM memory_conflicts WHERE status='open' AND (left_id=?1 OR right_id=?1))",[&id],|r|r.get(0)))?;
            if conflict || m.proposal.confidence < policy.memory.min_confidence {
                exclusions.push(format!(
                    "{id}: unresolved conflict or confidence below policy minimum"
                ));
                continue;
            }
        }
        hits.push(hit(m, 0, "eligible for deterministic ranking")?);
    }
    super::ranking::rank(&mut hits, &r.access, &r.query, &policy.memory);
    if semantic {
        crate::o2::semantic::augment(&mut hits, &prepared.semantic.scores, &policy.memory);
    }
    hits.truncate(policy.memory.search_limit as usize);
    Ok(Retrieval { hits, exclusions })
}
pub fn fts_rebuild(root: &Path) -> Result<()> {
    let _guard = store::lock(root)?;
    let (mut db, _) = database(root)?;
    let tx = sql(db.transaction())?;
    sql(tx.execute("INSERT INTO memory_fts(memory_fts) VALUES('rebuild')", []))?;
    let rows = {
        let mut q = sql(tx.prepare("SELECT id,project_id FROM memories ORDER BY id"))?;
        let rows = sql(q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| error(e.to_string()))?;
        rows
    };
    for (id, project) in rows {
        let record = load(&tx, &id, &project)?;
        save(&tx, &record)?;
    }
    sql(tx.execute(
        "DELETE FROM memory_search_projection WHERE memory_id NOT IN (SELECT id FROM memories)",
        [],
    ))?;
    sql(tx.execute("INSERT INTO memory_fts(memory_fts) VALUES('rebuild')", []))?;
    sql(tx.execute(
        "INSERT INTO memory_fts(memory_fts,rank) VALUES('integrity-check',1)",
        [],
    ))?;
    super::super::schema::audit(&tx, "memory_fts_rebuilt", "", serde_json::json!({}))?;
    sql(tx.commit())?;
    Ok(())
}

pub(crate) struct Retrieval {
    pub hits: Vec<Hit>,
    pub exclusions: Vec<String>,
}

pub(crate) fn candidates(
    db: &rusqlite::Connection,
    project: &str,
    r: &Search,
) -> Result<Vec<Memory>> {
    if r.query.len() > 512 {
        return Err(error("Memory query exceeds 512 bytes"));
    }
    let query = r
        .query
        .split_whitespace()
        .take(20)
        .map(|s| format!("\"{}\"", s.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ");
    let clause = if query.is_empty() {
        ""
    } else {
        " AND m.id IN (SELECT p.memory_id FROM memory_fts JOIN memory_search_projection p ON p.id=memory_fts.rowid WHERE memory_fts MATCH ?7)"
    };
    let statement=format!("SELECT m.id FROM memories m WHERE project_id=?1 AND (?2 OR status='active') AND (scope='project' OR (scope='task' AND scope_ref=?3) OR (scope='worktree' AND scope_ref=?4) OR (scope='tool' AND scope_ref=?5) OR (scope='feature' AND scope_ref=?6) OR (scope='user' AND scope_ref=?8)) {clause} ORDER BY updated_at DESC,id LIMIT {CANDIDATE_LIMIT}");
    let mut q = sql(db.prepare(&statement))?;
    // Always bind every index by retaining no-op references when FTS is absent.
    let mut rows = sql(q.query(rusqlite::params_from_iter(vec![
        rusqlite::types::Value::Text(project.into()),
        rusqlite::types::Value::Integer(i64::from(r.include_inactive)),
        r.access.task_id.clone().into(),
        r.access.worktree.clone().into(),
        r.access.tool_id.clone().into(),
        r.access.feature.clone().into(),
        rusqlite::types::Value::Text(query),
        r.access.user_id.clone().into(),
    ])))?;
    let mut records = vec![];
    while let Some(row) = sql(rows.next())? {
        let id: String = sql(row.get(0))?;
        let m = load(db, &id, project)?;
        if in_scope(&m.proposal, &r.access) {
            records.push(m);
        }
    }
    Ok(records)
}

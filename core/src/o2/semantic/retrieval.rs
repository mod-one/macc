//! Same authorization/freshness filters for benchmarks and request-local embeddings.
use super::*;
use memory::preflight::{PreparedFreshness, Verifier};
pub(crate) struct Pool {
    pub records: Vec<Memory>,
    pub prepared: PreparedFreshness,
    pub project: String,
    pub canonical: String,
}
pub(super) fn eligible(
    db: &rusqlite::Connection,
    project: &str,
    access: &Access,
    p: &MemoryPolicy,
) -> Result<Vec<Memory>> {
    let all = memory::search::candidates(
        db,
        project,
        &Search {
            query: String::new(),
            access: access.clone(),
            include_inactive: false,
        },
    )?;
    if all.len() >= memory::search::CANDIDATE_LIMIT {
        return Err(error(
            "Semantic corpus enumeration reached the source cap; FTS retained",
        ));
    }
    let mut records = vec![];
    for m in all {
        let conflict:bool=sql(db.query_row("SELECT EXISTS(SELECT 1 FROM memory_conflicts WHERE status='open' AND (left_id=?1 OR right_id=?1))",[&m.id],|r|r.get(0)))?;
        if m.proposal.confidence >= p.min_confidence && !conflict {
            records.push(m);
        }
    }
    if records.len() > p.semantic_candidates as usize {
        return Err(error("Eligible corpus exceeds semantic document cap"));
    }
    records.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(records)
}
pub(crate) fn prepare_pool(root: &Path, access: &Access) -> Result<Pool> {
    let p = policy::load(root)?;
    if !p.memory.enabled {
        return Err(error(
            "Enable Memory Ledger before benchmarking local semantics",
        ));
    }
    let (db, a) = readonly_database(root)?;
    let records = eligible(&db, &a.project_id, access, &p.memory)?;
    let canonical = corpus(&records, access)?;
    let target = providers::native_worktree(root, access.worktree.as_deref())?;
    let prepared = memory::preflight::prepare(root, &target, &db, &a.project_id, records.clone())?;
    let records = records
        .into_iter()
        .filter(|m| prepared.verified(&m.id))
        .collect();
    Ok(Pool {
        records,
        prepared,
        project: a.project_id,
        canonical,
    })
}
pub(crate) fn expand(root: &Path, r: &Search) -> Result<Option<PreparedFreshness>> {
    let (db, a) = readonly_database(root)?;
    let approved: bool = sql(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM o2_audit a WHERE a.kind='semantic_gate' AND json_extract(a.payload_json,'$.project')=?1 AND json_extract(a.payload_json,'$.enabled')=1 AND a.rowid=(SELECT max(b.rowid) FROM o2_audit b WHERE b.kind='semantic_gate' AND b.record_id=a.record_id))",
        [&a.project_id],
        |r| r.get(0),
    ))?;
    drop(db);
    if !approved {
        return Ok(None);
    }
    let started = std::time::Instant::now();
    let p = policy::load(root)?.memory;
    let mut pool = prepare_pool(root, &r.access)?;
    let descriptor = backend::backend(&p.semantic_backend)?.descriptor();
    let key = gate::key(
        &pool.project,
        &corpus(&pool.records, &r.access)?,
        &settings(&p)?,
        &descriptor.recipe,
    );
    let (db, _) = readonly_database(root)?;
    let Some(event) = gate::enabled(&db, &key)? else {
        return Ok(None);
    };
    drop(db);
    let (_, space) = backend::fit(&pool.records, &p)?;
    let scores = backend::scores(space.as_ref(), &r.query, &pool.records, &p)?;
    if started.elapsed().as_millis() > p.semantic_max_ms as u128 {
        return Err(error(
            "Local semantic retrieval exceeded preparation/training/query budget",
        ));
    }
    pool.prepared.semantic = Selection {
        scores,
        diagnostic: format!("Local semantic gate {event}; {}", descriptor.recipe),
        key,
        event,
        canonical: pool.canonical,
        pool: pool.records.iter().map(|m| m.id.clone()).collect(),
        query: r.query.clone(),
        settings: settings(&p)?,
    };
    Ok(Some(pool.prepared))
}
pub(crate) fn current(
    db: &rusqlite::Connection,
    project: &str,
    root: &Path,
    target: &Path,
    r: &Search,
    prepared: &PreparedFreshness,
    p: &MemoryPolicy,
) -> Result<bool> {
    let s = &prepared.semantic;
    if !p.semantic_enabled
        || settings(p)? != s.settings
        || r.include_inactive
        || s.query != r.query
        || s.key.is_empty()
        || gate::enabled(db, &s.key)?.as_deref() != Some(&s.event)
    {
        return Ok(false);
    }
    let records = eligible(db, project, &r.access, p)?;
    if corpus(&records, &r.access)? != s.canonical {
        return Ok(false);
    }
    let mut verifier = Verifier::new(root, target, db, p, prepared);
    for id in &s.pool {
        let Some(m) = records.iter().find(|m| m.id == *id) else {
            return Ok(false);
        };
        if verifier.check(m).is_err() {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(crate) fn augment(hits: &mut [Hit], scores: &BTreeMap<String, f64>, p: &MemoryPolicy) {
    for h in hits.iter_mut() {
        if let Some(score) = scores.get(&h.memory.id) {
            let boost = (*score * p.semantic_weight as f64).round() as i64;
            h.score += boost;
            h.rank_reason.push_str(&format!(
                "; local embedding cosine={score:.4}; semantic=+{boost}; approved benchmark required"
            ));
        }
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.memory.id.cmp(&b.memory.id)));
}
pub(super) fn ranked(
    db: &rusqlite::Connection,
    project: &str,
    records: &[Memory],
    access: &Access,
    query: &str,
    scores: Option<&BTreeMap<String, f64>>,
    p: &MemoryPolicy,
) -> Result<Vec<Hit>> {
    let lexical = memory::search::candidates(
        db,
        project,
        &Search {
            query: query.into(),
            access: access.clone(),
            include_inactive: false,
        },
    )?
    .into_iter()
    .map(|m| m.id)
    .collect::<std::collections::BTreeSet<_>>();
    let mut hits = records
        .iter()
        .filter(|m| lexical.contains(&m.id) || scores.is_some_and(|s| s.contains_key(&m.id)))
        .map(|m| memory::hit(m.clone(), 0, ""))
        .collect::<Result<Vec<_>>>()?;
    memory::ranking::rank(&mut hits, access, query, p);
    if let Some(scores) = scores {
        augment(&mut hits, scores, p);
    }
    memory::search::limit(&mut hits, p);
    Ok(hits)
}

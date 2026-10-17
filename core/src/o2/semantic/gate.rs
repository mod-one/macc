//! Explicit reviewed gate events reuse SQLite audit authority and consistent backups.
use super::*;
use rusqlite::OptionalExtension;
pub(super) fn key(project: &str, corpus: &str, settings: &str, recipe: &str) -> String {
    digest(format!("{project}:{corpus}:{settings}:{recipe}").as_bytes())
}
pub(super) fn enabled(db: &rusqlite::Connection, key: &str) -> Result<Option<String>> {
    let row:Option<(String,String)>=sql(db.query_row("SELECT id,payload_json FROM o2_audit WHERE kind='semantic_gate' AND record_id=?1 ORDER BY rowid DESC LIMIT 1",[key],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
    let Some((id, raw)) = row else {
        return Ok(None);
    };
    let value: serde_json::Value = store::json(raw.as_bytes())?;
    Ok((value["enabled"] == true).then_some(id))
}
pub(super) fn report(db: &rusqlite::Connection, id: &str, project: &str) -> Result<Report> {
    let raw:String=sql(db.query_row("SELECT payload_json FROM o2_audit WHERE kind='semantic_benchmark' AND record_id=?1 ORDER BY rowid DESC LIMIT 1",[id],|r|r.get(0)))?;
    let report: Report = store::json(raw.as_bytes())?;
    if report.project != project {
        return Err(error("Foreign benchmark report"));
    }
    Ok(report)
}
pub(super) fn verify_pool(
    root: &Path,
    db: &rusqlite::Connection,
    project: &str,
    access: &Access,
    prepared: &memory::PreparedFreshness,
    canonical: &str,
    expected: &str,
) -> Result<()> {
    let p = policy::load(root)?.memory;
    let records = retrieval::eligible(db, project, access, &p)?;
    if corpus(&records, access)? != canonical {
        return Err(error("Benchmark corpus changed; rerun inspection"));
    }
    let target = providers::native_worktree(root, access.worktree.as_deref())?;
    let mut verifier = memory::preflight::Verifier::new(root, &target, db, &p, prepared);
    let mut valid = vec![];
    for m in records {
        if prepared.verified(&m.id) {
            verifier.check(&m)?;
            valid.push(m);
        }
    }
    if corpus(&valid, access)? != expected {
        return Err(error("Verified benchmark inputs changed"));
    }
    Ok(())
}
pub fn apply_gate(
    root: &Path,
    id: &str,
    expected: &str,
    enable: bool,
    confirmed: bool,
) -> Result<serde_json::Value> {
    if !confirmed || std::env::var_os("MACC_SESSION_ROLE").is_some() {
        return Err(error(
            "Semantic gate requires a separate confirmed operator review",
        ));
    }
    let (db, a) = readonly_database(root)?;
    let report = report(&db, id, &a.project_id)?;
    drop(db);
    if benchmark::checksum(&report)? != expected {
        return Err(error(
            "Benchmark checksum changed; inspect the exact report",
        ));
    }
    let p = policy::load(root)?.memory;
    let descriptor = backend::backend(&p.semantic_backend)?.descriptor();
    if enable
        && (!report.passed
            || report.benchmark_recipe != BENCHMARK_RECIPE
            || report.settings != settings(&p)?
            || report.backend.recipe != descriptor.recipe
            || report.backend.id != descriptor.id)
    {
        return Err(error(
            "A passing current local benchmark is required to enable semantic retrieval",
        ));
    }
    let pool = if enable {
        Some(prepare_pool(root, &report.access)?)
    } else {
        None
    };
    let gate = key(
        &report.project,
        &report.corpus,
        &report.settings,
        &report.backend.recipe,
    );
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    let tx = sql(db.transaction())?;
    if enable && settings(&policy::load(root)?.memory)? != report.settings {
        return Err(error("Embedding settings changed during review"));
    }
    if let Some(pool) = pool {
        verify_pool(
            root,
            &tx,
            &a.project_id,
            &report.access,
            &pool.prepared,
            &report.canonical,
            &report.corpus,
        )?;
    }
    super::super::schema::audit(
        &tx,
        "semantic_gate",
        &gate,
        serde_json::json!({"enabled":enable,"report_id":id,"checksum":expected,"project":a.project_id}),
    )?;
    sql(tx.commit())?;
    Ok(serde_json::json!({"enabled":enable,"gate":gate,"report_id":id}))
}
pub fn status(root: &Path, access: &Access) -> Result<serde_json::Value> {
    let p = policy::load(root)?.memory;
    let descriptor = backend::backend(&p.semantic_backend)?.descriptor();
    let pool = prepare_pool(root, access)?;
    let (db, _) = readonly_database(root)?;
    let key = key(
        &pool.project,
        &corpus(&pool.records, access)?,
        &settings(&p)?,
        &descriptor.recipe,
    );
    let mut q=sql(db.prepare("SELECT record_id,payload_json FROM o2_audit WHERE kind='semantic_benchmark' ORDER BY rowid DESC LIMIT 100"))?;
    let rows = sql(q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))))?
        .map(sql)
        .collect::<Result<Vec<_>>>()?;
    let mut reports = vec![];
    for (_, raw) in rows {
        let r: Report = store::json(raw.as_bytes())?;
        if r.project == pool.project
            && serde_json::to_value(&r.access).ok() == serde_json::to_value(access).ok()
        {
            let approvable = r.passed
                && r.benchmark_recipe == BENCHMARK_RECIPE
                && r.settings == settings(&p)?
                && r.backend.recipe == descriptor.recipe
                && r.backend.id == descriptor.id
                && r.corpus == corpus(&pool.records, access)?
                && r.canonical == pool.canonical;
            reports.push(serde_json::json!({"approvable":approvable,"checksum":benchmark::checksum(&r)?,"report":r}));
        }
    }
    Ok(
        serde_json::json!({"configured":p.semantic_enabled,"backend":descriptor,"corpus":corpus(&pool.records,access)?,"documents":pool.records.len(),"approved_event":enabled(&db,&key)?,"reports":reports,"notice":"Local embeddings require a passing reviewed benchmark for this exact eligible corpus/settings. FTS remains available."}),
    )
}

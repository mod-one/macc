use super::*;
fn checked(
    root: &Path,
    db: &rusqlite::Connection,
    id: &str,
    checksum: &str,
) -> Result<(Report, Trial)> {
    let r: Report = load(db, "tuning_report", id)?;
    if hash(&r)? != checksum || !r.passed {
        return Err(error(
            "Inspect a passing exact report checksum before preparing policy adoption",
        ));
    }
    let t: Trial = load(db, "tuning_trial", &r.trial_id)?;
    let (_, a) = readonly_database(root)?;
    if t.project != a.project_id {
        return Err(error("Foreign policy trial"));
    }
    trials::evidence(root, &t.plan.protocol_path, &t.plan.protocol_sha256)?;
    let obs: Vec<Observation> = events(db, "tuning_observation", &t.id)?;
    for p in &r.pairs {
        let o = obs
            .iter()
            .find(|o| o.task_id == p.task_id)
            .ok_or_else(|| error("Missing sealed task evidence"))?;
        trials::evidence(root, &o.evidence_path, &o.evidence_sha256)?;
        if observations::pair(db, &t, o)?.telemetry_sha256 != p.telemetry_sha256 {
            return Err(error("Trial telemetry changed after evaluation"));
        }
    }
    Ok((r, t))
}
pub fn preview(root: &Path, id: &str, checksum: &str) -> Result<Value> {
    let (db, _) = readonly_database(root)?;
    let (_, t) = checked(root, &db, id, checksum)?;
    let current = policy::inspect(root)?;
    if current.policy != t.baseline {
        return Err(error(
            "Restore the complete baseline policy before reviewing adoption",
        ));
    }
    Ok(
        json!({"report_id":id,"checksum":checksum,"revision":current.revision,"before":current.policy,"after":t.candidate,"activation":"Explicit apply while stopped; next invocation/query. Provider deployment remains separate."}),
    )
}
pub fn apply(
    root: &Path,
    id: &str,
    checksum: &str,
    revision: &str,
    confirmed: bool,
) -> Result<Value> {
    operator(confirmed)?;
    let _guard = store::lock(root)?;
    store::require_stopped(root)?;
    let (mut db, _) = database(root)?;
    let (_, trial) = checked(root, &db, id, checksum)?;
    if trials::open(&db)?.is_some() {
        return Err(error("Finish the current trial before adopting policy"));
    }
    let active: i64 = sql(db.query_row("SELECT count(*) FROM session_leases", [], |r| r.get(0)))?;
    if active != 0 {
        return Err(error("Stop/recover task leases before adopting policy"));
    }
    // File config and SQLite are separate authorities: durable intent precedes the CAS.
    // A completion records the result; inspect policy revision to recover interrupted intents.
    let attempt = uuid::Uuid::new_v4().to_string();
    let next = crate::config::transaction::update(
        &crate::ProjectPaths::from_root(root),
        Some(revision),
        |c| {
            if policy::from_config(c) != trial.baseline {
                return Err(error("Policy changed; restore baseline and preview again"));
            }
            schema::audit(
                &db,
                "tuning_adoption_intent",
                &attempt,
                json!({"report_id":id,"checksum":checksum,"revision":revision,"before":trial.baseline,"after":trial.candidate}),
            )?;
            c.memory = Some(trial.candidate.memory.clone());
            c.context_engine = Some(trial.candidate.context_engine.clone());
            c.adaptive_compaction = Some(trial.candidate.adaptive_compaction.clone());
            c.code_intelligence = Some(trial.candidate.code_intelligence.clone());
            Ok(())
        },
    )?;
    let next_revision = crate::config::transaction::revision(&next)?;
    let tx = sql(db.transaction())?;
    schema::audit(
        &tx,
        "tuning_adopted",
        &attempt,
        json!({"report_id":id,"revision":next_revision}),
    )?;
    sql(tx.commit())?;
    Ok(json!({"applied":true,"revision":next_revision,"policy":trial.candidate,"report_id":id}))
}

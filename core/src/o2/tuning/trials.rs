use super::*;
pub(super) fn evidence(root: &Path, path: &str, expected: &str) -> Result<()> {
    let (_, bytes) = paths::source(root, path, 131072)?;
    paths::safe_text(std::str::from_utf8(&bytes).map_err(|e| error(e.to_string()))?)?;
    if digest(&bytes) != expected {
        return Err(error(
            "Trial evidence checksum changed; inspect the exact file",
        ));
    }
    Ok(())
}
pub(super) fn candidate(base: &policy::Policy, field: &str, value: f64) -> Result<policy::Policy> {
    if !FIELDS.contains(&field) || !value.is_finite() {
        return Err(error(
            "Only catalogued safe numeric policy controls can be tuned",
        ));
    }
    let mut raw = serde_json::to_value(base).map_err(|e| error(e.to_string()))?;
    let ptr = format!("/{}", field.replace('.', "/"));
    let slot = raw
        .pointer_mut(&ptr)
        .ok_or_else(|| error("Unknown tuning field"))?;
    *slot = if slot.is_u64() {
        if value.fract() != 0. || value < 0. || value > u64::MAX as f64 {
            return Err(error("This policy requires a whole number"));
        }
        json!(value as u64)
    } else {
        json!(value)
    };
    let next: policy::Policy = serde_json::from_value(raw).map_err(|e| error(e.to_string()))?;
    next.validate()?;
    if &next == base {
        return Err(error("Candidate must change the selected field"));
    }
    Ok(next)
}
pub(super) fn open(db: &rusqlite::Connection) -> Result<Option<Trial>> {
    let raw:Option<String>=sql(db.query_row("SELECT payload_json FROM o2_audit p WHERE p.kind='tuning_trial' AND NOT EXISTS(SELECT 1 FROM o2_audit r WHERE r.kind='tuning_report' AND json_extract(r.payload_json,'$.trial_id')=p.record_id) ORDER BY p.rowid DESC LIMIT 1",[],|r|r.get(0)).optional())?;
    raw.map(|s| store::json(s.as_bytes())).transpose()
}
pub fn create(root: &Path, plan: Plan, confirmed: bool) -> Result<Value> {
    operator(confirmed)?;
    bounded(&plan)?;
    let base = policy::load(root)?;
    if !base.context_engine.tuning_enabled {
        return Err(error(
            "Enable reviewed policy trials explicitly in Context policies first",
        ));
    }
    if plan.cases.len() < base.context_engine.tuning_min_pairs as usize || plan.cases.len() > 100 {
        return Err(error(
            "Pre-register 20–100 task pairs, respecting the configured minimum",
        ));
    }
    let mut ids = std::collections::BTreeSet::new();
    for c in &plan.cases {
        if c.task_id.trim().is_empty()
            || c.task_id.len() > 160
            || !ids.insert(&c.task_id)
            || !(40..=64).contains(&c.revision.len())
            || !c.revision.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(error(
                "Trial tasks must be distinct bounded IDs with full Git revisions",
            ));
        }
    }
    evidence(root, &plan.protocol_path, &plan.protocol_sha256)?;
    let next = candidate(&base, &plan.field, plan.candidate)?;
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    if policy::load(root)? != base {
        return Err(error("Policy changed during trial preparation"));
    }
    if open(&db)?.is_some() {
        return Err(error(
            "Finish or cancel the existing trial before creating another",
        ));
    }
    for c in &plan.cases {
        for arm in ["baseline", "candidate"] {
            let used:bool=sql(db.query_row("SELECT EXISTS(SELECT 1 FROM logical_sessions WHERE task_id=?1 UNION ALL SELECT 1 FROM task_checkpoints WHERE task_id=?1 UNION ALL SELECT 1 FROM context_bundles WHERE task_id=?1)",[c.execution_task(arm)],|r|r.get(0)))?;
            if used {
                return Err(error("Trial execution task identities must be unused: no previous sessions/checkpoints/context. Choose a new independent roster."));
            }
        }
    }
    let ordinal: u64 = sql(db.query_row(
        "SELECT count(*)+1 FROM o2_audit WHERE kind='tuning_trial'",
        [],
        |r| r.get(0),
    ))?;
    let trial = Trial {
        id: uuid::Uuid::new_v4().to_string(),
        project: a.project_id,
        plan,
        baseline: base.clone(),
        candidate: next,
        ordinal,
        alpha: base
            .context_engine
            .tuning_alpha
            .min(0.05 / (ordinal as f64 * (ordinal as f64 + 1.))),
    };
    let tx = sql(db.transaction())?;
    schema::audit(
        &tx,
        "tuning_trial",
        &trial.id,
        serde_json::to_value(&trial).map_err(|e| error(e.to_string()))?,
    )?;
    sql(tx.commit())?;
    receipt(trial)
}
pub fn inspect(root: &Path, id: Option<&str>) -> Result<Value> {
    let (db, a) = readonly_database(root)?;
    if let Some(id) = id {
        let trial: Trial = load(&db, "tuning_trial", id)?;
        if trial.project != a.project_id {
            return Err(error("Foreign policy trial"));
        }
        let observations: Vec<Observation> = events(&db, "tuning_observation", id)?;
        let stamps: Vec<Stamp> = events(&db, "tuning_invocation", id)?;
        let report:Option<String>=sql(db.query_row("SELECT payload_json FROM o2_audit WHERE kind='tuning_report' AND json_extract(payload_json,'$.trial_id')=?1 ORDER BY rowid DESC LIMIT 1",[id],|r|r.get(0)).optional())?;
        let report = report
            .map(|s| store::json::<Report>(s.as_bytes()).and_then(receipt))
            .transpose()?;
        let mut q=sql(db.prepare("SELECT kind,record_id,payload_json FROM o2_audit WHERE kind IN ('tuning_adoption_intent','tuning_adopted') AND json_extract(payload_json,'$.report_id') IN (SELECT record_id FROM o2_audit WHERE kind='tuning_report' AND json_extract(payload_json,'$.trial_id')=?1) ORDER BY rowid LIMIT 200"))?;
        let adoption = sql(q.query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        }))?
        .map(|row| {
            let (kind, id, raw) = sql(row)?;
            Ok(json!({"kind":kind,"attempt":id,"details":store::json::<Value>(raw.as_bytes())?}))
        })
        .collect::<Result<Vec<Value>>>()?;
        Ok(
            json!({"execution_tasks":trial.plan.cases.iter().map(|c|json!({"pair":c.task_id,"baseline":c.execution_task("baseline"),"candidate":c.execution_task("candidate")})).collect::<Vec<_>>(),"trial":trial,"observations":observations,"invocations":stamps,"result":report,"adoptions":adoption}),
        )
    } else {
        let mut q=sql(db.prepare("SELECT payload_json FROM o2_audit WHERE kind='tuning_trial' ORDER BY rowid DESC LIMIT 100"))?;
        let rows = sql(q.query_map([], |r| r.get::<_, String>(0)))?
            .map(sql)
            .collect::<Result<Vec<_>>>()?;
        let trials = rows
            .into_iter()
            .map(|s| store::json::<Trial>(s.as_bytes()))
            .collect::<Result<Vec<_>>>()?;
        Ok(
            json!({"trials":trials,"open":open(&db)?,"fields":FIELDS,"policy":policy::load(root)?.context_engine,"notice":"Pre-registered paired trials; exact incremental cost only. Operators attest quality and controlled execution. No automatic dispatch or policy change."}),
        )
    }
}

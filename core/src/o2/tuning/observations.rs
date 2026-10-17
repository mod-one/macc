use super::*;
#[derive(Default)]
struct Arm {
    cost: u64,
    ms: u64,
    currency: Option<String>,
    profiles: std::collections::BTreeSet<String>,
    records: Vec<store::usage::UsageRecord>,
}
fn arm(db: &rusqlite::Connection, stamps: &[&Stamp], expected: &str) -> Result<Arm> {
    if stamps.is_empty()
        || stamps[0].generation != 1
        || !stamps[0].clean
        || stamps[0].revision != expected
    {
        return Err(error(
            "Both task arms must start in fresh sessions at the pre-registered Git revision",
        ));
    }
    // A disabled capture interval may omit an entire session, not just a call
    // in an otherwise stamped session. Count the complete execution task ledger.
    let total: i64 = sql(db.query_row(
        "SELECT count(*) FROM invocation_usage WHERE task_id=?1",
        [&stamps[0].task],
        |r| r.get(0),
    ))?;
    if total != stamps.len() as i64 {
        return Err(error(
            "Trial execution task contains unstamped invocations; no whole session may be omitted",
        ));
    }
    let mut result = Arm::default();
    let mut sessions = std::collections::BTreeMap::new();
    for s in stamps {
        let raw: String = sql(db.query_row(
            "SELECT record_json FROM invocation_usage WHERE invocation_id=?1",
            [&s.invocation],
            |r| r.get(0),
        ))?;
        let r: store::usage::UsageRecord = store::json(raw.as_bytes())?;
        let end: Value = load(db, "tuning_finish", &s.invocation)?;
        let expected_generation = sessions.entry(r.logical_session_id.clone()).or_insert(0i64);
        *expected_generation += 1;
        if s.generation != r.generation
            || *expected_generation != r.generation
            || r.task_id != s.task
            || r.status != "recorded"
            || r.cost_quality != "exact"
            || r.measurement_source != "provider_structured"
            || r.model.as_ref().is_none_or(|m| m.is_empty())
            || r.incremental.cost.is_none()
            || r.elapsed_ms.is_none()
            || end["policy"].as_str() != Some(&s.policy)
        {
            return Err(error("Trial needs complete fresh-session telemetry, unchanged invocation policy, measured model and exact incremental billed cost; estimates/unknown are insufficient"));
        }
        let currency = r
            .currency
            .as_ref()
            .filter(|c| c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase()))
            .ok_or_else(|| error("Measured cost requires a currency"))?;
        if result.currency.as_ref().is_some_and(|c| c != currency) {
            return Err(error("Mixed currencies are not comparable"));
        }
        result.currency = Some(currency.clone());
        result.cost = result
            .cost
            .checked_add(r.incremental.cost.unwrap())
            .ok_or_else(|| error("Cost overflow"))?;
        result.ms = result
            .ms
            .checked_add(r.elapsed_ms.unwrap())
            .ok_or_else(|| error("Latency overflow"))?;
        result.profiles.insert(encoded(&(
            &r.tool_id,
            &r.model,
            &r.phase,
            &r.role,
            &r.schema,
            &r.provider_version,
            &s.execution,
        ))?);
        result.records.push(r);
    }
    // Every invocation of each trial session belongs to this arm, including failed calls.
    for (session, count) in sessions {
        let total: i64 = sql(db.query_row(
            "SELECT count(*) FROM invocation_usage WHERE session_id=?1",
            [session],
            |r| r.get(0),
        ))?;
        if total != count {
            return Err(error(
                "Trial session contains unstamped or omitted invocations",
            ));
        }
    }
    Ok(result)
}
pub(super) fn pair(
    db: &rusqlite::Connection,
    trial: &Trial,
    o: &Observation,
) -> Result<PairResult> {
    let c = trial
        .plan
        .cases
        .iter()
        .find(|c| c.task_id == o.task_id)
        .ok_or_else(|| error("Task is outside the pre-registered trial"))?;
    let stamps: Vec<Stamp> = events(db, "tuning_invocation", &trial.id)?;
    let filtered = |arm: &str| {
        stamps
            .iter()
            .filter(|s| s.task == c.execution_task(arm) && s.arm == arm)
            .collect::<Vec<_>>()
    };
    let a = filtered("baseline");
    let b = filtered("candidate");
    let index = trial
        .plan
        .cases
        .iter()
        .position(|x| x.task_id == c.task_id)
        .unwrap();
    let first = stamps
        .iter()
        .find(|s| s.task == c.execution_task("baseline") || s.task == c.execution_task("candidate"))
        .ok_or_else(|| error("No trial task invocations"))?;
    if first.arm
        != if index % 2 == 0 {
            "baseline"
        } else {
            "candidate"
        }
    {
        return Err(error(
            "Run task arms in the pre-registered alternating order",
        ));
    }
    // Arm sessions must be disjoint; phase retries stay in their original arm.
    let expected = hash(&trial.baseline)?;
    let proposed = hash(&trial.candidate)?;
    if a.iter().any(|s| s.policy != expected)
        || b.iter().any(|s| s.policy != proposed)
        || a.iter()
            .chain(b.iter())
            .any(|s| s.environment != first.environment)
    {
        return Err(error(
            "Trial policy or approved memory/provider corpus changed within a paired task",
        ));
    }
    let ar = arm(db, &a, &c.revision)?;
    let br = arm(db, &b, &c.revision)?;
    if ar.currency != br.currency || ar.profiles != br.profiles || ar.cost == 0 || ar.ms == 0 {
        return Err(error("Paired arms need matching tool/model/phase profiles, currency and positive baseline cost/latency"));
    }
    if ar.records.iter().any(|r| {
        br.records
            .iter()
            .any(|s| s.logical_session_id == r.logical_session_id)
    }) {
        return Err(error("Trial arms cannot share a native/logical session"));
    }
    let q = &o.candidate;
    let base = &o.baseline;
    let quality = q.accepted
        && q.tests_passed
        && q.spec_fidelity
        && q.max_finding_severity <= base.max_finding_severity
        && q.retries <= base.retries;
    Ok(PairResult {
        task_id: c.task_id.clone(),
        baseline_cost: ar.cost,
        candidate_cost: br.cost,
        baseline_ms: ar.ms,
        candidate_ms: br.ms,
        currency: ar.currency.unwrap(),
        quality_passed: quality,
        telemetry_sha256: hash(&(ar.records, br.records, o))?,
    })
}
pub fn record(root: &Path, id: &str, observation: Observation, confirmed: bool) -> Result<Value> {
    operator(confirmed)?;
    bounded(&observation)?;
    if observation.baseline.max_finding_severity > 4
        || observation.candidate.max_finding_severity > 4
        || observation.baseline.retries > 1000
        || observation.candidate.retries > 1000
    {
        return Err(error("Invalid quality severity/retry bounds"));
    }
    trials::evidence(
        root,
        &observation.evidence_path,
        &observation.evidence_sha256,
    )?;
    let _guard = store::lock(root)?;
    let (mut db, a) = database(root)?;
    let trial = trials::open(&db)?
        .filter(|t| t.id == id && t.project == a.project_id)
        .ok_or_else(|| error("Trial is closed or foreign"))?;
    if events::<Observation>(&db, "tuning_observation", id)?
        .iter()
        .any(|o| o.task_id == observation.task_id)
    {
        return Err(error(
            "Task quality is already sealed; do not replace unfavorable observations",
        ));
    }
    let result = pair(&db, &trial, &observation)?;
    let tx = sql(db.transaction())?;
    schema::audit(
        &tx,
        "tuning_observation",
        id,
        serde_json::to_value(&observation).map_err(|e| error(e.to_string()))?,
    )?;
    sql(tx.commit())?;
    Ok(json!({"observation":observation,"pair":result}))
}

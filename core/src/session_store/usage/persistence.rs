use super::*;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use std::path::Path;
fn id(key: &LeaseKey) -> String {
    format!("{}:{}", key.session_id, key.generation)
}
fn key_hash(key: &LeaseKey) -> String {
    digest(format!("{}:{}:{}", key.session_id, key.generation, key.lease_token).as_bytes())
}
fn encoded<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|e| error(e.to_string()))
}
pub fn begin(root: &Path, r: &BeginRequest) -> Result<CapturePlan> {
    let _guard = lock(root)?;
    let (mut db, _) = db::active(root)?;
    if !ready(&db)? {
        return Ok(CapturePlan {
            enabled: false,
            structured: false,
            max_output_bytes: 0,
            profile: None,
        });
    }
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    leases::verify(&tx, &r.key)?;
    let existing: Option<(String, String)> = sql(tx
        .query_row(
            "SELECT plan_json,lease_hash FROM invocation_usage WHERE invocation_id=?1",
            [id(&r.key)],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional())?;
    if let Some((plan, hash)) = existing {
        if hash != key_hash(&r.key) {
            return Err(error("Foreign usage invocation"));
        }
        return json(plan.as_bytes());
    }
    if r.phase.is_empty()
        || r.phase.len() > 40
        || !r
            .phase
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Err(error("Invalid usage phase"));
    }
    let identity: String = sql(tx.query_row(
        "SELECT identity_json FROM logical_sessions WHERE id=?1",
        [&r.key.session_id],
        |row| row.get(0),
    ))?;
    let identity: serde_json::Value = json(identity.as_bytes())?;
    let profile: Option<crate::tool::usage::UsageSpec> =
        serde_json::from_value(identity["execution_config"]["performer"]["usage"].clone())
            .map_err(|e| error(e.to_string()))?;
    if let Some(p) = &profile {
        p.validate()?;
    }
    let policy = policy::load(root)?.usage;
    let plan = CapturePlan {
        enabled: true,
        structured: policy.capture_mode == 1 && profile.is_some(),
        max_output_bytes: policy.max_output_bytes,
        profile,
    };
    let record = UsageRecord {
        invocation_id: id(&r.key),
        context_bundle_id: crate::o2::context_usage::bundle_for_invocation(&tx, &r.key)?,
        logical_session_id: r.key.session_id.clone(),
        generation: r.key.generation,
        task_id: identity["task_id"].as_str().unwrap_or_default().into(),
        role: identity["role"].as_str().unwrap_or_default().into(),
        tool_id: identity["tool_id"].as_str().unwrap_or_default().into(),
        phase: r.phase.clone(),
        model: None,
        provider_version: None,
        schema: plan.profile.as_ref().map(|p| p.schema.clone()),
        measurement_source: "runner_pending".into(),
        token_quality: "unknown".into(),
        cost_quality: "unknown".into(),
        currency: None,
        reported: Counters::default(),
        incremental: Counters::default(),
        notes: vec!["usage_pending_or_interrupted".into()],
        status: "pending".into(),
        outcome: None,
        elapsed_ms: None,
    };
    sql(tx.execute("INSERT INTO invocation_usage(invocation_id,session_id,generation,lease_hash,plan_json,record_json,status,started_at,task_id,tool_id,phase,cost_quality) VALUES(?1,?2,?3,?4,?5,?6,'pending',?7,?8,?9,?10,'unknown')",params![record.invocation_id,r.key.session_id,r.key.generation,key_hash(&r.key),encoded(&plan)?,encoded(&record)?,now(),record.task_id,record.tool_id,record.phase]))?;
    crate::o2::context_usage::record_items(&tx, &record)?;
    db::event(
        &tx,
        &r.key.session_id,
        "usage_started",
        serde_json::json!({"invocation":record.invocation_id,"phase":r.phase}),
    )?;
    sql(tx.commit())?;
    Ok(plan)
}
pub fn end(root: &Path, r: &EndRequest) -> Result<UsageRecord> {
    if r.raw.len() > 16 * 1024 * 1024 {
        return Err(error("Usage output exceeds transport bound"));
    }
    if ![
        "success",
        "failure",
        "interrupted",
        "quota",
        "precondition_unmet",
    ]
    .contains(&r.outcome.as_str())
    {
        return Err(error("Invalid usage outcome"));
    }
    let receipt = digest(format!("{}:{}:{}", r.outcome, r.omitted, r.raw).as_bytes());
    let _guard = lock(root)?;
    let (mut db, _) = db::active(root)?;
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let(raw,plan,hash,previous_receipt,started):(String,String,String,Option<String>,i64)=sql(tx.query_row("SELECT record_json,plan_json,lease_hash,receipt,started_at FROM invocation_usage WHERE invocation_id=?1",[id(&r.key)],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))))?;
    if hash != key_hash(&r.key) {
        return Err(error("Foreign usage invocation"));
    }
    let mut record: UsageRecord = json(raw.as_bytes())?;
    if let Some(previous) = previous_receipt {
        if previous != receipt {
            return Err(error(
                "Usage receipt conflict; invocation already finalized",
            ));
        }
        return Ok(record);
    }
    leases::verify(&tx, &r.key)?;
    let plan: CapturePlan = json(plan.as_bytes())?;
    let mut observation = if r.omitted || r.raw.len() as u64 > plan.max_output_bytes {
        model::Observation {
            notes: vec!["output_exceeds_capture_bound".into()],
            ..Default::default()
        }
    } else {
        parser::observe(&r.raw, plan.profile.as_ref())
    };
    let (provider, uses): (Option<String>, i64) = sql(tx.query_row(
        "SELECT provider_session_id,use_count FROM logical_sessions WHERE id=?1",
        [&r.key.session_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    if observation.session.is_some() && observation.session != provider {
        observation.counters = Counters::default();
        observation.notes.push("provider_session_mismatch".into());
    }
    if !r.omitted
        && r.raw.len() as u64 <= plan.max_output_bytes
        && (observation.session.is_none() || observation.session == provider)
    {
        crate::o2::compaction::observe_pressure(&tx, root, &r.key, &r.raw)?;
    }
    record.reported = observation.counters;
    record.notes = observation.notes;
    record.model = observation.model;
    if let Some(p) = &plan.profile {
        let scope = encoded(&(
            record.tool_id.as_str(),
            provider.as_deref().unwrap_or(&r.key.session_id),
            p.schema.as_str(),
            record.model.as_deref(),
        ))?;
        let previous: Option<String> = sql(tx
            .query_row(
                "SELECT reported_json FROM usage_watermarks WHERE scope=?1",
                [&scope],
                |row| row.get(0),
            )
            .optional())?;
        let previous: Option<Counters> = previous.map(|s| json(s.as_bytes())).transpose()?;
        let pending_gap:bool=sql(tx.query_row("SELECT EXISTS(SELECT 1 FROM invocation_usage WHERE session_id=?1 AND generation<?2 AND status='pending')",params![r.key.session_id,r.key.generation],|row|row.get(0)))?;
        let previous = if pending_gap {
            record.notes.push("earlier_invocation_usage_pending".into());
            None
        } else {
            previous
        };
        // Imported transcripts may have a zero legacy use count. Only an
        // explicitly reserved new logical session can establish a zero baseline.
        let reserved_new: bool = sql(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_events WHERE logical_session_id=?1 AND event_type='session_reserved')",
            [&r.key.session_id], |row| row.get(0),
        ))?;
        let (delta, notes) = normalization::incremental(
            &record.reported,
            previous.as_ref(),
            uses == 1
                && r.key.generation == 1
                && reserved_new
                && provider.is_some()
                && observation.session.as_ref() == provider.as_ref(),
            &p.token_scope,
            &p.cost_scope,
        );
        record.incremental = delta;
        record.notes.extend(notes);
        record.currency = p.currency.clone();
        record.token_quality = if record.incremental.values()[..5].iter().any(Option::is_some) {
            "exact"
        } else {
            "unknown"
        }
        .into();
        record.cost_quality = if record.incremental.cost.is_some() {
            p.cost_quality.as_str()
        } else {
            "unknown"
        }
        .into();
        sql(tx.execute("INSERT INTO usage_watermarks VALUES(?1,?2) ON CONFLICT(scope) DO UPDATE SET reported_json=excluded.reported_json",params![scope,encoded(&record.reported)?]))?;
    }
    record.measurement_source = if record.reported.values().iter().any(Option::is_some) {
        "provider_structured"
    } else {
        "unavailable"
    }
    .into();
    record.status = "recorded".into();
    record.outcome = Some(r.outcome.clone());
    record.elapsed_ms = Some((now() - started).max(0) as u64 * 1000);
    let c = &record.incremental;
    sql(tx.execute("UPDATE invocation_usage SET record_json=?2,receipt=?3,status='recorded',currency=?4,cost_quality=?5,input_uncached=?6,cached_read=?7,cached_write=?8,output=?9,reasoning=?10,cost=?11 WHERE invocation_id=?1",params![record.invocation_id,encoded(&record)?,receipt,record.currency,record.cost_quality,c.input_uncached,c.cached_read,c.cached_write,c.output,c.reasoning,c.cost]))?;
    db::event(
        &tx,
        &r.key.session_id,
        "usage_recorded",
        serde_json::json!({"invocation":record.invocation_id,"quality":record.token_quality,"source":record.measurement_source}),
    )?;
    sql(tx.commit())?;
    Ok(record)
}
pub fn inspect(root: &Path) -> Result<UsageView> {
    let _guard = lock(root)?;
    if !is_sqlite(root)? {
        return Ok(UsageView::default());
    }
    let (db, _) = db::active(root)?;
    if !ready(&db)? {
        return Ok(UsageView::default());
    }
    let mut q = sql(db.prepare(
        "SELECT record_json FROM invocation_usage ORDER BY started_at DESC,rowid DESC LIMIT 200",
    ))?;
    let rows = sql(q.query_map([], |r| r.get::<_, String>(0)))?;
    let records = rows
        .map(|r| sql(r).and_then(|s| json(s.as_bytes())))
        .collect::<Result<Vec<UsageRecord>>>()?;
    let mut q=sql(db.prepare("SELECT task_id,tool_id,phase,currency,cost_quality,count(*),sum(CASE WHEN input_uncached IS NOT NULL OR cached_read IS NOT NULL OR cached_write IS NOT NULL OR output IS NOT NULL OR reasoning IS NOT NULL OR cost IS NOT NULL THEN 1 ELSE 0 END),sum(input_uncached),sum(cached_read),sum(cached_write),sum(output),sum(reasoning),sum(cost),count(input_uncached),count(cached_read),count(cached_write),count(output),count(reasoning),count(cost) FROM invocation_usage GROUP BY task_id,tool_id,phase,currency,cost_quality ORDER BY task_id,tool_id,phase"))?;
    let totals = sql(q.query_map([], |r| {
        Ok(UsageTotal {
            task_id: r.get(0)?,
            tool_id: r.get(1)?,
            phase: r.get(2)?,
            currency: r.get(3)?,
            cost_quality: r.get(4)?,
            invocations: r.get(5)?,
            measured_invocations: r.get(6)?,
            coverage: Counters {
                input_uncached: r.get(13)?,
                cached_read: r.get(14)?,
                cached_write: r.get(15)?,
                output: r.get(16)?,
                reasoning: r.get(17)?,
                cost: r.get(18)?,
            },
            counters: Counters {
                input_uncached: r.get(7)?,
                cached_read: r.get(8)?,
                cached_write: r.get(9)?,
                output: r.get(10)?,
                reasoning: r.get(11)?,
                cost: r.get(12)?,
            },
        })
    }))?
    .map(sql)
    .collect::<Result<Vec<_>>>()?;
    Ok(UsageView {
        ready: true,
        records,
        totals,
    })
}

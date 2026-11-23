use super::*;
use crate::process_ownership::process_identity;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::path::Path;

struct ActiveClaim {
    id: String,
    token: String,
    generation: i64,
    provider: Option<String>,
    identity: String,
    process: String,
    expires: i64,
    status: String,
}

pub(super) fn validate_import_identity(root: &Path, value: &Value) -> Result<()> {
    let get = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| error(format!("Missing identity field {key}")))
    };
    let config = value
        .get("execution_config")
        .ok_or_else(|| error("Missing execution configuration"))?;
    let spec = json!({"id":get("tool_id")?,"performer":config["performer"],"model_tiers":config["model_tiers"]});
    let routing = crate::coordinator::session_identity::SessionRouting {
        tier: config["tier"]
            .as_str()
            .ok_or_else(|| error("Invalid routing tier"))?
            .into(),
        mode: config["routing"]
            .as_str()
            .ok_or_else(|| error("Invalid routing mode"))?
            .into(),
    };
    let expected = crate::coordinator::session_identity::execution_identity(
        root,
        Path::new(get("execution_root")?),
        get("task_id")?,
        get("role")?,
        get("tool_id")?,
        &spec,
        &routing,
    );
    if expected.as_ref() != Some(value) {
        return Err(error("Invalid or incompatible ownership identity"));
    }
    Ok(())
}
pub(super) fn recover_in(db: &rusqlite::Connection) -> Result<usize> {
    let mut q=sql(db.prepare("SELECT logical_session_id,owner_process_id,process_identity,expires_at FROM session_leases"))?;
    let rows = sql(q.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
        ))
    }))?;
    let mut orphaned = Vec::new();
    for row in rows {
        let (id, pid, start, expires) = sql(row)?;
        // A live process (including PID reuse) never permits automatic takeover.
        if !crate::coordinator::helpers::is_pid_running(pid) && expires <= now() {
            orphaned.push(id);
        } else {
            let _ = start;
        }
    }
    drop(q);
    for id in &orphaned {
        sql(db.execute(
            "UPDATE logical_sessions SET status='abandoned',updated_at=?2 WHERE id=?1",
            params![id, now()],
        ))?;
        sql(db.execute(
            "DELETE FROM session_leases WHERE logical_session_id=?1",
            [id],
        ))?;
        db::event(
            db,
            id,
            "session_abandoned",
            json!({"reason":"expired lease and dead owner; provider exit unverified"}),
        )?;
    }
    Ok(orphaned.len())
}

/// Read-only checkpoint checks ignore only leases eligible for native recovery.
/// A live process, PID reuse or a dead owner before expiry still excludes takeover.
pub(super) fn checkpoint_idle(db: &rusqlite::Connection, worktree: &str) -> Result<()> {
    let mut query = sql(db.prepare(
        "SELECT owner_process_id,expires_at FROM session_leases WHERE owner_worktree_id=?1",
    ))?;
    let rows = sql(query.query_map([worktree], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
    }))?;
    for row in rows {
        let (pid, expires) = sql(row)?;
        if crate::coordinator::helpers::is_pid_running(pid) || expires > now() {
            return Err(error(format!("Checkpoint requires an idle retained worktree; active lease exists (owner pid={pid}, expires_at={expires}); stop the verified owner or wait for dead-owner expiry before recovery")));
        }
    }
    Ok(())
}
pub fn acquire(root: &Path, request: &AcquireRequest) -> Result<LeaseGrant> {
    let _guard = lock(root)?;
    let (mut db, a) = db::active(root)?;
    validate_import_identity(root, &request.identity)?;
    if request.generate_provider_id {
        let create = &request.identity["execution_config"]["performer"]["session"]["create"];
        if create["command"].as_str().is_none_or(|s| s.is_empty()) || !create["args"].is_array() {
            return Err(error(
                "Generated provider IDs require a declared adapter create capability",
            ));
        }
    }
    if request.request_id.trim().is_empty() {
        return Err(error("Request ID required"));
    }
    let process = process_identity(request.owner_pid)
        .ok_or_else(|| error("Owner process cannot be verified"))?;
    let policy = policy::load(root)?;
    let decision_pin = crate::coordinator::decisions::checkpoint_binding(
        root,
        request.identity["task_id"].as_str().unwrap_or_default(),
    )?;
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    recover_in(&tx)?;
    // Stable request IDs make a lost response retry idempotent, without sharing
    // one lease between callers or identities.
    let existing:Option<ActiveClaim>=sql(tx.query_row(
        "SELECT s.id,l.lease_token,l.fencing_counter,s.provider_session_id,s.identity_json,l.process_identity,l.expires_at,s.status
         FROM logical_sessions s JOIN session_leases l ON l.logical_session_id=s.id WHERE l.owner_run_id=?1",
        [&request.request_id],|r|Ok(ActiveClaim{id:r.get(0)?,token:r.get(1)?,generation:r.get(2)?,provider:r.get(3)?,identity:r.get(4)?,process:r.get(5)?,expires:r.get(6)?,status:r.get(7)?})).optional())?;
    if let Some(ActiveClaim {
        id,
        token,
        generation,
        provider,
        identity,
        process: start,
        expires,
        status,
    }) = existing
    {
        let stored: Value = json(identity.as_bytes())?;
        if stored != request.identity || start != process || expires < now() || status != "active" {
            return Err(error(
                "Request ID belongs to a different or expired invocation",
            ));
        }
        verify_decision_pin(root, &tx, &id, generation)?;
        // is_new belongs to this invocation, not the provider's existence now.
        let is_new: bool = sql(tx.query_row(
            "SELECT use_count=1 FROM logical_sessions WHERE id=?1",
            [&id],
            |r| r.get(0),
        ))?;
        return Ok(LeaseGrant {
            key: LeaseKey {
                session_id: id,
                lease_token: token,
                generation,
            },
            provider_session_id: provider,
            is_new,
            heartbeat_seconds: policy.lease.heartbeat_seconds,
        });
    }
    let used: bool = sql(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_requests WHERE request_id=?1)",
        [&request.request_id],
        |r| r.get(0),
    ))?;
    if used {
        return Err(error(
            "This invocation request has already completed; use a new request ID",
        ));
    }
    checkpoints::rotation_guard(&tx, &a, request, &policy)?;
    let v = &request.identity;
    let identity = v.to_string();
    let resumable = v
        .pointer("/execution_config/performer/session/enabled")
        .and_then(Value::as_bool)
        == Some(true);
    let picked:Option<(String,Option<String>,i64)>=sql(tx.query_row(
        "SELECT id,provider_session_id,generation FROM logical_sessions WHERE project_id=?1 AND identity_json=?2
         AND status='available' AND provider_session_id IS NOT NULL AND updated_at>=?3 AND ?5=1
         AND NOT EXISTS(SELECT 1 FROM session_leases WHERE logical_session_id=id)
         ORDER BY (provider_session_id=?4) DESC,updated_at DESC,id LIMIT 1",
        params![a.project_id,identity,now()-(policy.lifecycle.max_idle_hours*3600) as i64,request.preferred_provider_id,resumable],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional())?;
    let is_new = picked.is_none();
    let (id, provider, prior) = if let Some(row) = picked {
        row
    } else {
        let id = uuid::Uuid::new_v4().to_string();
        let provider = request
            .generate_provider_id
            .then(|| uuid::Uuid::new_v4().to_string());
        let lineage:String=sql(tx.query_row("SELECT lineage_id FROM logical_sessions WHERE project_id=?1 AND task_id=?2 AND role=?3 ORDER BY created_at DESC LIMIT 1",
            params![a.project_id,v["task_id"].as_str(),v["role"].as_str()],|r|r.get(0)).optional())?.unwrap_or_else(||uuid::Uuid::new_v4().to_string());
        sql(tx.execute("INSERT INTO logical_sessions(id,project_id,task_id,role,tool_id,config_fingerprint,environment_fingerprint,
         identity_json,provider_session_id,lineage_id,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'created',?11,?11)",
            params![id,a.project_id,v["task_id"].as_str(),v["role"].as_str(),v["tool_id"].as_str(),
                digest(v["execution_config"].to_string().as_bytes()),digest(v["execution_root"].to_string().as_bytes()),identity,provider,lineage,now()]))?;
        (id, provider, 0)
    };
    let generation = prior
        .checked_add(1)
        .ok_or_else(|| error("Session generation exhausted"))?;
    let token = uuid::Uuid::new_v4().to_string();
    sql(tx.execute("UPDATE logical_sessions SET status='active',generation=?2,use_count=use_count+1,updated_at=?3,last_verified_at=?3 WHERE id=?1",params![id,generation,now()]))?;
    sql(tx.execute(
        "INSERT INTO session_leases VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            id,
            token,
            generation,
            request.owner_pid,
            process,
            request.request_id,
            v["execution_root"].as_str(),
            now(),
            now() + policy.lease.ttl_seconds as i64
        ],
    ))?;
    sql(tx.execute(
        "INSERT INTO session_requests VALUES(?1,?2,?3)",
        params![request.request_id, id, generation],
    ))?;
    if let Some(pin) = decision_pin {
        db::event(
            &tx,
            &id,
            "decision_context_pinned",
            json!({"generation":generation,"binding":pin}),
        )?;
    }
    db::event(
        &tx,
        &id,
        if is_new {
            "session_reserved"
        } else {
            "session_reused"
        },
        json!({"generation":generation,"role":v["role"],"tool":v["tool_id"],"task":v["task_id"],"checkpoint_id":request.checkpoint_id}),
    )?;
    sql(tx.commit())?;
    Ok(LeaseGrant {
        key: LeaseKey {
            session_id: id,
            lease_token: token,
            generation,
        },
        provider_session_id: provider,
        is_new,
        heartbeat_seconds: policy.lease.heartbeat_seconds,
    })
}
pub(crate) fn verify(db: &rusqlite::Connection, key: &LeaseKey) -> Result<()> {
    let row:Option<(i64,String,i64)>=sql(db.query_row("SELECT owner_process_id,process_identity,expires_at FROM session_leases WHERE logical_session_id=?1 AND lease_token=?2 AND fencing_counter=?3",
        params![key.session_id,key.lease_token,key.generation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional())?;
    match row {
        Some((pid, start, expires))
            if expires >= now() && process_identity(pid).as_deref() == Some(&start) =>
        {
            Ok(())
        }
        _ => Err(error("Stale, expired or foreign lease token/generation")),
    }
}
pub fn bind(root: &Path, key: &LeaseKey, provider: &str) -> Result<()> {
    if provider.is_empty() || provider.len() > 512 {
        return Err(error("Invalid provider session ID"));
    }
    mutate(root, key, |tx| {
        let current: Option<String> = sql(tx.query_row(
            "SELECT provider_session_id FROM logical_sessions WHERE id=?1",
            [&key.session_id],
            |r| r.get(0),
        ))?;
        if current.as_deref().is_some_and(|id| id != provider) {
            return Err(error("Provider ID cannot change within a logical session"));
        }
        sql(tx.execute(
            "UPDATE logical_sessions SET provider_session_id=?2,updated_at=?3 WHERE id=?1",
            params![key.session_id, provider, now()],
        ))?;
        db::event(
            tx,
            &key.session_id,
            "session_bound",
            json!({"generation":key.generation}),
        )
    })
}
/// Only the current, live owner may wait past the normal authority lock timeout.
/// Verification runs again inside the write transaction after acquisition.
fn lease_lock(root: &Path, key: &LeaseKey) -> Result<crate::fs_lock::AdvisoryLock> {
    lease_lock_after(root, key, std::time::Duration::from_secs(8))
}

pub(super) fn lease_lock_after(
    root: &Path,
    key: &LeaseKey,
    first_wait: std::time::Duration,
) -> Result<crate::fs_lock::AdvisoryLock> {
    match super::lock_with_timeout(root, first_wait) {
        Err(crate::MaccError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::TimedOut =>
        {
            let (db, _) = db::readonly(root)?;
            verify(&db, key)?;
            let expires: i64 = sql(db.query_row(
                "SELECT expires_at FROM session_leases WHERE logical_session_id=?1 AND lease_token=?2 AND fencing_counter=?3",
                params![key.session_id, key.lease_token, key.generation], |r| r.get(0),
            ))?;
            let remaining = u64::try_from(expires.saturating_sub(now())).unwrap_or(0);
            drop(db);
            super::lock_with_timeout(root, std::time::Duration::from_secs(remaining))
        }
        result => result,
    }
}
pub fn heartbeat(root: &Path, key: &LeaseKey) -> Result<()> {
    mutate(root, key, |tx| {
        sql(tx.execute("UPDATE session_leases SET expires_at=?2+(expires_at-heartbeat_at),heartbeat_at=?2 WHERE logical_session_id=?1",params![key.session_id,now()]))?;
        Ok(())
    })
}
pub fn release(root: &Path, key: &LeaseKey, clean: bool) -> Result<()> {
    let _guard = lease_lock(root, key)?;
    let (mut db, _) = db::active(root)?;
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    // Repeated release is harmless only for the exact last generation/token.
    let done:bool=sql(tx.query_row("SELECT EXISTS(SELECT 1 FROM session_events WHERE logical_session_id=?1 AND event_type='session_released' AND payload_json=?2)",
        params![key.session_id,json!({"generation":key.generation,"token_hash":digest(key.lease_token.as_bytes())}).to_string()],|r|r.get(0)))?;
    if done {
        return Ok(());
    }
    verify(&tx, key)?;
    sql(tx.execute("UPDATE logical_sessions SET status=CASE WHEN ?2 AND provider_session_id IS NOT NULL THEN 'available' ELSE 'abandoned' END,updated_at=?3 WHERE id=?1",params![key.session_id,clean,now()]))?;
    sql(tx.execute(
        "DELETE FROM session_leases WHERE logical_session_id=?1",
        [&key.session_id],
    ))?;
    db::event(
        &tx,
        &key.session_id,
        "session_released",
        json!({"generation":key.generation,"token_hash":digest(key.lease_token.as_bytes())}),
    )?;
    sql(tx.commit())?;
    Ok(())
}
fn mutate(
    root: &Path,
    key: &LeaseKey,
    operation: impl FnOnce(&rusqlite::Connection) -> Result<()>,
) -> Result<()> {
    with_verified_lease(root, key, |tx| {
        verify_decision_pin(root, tx, &key.session_id, key.generation)?;
        operation(tx)
    })
}
pub(super) fn with_verified_lease<T>(
    root: &Path,
    key: &LeaseKey,
    operation: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T>,
) -> Result<T> {
    let _guard = lease_lock(root, key)?;
    let (mut db, _) = db::active(root)?;
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    verify(&tx, key)?;
    let result = operation(&tx)?;
    sql(tx.commit())?;
    Ok(result)
}
pub fn recover(root: &Path) -> Result<usize> {
    let _guard = lock(root)?;
    let (mut db, _) = db::active(root)?;
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let count = recover_in(&tx)?;
    sql(tx.commit())?;
    Ok(count)
}
pub fn close(root: &Path, id: &str) -> Result<()> {
    let _guard = lock(root)?;
    let (mut db, _) = db::active(root)?;
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let leased: bool = sql(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM session_leases WHERE logical_session_id=?1)",
        [id],
        |r| r.get(0),
    ))?;
    if leased {
        return Err(error("Cannot close a leased session; stop its owner first"));
    }
    let closed: bool = sql(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM logical_sessions WHERE id=?1 AND status='closed')",
        [id],
        |r| r.get(0),
    ))?;
    if closed {
        return Ok(());
    }
    let count = sql(tx.execute(
        "UPDATE logical_sessions SET status='closed',updated_at=?2 WHERE id=?1",
        params![id, now()],
    ))?;
    if count == 0 {
        return Err(error("Unknown logical session"));
    }
    db::event(&tx, id, "session_closed", json!({}))?;
    sql(tx.commit())?;
    Ok(())
}

pub fn read_hint(root: &Path, identity: &Value) -> Result<Option<String>> {
    let _guard = lock(root)?;
    let (db, a) = db::active(root)?;
    let p = policy::load(root)?;
    sql(db.query_row("SELECT provider_session_id FROM logical_sessions WHERE project_id=?1 AND identity_json=?2 AND status='available' AND provider_session_id IS NOT NULL AND updated_at>=?3 ORDER BY updated_at DESC,id LIMIT 1",
        params![a.project_id,identity.to_string(),now()-(p.lifecycle.max_idle_hours*3600) as i64],|r|r.get(0)).optional())
}

pub(super) fn verify_decision_pin(
    root: &Path,
    db: &rusqlite::Connection,
    id: &str,
    generation: i64,
) -> Result<()> {
    if !crate::coordinator::decisions::policy(root)?.enabled {
        return Ok(());
    }
    let task: String = sql(db.query_row(
        "SELECT task_id FROM logical_sessions WHERE id=?1",
        [id],
        |r| r.get(0),
    ))?;
    let stored:Option<String>=sql(db.query_row("SELECT payload_json FROM session_events WHERE logical_session_id=?1 AND event_type='decision_context_pinned' AND json_extract(payload_json,'$.generation')=?2 ORDER BY rowid DESC LIMIT 1",params![id,generation],|r|r.get(0)).optional())?;
    let old = stored
        .map(|s| json::<Value>(s.as_bytes()).map(|v| v["binding"].clone()))
        .transpose()?;
    let current = crate::coordinator::decisions::checkpoint_binding(root, &task)?;
    if !crate::coordinator::decisions::compatible_checkpoint_binding(&old, &current) {
        return Err(error("DM_STALE: provider lease has obsolete binding decisions; preserve work and acquire a fresh invocation"));
    }
    Ok(())
}

//! Redacted runtime observations, separate from Apply's control-session proof.
use super::*;
use rusqlite::Connection;

pub(crate) fn record_in(
    db: &Connection,
    c: &Credentials,
    scope: &agent::Scope,
    state: &str,
) -> Result<()> {
    if scope.role == "mcp_control" {
        return Ok(());
    }
    super::super::schema::audit(
        db,
        "mcp_activity",
        &c.session_id,
        json!({"generation":c.generation,"tool_id":scope.access.tool_id,"state":state}),
    )
}

pub(crate) fn record(root: &Path, c: &Credentials, state: &str) -> Result<()> {
    let _guard = store::lock(root)?;
    let (db, _) = database(root)?;
    let scope = agent::authorize(root, &db, c)?;
    record_in(&db, c, &scope, state)
}

pub(super) fn observations(db: &Connection, tool: &str) -> Result<Value> {
    let mut statement = sql(db.prepare("SELECT a.record_id,a.payload_json,(SELECT l.lease_token FROM logical_sessions s JOIN session_leases l ON l.logical_session_id=s.id WHERE s.id=a.record_id AND s.status='active' AND s.generation=json_extract(a.payload_json,'$.generation')) FROM o2_audit a WHERE kind='mcp_activity' AND json_extract(payload_json,'$.tool_id')=?1 ORDER BY rowid DESC LIMIT 1000"))?;
    let rows = sql(statement.query_map([tool], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    }))?;
    let mut available = false;
    let mut connected = false;
    let mut consulted = 0usize;
    let mut validity = std::collections::BTreeMap::new();
    for row in rows {
        let (session_id, raw, token) = sql(row)?;
        let event: Value = store::json(raw.as_bytes())?;
        let generation = event["generation"].as_i64().unwrap_or(0);
        let live = *validity
            .entry((session_id.clone(), generation))
            .or_insert_with(|| {
                token.is_some_and(|lease_token| {
                    store::verify_lease(
                        db,
                        &store::LeaseKey {
                            session_id,
                            generation,
                            lease_token,
                        },
                    )
                    .is_ok()
                })
            });
        if event["state"] == "consulted" {
            consulted += 1;
        }
        if live && event["state"] == "available" {
            available = true;
        }
        if live && event["state"] == "connected" {
            connected = true;
        }
    }
    Ok(
        json!({"available_in_run":available,"connected_in_run":connected,"consulted":consulted>0,"consultations_observed":consulted,"observation_limit":1000}),
    )
}

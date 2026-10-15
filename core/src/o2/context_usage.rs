//! Attribute estimated context items to the exact fenced invocation, never billing counters.
use super::*;
use rusqlite::{params, OptionalExtension};
use store::{usage::UsageRecord, LeaseKey};

pub(crate) fn bundle_for_invocation(
    db: &rusqlite::Connection,
    key: &LeaseKey,
) -> Result<Option<String>> {
    if !ready(db)? {
        return Ok(None);
    }
    let payload: Option<String> = sql(db.query_row(
        "SELECT payload_json FROM session_events WHERE logical_session_id=?1 AND event_type IN ('session_reserved','session_reused') ORDER BY rowid DESC LIMIT 1",
        [&key.session_id], |r| r.get(0)).optional())?;
    let Some(raw) = payload else {
        return Ok(None);
    };
    let event: serde_json::Value = store::json(raw.as_bytes())?;
    if event["generation"].as_i64() != Some(key.generation) {
        return Ok(None);
    }
    let Some(checkpoint) = event["checkpoint_id"].as_str() else {
        return Ok(None);
    };
    sql(db
        .query_row(
            "SELECT id FROM context_bundles WHERE checkpoint_id=?1 ORDER BY rowid DESC LIMIT 1",
            [checkpoint],
            |r| r.get(0),
        )
        .optional())
}
pub(crate) fn record_items(db: &rusqlite::Connection, record: &UsageRecord) -> Result<()> {
    let Some(id) = &record.context_bundle_id else {
        return Ok(());
    };
    let raw: String = sql(db.query_row(
        "SELECT manifest_json FROM context_bundles WHERE id=?1",
        [id],
        |r| r.get(0),
    ))?;
    let bundle: context::Bundle = store::json(raw.as_bytes())?;
    for item in &bundle.selected {
        sql(db.execute(
            "INSERT INTO context_usage_items VALUES(?1,'memory',?2,?3)",
            params![record.invocation_id, item.id, item.estimated_tokens],
        ))?;
    }
    let mandatory =
        serde_json::to_string(&bundle.mandatory_task).map_err(|e| error(e.to_string()))?;
    sql(db.execute(
        "INSERT INTO context_usage_items VALUES(?1,'mandatory_prd',?2,?3)",
        params![
            record.invocation_id,
            bundle.checkpoint_id,
            mandatory.chars().count().div_ceil(4) as u64
        ],
    ))?;
    Ok(())
}

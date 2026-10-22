//! Transport-independent lookup of current versioned presentation receipts.
use super::*;
use rusqlite::Connection;

pub(crate) fn presented(
    db: &Connection,
    scope: &Scope,
    c: &Credentials,
    id: &str,
    checksum: &str,
    offset: usize,
) -> Result<Option<(usize, &'static str)>> {
    if let Some(cp) = &scope.checkpoint {
        let mut statement =
            sql(db.prepare("SELECT manifest_json FROM context_bundles WHERE checkpoint_id=?1"))?;
        let rows = sql(statement.query_map([&cp.id], |r| r.get::<_, String>(0)))?;
        for row in rows {
            let bundle: context::Bundle = store::json(sql(row)?.as_bytes())?;
            if bundle
                .selected
                .iter()
                .any(|s| s.id == id && s.checksum == checksum)
            {
                return Ok(Some((usize::MAX, "push")));
            }
        }
    }
    let mut statement = sql(db.prepare("SELECT payload_json FROM o2_audit WHERE kind='mcp_fragment_delivered' AND record_id=?1 ORDER BY rowid DESC LIMIT 512"))?;
    let rows = sql(statement.query_map([&c.session_id], |r| r.get::<_, String>(0)))?;
    let mut end = offset;
    let mut spans = vec![];
    for row in rows {
        let v: Value = store::json(sql(row)?.as_bytes())?;
        if v["generation"] == c.generation && v["id"] == id && v["checksum"] == checksum {
            spans.push((
                v["start"].as_u64().unwrap_or(0) as usize,
                v["end"].as_u64().unwrap_or(0) as usize,
            ));
        }
    }
    spans.sort_unstable();
    for (start, stop) in spans {
        if start <= end && stop > end {
            end = stop;
        }
    }
    Ok((end > offset).then_some((end, "mcp")))
}

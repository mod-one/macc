//! Reviewed, fixed-sample task trials. No scheduler, paid calls or implicit policy writes.
mod adoption;
mod capture;
mod model;
mod observations;
mod statistics;
#[cfg(test)]
mod tests;
mod trials;
use super::*;
pub use adoption::{apply, preview};
pub(crate) use capture::{capture, finish};
pub use model::*;
pub use observations::record;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
pub use statistics::close;
pub use trials::{create, inspect};
fn encoded<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|e| error(e.to_string()))
}
fn hash<T: Serialize>(v: &T) -> Result<String> {
    Ok(digest(encoded(v)?.as_bytes()))
}
fn operator(confirmed: bool) -> Result<()> {
    if !confirmed || std::env::var_os("MACC_SESSION_ROLE").is_some() {
        return Err(error(
            "Policy trials require a separate confirmed operator action",
        ));
    }
    Ok(())
}
fn bounded<T: Serialize>(v: &T) -> Result<()> {
    let raw = encoded(v)?;
    if raw.len() > 262144 {
        return Err(error("Policy trial request exceeds 256 KiB"));
    }
    paths::safe_text(&raw)
}
fn load<T: serde::de::DeserializeOwned>(
    db: &rusqlite::Connection,
    kind: &str,
    id: &str,
) -> Result<T> {
    let raw:String=sql(db.query_row("SELECT payload_json FROM o2_audit WHERE kind=?1 AND record_id=?2 ORDER BY rowid DESC LIMIT 1",[kind,id],|r|r.get(0)))?;
    store::json(raw.as_bytes())
}
fn events<T: serde::de::DeserializeOwned>(
    db: &rusqlite::Connection,
    kind: &str,
    id: &str,
) -> Result<Vec<T>> {
    let mut q=sql(db.prepare("SELECT payload_json FROM o2_audit WHERE kind=?1 AND record_id=?2 ORDER BY rowid LIMIT 1001"))?;
    let rows = sql(q.query_map([kind, id], |r| r.get::<_, String>(0)))?
        .map(sql)
        .collect::<Result<Vec<_>>>()?;
    if rows.len() > 1000 {
        return Err(error("Policy trial event bound exceeded"));
    }
    rows.into_iter()
        .map(|raw| store::json(raw.as_bytes()))
        .collect()
}
fn receipt<T: Serialize>(report: T) -> Result<Value> {
    Ok(json!({"checksum":hash(&report)?,"report":report}))
}

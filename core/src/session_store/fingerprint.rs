//! Logical fingerprints ignore SQLite page layout and WAL checkpoint timing.
use super::*;
use rusqlite::{types::ValueRef, Connection};
use sha2::{Digest, Sha256};
fn field(hash: &mut Sha256, tag: u8, bytes: &[u8]) {
    hash.update([tag]);
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}
pub(crate) fn connection(db: &Connection) -> Result<String> {
    let mut schema = sql(db.prepare(
        "SELECT type,name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type,name",
    ))?;
    let entries = sql(schema.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    }))?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(|e| error(e.to_string()))?;
    let mut state = Sha256::new();
    for (kind, name, definition) in entries {
        field(&mut state, 0, kind.as_bytes());
        field(&mut state, 1, name.as_bytes());
        field(&mut state, 2, definition.unwrap_or_default().as_bytes());
        if kind != "table" {
            continue;
        }
        let mut q = sql(db.prepare(&format!("SELECT * FROM \"{}\"", name.replace('"', "\"\""))))?;
        let width = q.column_count();
        let mut rows = sql(q.query([]))?;
        let mut hashes = Vec::new();
        while let Some(row) = sql(rows.next())? {
            let mut hash = Sha256::new();
            for i in 0..width {
                match sql(row.get_ref(i))? {
                    ValueRef::Null => field(&mut hash, 0, &[]),
                    ValueRef::Integer(n) => field(&mut hash, 1, &n.to_le_bytes()),
                    ValueRef::Real(n) => field(&mut hash, 2, &n.to_bits().to_le_bytes()),
                    ValueRef::Text(b) => field(&mut hash, 3, b),
                    ValueRef::Blob(b) => field(&mut hash, 4, b),
                }
            }
            hashes.push(hash.finalize().to_vec());
        }
        hashes.sort();
        for hash in hashes {
            field(&mut state, 3, &hash);
        }
    }
    Ok(format!("sha256:{:x}", state.finalize()))
}
pub(crate) fn current(root: &Path) -> Result<String> {
    let _guard = lock(root)?;
    let (mut db, _) = db::active(root)?;
    let tx = sql(db.transaction())?;
    connection(&tx)
}

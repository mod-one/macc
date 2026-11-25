//! Keep large Git path inventories retrievable without spending the prompt budget on them.
use super::{digest, error, schema, Checkpoint, Result};
use rusqlite::{params, Connection};

pub(super) struct Inventory {
    sha256: String,
    paths: Vec<u8>,
}

impl Inventory {
    /// Project only the optional path listing. Git content identity, the task,
    /// decisions and unresolved findings always stay in the checkpoint.
    pub(super) fn project(c: &mut Checkpoint, limit: u64, setting: &str) -> Result<Option<Self>> {
        if c.changed_files.is_empty()
            || serde_json::to_vec(c)
                .map_err(|e| error(e.to_string()))?
                .len() as u64
                <= limit
        {
            schema::check_size(c, limit, setting)?;
            return Ok(None);
        }
        let paths = serde_json::to_vec(&c.changed_files).map_err(|e| error(e.to_string()))?;
        let sha256 = digest(&paths);
        c.validation["changed_file_inventory"] = serde_json::json!({
            "sha256": sha256, "count": c.changed_files.len(), "table": "checkpoint_file_inventories"
        });
        c.changed_files.clear();
        schema::check_size(c, limit, setting)?;
        Ok(Some(Self { sha256, paths }))
    }

    pub(super) fn store(&self, db: &Connection) -> Result<()> {
        super::sql(db.execute_batch("CREATE TABLE IF NOT EXISTS checkpoint_file_inventories (sha256 TEXT PRIMARY KEY, paths_json BLOB NOT NULL)"))?;
        super::sql(db.execute(
            "INSERT OR IGNORE INTO checkpoint_file_inventories VALUES (?1, ?2)",
            params![self.sha256, self.paths],
        ))?;
        Ok(())
    }
}

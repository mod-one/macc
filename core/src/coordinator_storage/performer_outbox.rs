//! Import terminal evidence left by a performer while the IPC listener was offline.
use super::{sql_err, write_transaction, CoordinatorStoragePaths, SqliteStorage};
use crate::coordinator::CoordinatorEventRecord;
use crate::{MaccError, ProjectPaths, Result};
use std::{fs, io, path::Path};

const DIRECTORY: &str = ".macc/state/performer-outbox";

fn io_error(path: &Path, source: io::Error) -> MaccError {
    MaccError::Io {
        path: path.display().to_string(),
        action: "replay terminal performer event".into(),
        source,
    }
}

fn import(store: &SqliteStorage, raw: &[u8]) -> Result<std::result::Result<(), String>> {
    let event: CoordinatorEventRecord = match serde_json::from_slice(raw) {
        Ok(event) => event,
        Err(error) => return Ok(Err(error.to_string())),
    };
    if !matches!(event.event_type.as_str(), "phase_result" | "failed")
        || !event.is_performer_runtime_event()
    {
        return Ok(Err("Expected a terminal performer event".into()));
    }
    if let Err(reason) = event.validate_performer_runtime_event() {
        return Ok(Err(reason));
    }
    if event
        .extra
        .get("performer_generation")
        .and_then(serde_json::Value::as_str)
        .is_none_or(|generation| generation.is_empty())
    {
        return Ok(Err(
            "Missing launch generation on offline terminal event".into()
        ));
    }
    let mut conn = store.open()?;
    let tx = write_transaction(&mut conn)?;
    // Bind to the retained assignment even across a coordinator restart. An
    // event from an earlier claim cannot publish into a replacement worker.
    if !current_claim(&tx, &event)? {
        return Ok(Err(
            "E418: terminal event belongs to a superseded or unknown assignment".into(),
        ));
    }
    store.append_event_record_in_tx(&tx, &event)?;
    tx.commit().map_err(sql_err)?;
    Ok(Ok(()))
}

/// A retry can reuse a claim while launching a new process generation.
pub(crate) fn current_claim(
    conn: &rusqlite::Connection,
    event: &CoordinatorEventRecord,
) -> Result<bool> {
    let generation = event
        .extra
        .get("performer_generation")
        .and_then(serde_json::Value::as_str);
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_runtime
        WHERE task_id=?1 AND run_id=?2 AND claim_id=?3 AND coordinator_epoch=?4 AND phase=?5
        AND (?6 IS NULL OR json_extract(payload_json,'$.performer_group.generation')=?6))",
        rusqlite::params![
            event.task_id,
            event.run_id,
            event.claim_id,
            event.coordinator_epoch,
            event.phase,
            generation
        ],
        |row| row.get(0),
    )
    .map_err(sql_err)
}

pub(crate) fn drain(root: &Path) -> Result<()> {
    let dir = root.join(DIRECTORY);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(&dir, error)),
    };
    let store = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(
        &ProjectPaths::from_root(root),
    ));
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| io_error(&dir, e))?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json")
            && entry.file_type().map_err(|e| io_error(&path, e))?.is_file()
        {
            files.push(path);
        }
    }
    files.sort();
    for path in files {
        let raw = fs::read(&path).map_err(|e| io_error(&path, e))?;
        match import(&store, &raw)? {
            Ok(()) => fs::remove_file(&path).map_err(|e| io_error(&path, e))?,
            Err(reason) => {
                tracing::warn!(path=%path.display(),%reason,"Retaining rejected terminal evidence");
                fs::rename(&path, path.with_extension("rejected"))
                    .map_err(|e| io_error(&path, e))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn replay_binds_terminal_evidence_to_the_original_assignment_and_deduplicates() {
        let root = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(root.path());
        let store = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        let conn = store.open().unwrap();
        store.init_schema(&conn).unwrap();
        conn.execute("INSERT INTO task_runtime(task_id,claim_id,run_id,coordinator_epoch,workflow_state,runtime_status,phase,updated_at,payload_json) VALUES('T','claim','old-run',2,'claimed','running','dev',CURRENT_TIMESTAMP,?1)",[r#"{"performer_group":{"generation":"original"}}"#]).unwrap();
        let dir = root.path().join(DIRECTORY);
        fs::create_dir_all(&dir).unwrap();
        let event = json!({"schema_version":"1","event_id":"evt","seq":1,"task_id":"T","source":"performer:test","ts":"2026-01-01T00:01:00Z","type":"phase_result","status":"failed","phase":"dev","run_id":"old-run","claim_id":"claim","coordinator_epoch":2,"performer_generation":"original","payload":{"message":"provider timeout"}});
        for field in [
            "run_id",
            "claim_id",
            "phase",
            "performer_generation",
            "coordinator_epoch",
        ] {
            let mut other = event.clone();
            other["event_id"] = json!(field);
            other[field] = if field == "coordinator_epoch" {
                json!(3)
            } else {
                json!("replacement")
            };
            fs::write(dir.join(format!("{field}.json")), other.to_string()).unwrap();
        }
        fs::write(dir.join("incomplete.tmp"), "{").unwrap();
        fs::write(dir.join("bad.json"), "{}").unwrap();
        for _ in 0..2 {
            fs::write(dir.join("terminal.json"), event.to_string()).unwrap();
            drain(root.path()).unwrap();
            assert!(!dir.join("terminal.json").exists());
        }
        assert_eq!(store.load_recent_events(20).unwrap().len(), 1);
        assert!(store.task_has_terminal_result_for_claim("T", "claim"));
        assert!(dir.join("bad.rejected").exists());
        assert!(dir.join("performer_generation.rejected").exists());
        assert!(dir.join("incomplete.tmp").exists());
    }
}

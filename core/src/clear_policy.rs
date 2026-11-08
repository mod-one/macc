//! One retention policy for clear execution and preview, independent from Git visibility.
use crate::{ProjectPaths, Result};
pub const PRESERVED: &[&str] = &[
    ".gitignore",
    ".macc/macc.yaml",
    ".macc/catalog",
    ".macc/persistent",
    ".macc/skills.lock.json",
    ".macc/memory.db",
    ".macc/memory.db-wal",
    ".macc/memory.db-shm",
    ".macc/state/tool-sessions.json",
    ".macc/state/tool-sessions.json.lock",
];
pub fn retained(relative: &str) -> bool {
    PRESERVED
        .iter()
        .any(|p| relative == *p || relative.starts_with(&format!("{p}/")))
}
pub fn existing_preserved(paths: &ProjectPaths) -> Vec<String> {
    PRESERVED
        .iter()
        .filter(|p| paths.root.join(p).exists())
        .map(|p| p.to_string())
        .collect()
}
/// Includes worktree runner PIDs, not only coordinator/supervisor ownership.
pub fn require_stopped(root: &std::path::Path) -> Result<()> {
    crate::session_store::require_stopped(root)?;
    let primary = crate::persistent::primary_root(root).unwrap_or_else(|_| root.into());
    if primary != root {
        crate::session_store::require_stopped(&primary)?;
    }
    require_idle_leases(&primary)?;
    let paths = crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(
        &ProjectPaths::from_root(&primary),
    );
    if paths.registry_json_path.exists() {
        let bytes = std::fs::read(&paths.registry_json_path).map_err(crate::persistent::error)?;
        let value = serde_json::from_slice(&bytes).map_err(crate::persistent::error)?;
        let registry = crate::coordinator::model::TaskRegistry::from_value(&value)?;
        if registry.tasks.iter().any(|t| {
            t.task_runtime
                .pid
                .is_some_and(crate::coordinator::helpers::is_pid_running)
        }) {
            return Err(crate::persistent::error(
                "Stop active task runners before clearing or migrating",
            ));
        }
    }
    require_idle_database(&paths.sqlite_path, "task_runtime", "pid")?;
    Ok(())
}

fn require_idle_leases(root: &std::path::Path) -> Result<()> {
    require_idle_database(
        &root.join(".macc/memory.db"),
        "session_leases",
        "owner_process_id",
    )
}
/// Read-only inspection: a safety check must not create coordinator storage.
fn require_idle_database(path: &std::path::Path, table: &str, column: &str) -> Result<()> {
    use std::io::Read;
    if !path.exists() {
        return Ok(());
    }
    let mut header = [0u8; 16];
    if std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut header))
        .is_err()
        || &header != b"SQLite format 3\0"
    {
        return Ok(());
    }
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(crate::persistent::error)?;
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1)",
            [table],
            |r| r.get(0),
        )
        .map_err(crate::persistent::error)?;
    if exists {
        // Identifiers are internal constants supplied above, never user input.
        let mut q = db
            .prepare(&format!(
                "SELECT {column} FROM {table} WHERE {column} IS NOT NULL"
            ))
            .map_err(crate::persistent::error)?;
        for pid in q
            .query_map([], |r| r.get::<_, i64>(0))
            .map_err(crate::persistent::error)?
        {
            if crate::coordinator::helpers::is_pid_running(pid.map_err(crate::persistent::error)?) {
                return Err(crate::persistent::error(
                    "Stop active tool/task session owners before clearing or migrating",
                ));
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preservation_uses_path_boundaries() {
        for p in [
            ".macc/catalog/custom.json",
            ".macc/persistent/control.sqlite-wal",
            ".macc/macc.yaml",
        ] {
            assert!(retained(p));
        }
        for p in [".macc/catalogue/x", ".macc/persistent-old/x", ".macc/tmp/x"] {
            assert!(!retained(p));
        }
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    fn clear_and_preview_preserve_configuration_catalogs_authority_and_o2() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        for rel in [
            ".macc/macc.yaml",
            ".macc/catalog/nested/custom.json",
            ".macc/persistent/control.sqlite",
            ".macc/persistent/broker/evidence",
            ".macc/memory.db",
            ".macc/tmp/generated",
        ] {
            let p = paths.root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "keep").unwrap();
            crate::record_managed_path(&paths, rel).unwrap();
        }
        for rel in [".macc/catalog", ".macc/persistent", ".macc/tmp", ".macc"] {
            crate::record_managed_path(&paths, rel).unwrap();
        }
        let preview = crate::clear_dry_run(&paths).unwrap();
        let result = crate::clear(&paths).unwrap();
        assert_eq!(
            (preview.removed, preview.skipped),
            (result.removed, result.skipped)
        );
        assert!(!paths.root.join(".macc/tmp/generated").exists());
        for p in existing_preserved(&paths) {
            assert!(paths.root.join(p).exists());
        }
        assert_eq!(
            std::fs::read_to_string(paths.root.join(".macc/persistent/broker/evidence")).unwrap(),
            "keep"
        );
        assert!(paths.config_path.exists());
        assert!(paths.catalog_dir.join("nested/custom.json").exists());
    }
    #[test]
    fn clear_refuses_a_live_coordinator() {
        let t = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(t.path());
        let handle = crate::process_ownership::ProcessHandle {
            kind: crate::process_ownership::ProcessKind::Coordinator,
            project_root: t.path().into(),
            pid: Some(std::process::id() as i32),
        };
        crate::service::process_ownership::register_process(t.path(), handle).unwrap();
        assert!(crate::clear(&paths).is_err());
    }
}

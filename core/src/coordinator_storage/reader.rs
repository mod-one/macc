//! Non-initializing reads of the project's authoritative coordinator registry.
use super::*;
use crate::{config::CoordinatorConfig, coordinator::types::CoordinatorEnvConfig};

pub struct CoordinatorReader {
    pub(crate) sqlite: SqliteStorage,
    pub(crate) mode: CoordinatorStorageMode,
    fallback: bool,
}

impl CoordinatorReader {
    pub fn for_project(paths: &ProjectPaths) -> Result<Self> {
        Self::configured(paths, None, &CoordinatorEnvConfig::default())
    }

    pub fn configured(
        paths: &ProjectPaths,
        cfg: Option<&CoordinatorConfig>,
        env: &CoordinatorEnvConfig,
    ) -> Result<Self> {
        let saved = if cfg.is_none()
            && paths
                .config_path
                .try_exists()
                .map_err(|e| MaccError::Validation(e.to_string()))?
        {
            crate::load_canonical_config(&paths.config_path)?
                .automation
                .coordinator
        } else {
            None
        };
        let cfg = cfg.or(saved.as_ref());
        let mut storage = CoordinatorStoragePaths::from_project_paths(paths);
        if let Some(file) = cfg.and_then(|c| c.task_registry_file.as_ref()) {
            storage.registry_json_path = paths.root.join(file);
        }
        Ok(Self {
            sqlite: SqliteStorage::read_only(storage),
            mode: crate::coordinator::engine::resolve_storage_mode(env, cfg)?,
            fallback: env
                .legacy_json_fallback
                .or_else(|| cfg.and_then(|c| c.legacy_json_fallback))
                .unwrap_or(false),
        })
    }

    pub(crate) fn uses_sqlite(&self) -> bool {
        self.mode != CoordinatorStorageMode::Json && self.sqlite.paths.sqlite_path.exists()
    }

    fn read<T>(
        &self,
        sqlite: impl FnOnce(&SqliteStorage) -> Result<T>,
        json: impl FnOnce(&CoordinatorStoragePaths) -> Result<T>,
        empty: impl FnOnce() -> T,
    ) -> Result<T> {
        if self.uses_sqlite() {
            match sqlite(&self.sqlite) {
                Err(_) if self.fallback => (),
                result => return result,
            }
        } else if self.mode != CoordinatorStorageMode::Json && !self.fallback {
            return Ok(empty());
        }
        json(&self.sqlite.paths)
    }

    pub fn registry(&self) -> Result<TaskRegistry> {
        self.read(
            SqliteStorage::load_registry,
            |paths| {
                read_registry_or_default(
                    &paths.registry_json_path,
                    "read coordinator registry json",
                )
            },
            default_registry,
        )
    }

    pub fn snapshot(&self) -> Result<CoordinatorSnapshot> {
        self.read(
            |s| s.load_snapshot(),
            |paths| JsonStorage::new(paths.clone()).load_snapshot(),
            CoordinatorSnapshot::empty,
        )
    }

    pub fn recent_events(&self, limit: usize) -> Result<Vec<CoordinatorEventRecord>> {
        self.read(
            |s| s.load_recent_events(limit),
            |paths| {
                let mut events = JsonStorage::new(paths.clone()).load_snapshot()?.events;
                Ok(events.split_off(events.len().saturating_sub(limit)))
            },
            Vec::new,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_poll_does_not_create_or_migrate_storage() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(dir.path());
        let reader = CoordinatorReader::for_project(&paths).unwrap();
        let db = &reader.sqlite.paths.sqlite_path;
        assert!(reader.registry().unwrap().tasks.is_empty());
        assert!(!db.exists());

        // An existing database without coordinator tables belongs to the writer
        // to initialize. Even if a read fails it must not change the schema.
        fs::create_dir_all(db.parent().unwrap()).unwrap();
        let conn = Connection::open(db).unwrap();
        conn.execute_batch("CREATE TABLE sentinel (id INTEGER)")
            .unwrap();
        assert!(reader.registry().is_err());
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 1);
        assert!(reader
            .sqlite
            .open()
            .unwrap()
            .execute_batch("CREATE TABLE forbidden (id INTEGER)")
            .is_err());
    }
}

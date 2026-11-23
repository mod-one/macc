//! Portable task registry payload shared by JSON and SQLite coordinator storage.
use super::{sessions, SaveBundleManifest};
use crate::{
    config::{load_canonical_config, CoordinatorConfigResolved},
    coordinator::model::TaskRegistry,
    coordinator_storage::{
        CoordinatorStorageMode, CoordinatorStoragePaths, JsonStorage, SqliteStorage,
    },
    MaccError, ProjectPaths, Result,
};
use std::{fs, path::Path};

pub(super) const PAYLOAD: &str = "coordinator/task_registry.json";

pub(super) struct RegistryStore {
    mirror_json: bool,
    mode: CoordinatorStorageMode,
    paths: CoordinatorStoragePaths,
}

impl RegistryStore {
    pub(super) fn resolve(paths: &ProjectPaths, config: &Path) -> Result<Self> {
        let cfg = if config.try_exists().map_err(registry_error)? {
            Some(load_canonical_config(config)?)
        } else {
            None
        };
        let cfg = CoordinatorConfigResolved::resolve(
            cfg.as_ref().and_then(|c| c.automation.coordinator.as_ref()),
        );
        let mode = cfg.storage_mode.parse().map_err(MaccError::Validation)?;
        Ok(Self {
            mirror_json: cfg.json_compat || mode == CoordinatorStorageMode::DualWrite,
            mode,
            paths: CoordinatorStoragePaths::from_project_paths(paths),
        })
    }

    pub(super) fn target(&self) -> &Path {
        match self.mode {
            CoordinatorStorageMode::Json => &self.paths.registry_json_path,
            _ => &self.paths.sqlite_path,
        }
    }

    pub(super) fn preflight(&self, paths: &ProjectPaths) -> Result<()> {
        use super::restore::files::destination;
        destination("coordinator registry", &paths.root, self.target(), false)?;
        if self.mirror_json {
            destination(
                "registry mirror",
                &paths.root,
                &self.paths.registry_json_path,
                false,
            )?;
        }
        crate::session_store::require_stopped(&paths.root)
    }

    pub(super) fn restore(&self, registry: &TaskRegistry) -> Result<()> {
        if self.mode != CoordinatorStorageMode::Json {
            SqliteStorage::new(self.paths.clone()).replace_registry(registry)?;
        }
        if self.mode == CoordinatorStorageMode::Json || self.mirror_json {
            JsonStorage::new(self.paths.clone()).replace_registry(registry)?;
        }
        Ok(())
    }
}

pub(super) fn capture(paths: &ProjectPaths, bundle: &Path) -> Result<String> {
    let store = RegistryStore::resolve(paths, &paths.config_path)?;
    let registry = match store.mode {
        CoordinatorStorageMode::Json => read_registry(store.target())?,
        _ => {
            let registry = SqliteStorage::read_only(store.paths).load_registry()?;
            validate_task_ids(&registry)?;
            registry
        }
    };
    let bytes = serde_json::to_vec_pretty(&registry).map_err(registry_error)?;
    let target = bundle.join(PAYLOAD);
    crate::atomic_write(paths, &target, &bytes)?;
    let written = fs::read(&target).map_err(registry_error)?;
    if written != bytes {
        return Err(MaccError::Validation(
            "Incomplete coordinator registry snapshot".into(),
        ));
    }
    Ok(super::integrity::sha256(&written))
}

fn read_registry(path: &Path) -> Result<TaskRegistry> {
    let bytes = fs::read(path).map_err(|source| MaccError::Io {
        path: path.display().to_string(),
        action: "read coordinator registry snapshot".into(),
        source,
    })?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(registry_error)?;
    if !value["tasks"].is_array() {
        return Err(MaccError::Validation(
            "Missing tasks array in coordinator registry snapshot".into(),
        ));
    }
    let registry = TaskRegistry::from_value(&value)?;
    validate_task_ids(&registry)?;
    Ok(registry)
}

fn validate_task_ids(registry: &TaskRegistry) -> Result<()> {
    let mut ids = std::collections::HashSet::new();
    for task in &registry.tasks {
        if task.id.trim().is_empty() || !ids.insert(&task.id) {
            return Err(registry_error("missing or duplicate task ID"));
        }
    }
    Ok(())
}

pub(super) fn unpack(bundle: &Path, manifest: &SaveBundleManifest) -> Result<TaskRegistry> {
    let relative = match manifest.paths.task_registry.as_deref() {
        Some(PAYLOAD) if manifest.hashes.task_registry.is_some() => PAYLOAD,
        // Legacy bundles named the directory instead of the registry file.
        Some("automation/task") if manifest.hashes.task_registry.is_none() => {
            "automation/task/task_registry.json"
        }
        _ => {
            return Err(MaccError::Validation(
                "MACC-RESTORE-2003: Missing coordinator registry path or checksum".into(),
            ))
        }
    };
    let path = sessions::payload(bundle, relative)?;
    if let Some(expected) = &manifest.hashes.task_registry {
        if &super::compute_file_sha256(&path).map_err(registry_error)? != expected {
            return Err(MaccError::Validation(
                "MACC-RESTORE-2003: Coordinator registry checksum mismatch".into(),
            ));
        }
    }
    read_registry(&path)
}

fn registry_error(error: impl std::fmt::Display) -> MaccError {
    MaccError::Validation(format!("Coordinator registry snapshot: {error}"))
}

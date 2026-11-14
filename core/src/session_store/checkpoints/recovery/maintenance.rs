//! Lease recovery and checkpoint retirement preserve history and never grant delivery.
use super::*;
use crate::coordinator_storage::{
    CoordinatorStorage, CoordinatorStoragePaths, JsonStorage, SqliteStorage,
};

pub(super) fn registry(root: &Path) -> Result<crate::coordinator::model::TaskRegistry> {
    let paths = crate::ProjectPaths::from_root(root);
    let config = if paths.config_path.exists() {
        crate::load_canonical_config(&paths.config_path)?
    } else {
        Default::default()
    };
    let mut storage = CoordinatorStoragePaths::from_project_paths(&paths);
    let cfg = config.automation.coordinator.as_ref();
    if let Some(file) = cfg.and_then(|c| c.task_registry_file.as_ref()) {
        storage.registry_json_path = root.join(file);
    }
    if cfg.and_then(|c| c.storage_mode.as_deref()) != Some("json") && storage.sqlite_path.exists() {
        let sqlite = SqliteStorage::new(storage.clone());
        if sqlite.has_snapshot_data()? {
            return sqlite.load_registry();
        }
    }
    if storage.registry_json_path.exists() {
        Ok(JsonStorage::new(storage).load_snapshot()?.registry)
    } else {
        Ok(Default::default())
    }
}

pub(super) fn delivered(task: &crate::coordinator::model::Task) -> bool {
    matches!(task.state.as_str(), "merged" | "done")
}

pub fn maintain(root: &Path) -> Result<usize> {
    if !root.join(".macc/memory.db").exists()
        || !authority(root)?.is_some_and(|a| a.mode == "sqlite")
    {
        return Ok(0);
    }
    let registry = registry(root)?;
    let _guard = lock(root)?;
    let (mut db, a) = db::active(root)?;
    if !ready(&db)? {
        return Ok(0);
    }
    let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
    let mut count = super::super::super::leases::recover_in(&tx)?;
    for task in registry.tasks.iter().filter(|t| delivered(t)) {
        let changed = sql(tx.execute("UPDATE task_checkpoints SET validation_state='retired' WHERE project_id=?1 AND task_id=?2 AND validation_state!='retired'", params![a.project_id,task.id]))?;
        if changed > 0 {
            db::event(
                &tx,
                &task.id,
                "checkpoints_retired",
                json!({"reason":"Task already delivered; checkpoints retained as historical evidence","count":changed}),
            )?;
            count += changed;
        }
    }
    sql(tx.commit())?;
    Ok(count)
}

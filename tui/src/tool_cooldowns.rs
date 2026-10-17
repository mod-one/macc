use crate::state::ThrottledToolInfo;
use macc_core::coordinator_storage::{CoordinatorStoragePaths, SqliteStorage};
use macc_core::ProjectPaths;

pub fn load(paths: Option<&ProjectPaths>) -> Result<Vec<ThrottledToolInfo>, String> {
    let Some(paths) = paths else {
        return Ok(Vec::new());
    };
    let storage = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(paths));
    let now = chrono::Utc::now().timestamp();
    Ok(storage
        .load_throttle_registry()
        .map_err(|error| error.to_string())?
        .into_values()
        .filter(|entry| entry.throttled_until as i64 > now)
        .filter_map(|entry| {
            chrono::DateTime::from_timestamp(entry.throttled_until as i64, 0).map(|until| {
                ThrottledToolInfo {
                    tool_id: entry.tool_id,
                    throttled_until: until.to_rfc3339(),
                    display_until: until.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
                    backoff_seconds: entry.backoff_seconds,
                    consecutive_count: entry.consecutive_429_count,
                }
            })
        })
        .collect())
}

pub fn label(entries: &[ThrottledToolInfo]) -> String {
    let now = chrono::Utc::now();
    entries
        .iter()
        .filter_map(|entry| {
            let until = chrono::DateTime::parse_from_rfc3339(&entry.throttled_until).ok()?;
            let seconds = until.signed_duration_since(now).num_seconds().max(0);
            Some(format!(
                "{}: available in {}h {:02}m {:02}s",
                entry.tool_id,
                seconds / 3600,
                (seconds % 3600) / 60,
                seconds % 60
            ))
        })
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quota_is_visible_without_task_delay_and_reset_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(dir.path());
        let storage = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
        let entry = macc_core::coordinator::rate_limit::ToolThrottleState {
            tool_id: "codex".into(),
            throttled_until: chrono::Utc::now().timestamp() as u64 + 3600,
            ..Default::default()
        };
        storage.upsert_tool_throttle("codex", &entry).unwrap();
        let entries = load(Some(&paths)).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(label(&entries).contains("codex: available in"));
        storage.delete_tool_throttle("codex").unwrap();
        assert!(load(Some(&paths)).unwrap().is_empty());
    }
}

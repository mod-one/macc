//! Delivered identities must not certify changed contracts; pending tasks can
//! still be revised through explicit queue acceptance before they are delivered.
use super::*;
use crate::{config::CoordinatorConfig, coordinator::model::TaskRegistry};
fn delivered(progress: &Progress, ids: &mut HashSet<String>) {
    let completed = if progress.status == "completed" {
        progress.entries.len()
    } else {
        progress.next
    };
    for entry in progress.entries.iter().take(completed) {
        ids.extend(entry.task_ids.iter().cloned());
    }
}
pub(super) fn check(
    root: &Path,
    config: &CoordinatorConfig,
    previous: &Progress,
    entries: &[Entry],
) -> Result<()> {
    let mut ids = HashSet::new();
    delivered(previous, &mut ids);
    let registry = super::transition::registry(root, config)?;
    ids.extend(
        registry
            .tasks
            .iter()
            .filter(|t| t.satisfies_dependants())
            .map(|t| t.id.clone()),
    );
    let mut recorded = previous.entries.clone();
    let history = root.join(".macc/state/prd-queue-history");
    if history.exists() {
        for entry in std::fs::read_dir(history).map_err(|e| invalid(e.to_string()))? {
            let dir = entry.map_err(|e| invalid(e.to_string()))?.path();
            let path = dir.join("progress.json");
            if !path.exists() {
                continue;
            }
            let p: Progress =
                serde_json::from_slice(&read(&path)?).map_err(|e| invalid(e.to_string()))?;
            delivered(&p, &mut ids);
            recorded.extend(p.entries);
            let path = dir.join("registry.json");
            if path.exists() {
                let registry: TaskRegistry =
                    serde_json::from_slice(&read(&path)?).map_err(|e| invalid(e.to_string()))?;
                ids.extend(
                    registry
                        .tasks
                        .iter()
                        .filter(|t| t.satisfies_dependants())
                        .map(|t| t.id.clone()),
                );
            }
        }
    }
    for entry in entries
        .iter()
        .filter(|entry| !previous.entries.contains(entry))
    {
        for old in recorded.iter().filter(|old| *old != entry) {
            if let Some(id) = entry
                .task_ids
                .iter()
                .find(|id| ids.contains(*id) && old.task_ids.contains(id))
            {
                return Err(invalid(format!("{}: new or changed PRD {} reuses recorded task {id} already delivered under another contract; retain the original input or assign a fresh task identifier. Reset does not waive delivery verification.",super::transition::CODE,entry.path)));
            }
        }
    }
    Ok(())
}

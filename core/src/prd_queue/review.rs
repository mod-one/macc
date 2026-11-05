//! Revision-bound review for explicit acceptance in graphical clients.
use super::*;
use crate::config::CoordinatorConfig;
#[derive(Debug, Serialize, Deserialize)]
pub struct Review {
    pub revision: String,
    pub previous: Vec<Entry>,
    pub proposed: Vec<Entry>,
    pub completed: usize,
    pub changes: Vec<String>,
    pub automatic: bool,
    pub explanation: String,
}
pub fn inspect(root: &Path, config: &CoordinatorConfig) -> Result<Review> {
    let old = load(root)?;
    let proposed = validate(root, &config.prd_files)?;
    let mut changes = Vec::new();
    for entry in &proposed {
        match old.entries.iter().find(|e| e.path == entry.path) {
            None => changes.push(format!("Added {}", entry.path)),
            Some(previous) if previous != entry => changes.push(format!("Modified {}", entry.path)),
            _ => {}
        }
    }
    for entry in &old.entries {
        if !proposed.iter().any(|e| e.path == entry.path) {
            changes.push(format!("Removed {}", entry.path));
        }
    }
    if changes.is_empty() && old.entries != proposed {
        changes.push("Order changed".into());
    }
    let preview = transition::preview(root, config, &proposed);
    let (automatic,explanation)=match preview {
        Ok(_) => (true,"Safe changes are accepted automatically by Sync or Run. Explicit reset starts at the first PRD and preserves existing delivery/worktree evidence.".into()),
        Err(e) => (false,e.to_string()),
    };
    let revision = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(config, &old, &proposed)).map_err(|e| invalid(e.to_string()))?
        )
    );
    Ok(Review {
        revision,
        previous: old.entries,
        proposed,
        completed: old.next,
        changes,
        automatic,
        explanation,
    })
}
pub fn accept(
    root: &Path,
    config: &CoordinatorConfig,
    revision: &str,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(invalid("Confirm the PRD queue transition first"));
    }
    let _lock = crate::fs_lock::AdvisoryLock::acquire(
        &root.join(".macc/state/prd-queue.lock"),
        std::time::Duration::ZERO,
        "PRD queue acceptance",
    )?;
    if inspect(root, config)?.revision != revision {
        return Err(invalid(
            "PRD queue changed; refresh and confirm a new review",
        ));
    }
    // Explicit acceptance does not bypass active-worker or unintegrated-work guards.
    transition::reset_locked(root, config)
}

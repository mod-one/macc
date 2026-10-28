//! Automatic checkpoint migration is limited to idle managed worker slots.
use std::path::Path;
fn slot(root: &Path, path: &Path, task_id: &str) -> bool {
    let Ok(pool) = root.join(".macc/worktree").canonicalize() else {
        return false;
    };
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    if path.parent() != Some(pool.as_path()) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    let suffix = name
        .strip_prefix("worker-")
        .or_else(|| name.strip_prefix("slot-"));
    if !suffix.is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    if super::git_state::verify_repository(root, &path).is_err() {
        return false;
    }
    if let Ok(Some(metadata)) = crate::read_worktree_metadata(&path) {
        if metadata.id != name {
            return false;
        }
    }
    let prd = std::fs::read(path.join("worktree.prd.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    prd.as_ref()
        .and_then(|p| p["tasks"].as_array())
        .is_some_and(|tasks| tasks.iter().any(|t| t["id"] == task_id))
}
pub(super) fn allowed(root: &Path, previous: &str, current: &str, task_id: &str) -> bool {
    slot(root, Path::new(previous), task_id) && slot(root, Path::new(current), task_id)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_actual_pool_slots_with_matching_task_can_migrate() {
        let t = crate::session_store::checkpoints::tests::fixture();
        let root = t.path();
        let first = root.join(".macc/worktree/worker-01");
        let second = root.join(".macc/worktree/worker-02");
        let foreign = root.join("worktrees/worker-03");
        for (index, path) in [&first, &second, &foreign].iter().enumerate() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(root)
                .args([
                    "worktree",
                    "add",
                    "-b",
                    &format!("slot-test-{index}"),
                    path.to_str().unwrap(),
                    "main",
                ])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            std::fs::copy(
                root.join("worktree.prd.json"),
                path.join("worktree.prd.json"),
            )
            .unwrap();
        }
        assert!(allowed(
            root,
            first.to_str().unwrap(),
            second.to_str().unwrap(),
            "A"
        ));
        assert!(!allowed(
            root,
            first.to_str().unwrap(),
            root.to_str().unwrap(),
            "A"
        ));
        assert!(!allowed(
            root,
            root.to_str().unwrap(),
            second.to_str().unwrap(),
            "A"
        ));
        assert!(!allowed(
            root,
            first.to_str().unwrap(),
            foreign.to_str().unwrap(),
            "A"
        ));
        assert!(!allowed(
            root,
            first.to_str().unwrap(),
            second.to_str().unwrap(),
            "unknown-task"
        ));
    }
}

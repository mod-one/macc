//! Enforce the boundary between prerequisite repair and unfinished task delivery.
use super::repair::git;
use macc_core::supervisor::incident::Incident;
use macc_core::{MaccError, Result};
use std::{collections::BTreeSet, path::Path};

pub fn prepare(worktree: &Path, incident: &Incident) -> Result<()> {
    git(worktree, &["add", "--all"])?;
    // Omit newly generated dependency/cache files while preserving deletions
    // of previously committed caches and retaining all evidence on disk.
    let added = git(
        worktree,
        &["diff", "--cached", "--name-only", "--diff-filter=A", "-z"],
    )?;
    for path in added.split('\0').filter(|p| !p.is_empty()) {
        if path.split('/').any(|s| {
            matches!(
                s,
                ".vite" | "node_modules" | "__pycache__" | ".pytest_cache"
            )
        }) {
            git(worktree, &["restore", "--staged", "--", path])?;
        }
    }
    let summary = git(worktree, &["diff", "--cached", "--stat"])?;
    let record = worktree.join(".macc/supervisor-staged-diff.txt");
    std::fs::write(&record, summary).map_err(|source| MaccError::Io {
        path: record.display().to_string(),
        action: "record supervisor staged changes".into(),
        source,
    })?;
    let staged: BTreeSet<_> = git(worktree, &["diff", "--cached", "--name-only", "-z"])?
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect();
    for task in &incident.tasks {
        let Some(worker) = task.worktree_path().map(Path::new).filter(|p| p.exists()) else {
            continue;
        };
        let changed = git(worker, &["diff", "HEAD", "--name-only", "-z"])?;
        let new = git(
            worker,
            &["ls-files", "--others", "--exclude-standard", "-z"],
        )?;
        if let Some(path) = changed
            .split('\0')
            .chain(new.split('\0'))
            .find(|p| staged.contains(*p))
        {
            return Err(MaccError::Validation(format!(
                "Supervisor repair overlaps unfinished task {} at {path}; repair and worker work retained. Repair prerequisites separately; resume the task for its implementation and acceptance instead of copying its delivery into a supervisor commit.", task.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_caches_are_unstaged_but_evidence_and_source_remain() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path();
        git(p, &["init", "-q"]).unwrap();
        git(p, &["config", "user.name", "Test"]).unwrap();
        git(p, &["config", "user.email", "test@example.invalid"]).unwrap();
        std::fs::write(p.join(".gitignore"), ".macc/\n").unwrap();
        git(p, &["add", "."]).unwrap();
        git(p, &["commit", "-qm", "initial"]).unwrap();
        std::fs::create_dir_all(p.join("docs/evidence/.vite/deps")).unwrap();
        std::fs::create_dir_all(p.join(".macc")).unwrap();
        std::fs::write(p.join("docs/evidence/.vite/deps/cache.js"), "cache").unwrap();
        std::fs::write(p.join("docs/evidence/results.txt"), "proof").unwrap();
        let incident = Incident {
            id: "i".into(),
            run_id: "r".into(),
            epoch: 1,
            reason: "test".into(),
            tasks: vec![],
            recent_events: vec![],
            run_status: None,
        };
        prepare(p, &incident).unwrap();
        let staged = git(p, &["diff", "--cached", "--name-only"]).unwrap();
        assert!(staged.contains("results.txt"));
        assert!(!staged.contains("cache.js"));
        assert!(p.join("docs/evidence/.vite/deps/cache.js").exists());
    }
}

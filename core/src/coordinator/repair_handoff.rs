//! Reconcile a stopped task's full content with repaired source without certifying delivery.
use crate::coordinator::model::Task;
use crate::{MaccError, Result};
use std::path::Path;

pub const RESUME_KEY: &str = "repair_resume";

fn git(path: &Path, args: &[&str]) -> Result<String> {
    let out = crate::git::run_git_output_mapped(path, args, "reconcile retained task work")?;
    if !out.status.success() {
        return Err(MaccError::Validation(format!(
            "Retained task reconciliation failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    let text = String::from_utf8(out.stdout).map_err(|e| {
        MaccError::Validation(format!(
            "Unsupported non-UTF-8 Git output during task reconciliation: {e}"
        ))
    })?;
    Ok(if args.contains(&"-z") {
        text
    } else {
        text.trim().into()
    })
}

/// Full content and original index are pinned under private refs before any checkout.
/// Conflicts are computed off-worktree: the worker remains untouched on rejection.
/// Successful reconciliation keeps the branch and leaves unfinished work uncommitted.
pub fn reconcile(root: &Path, task: &Task, reference: &str) -> Result<String> {
    let path = task.worktree_path().map(Path::new).ok_or_else(|| {
        MaccError::Validation(format!("Task {} has no retained worktree", task.id))
    })?;
    let branch = task
        .branch()
        .ok_or_else(|| MaccError::Validation("Retained task has no branch".into()))?;
    let canonical = std::fs::canonicalize(path).map_err(|source| MaccError::Io {
        path: path.display().to_string(),
        action: "locate retained task".into(),
        source,
    })?;
    let pool =
        std::fs::canonicalize(root.join(".macc/worktree")).map_err(|source| MaccError::Io {
            path: root.display().to_string(),
            action: "locate worker pool".into(),
            source,
        })?;
    if canonical.parent() != Some(pool.as_path())
        || crate::git::git_common_dir(path)? != crate::git::git_common_dir(root)?
        || git(path, &["branch", "--show-current"])? != branch
    {
        return Err(MaccError::Validation(
            "Retained worktree ownership changed; refusing reconciliation".into(),
        ));
    }
    if task
        .task_runtime
        .pid
        .is_some_and(crate::coordinator::helpers::is_pid_running)
    {
        return Err(MaccError::Validation(
            "Retained worker is still running; reconciliation deferred".into(),
        ));
    }
    let _lock = crate::fs_lock::AdvisoryLock::acquire(
        &path.join(".macc/repair-handoff.lock"),
        std::time::Duration::ZERO,
        "retained task reconciliation",
    )?;
    let head = git(path, &["rev-parse", "HEAD"])?;
    let target = git(
        root,
        &["rev-parse", "--verify", &format!("refs/heads/{reference}")],
    )?;
    let tree = crate::git::worktree_content_tree(path)?;
    let index_tree = git(path, &["write-tree"])?;
    let snapshot = git(
        path,
        &[
            "commit-tree",
            &tree,
            "-p",
            &head,
            "-m",
            "wip: preserve retained task before prerequisite repair",
        ],
    )?;
    let index = git(
        path,
        &[
            "commit-tree",
            &index_tree,
            "-p",
            &head,
            "-m",
            "wip: preserve retained task index",
        ],
    )?;
    let backup = format!("refs/macc/recovery/{snapshot}-{index}");
    git(root, &["update-ref", &backup, &snapshot])?;
    git(root, &["update-ref", &format!("{backup}-index"), &index])?;
    let log = root.join(".macc/log/repair-handoff");
    std::fs::create_dir_all(&log).map_err(|source| MaccError::Io {
        path: log.display().to_string(),
        action: "archive retained task recovery".into(),
        source,
    })?;
    let record = log.join(format!("{snapshot}-{index}-{target}.json"));
    std::fs::write(
        &record,
        serde_json::to_vec_pretty(&serde_json::json!({
            "task_id": task.id, "worktree": path, "branch": branch, "original_head": head,
            "reference": target, "content_ref": backup, "index_ref": format!("{backup}-index"),
            "status": "preserved", "delivery": false
        }))
        .map_err(|e| MaccError::Validation(e.to_string()))?,
    )
    .map_err(|source| MaccError::Io {
        path: record.display().to_string(),
        action: "archive retained task recovery".into(),
        source,
    })?;
    if head == target {
        return Ok(target);
    }
    let merged = merge_tree(path, &snapshot, &target, &backup)?;
    // Preserve committed worker history independently of its unfinished edits.
    let base_tree = merge_tree(path, &head, &target, &backup)?;
    let base = if git(path, &["rev-parse", &format!("{target}^{{tree}}")])? == base_tree {
        // A merge commit still retains divergent committed worker ancestry.
        let ancestor = crate::git::run_git_output_mapped(
            path,
            &["merge-base", "--is-ancestor", &head, &target],
            "inspect repair ancestry",
        )?;
        if ancestor.status.success() {
            target.clone()
        } else {
            git(
                path,
                &[
                    "commit-tree",
                    &base_tree,
                    "-p",
                    &head,
                    "-p",
                    &target,
                    "-m",
                    "wip: reconcile prerequisite repair with retained task history",
                ],
            )?
        }
    } else {
        git(
            path,
            &[
                "commit-tree",
                &base_tree,
                "-p",
                &head,
                "-p",
                &target,
                "-m",
                "wip: reconcile prerequisite repair with retained task history",
            ],
        )?
    };
    // Ignored local files are not in the snapshot; never overwrite one with a repair.
    let ignored = git(
        path,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
        ],
    )?;
    let changed = git(path, &["diff", "--name-only", "-z", &tree, &merged])?;
    if ignored.split('\0').filter(|p| !p.is_empty()).any(|p| {
        changed
            .split('\0')
            .filter(|p| !p.is_empty())
            .any(|c| c == p || c.starts_with(&format!("{p}/")) || p.starts_with(&format!("{c}/")))
    }) {
        return Err(MaccError::Validation(format!(
            "Repair overlaps ignored local files; work preserved at {backup}"
        )));
    }
    if git(path, &["rev-parse", "HEAD"])? != head
        || crate::git::worktree_content_tree(path)? != tree
    {
        return Err(MaccError::Validation(format!(
            "Worker changed during reconciliation; work preserved at {backup}"
        )));
    }
    // Two-tree checkout through a private index preserves untracked additions in
    // the snapshot. Git refuses changed files; no reset/clean or forced checkout.
    let scratch = path.join(".macc/repair-checkout-index");
    let env_path = scratch.to_string_lossy();
    let env = [("GIT_INDEX_FILE", env_path.as_ref())];
    let apply = |args: &[&str]| -> Result<()> {
        let out =
            crate::git::run_git_output_with_env(path, args, &env, "apply reconciled task content")?;
        if !out.status.success() {
            return Err(MaccError::Validation(format!(
                "Task reconciliation checkout refused; original work pinned at {backup}: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        Ok(())
    };
    let applied = apply(&["read-tree", &tree])
        .and_then(|_| apply(&["update-index", "--refresh"]))
        .and_then(|_| apply(&["read-tree", "-m", "-u", &tree, &merged]));
    let _ = std::fs::remove_file(&scratch);
    applied?;
    git(
        path,
        &["update-ref", &format!("refs/heads/{branch}"), &base, &head],
    )?;
    git(path, &["read-tree", &base])?;
    Ok(base)
}

fn merge_tree(path: &Path, local: &str, target: &str, backup: &str) -> Result<String> {
    let out = crate::git::run_git_output_mapped(
        path,
        &["merge-tree", "--write-tree", local, target],
        "merge retained task with repair",
    )?;
    if !out.status.success() {
        return Err(MaccError::Validation(format!(
            "Repair conflicts with retained task; worker unchanged, original work at {backup}. Resolve the three-way conflict before unblocking: {} {}",
            String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .into())
}

#[cfg(test)]
#[path = "repair_handoff_tests.rs"]
mod tests;

//! Git operations for task delivery use creation evidence from the affected checkout.
use crate::{project_ownership, MaccError, ProjectPaths, Result};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

fn checked(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = super::run_git_output(root, args, "prepare task changes")?;
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: format!("git {}", args[0]),
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(output.stdout)
}

fn owned_files(root: &Path) -> Result<BTreeSet<String>> {
    Ok(project_ownership::read(&ProjectPaths::from_root(root))?
        .files
        .into_keys()
        .collect())
}

/// Keep existing tracked files editable. Use exact paths, including inside user directories.
pub fn stage_task_changes(root: &Path) -> Result<bool> {
    if checked(root, &["rev-parse", "--show-prefix"])? != b"\n" {
        return Err(MaccError::Validation(
            "Task staging requires the worktree root".into(),
        ));
    }
    let mut private = owned_files(root)?;
    if !private.is_empty() && super::rev_parse_verify(root, "HEAD")? {
        let tracked = checked(root, &["ls-tree", "-rz", "--name-only", "HEAD"])?;
        for name in tracked
            .split(|b| *b == 0)
            .filter_map(|s| std::str::from_utf8(s).ok())
        {
            private.remove(name);
        }
    }
    let exclusions: Vec<_> = private
        .iter()
        .map(|p| format!(":(top,exclude,literal){p}"))
        .collect();
    let mut add = vec!["add", "-A", "--", "."];
    add.extend(exclusions.iter().map(String::as_str));
    checked(root, &add)?;
    // An AI or an interrupted attempt may already have staged a private addition.
    if !private.is_empty() {
        let staged = checked(
            root,
            &["diff", "--cached", "--name-only", "--no-renames", "-z"],
        )?;
        let mut reset = vec!["--literal-pathspecs", "reset", "-q", "--"];
        reset.extend(
            staged
                .split(|b| *b == 0)
                .filter_map(|s| std::str::from_utf8(s).ok())
                .filter(|p| private.contains(*p)),
        );
        if reset.len() > 4 {
            checked(root, &reset)?;
        }
    }
    Ok(!checked(root, &["diff", "--cached", "--name-only", "-z"])?.is_empty())
}

/// Inspect the actual integrated tree, including commits made directly by a performer.
pub(crate) fn private_additions(
    root: &Path,
    sources: &[PathBuf],
    base: &str,
    candidate: &str,
) -> Result<Vec<String>> {
    let mut owned = owned_files(root)?;
    for checkout in sources {
        owned.extend(owned_files(checkout)?);
    }
    if owned.is_empty() {
        return Ok(vec![]);
    }
    let added = checked(
        root,
        &[
            "diff",
            "--no-renames",
            "--diff-filter=A",
            "--name-only",
            "-z",
            base,
            candidate,
            "--",
        ],
    )?;
    Ok(added
        .split(|b| *b == 0)
        .filter_map(|s| std::str::from_utf8(s).ok())
        .filter(|p| owned.contains(*p))
        .map(str::to_owned)
        .collect())
}

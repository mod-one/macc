//! Git operations for task delivery use creation evidence from the affected checkout.
use crate::{project_ownership, MaccError, ProjectPaths, Result};
use std::{
    collections::BTreeSet,
    io::{Seek, Write},
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn checked(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = super::run_git_output(root, args, "prepare task changes")?;
    checked_output(args[0], output)
}

fn checked_output(operation: &str, output: Output) -> Result<Vec<u8>> {
    if !output.status.success() {
        return Err(MaccError::Git {
            operation: format!("git {operation}"),
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

/// Pass exact filenames as bytes, independent of shell syntax or argument limits.
fn apply_paths<'a>(
    root: &Path,
    command: &str,
    paths: impl Iterator<Item = &'a [u8]>,
) -> Result<()> {
    let mut paths = paths.peekable();
    if paths.peek().is_none() {
        return Ok(());
    }
    let io = |source| MaccError::Io {
        path: root.display().to_string(),
        action: format!("prepare git {command} pathspec"),
        source,
    };
    let mut input = std::io::BufWriter::new(tempfile::tempfile().map_err(io)?);
    for name in paths {
        input.write_all(name).map_err(io)?;
        input.write_all(&[0]).map_err(io)?;
    }
    input.rewind().map_err(io)?;
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "--literal-pathspecs",
            command,
            "--pathspec-file-nul",
            "--pathspec-from-file=-",
        ])
        .stdin(input.into_inner().map_err(|e| io(e.into_error()))?)
        .output()
        .map_err(io)?;
    checked_output(command, output)?;
    Ok(())
}

/// Only an untracked file with unchanged, checkout-local creation evidence can
/// be present during review. Staged or tracked edits always need inspection.
pub(crate) fn review_worktree_change(root: &Path) -> Result<Option<String>> {
    let status = checked(
        root,
        &[
            "status",
            "--porcelain=v1",
            "--no-renames",
            "--untracked-files=all",
            "-z",
        ],
    )?;
    let paths = ProjectPaths::from_root(root);
    let mut inventory = None;
    for entry in status.split(|b| *b == 0).filter(|s| !s.is_empty()) {
        if let Some(name) = entry
            .strip_prefix(b"?? ")
            .and_then(|s| std::str::from_utf8(s).ok())
        {
            if inventory.is_none() {
                inventory = Some(project_ownership::read(&paths)?);
            }
            if inventory
                .as_ref()
                .is_some_and(|i| i.owns_unchanged(&paths, name))
            {
                continue;
            }
        }
        return Ok(Some(
            String::from_utf8_lossy(entry).escape_debug().to_string(),
        ));
    }
    Ok(None)
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
    let is_private = |name: &[u8]| std::str::from_utf8(name).is_ok_and(|p| private.contains(p));
    // Include intent-to-add entries, which a cached diff can omit.
    if !private.is_empty() {
        let index = checked(root, &["ls-files", "--cached", "-z"])?;
        apply_paths(
            root,
            "reset",
            index
                .split(|b| *b == 0)
                .filter(|name| !name.is_empty() && is_private(name)),
        )?;
    }
    checked(root, &["add", "-u", "--", "."])?;
    let candidates = checked(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    apply_paths(
        root,
        "add",
        candidates
            .split(|b| *b == 0)
            .filter(|name| !name.is_empty() && !is_private(name)),
    )?;
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

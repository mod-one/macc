//! Interrupted validation may resume only from one of its two known trees.
use super::{git, io, write_json, MaccError, Result, Value, OWNER, RESTORE};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn retained(path: &Path, reason: impl std::fmt::Display) -> MaccError {
    MaccError::Validation(format!(
        "Supervisor worktree {} preserved: {reason}",
        path.display()
    ))
}

fn canonical(path: &Path) -> Result<PathBuf> {
    path.canonicalize().map_err(|e| io(path, e))
}

pub(super) fn owner(root: &Path, path: &Path, incident: &str) -> Result<Value> {
    let slot = canonical(path)?;
    if path.parent() != Some(root.join(".macc/worktree").as_path())
        || slot
            != canonical(root)?
                .join(".macc/worktree")
                .join(path.file_name().unwrap_or_default())
        || canonical(Path::new(&git(path, &["rev-parse", "--show-toplevel"])?))? != slot
        || canonical(&macc_core::git::git_common_dir(root)?)?
            != canonical(&macc_core::git::git_common_dir(path)?)?
    {
        return Err(retained(path, "repository or slot path differs"));
    }
    let marker = path.join(OWNER);
    let value: Value = serde_json::from_slice(&std::fs::read(&marker).map_err(|e| io(&marker, e))?)
        .map_err(|e| retained(path, e))?;
    let actual = git(path, &["branch", "--show-current"])?;
    let base = value["base"]
        .as_str()
        .ok_or_else(|| retained(path, "missing base"))?;
    let legacy = format!(
        "macc/supervisor/{}-{}",
        super::super::report::slug(incident),
        base.get(..12).unwrap_or("")
    );
    let matches = match value["repair_branch"].as_str() {
        Some(expected) => actual == expected && actual.starts_with("macc/supervisor/"),
        None => {
            actual == legacy
                || actual
                    .strip_prefix(&format!("{legacy}-"))
                    .is_some_and(|n| n.parse::<u32>().is_ok())
        }
    };
    if value["incident_id"].as_str() != Some(incident) || !matches {
        return Err(retained(path, format!("expected incident {incident}, owner {}, expected branch {}, current branch {actual}", value["incident_id"], value["repair_branch"])));
    }
    if !macc_core::git::worktrees_for_branch(root, &actual)?
        .iter()
        .any(|registered| registered.canonicalize().ok().as_ref() == Some(&slot))
    {
        return Err(retained(path, "branch is not registered to this worktree"));
    }
    Ok(value)
}

#[derive(Serialize, Deserialize)]
struct Marker {
    incident: String,
    branch: String,
    head: String,
    git_dir: PathBuf,
    tree: String,
}

fn git_dir(path: &Path) -> Result<PathBuf> {
    canonical(Path::new(&git(path, &["rev-parse", "--absolute-git-dir"])?))
}

// read-tree can overwrite ignored files. Check the incoming additions on disk,
// including symlink or file ancestors, before updating either tree.
fn switch_tree(path: &Path, from: &str, to: &str) -> Result<()> {
    let added = git(
        path,
        &[
            "diff-tree",
            "--no-commit-id",
            "-r",
            "--no-renames",
            "--diff-filter=A",
            "--name-only",
            "-z",
            from,
            to,
            "--",
        ],
    )?;
    for name in added.split_terminator('\0') {
        let target = path.join(name);
        let mut prefix = path.to_path_buf();
        for part in Path::new(name).components() {
            prefix.push(part);
            match std::fs::symlink_metadata(&prefix) {
                Ok(meta) if prefix == target || !meta.is_dir() => {
                    return Err(retained(
                        path,
                        format!("incoming file {name} overlaps {}", prefix.display()),
                    ));
                }
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(e) => return Err(io(&prefix, e)),
            }
        }
    }
    git(path, &["read-tree", "-m", "-u", from, to])?;
    Ok(())
}

pub fn enter_baseline(root: &Path, path: &Path, incident: &str) -> Result<()> {
    let owner = owner(root, path, incident)?;
    let head = git(path, &["rev-parse", "HEAD"])?;
    if path.join(RESTORE).try_exists().map_err(|e| io(path, e))? || owner["base"] != head {
        return Err(retained(
            path,
            "baseline already pending or HEAD differs from incident base",
        ));
    }
    git(path, &["add", "--all"])?;
    let marker = Marker {
        incident: incident.into(),
        branch: git(path, &["branch", "--show-current"])?,
        head,
        git_dir: git_dir(path)?,
        tree: git(path, &["write-tree"])?,
    };
    write_json(&path.join(RESTORE), &serde_json::json!(marker))?;
    switch_tree(path, &marker.tree, &marker.head)
}

pub fn restore_repair(root: &Path, path: &Path, incident: &str) -> Result<()> {
    let owner = owner(root, path, incident)?;
    let file = path.join(RESTORE);
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io(&file, e)),
    };
    let m: Marker = serde_json::from_slice(&bytes).map_err(|_| {
        retained(
            path,
            "legacy or unreadable restoration marker; inspect the saved tree manually",
        )
    })?;
    let head = git(path, &["rev-parse", "HEAD"])?;
    if m.incident != incident
        || m.branch != git(path, &["branch", "--show-current"])?
        || m.git_dir != git_dir(path)?
        || m.head != head
        || owner["base"] != head
    {
        return Err(retained(path,format!("incident, branch, worktree or HEAD differs from restoration marker (expected {}, current {head})",m.head)));
    }
    if m.tree.len() != head.len()
        || !m.tree.bytes().all(|b| b.is_ascii_hexdigit())
        || git(path, &["cat-file", "-t", &m.tree])? != "tree"
    {
        return Err(retained(path, "invalid repair tree"));
    }
    let index = git(path, &["write-tree"])?;
    let base = git(path, &["rev-parse", "HEAD^{tree}"])?;
    let modified = git(path, &["diff", "--name-status"])?;
    let untracked = git(path, &["ls-files", "--others", "--exclude-standard"])?;
    if (index != base && index != m.tree) || !modified.is_empty() || !untracked.is_empty() {
        let staged = git(path, &["diff", "--cached", "--name-status"])?;
        return Err(retained(path,format!("baseline changed; staged [{staged}], modified [{modified}], untracked [{untracked}]")));
    }
    if index != m.tree {
        switch_tree(path, &base, &m.tree)?;
    }
    std::fs::remove_file(&file).map_err(|e| io(&file, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn parked() -> (tempfile::TempDir, PathBuf) {
        let root = super::super::tests::fixture();
        let target = super::super::target(root.path(), None).unwrap();
        let slot = super::super::prepare(root.path(), "a", &target, false).unwrap();
        fs::write(slot.join("source.txt"), "repair").unwrap();
        enter_baseline(root.path(), &slot, "a").unwrap();
        assert_eq!(fs::read(slot.join("source.txt")).unwrap(), b"original\n");
        (root, slot)
    }

    #[test]
    fn unchanged_baseline_recovers_only_the_saved_repair() {
        let (root, slot) = parked();
        restore_repair(root.path(), &slot, "a").unwrap();
        assert_eq!(fs::read(slot.join("source.txt")).unwrap(), b"repair");
        assert_eq!(git(&slot, &["show", ":source.txt"]).unwrap(), "repair");
        assert!(!slot.join(RESTORE).exists());
    }

    #[test]
    fn interrupted_baseline_preserves_index_working_copy_and_untracked_files() {
        let (root, slot) = parked();
        fs::write(slot.join("source.txt"), "manual staged").unwrap();
        git(&slot, &["add", "source.txt"]).unwrap();
        fs::write(slot.join("source.txt"), "manual unstaged").unwrap();
        fs::write(slot.join("notes"), "keep").unwrap();
        let target = super::super::target(root.path(), None).unwrap();
        let err = super::super::prepare(root.path(), "a", &target, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("notes") && err.contains("source.txt"), "{err}");
        assert_eq!(
            git(&slot, &["show", ":source.txt"]).unwrap(),
            "manual staged"
        );
        assert_eq!(
            fs::read(slot.join("source.txt")).unwrap(),
            b"manual unstaged"
        );
        assert_eq!(fs::read(slot.join("notes")).unwrap(), b"keep");
        assert!(slot.join(RESTORE).exists());
    }

    #[test]
    fn ignored_file_at_incoming_repair_path_survives() {
        let root = super::super::tests::fixture();
        let target = super::super::target(root.path(), None).unwrap();
        let slot = super::super::prepare(root.path(), "a", &target, false).unwrap();
        fs::write(slot.join("notes"), "repair").unwrap();
        enter_baseline(root.path(), &slot, "a").unwrap();
        // Use the repository's local excludes so the baseline stays clean.
        let local = macc_core::git::git_common_dir(&slot)
            .unwrap()
            .join("info/exclude");
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(local)
            .unwrap()
            .write_all(b"\n/notes\n")
            .unwrap();
        fs::write(slot.join("notes"), "manual ignored").unwrap();
        assert!(restore_repair(root.path(), &slot, "a")
            .unwrap_err()
            .to_string()
            .contains("notes"));
        assert_eq!(fs::read(slot.join("notes")).unwrap(), b"manual ignored");
        assert!(slot.join(RESTORE).exists());
    }

    #[test]
    fn cleanup_requires_matching_project_incident_branch_head_and_tree() {
        let (root, slot) = parked();
        let cleanup = |root: &Path, incident: &str| {
            super::super::finish(root, incident, &slot, &root.join("logs"), true, false)
        };
        assert!(cleanup(root.path(), "other").is_err());
        assert!(cleanup(super::super::tests::fixture().path(), "a").is_err());
        let branch = git(&slot, &["branch", "--show-current"]).unwrap();
        git(&slot, &["switch", "-c", "personal"]).unwrap();
        assert!(cleanup(root.path(), "a").is_err());
        git(&slot, &["switch", &branch]).unwrap();
        let head = git(&slot, &["rev-parse", "HEAD"]).unwrap();
        let file = slot.join(RESTORE);
        let original = fs::read(&file).unwrap();
        let mut invalid: Value = serde_json::from_slice(&original).unwrap();
        invalid["tree"] = Value::String(head.clone());
        write_json(&file, &invalid).unwrap();
        assert!(cleanup(root.path(), "a").is_err());
        fs::write(&file, &head).unwrap();
        assert!(cleanup(root.path(), "a").is_err());
        fs::write(&file, &original).unwrap();
        git(&slot, &["commit", "--allow-empty", "-qm", "manual"]).unwrap();
        assert!(cleanup(root.path(), "a").is_err());
        assert_eq!(fs::read(file).unwrap(), original);
        assert_eq!(fs::read(slot.join("source.txt")).unwrap(), b"original\n");
    }
}

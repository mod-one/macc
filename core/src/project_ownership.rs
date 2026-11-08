//! Provenance for generated project files. Existing user files are never adopted.
use crate::{MaccError, ProjectPaths, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
const RECORD: &str = ".macc/persistent/generated-project-files.json";
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Inventory {
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub directories: BTreeSet<String>,
    #[serde(default)]
    pub branches: BTreeSet<String>,
    #[serde(default)]
    pub protected: BTreeSet<String>,
}
pub(crate) fn read(paths: &ProjectPaths) -> Result<Inventory> {
    let path = paths.root.join(RECORD);
    validate_path(paths, RECORD)?;
    crate::persistent::no_symlink(&paths.macc_dir)?;
    crate::persistent::no_symlink(&paths.macc_dir.join("persistent"))?;
    crate::persistent::no_symlink(&path)?;
    let inventory: Inventory = match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            MaccError::Validation(format!("Invalid generated-file provenance: {e}"))
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Inventory::default(),
        Err(e) => return Err(crate::persistent::error(e)),
    };
    for relative in inventory
        .files
        .keys()
        .chain(inventory.directories.iter())
        .chain(inventory.protected.iter())
    {
        validate_path(paths, relative)?;
    }
    Ok(inventory)
}

pub(crate) fn save(paths: &ProjectPaths, value: &Inventory) -> Result<()> {
    crate::atomic_write(
        paths,
        &paths.root.join(RECORD),
        &serde_json::to_vec_pretty(value).map_err(crate::persistent::error)?,
    )
}
/// Retain the exact creation/hash evidence before Clear prunes the live inventory.
pub(crate) fn archive_before_clear(paths: &ProjectPaths, archive: &Path) -> Result<()> {
    let _guard = lock(paths)?;
    crate::atomic_write(
        paths,
        &archive.join("generated-project-files-before.json"),
        &serde_json::to_vec_pretty(&read(paths)?).map_err(crate::persistent::error)?,
    )
}
pub(crate) fn digest(path: &Path) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(std::fs::read(path).map_err(crate::persistent::error)?)
    ))
}
/// Capture before parent creation, then record only successful MACC creations/updates.
pub(crate) fn missing_parents(paths: &ProjectPaths, path: &Path) -> Vec<String> {
    let mut parents = vec![];
    let mut current = path.parent();
    while let Some(parent) = current {
        if parent == paths.root || parent.exists() {
            break;
        }
        if let Some(rel) = crate::relative_to_root(paths, parent) {
            parents.push(rel);
        }
        current = parent.parent();
    }
    parents
}
pub(crate) fn record_write(
    paths: &ProjectPaths,
    path: &Path,
    existed: bool,
    parents: Vec<String>,
    previous: Option<&[u8]>,
) -> Result<()> {
    let Some(rel) = crate::relative_to_root(paths, path) else {
        return Ok(());
    };
    if rel == ".gitignore" || rel.starts_with(".macc/") {
        return Ok(());
    }
    validate_path(paths, &rel)?;
    let _guard = lock(paths)?;
    let mut inventory = read(paths)?;
    if existed {
        if inventory.protected.contains(&rel) {
            return Ok(());
        }
        let Some(expected) = inventory.files.get(&rel) else {
            return Ok(());
        };
        let previous = previous.map(|bytes| format!("{:x}", Sha256::digest(bytes)));
        if previous.as_ref() != Some(expected) {
            // Keep creation provenance (and its private Git exclusion), but
            // permanently relinquish deletion ownership for this surviving file.
            inventory.protected.insert(rel);
            return save(paths, &inventory);
        }
    }
    inventory.protected.remove(&rel);
    inventory.files.insert(rel, digest(path)?);
    inventory.directories.extend(parents);
    save(paths, &inventory)
}
pub fn record_branch(root: &Path, branch: &str) -> Result<()> {
    let root = crate::persistent::primary_root(root).unwrap_or_else(|_| root.to_path_buf());
    let paths = ProjectPaths::from_root(&root);
    let _guard = lock(&paths)?;
    let mut inventory = read(&paths)?;
    inventory
        .branches
        .insert(branch.trim_start_matches("refs/heads/").into());
    save(&paths, &inventory)
}
pub(crate) fn record_directory(paths: &ProjectPaths, relative: &str) -> Result<()> {
    if relative == ".macc" || relative.starts_with(".macc/") {
        return Ok(());
    }
    let _guard = lock(paths)?;
    let mut inventory = read(paths)?;
    inventory.directories.insert(relative.into());
    save(paths, &inventory)
}

/// Reject traversal and symlinked parent directories before reading or deleting a
/// provenance path. A final symlink is also unowned user data.
pub(crate) fn validate_path(paths: &ProjectPaths, relative: &str) -> Result<()> {
    if crate::normalize_relative_path(relative).as_deref() != Some(relative)
        || relative == ".git"
        || relative.starts_with(".git/")
    {
        return Err(crate::persistent::error("Unsafe generated project path"));
    }
    let mut path = paths.root.clone();
    for part in Path::new(relative).components() {
        path.push(part);
        crate::persistent::no_symlink(&path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preexisting_tool_files_are_never_adopted_hidden_or_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        std::fs::create_dir_all(paths.root.join(".claude")).unwrap();
        std::fs::write(paths.root.join(".claude/settings.json"), "user").unwrap();
        std::fs::write(paths.root.join("CLAUDE.md"), "user").unwrap();
        for rel in [
            ".claude/settings.json",
            "CLAUDE.md",
            ".claude/hooks/new-hook.sh",
            "AGENTS.md",
        ] {
            crate::write_if_changed(&paths, rel, &paths.root.join(rel), b"generated", |_| Ok(()))
                .unwrap();
        }
        let report = crate::clear(&paths).unwrap();
        assert!(report.failures.is_empty());
        assert!(paths.root.join("CLAUDE.md").exists());
        assert!(paths.root.join(".claude/settings.json").exists());
        assert!(!paths.root.join("AGENTS.md").exists());
        assert!(!paths.root.join(".claude/hooks").exists());
    }
    #[test]
    fn changed_generated_content_is_retained_across_repeated_clear() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        let target = paths.root.join(".codex/config.toml");
        crate::write_if_changed(&paths, ".codex/config.toml", &target, b"generated", |_| {
            Ok(())
        })
        .unwrap();
        std::fs::write(&target, "user modified").unwrap();
        for _ in 0..2 {
            let report = crate::clear(&paths).unwrap();
            assert!(report.failures.is_empty());
            assert!(report.items.iter().any(|i| i.path == ".codex/config.toml"
                && i.disposition == crate::clear_plan::Disposition::RetainUser));
            assert_eq!(std::fs::read_to_string(&target).unwrap(), "user modified");
        }
    }
    #[test]
    fn untrusted_inventory_cannot_escape_project() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        let mut value = Inventory::default();
        value.files.insert("../victim".into(), "hash".into());
        save(&paths, &value).unwrap();
        assert!(crate::clear_dry_run(&paths).is_err());
    }
}

pub(crate) fn lock(paths: &ProjectPaths) -> Result<crate::fs_lock::AdvisoryLock> {
    if crate::git::git_common_dir(&paths.root).is_ok() {
        crate::persistent::prepare(&paths.root)?;
    }
    let relative = ".macc/persistent/generated-project-files.lock";
    validate_path(paths, relative)?;
    crate::fs_lock::AdvisoryLock::acquire(
        &paths.root.join(relative),
        std::time::Duration::from_secs(5),
        "generated project file inventory",
    )
}

/// Shared clear/supervisor guard: ignored configuration is disposable only when
/// its MACC creation and unchanged bytes are proven. `.macc` evidence is archived
/// by the caller before removal.
pub fn unowned_ignored_files(worktree: &Path) -> Result<Vec<String>> {
    let paths = ProjectPaths::from_root(worktree);
    let inventory = read(&paths)?;
    let output = crate::git::run_git_output_mapped(
        worktree,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
        ],
        "inspect private worktree files",
    )?;
    if !output.status.success() {
        return Err(crate::persistent::error(
            "Cannot inspect ignored worktree files; slot retained",
        ));
    }
    let mut unowned = vec![];
    for name in output.stdout.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let name = String::from_utf8_lossy(name).into_owned();
        if name.starts_with(".macc/") {
            continue;
        }
        if inventory.protected.contains(&name)
            || !inventory.files.get(&name).is_some_and(|hash| {
                digest(&worktree.join(&name)).is_ok_and(|actual| &actual == hash)
            })
        {
            unowned.push(name);
        }
    }
    Ok(unowned)
}

#[cfg(test)]
mod clearing_exclusions_tests {
    use super::*;
    #[test]
    fn clear_releases_removed_file_rules_without_rewriting_user_rules() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        std::fs::write(paths.root.join(".gitignore"), "# user\n*.private\n").unwrap();
        crate::write_if_changed(
            &paths,
            "AGENTS.md",
            &paths.root.join("AGENTS.md"),
            b"generated",
            |_| Ok(()),
        )
        .unwrap();
        crate::ensure_gitignore_entries(&paths, crate::BASELINE_IGNORE_ENTRIES, None).unwrap();
        assert!(std::fs::read_to_string(paths.root.join(".gitignore"))
            .unwrap()
            .contains("/.macc/*"));
        crate::clear(&paths).unwrap();
        let after = std::fs::read_to_string(paths.root.join(".gitignore")).unwrap();
        assert!(after.starts_with("# user\n*.private\n"));
        assert!(!after.contains("/AGENTS.md"));
        assert!(after.contains("/.macc/*"));
    }
}

#[cfg(all(test, unix))]
mod linked_user_directory_tests {
    use super::*;
    #[test]
    fn project_generation_refuses_linked_user_directory_before_writing_external_file() {
        let tmp = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        std::fs::write(external.path().join("settings.json"), "user settings").unwrap();
        std::os::unix::fs::symlink(external.path(), paths.root.join("tool-settings")).unwrap();
        assert!(crate::write_if_changed(
            &paths,
            "tool-settings/settings.json",
            &paths.root.join("tool-settings/settings.json"),
            b"generated",
            |_| Ok(())
        )
        .is_err());
        assert_eq!(
            std::fs::read_to_string(external.path().join("settings.json")).unwrap(),
            "user settings"
        );
    }
}

#[cfg(test)]
mod reapplied_user_changes_tests {
    use super::*;
    #[test]
    fn later_generation_never_reclaims_user_content_preserved_by_a_merge() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = ProjectPaths::from_root(tmp.path());
        let target = paths.root.join("settings.json");
        crate::write_if_changed(&paths, "settings.json", &target, b"generated", |_| Ok(()))
            .unwrap();
        std::fs::write(&target, "generated + user permission").unwrap();
        crate::write_if_changed(
            &paths,
            "settings.json",
            &target,
            b"updated generation + user permission",
            |_| Ok(()),
        )
        .unwrap();
        assert!(read(&paths).unwrap().files.contains_key("settings.json"));
        assert!(read(&paths).unwrap().protected.contains("settings.json"));
        // Historical user ownership also wins when generation happens to
        // recreate the original bytes, which match the creation fingerprint.
        crate::write_if_changed(&paths, "settings.json", &target, b"generated", |_| Ok(()))
            .unwrap();
        crate::clear(&paths).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "generated");
        crate::write_if_changed(
            &paths,
            "settings.json",
            &target,
            b"updated generation + user permission",
            |_| Ok(()),
        )
        .unwrap();
        crate::clear(&paths).unwrap();
        assert_eq!(
            std::fs::read_to_string(target).unwrap(),
            "updated generation + user permission"
        );
    }
}

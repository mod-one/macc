//! One read-only disposition plan for all clients and the actual cleanup.
mod execution;
#[cfg(test)]
mod tests;
mod worktrees;
use crate::{ClearReport, ProjectPaths, Result};
pub use execution::execute;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    Preserve,
    Archive,
    Remove,
    RemoveWorktree,
    RetainWorktree,
    RetainUser,
    RemoveBranch,
    RetainBranch,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub path: String,
    pub disposition: Disposition,
    pub reason: String,
    pub bytes: u64,
    pub branch: Option<String>,
    pub owned: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Plan {
    pub items: Vec<Item>,
}
pub(super) fn size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| size(&e.path()))
                .sum()
        })
        .unwrap_or(0)
}
fn item(paths: &ProjectPaths, relative: &str, disposition: Disposition, reason: &str) -> Item {
    Item {
        path: relative.into(),
        disposition,
        reason: reason.into(),
        bytes: size(&paths.root.join(relative)),
        branch: None,
        owned: false,
    }
}
pub fn preview(paths: &ProjectPaths) -> Result<Plan> {
    crate::persistent::no_symlink(&paths.macc_dir)?;
    crate::persistent::no_symlink(&paths.macc_dir.join("state"))?;
    let mut items = worktrees::inspect(paths)?;
    let slots: BTreeSet<_> = items
        .iter()
        .filter(|i| {
            matches!(
                i.disposition,
                Disposition::RemoveWorktree | Disposition::RetainWorktree
            )
        })
        .map(|i| i.path.clone())
        .collect();
    if paths.macc_dir.is_dir() {
        for entry in std::fs::read_dir(&paths.macc_dir).map_err(crate::persistent::error)? {
            let entry = entry.map_err(crate::persistent::error)?;
            let rel = format!(".macc/{}", entry.file_name().to_string_lossy());
            if rel == ".macc/worktree"
                && !entry
                    .file_type()
                    .map_err(crate::persistent::error)?
                    .is_symlink()
            {
                for child in std::fs::read_dir(entry.path()).map_err(crate::persistent::error)? {
                    let child = child.map_err(crate::persistent::error)?;
                    let name = format!("{rel}/{}", child.file_name().to_string_lossy());
                    if !slots.contains(&name) {
                        items.push(item(paths, &name, Disposition::RetainWorktree, "Unregistered slot; retain until ownership/work disposition is established"));
                    }
                }
            } else if slots
                .iter()
                .any(|slot| slot == &rel || slot.starts_with(&format!("{rel}/")))
            {
                if !slots.contains(&rel) {
                    items.push(item(
                        paths,
                        &rel,
                        Disposition::RetainWorktree,
                        "Contains a registered worktree outside the MACC pool; data protected",
                    ));
                }
            } else if crate::clear_policy::retained(&rel) {
                items.push(item(
                    paths,
                    &rel,
                    Disposition::Preserve,
                    if matches!(
                        rel.as_str(),
                        ".macc/macc.yaml" | ".macc/catalog" | ".macc/skills.lock.json"
                    ) {
                        "Portable project setup (eligible for Git)"
                    } else {
                        "Local durable authority / O2 data (ignored by Git)"
                    },
                ));
            } else if matches!(
                rel.as_str(),
                ".macc/tmp"
                    | ".macc/code-intelligence"
                    | ".macc/context"
                    | ".macc/automation"
                    | ".macc/runner"
            ) {
                items.push(item(
                    paths,
                    &rel,
                    Disposition::Remove,
                    "Rebuildable MACC runtime",
                ));
            } else {
                items.push(item(paths, &rel, Disposition::Archive, "Evidence, recovery mapping, drafts or unclassified data; preserved in local clear history"));
            }
        }
    }
    // The JSON/SQLite authority marker must stay paired with the live O2 database.
    if paths.macc_dir.join("state").is_dir()
        && !std::fs::symlink_metadata(paths.macc_dir.join("state"))
            .map_err(crate::persistent::error)?
            .file_type()
            .is_symlink()
    {
        items.retain(|i| i.path != ".macc/state");
        for entry in
            std::fs::read_dir(paths.macc_dir.join("state")).map_err(crate::persistent::error)?
        {
            let entry = entry.map_err(crate::persistent::error)?;
            let rel = format!(".macc/state/{}", entry.file_name().to_string_lossy());
            let keep = crate::clear_policy::retained(&rel);
            items.push(item(
                paths,
                &rel,
                if keep {
                    Disposition::Preserve
                } else {
                    Disposition::Archive
                },
                if keep {
                    "Local O2 authority marker; ignored by Git"
                } else {
                    "Coordinator recovery state, archived before reset"
                },
            ));
        }
    }
    let inventory = crate::project_ownership::read(paths)?;
    for (rel, hash) in &inventory.files {
        crate::project_ownership::validate_path(paths, rel)?;
        if paths.root.join(rel).exists() {
            let unchanged = !inventory.protected.contains(rel)
                && crate::project_ownership::digest(&paths.root.join(rel))? == *hash;
            items.push(item(
                paths,
                rel,
                if unchanged {
                    Disposition::Remove
                } else {
                    Disposition::RetainUser
                },
                if unchanged {
                    "Unmodified MACC-created project file"
                } else {
                    "MACC-created file modified since generation; user work protected"
                },
            ));
        }
    }
    for rel in &inventory.protected {
        if !inventory.files.contains_key(rel) && paths.root.join(rel).exists() {
            items.push(item(
                paths,
                rel,
                Disposition::RetainUser,
                "Protected preexisting or unverifiable user data",
            ));
        }
    }
    // Old manifests prove creation, but cannot prove that the content stayed unchanged.
    for rel in crate::load_managed_paths(paths)? {
        if rel == ".gitignore"
            || rel == ".macc"
            || rel.starts_with(".macc/")
            || inventory.files.contains_key(&rel)
            || !paths.root.join(&rel).exists()
        {
            continue;
        }
        items.push(item(
            paths,
            &rel,
            Disposition::RetainUser,
            "Older ownership record has no content fingerprint; preserve rather than infer",
        ));
    }
    items.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Plan { items })
}
impl Plan {
    pub fn report(&self) -> ClearReport {
        ClearReport {
            archived: self
                .items
                .iter()
                .filter(|i| i.disposition == Disposition::Archive)
                .count(),
            removed_worktrees: self
                .items
                .iter()
                .filter(|i| i.disposition == Disposition::RemoveWorktree)
                .count(),
            removed_branches: self
                .items
                .iter()
                .filter(|i| i.disposition == Disposition::RemoveBranch)
                .count(),
            removed: self
                .items
                .iter()
                .filter(|i| i.disposition == Disposition::Remove)
                .count(),
            skipped: self
                .items
                .iter()
                .filter(|i| {
                    matches!(
                        i.disposition,
                        Disposition::Preserve
                            | Disposition::RetainWorktree
                            | Disposition::RetainUser
                            | Disposition::RetainBranch
                    )
                })
                .count(),
            items: self.items.clone(),
            ..Default::default()
        }
    }
}

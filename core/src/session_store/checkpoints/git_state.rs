//! Read-only repository evidence. No checkout, staging, commit or cleanup.
use super::super::{digest, error};
use crate::Result;
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, process::Command};
#[derive(Clone)]
pub(super) struct GitState {
    pub revision: String,
    pub branch: String,
    pub digest: String,
    pub content_digest: String,
    pub changed: Vec<String>,
}
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| error(e.to_string()))?;
    if !out.status.success() {
        return Err(error(format!(
            "Checkpoint Git evidence unavailable: {}",
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(out.stdout)
}
pub(super) fn text(root: &Path, args: &[&str]) -> Result<String> {
    String::from_utf8(git(root, args)?)
        .map(|s| s.trim().to_string())
        .map_err(|e| error(e.to_string()))
}
fn names(bytes: &[u8]) -> Result<Vec<String>> {
    bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| {
            String::from_utf8(p.to_vec())
                .map_err(|_| error("Non-UTF8 filename requires checkpoint reconciliation"))
        })
        .collect()
}
pub(super) fn snapshot(root: &Path) -> Result<GitState> {
    snapshot_for(root, None)
}
pub(super) fn snapshot_for(
    root: &Path,
    repair: Option<&super::merge_repair::MergeRepair>,
) -> Result<GitState> {
    if repair.is_none() && !git(root, &["ls-files", "--unmerged", "-z"])?.is_empty() {
        return Err(error(
            "Unresolved Git index entries require reconciliation before checkpoint restore",
        ));
    }
    for operation in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
    ] {
        let location = git(
            root,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                operation,
            ],
        )?;
        if !(repair.is_some() && operation == "MERGE_HEAD")
            && Path::new(String::from_utf8_lossy(&location).trim()).exists()
        {
            return Err(error(format!(
                "Incomplete Git operation {operation}; retained work was preserved"
            )));
        }
    }
    let revision = String::from_utf8_lossy(&git(root, &["rev-parse", "HEAD"])?)
        .trim()
        .to_string();
    let branch = if let Some(repair) = repair {
        format!("merge:{}:{}", repair.base_revision, repair.feature_revision)
    } else {
        text(root, &["symbolic-ref", "--short", "HEAD"])?
    };
    // Index identity captures partially staged changes independently of file bytes.
    let index = git(root, &["ls-files", "--stage", "-z"])?;
    let mut evidence = Vec::new();
    let mut files = names(&git(
        root,
        &["ls-files", "-co", "--exclude-standard", "-z"],
    )?)?;
    files.sort();
    files.dedup();
    if files.len() > 100_000 {
        return Err(error("Checkpoint file inventory exceeds safety bound"));
    }
    for name in files {
        // Runtime state is never part of the implementation digest.
        if name == ".macc" || name.starts_with(".macc/") || name == "worktree.prd.json" {
            continue;
        }
        let path = root.join(&name);
        evidence.extend_from_slice(name.as_bytes());
        evidence.push(0);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    evidence.extend_from_slice(&meta.permissions().mode().to_le_bytes());
                }
                if meta.is_symlink() {
                    evidence.extend_from_slice(
                        std::fs::read_link(path)
                            .map_err(|e| error(e.to_string()))?
                            .to_string_lossy()
                            .as_bytes(),
                    );
                } else if meta.is_file() {
                    if meta.len() > 64 * 1024 * 1024 {
                        return Err(error("Checkpoint file exceeds 64 MiB hashing bound"));
                    }
                    evidence.extend_from_slice(
                        digest(&std::fs::read(path).map_err(|e| error(e.to_string()))?).as_bytes(),
                    );
                } else {
                    return Err(error(
                        "Checkpoint cannot verify submodules or special files",
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                evidence.extend_from_slice(b"deleted")
            }
            Err(e) => return Err(error(e.to_string())),
        }
        evidence.push(0);
    }
    let mut changed = names(&git(root, &["diff", "--name-only", "HEAD", "-z"])?)?;
    changed.extend(names(&git(
        root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?)?);
    changed.retain(|p| !p.starts_with(".macc/") && p != "worktree.prd.json");
    changed.sort();
    changed.dedup();
    Ok(GitState {
        revision,
        branch,
        content_digest: digest(&evidence),
        digest: digest(&[index.as_slice(), evidence.as_slice()].concat()),
        changed,
    })
}
pub(super) fn task_sources(root: &Path, task: &str) -> Result<(Value, BTreeMap<String, String>)> {
    let path = root.join("worktree.prd.json");
    let raw = std::fs::read(&path)
        .map_err(|e| error(format!("Mandatory worktree.prd.json unavailable: {e}")))?;
    if raw.len() > 1024 * 1024 {
        return Err(error("Task PRD exceeds checkpoint input bound"));
    }
    let prd: Value = serde_json::from_slice(&raw).map_err(|e| error(e.to_string()))?;
    task_sources_from(root, task, prd)
}

/// Normalize and hash the projected contract without writing the worker.
pub(super) fn task_sources_from(
    root: &Path,
    task: &str,
    mut prd: Value,
) -> Result<(Value, BTreeMap<String, String>)> {
    let mut selected = prd["tasks"]
        .as_array()
        .and_then(|a| a.iter().find(|t| t["id"] == task))
        .cloned()
        .ok_or_else(|| error("Checkpoint task is absent from the worktree PRD"))?;
    if let Some(object) = selected.as_object_mut() {
        for key in [
            "state",
            "task_runtime",
            "task_worktree",
            "task_tool",
            "coordinator",
            "passes",
            "updated_at",
            "state_changed_at",
            "review",
        ] {
            object.remove(key);
        }
    }
    prd["tasks"] = serde_json::json!([selected]);
    let mut hashes = BTreeMap::new();
    hashes.insert(
        "worktree.prd.json".into(),
        digest(prd.to_string().as_bytes()),
    );
    // Only explicit source paths are opened; never follow arbitrary PRD prose.
    for key in ["source_refs", "spec_files", "references"] {
        if let Some(refs) = prd["tasks"][0][key].as_array() {
            for reference in refs.iter().filter_map(Value::as_str) {
                let rel = reference
                    .strip_prefix("spec:")
                    .unwrap_or(reference)
                    .split('#')
                    .next()
                    .unwrap_or("");
                if rel.contains("://") || rel.starts_with("prd:") || rel.starts_with("log:") {
                    continue;
                }
                let source = root
                    .join(rel)
                    .canonicalize()
                    .map_err(|e| error(format!("Checkpoint source {rel}: {e}")))?;
                if !source.starts_with(root.canonicalize().map_err(|e| error(e.to_string()))?) {
                    return Err(error("Checkpoint source escapes the execution root"));
                }
                let bytes = std::fs::read(source).map_err(|e| error(e.to_string()))?;
                hashes.insert(rel.into(), digest(&bytes));
            }
        }
    }
    Ok((prd, hashes))
}

pub(super) fn verify_repository(root: &Path, worktree: &Path) -> Result<()> {
    fn common(path: &Path) -> Result<std::path::PathBuf> {
        let out = git(
            path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        Path::new(String::from_utf8_lossy(&out).trim())
            .canonicalize()
            .map_err(|e| error(e.to_string()))
    }
    if common(root)? != common(worktree)? {
        return Err(error("Checkpoint worktree belongs to another Git project"));
    }
    if let Some(metadata) = crate::read_worktree_metadata(worktree)? {
        let branch = String::from_utf8_lossy(&git(worktree, &["symbolic-ref", "--short", "HEAD"])?)
            .trim()
            .to_string();
        if metadata.branch != branch {
            return Err(error(
                "Retained worktree branch differs from its MACC identity",
            ));
        }
    }
    Ok(())
}
pub(super) fn descends_from(root: &Path, revision: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["merge-base", "--is-ancestor", revision, "HEAD"])
        .status()
        .is_ok_and(|s| s.success())
}

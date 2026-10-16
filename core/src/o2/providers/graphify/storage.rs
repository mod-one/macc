//! File-level ownership proofs. Never remove added or independently changed files.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct FileProof {
    hash: Option<String>,
    link: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    schema: u32,
    files: BTreeMap<String, FileProof>,
}
const MARKER: &str = ".macc-generation.json";
fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, FileProof>) -> Result<()> {
    for item in std::fs::read_dir(dir).map_err(|e| error(e.to_string()))? {
        let item = item.map_err(|e| error(e.to_string()))?;
        let path = item.path();
        if path == base.join(MARKER) {
            continue;
        }
        let meta = std::fs::symlink_metadata(&path).map_err(|e| error(e.to_string()))?;
        let key = path
            .strip_prefix(base)
            .map_err(|e| error(e.to_string()))?
            .to_str()
            .ok_or_else(|| error("Non-UTF8 provider path"))?
            .to_string();
        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&path).map_err(|e| error(e.to_string()))?;
            // Venv interpreter links are expected; never traverse them during ownership scans.
            out.insert(
                key,
                FileProof {
                    hash: None,
                    link: Some(target.to_string_lossy().into_owned()),
                },
            );
        } else if meta.is_dir() {
            out.insert(
                key,
                FileProof {
                    hash: None,
                    link: None,
                },
            );
            walk(base, &path, out)?;
        } else if meta.is_file() {
            if meta.len() > 268435456 {
                return Err(error("Provider file exceeds ownership scan bound"));
            }
            out.insert(
                key,
                FileProof {
                    hash: Some(digest(&store::bytes(&path)?)),
                    link: None,
                },
            );
        } else {
            return Err(error("Provider special file rejected"));
        }
        if out.len() > 50000 {
            return Err(error("Provider file-count bound exceeded"));
        }
    }
    Ok(())
}
pub(super) fn seal(root: &Path, generation: &str) -> Result<String> {
    let base = owned(root, generation)?;
    let mut files = BTreeMap::new();
    walk(&base, &base, &mut files)?;
    let raw =
        serde_json::to_vec(&Manifest { schema: 1, files }).map_err(|e| error(e.to_string()))?;
    store::write(root, &base.join(MARKER), &raw)?;
    Ok(digest(&raw))
}
pub(super) fn verify(root: &Path, generation: &str, expected: &str) -> Result<()> {
    let base = owned(root, generation)?;
    let raw = store::bytes(&base.join(MARKER))?;
    if digest(&raw) != expected {
        return Err(error("Provider generation manifest changed"));
    }
    let manifest: Manifest = store::json(&raw)?;
    if manifest.schema != 1 {
        return Err(error("Unsupported provider generation schema"));
    }
    let mut current = BTreeMap::new();
    walk(&base, &base, &mut current)?;
    if current != manifest.files {
        return Err(error(
            "Provider generation missing/changed/unowned files; reinstall explicitly",
        ));
    }
    Ok(())
}
pub(super) fn remove(root: &Path, generation: &str, expected: &str) -> Result<bool> {
    let base = owned(root, generation)?;
    if !base.exists() {
        return Ok(false);
    }
    // A crash may occur after removing the manifest but before its empty root.
    // Only an empty, already receipted root is recoverable without its proof.
    if !base.join(MARKER).exists()
        && std::fs::read_dir(&base)
            .map_err(|e| error(e.to_string()))?
            .next()
            .is_none()
    {
        std::fs::remove_dir(&base).map_err(|e| error(e.to_string()))?;
        return Ok(false);
    }
    let raw = store::bytes(&base.join(MARKER))?;
    if digest(&raw) != expected {
        return Err(error("Uninstall manifest changed; preserving generation"));
    }
    let manifest: Manifest = store::json(&raw)?;
    let mut current = BTreeMap::new();
    walk(&base, &base, &mut current)?;
    for (path, proof) in &manifest.files {
        if current.get(path) == Some(proof) && (proof.hash.is_some() || proof.link.is_some()) {
            let file = base.join(path);
            std::fs::remove_file(&file).map_err(|e| error(e.to_string()))?;
        }
    }
    // Remove only directories present in the sealed ownership manifest.
    // New empty user directories must survive just like new files.
    for (path, proof) in manifest.files.iter().rev() {
        if proof.hash.is_none() && proof.link.is_none() && current.get(path) == Some(proof) {
            let _ = std::fs::remove_dir(base.join(path));
        }
    }
    let mut remaining = BTreeMap::new();
    walk(&base, &base, &mut remaining)?;
    if remaining.is_empty() {
        std::fs::remove_file(base.join(MARKER)).map_err(|e| error(e.to_string()))?;
        let _ = std::fs::remove_dir(&base);
    }
    Ok(base.exists())
}
pub(super) fn owned(root: &Path, path: &str) -> Result<std::path::PathBuf> {
    super::super::lifecycle::owned_path(root, "graphify", path)
}
pub(super) fn attempt(root: &Path, plan: &Plan) -> Result<String> {
    let prefix = plan
        .affected_paths
        .first()
        .ok_or_else(|| error("Missing reviewed generation path"))?;
    let relative = format!("{prefix}/attempt-{}", uuid::Uuid::new_v4());
    let path = owned(root, &relative)?;
    std::fs::create_dir_all(&path).map_err(|e| error(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| error(e.to_string()))?;
    }
    Ok(relative)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clean_removal_prunes_empty_dirs_and_preserves_changed_files() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        let generation = ".macc/code-intelligence/graphify/generations/test";
        let base = owned(root, generation).unwrap();
        std::fs::create_dir_all(base.join("empty/nested")).unwrap();
        std::fs::write(base.join("owned"), "original").unwrap();
        let hash = seal(root, generation).unwrap();
        verify(root, generation, &hash).unwrap();
        assert!(!remove(root, generation, &hash).unwrap());
        assert!(!base.exists());
        std::fs::create_dir_all(&base).unwrap();
        assert!(!remove(root, generation, &hash).unwrap());
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("unreceipted"), "preserve").unwrap();
        assert!(remove(root, generation, &hash).is_err());
        std::fs::remove_file(base.join("unreceipted")).unwrap();
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("owned"), "original").unwrap();
        let hash = seal(root, generation).unwrap();
        std::fs::write(base.join("owned"), "changed by user").unwrap();
        std::fs::create_dir_all(base.join("user-empty-directory")).unwrap();
        assert!(verify(root, generation, &hash).is_err());
        assert!(remove(root, generation, &hash).unwrap());
        assert!(base.join("user-empty-directory").is_dir());
        assert_eq!(
            std::fs::read_to_string(base.join("owned")).unwrap(),
            "changed by user"
        );
    }
    #[cfg(unix)]
    #[test]
    fn dangling_parent_symlinks_are_rejected() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join(".macc")).unwrap();
        std::os::unix::fs::symlink("/not-present", t.path().join(".macc/code-intelligence"))
            .unwrap();
        assert!(owned(t.path(), ".macc/code-intelligence/graphify/new").is_err());
    }
}

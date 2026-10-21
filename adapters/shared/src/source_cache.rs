//! Publish verified source snapshots once; readers never checkout into a published tree.
mod http;
mod migration;
mod references;
pub fn materialize_http(
    paths: &ProjectPaths,
    source: &Source,
    quiet: bool,
    offline: bool,
    refresh: bool,
) -> Result<(PathBuf, SourceProvenance)> {
    http::materialize(paths, source, quiet, offline, refresh)
}
use crate::fetch;
use macc_core::{
    catalog::{Source, SourceKind},
    fs_lock::AdvisoryLock,
    resolve::SourceProvenance,
    MaccError, ProjectPaths, Result,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
#[cfg(test)]
mod tests;

#[derive(Serialize, Deserialize)]
struct Receipt {
    source: Source,
    tree_sha256: String,
}
fn io(path: &Path, action: &str, source: std::io::Error) -> MaccError {
    MaccError::Io {
        path: path.display().to_string(),
        action: action.into(),
        source,
    }
}
fn bases(paths: &ProjectPaths) -> Vec<PathBuf> {
    let mut out = vec![paths.cache_dir.clone()];
    if let Ok(root) = macc_core::git::primary_worktree_path(&paths.root) {
        let project = macc_core::ProjectPaths::from_root(&root).cache_dir;
        if !out.contains(&project) {
            out.push(project);
        }
    }
    if let Some(user) = paths.user_cache_dir() {
        out.push(user);
    }
    out
}
fn writable(paths: &ProjectPaths) -> Result<PathBuf> {
    for base in paths
        .user_cache_dir()
        .into_iter()
        .chain(std::iter::once(paths.cache_dir.clone()))
    {
        if fs::create_dir_all(base.join("sources-v2")).is_ok()
            && tempfile::Builder::new()
                .prefix("write-probe-")
                .tempfile_in(&base)
                .is_ok()
        {
            return Ok(base);
        }
    }
    Err(MaccError::Validation(
        "No writable MACC source cache".into(),
    ))
}
fn digest(root: &Path) -> Result<String> {
    fn walk(root: &Path, rel: &Path, hash: &mut Sha256) -> Result<()> {
        let dir = root.join(rel);
        let mut entries = fs::read_dir(&dir)
            .map_err(|e| io(&dir, "read cache tree", e))?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|e| io(&dir, "read cache entry", e))?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let relative = rel.join(entry.file_name());
            let kind = entry
                .file_type()
                .map_err(|e| io(&path, "inspect cache file", e))?;
            hash.update(relative.to_string_lossy().as_bytes());
            hash.update([0]);
            if kind.is_dir() {
                hash.update(b"dir");
                walk(root, &relative, hash)?;
            } else if kind.is_file() {
                let bytes = fs::read(&path).map_err(|e| io(&path, "hash cache file", e))?;
                hash.update(b"file");
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = entry
                        .metadata()
                        .map_err(|e| io(&path, "hash executable mode", e))?
                        .permissions()
                        .mode()
                        & 0o111;
                    hash.update(mode.to_le_bytes());
                }
                hash.update((bytes.len() as u64).to_le_bytes());
                hash.update(bytes);
            } else if kind.is_symlink() {
                hash.update(b"link");
                hash.update(
                    fs::read_link(&path)
                        .map_err(|e| io(&path, "hash cache link", e))?
                        .to_string_lossy()
                        .as_bytes(),
                );
            } else {
                return Err(MaccError::Validation(format!(
                    "Unsupported cache entry {}",
                    path.display()
                )));
            }
            hash.update([0]);
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    walk(root, Path::new(""), &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}
fn freeze(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root).map_err(|e| io(root, "freeze snapshot", e))? {
        let entry = entry.map_err(|e| io(root, "read snapshot file", e))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|e| io(&path, "inspect snapshot permissions", e))?;
        if kind.is_dir() {
            freeze(&path)?;
        } else if kind.is_file() {
            let mut permissions = entry
                .metadata()
                .map_err(|e| io(&path, "read snapshot permissions", e))?
                .permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&path, permissions)
                .map_err(|e| io(&path, "make snapshot read only", e))?;
        }
    }
    Ok(())
}
fn verify(root: &Path, source: &Source) -> Result<SourceProvenance> {
    let file = root.join("receipt.json");
    let receipt: Receipt = serde_json::from_slice(
        &fs::read(&file).map_err(|e| io(&file, "read snapshot receipt", e))?,
    )
    .map_err(|e| MaccError::Validation(format!("Invalid snapshot {}: {e}", file.display())))?;
    if receipt.source.url != source.url
        || receipt.source.kind != source.kind
        || (source.kind == SourceKind::Http
            && source.checksum.is_some()
            && source.checksum != receipt.source.checksum)
        || (macc_core::skills_catalog::locking::is_sha(&source.reference)
            && receipt.source.reference != source.reference)
        || digest(&root.join("tree"))? != receipt.tree_sha256
    {
        return Err(MaccError::Validation(format!("Source snapshot integrity check failed: {}; retain it for inspection and rebuild the affected cache entry",root.display())));
    }
    Ok(SourceProvenance {
        requested: source.clone(),
        resolved: receipt.source,
        cache_key: format!("sources-v2/{}", root.file_name().unwrap().to_string_lossy()),
        tracking_ref: None,
    })
}
fn find(paths: &ProjectPaths, source: &Source) -> Result<Option<(PathBuf, SourceProvenance)>> {
    for base in bases(paths) {
        let exact = base.join("sources-v2").join(source.cache_key());
        if exact.is_dir() {
            let provenance = verify(&exact, source)?;
            return Ok(Some((exact.join("tree"), provenance)));
        }
        // A previously materialized superset can satisfy a narrower set of paths offline.
        if !macc_core::skills_catalog::locking::is_sha(&source.reference) {
            continue;
        }
        let Ok(entries) = fs::read_dir(base.join("sources-v2")) else {
            continue;
        };
        for entry in entries.flatten().filter(|e| e.path().is_dir()) {
            let root = entry.path();
            let receipt = fs::read(root.join("receipt.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<Receipt>(&b).ok());
            if receipt.as_ref().is_some_and(|r| {
                r.source.url == source.url
                    && r.source.kind == source.kind
                    && r.source.reference == source.reference
            }) && !source.subpaths.is_empty()
                && source
                    .subpaths
                    .iter()
                    .all(|p| root.join("tree").join(p).exists())
            {
                let provenance = verify(&root, source)?;
                return Ok(Some((root.join("tree"), provenance)));
            }
        }
    }
    Ok(None)
}
fn cached(paths: &ProjectPaths, source: &Source) -> Result<Option<(PathBuf, SourceProvenance)>> {
    if let Some(found) = find(paths, source)? {
        return Ok(Some(found));
    }
    for candidate in bases(paths).into_iter().map(|base| {
        base.join("source-aliases")
            .join(format!("{}.json", source.cache_key()))
    }) {
        if let Some(resolved) = fs::read(candidate)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Source>(&bytes).ok())
        {
            if resolved.url == source.url
                && resolved.kind == source.kind
                && (macc_core::skills_catalog::locking::is_sha(&resolved.reference)
                    || resolved.checksum.is_some())
            {
                if let Some((root, mut provenance)) = find(paths, &resolved)? {
                    provenance.requested = source.clone();
                    return Ok(Some((root, provenance)));
                }
            }
        }
    }
    Ok(None)
}
fn publish_alias(base: &Path, source: &Source, resolved: &Source) -> Result<()> {
    let alias = base
        .join("source-aliases")
        .join(format!("{}.json", source.cache_key()));
    fs::create_dir_all(alias.parent().unwrap())
        .map_err(|e| io(&alias, "create alias directory", e))?;
    let temporary = alias.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(
        &temporary,
        serde_json::to_vec(resolved).map_err(|e| MaccError::Validation(e.to_string()))?,
    )
    .map_err(|e| io(&temporary, "write source alias", e))?;
    fs::rename(&temporary, &alias).map_err(|e| io(&alias, "publish source alias", e))
}
fn git(dir: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(dir)
        .output()
        .map_err(|e| io(dir, "run source cache git command", e))?;
    if !output.status.success() {
        return Err(MaccError::Validation(format!(
            "Git source preparation failed ({:?}, {}): {}",
            args.first(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}
/// `refresh` is reserved for an explicit update command. Normal Apply uses cached aliases/SHAs.
pub fn materialize_git(
    paths: &ProjectPaths,
    source: &Source,
    quiet: bool,
    offline: bool,
    refresh: bool,
) -> Result<(PathBuf, SourceProvenance)> {
    if !refresh || macc_core::skills_catalog::locking::is_sha(&source.reference) {
        if let Some(found) = cached(paths, source)? {
            return Ok(found);
        }
    }
    let base = writable(paths)?;
    materialize_at(paths, source, quiet, offline, refresh, &base)
}
fn materialize_at(
    paths: &ProjectPaths,
    source: &Source,
    quiet: bool,
    offline: bool,
    refresh: bool,
    base: &Path,
) -> Result<(PathBuf, SourceProvenance)> {
    let refresh = refresh && !macc_core::skills_catalog::locking::is_sha(&source.reference);
    if source
        .subpaths
        .iter()
        .any(|path| !macc_core::skills_catalog::locking::relative(path))
    {
        return Err(MaccError::Validation("Unsafe source subpath".into()));
    }
    if source.kind != SourceKind::Git {
        return Err(MaccError::Validation("Expected Git source".into()));
    }
    if !refresh {
        if let Some(found) = find(paths, source)? {
            return Ok(found);
        }
    }
    let _alias_lock = AdvisoryLock::acquire(
        &base
            .join("source-locks")
            .join(format!("{}.lock", source.cache_key())),
        Duration::from_secs(60),
        "source resolution",
    )?;
    if !refresh {
        for candidate in bases(paths).into_iter().map(|b| {
            b.join("source-aliases")
                .join(format!("{}.json", source.cache_key()))
        }) {
            if let Ok(bytes) = fs::read(candidate) {
                if let Ok(resolved) = serde_json::from_slice::<Source>(&bytes) {
                    if resolved.url == source.url
                        && resolved.kind == source.kind
                        && macc_core::skills_catalog::locking::is_sha(&resolved.reference)
                    {
                        if let Some((root, mut provenance)) = find(paths, &resolved)? {
                            provenance.requested = source.clone();
                            return Ok((root, provenance));
                        }
                    }
                }
            }
        }
        if let Some(found) = find(paths, source)? {
            return Ok(found);
        }
    }
    let legacy = if refresh {
        None
    } else {
        migration::legacy_git(paths, source)
    };
    if offline && legacy.is_none() {
        return Err(MaccError::Validation(format!(
            "Offline: no verified snapshot for {} at {}",
            source.url, source.reference
        )));
    }
    let update_sha = if refresh {
        Some(references::resolve(source)?)
    } else {
        None
    };
    if let Some(sha) = &update_sha {
        let mut exact = source.clone();
        exact.reference = sha.clone();
        if let Some((root, mut provenance)) = find(paths, &exact)? {
            provenance.requested = source.clone();
            publish_alias(base, source, &provenance.resolved)?;
            return Ok((root, provenance));
        }
    }
    if !quiet {
        println!(
            "Preparing immutable source {} @ {}",
            source.url, source.reference
        );
    }
    let staging = tempfile::Builder::new()
        .prefix("source-")
        .tempdir_in(base)
        .map_err(|e| io(base, "create snapshot staging", e))?;
    let clone_source = legacy
        .as_ref()
        .map(|(path, _)| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.url.clone());
    git(
        staging.path(),
        &[
            "clone",
            "--no-checkout",
            "--filter=blob:none",
            "--",
            &clone_source,
            "tree",
        ],
    )?;
    let tree = staging.path().join("tree");
    if !source.subpaths.is_empty()
        && source
            .subpaths
            .iter()
            .all(|path| !path.is_empty() && path != ".")
    {
        fetch::enable_sparse_checkout(&tree)?;
        fetch::set_sparse_paths(&tree, &source.subpaths)?;
    }
    let sha = if let Some(sha) = update_sha {
        sha
    } else if let Some((_, sha)) = legacy {
        sha
    } else {
        fetch::resolve_ref_to_sha(
            &tree,
            if source.reference.is_empty() {
                "HEAD"
            } else {
                &source.reference
            },
        )?
    };
    git(&tree, &["checkout", "--detach", &sha])?;
    fs::remove_dir_all(tree.join(".git"))
        .map_err(|e| io(&tree, "remove mutable Git metadata", e))?;
    let mut resolved = source.clone();
    resolved.reference = sha;
    let target = base.join("sources-v2").join(resolved.cache_key());
    let receipt = Receipt {
        source: resolved.clone(),
        tree_sha256: digest(&tree)?,
    };
    fs::write(
        staging.path().join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).map_err(|e| MaccError::Validation(e.to_string()))?,
    )
    .map_err(|e| io(staging.path(), "write snapshot receipt", e))?;
    let _publication = AdvisoryLock::acquire(
        &base
            .join("source-locks")
            .join(format!("publish-{}.lock", resolved.cache_key())),
        Duration::from_secs(60),
        "snapshot publication",
    )?;
    if !target.exists() {
        freeze(&tree)?;
        fs::rename(staging.path(), &target)
            .map_err(|e| io(&target, "publish snapshot atomically", e))?;
    }
    let mut provenance = verify(&target, &resolved)?;
    provenance.requested = source.clone();
    publish_alias(base, source, &resolved)?;
    Ok((target.join("tree"), provenance))
}

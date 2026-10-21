//! Content-addressed archive snapshots share the same publication contract as Git snapshots.
use super::*;
pub(super) fn materialize(
    paths: &ProjectPaths,
    source: &Source,
    quiet: bool,
    offline: bool,
    refresh: bool,
) -> Result<(PathBuf, SourceProvenance)> {
    if source
        .subpaths
        .iter()
        .any(|path| !macc_core::skills_catalog::locking::relative(path))
    {
        return Err(MaccError::Validation("Unsafe archive subpath".into()));
    }
    if !refresh {
        if let Some(found) = cached(paths, source)? {
            return Ok(found);
        }
    }
    let base = writable(paths)?;
    let _lock = AdvisoryLock::acquire(
        &base
            .join("source-locks")
            .join(format!("http-{}.lock", source.cache_key())),
        Duration::from_secs(60),
        "archive preparation",
    )?;
    let alias = base
        .join("source-aliases")
        .join(format!("{}.json", source.cache_key()));
    if !refresh {
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
                    && resolved.checksum.is_some()
                {
                    if let Some((tree, mut provenance)) = find(paths, &resolved)? {
                        provenance.requested = source.clone();
                        return Ok((tree, provenance));
                    }
                }
            }
        }
    }
    let mut requested = source.clone();
    if refresh {
        requested.checksum = None;
    }
    let archive = fetch::download_source_raw_refresh(paths, &requested, quiet, offline, refresh)?;
    let mut resolved = requested.clone();
    resolved.checksum = Some(macc_core::skills_catalog::file_digest(&archive)?);
    if let Some((tree, mut provenance)) = find(paths, &resolved)? {
        provenance.requested = source.clone();
        return Ok((tree, provenance));
    }
    let staging = tempfile::Builder::new()
        .prefix("archive-")
        .tempdir_in(&base)
        .map_err(|e| io(&base, "create archive staging", e))?;
    let tree = staging.path().join("tree");
    fetch::unpack_archive(&archive, &tree)?;
    let receipt = Receipt {
        source: resolved.clone(),
        tree_sha256: digest(&tree)?,
    };
    fs::write(
        staging.path().join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).map_err(|e| MaccError::Validation(e.to_string()))?,
    )
    .map_err(|e| io(staging.path(), "write archive snapshot receipt", e))?;
    let target = base.join("sources-v2").join(resolved.cache_key());
    let _publish = AdvisoryLock::acquire(
        &base
            .join("source-locks")
            .join(format!("publish-{}.lock", resolved.cache_key())),
        Duration::from_secs(60),
        "archive publication",
    )?;
    if !target.exists() {
        freeze(&tree)?;
        fs::rename(staging.path(), &target)
            .map_err(|e| io(&target, "publish archive snapshot", e))?;
    }
    fs::create_dir_all(alias.parent().unwrap())
        .map_err(|e| io(&alias, "create archive alias directory", e))?;
    let temporary = alias.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(
        &temporary,
        serde_json::to_vec(&resolved).map_err(|e| MaccError::Validation(e.to_string()))?,
    )
    .map_err(|e| io(&temporary, "write archive alias", e))?;
    fs::rename(&temporary, &alias).map_err(|e| io(&alias, "publish archive alias", e))?;
    let mut provenance = verify(&target, &resolved)?;
    provenance.requested = source.clone();
    Ok((target.join("tree"), provenance))
}

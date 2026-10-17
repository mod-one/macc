//! Incremental by file hash: unchanged declarations are reused, deletions are pruned.
use super::*;
pub(in crate::o2::providers) fn load(
    root: &Path,
    receipt: Option<&Receipt>,
    p: &crate::o2::policy::CodePolicy,
) -> Result<Option<Map>> {
    if !p.native_incremental {
        return Ok(None);
    }
    let Some(r) = receipt else { return Ok(None) };
    let (Some(path), Some(expected)) = (&r.index, &r.index_sha256) else {
        return Ok(None);
    };
    let path = lifecycle::owned_path(root, "native", path)?;
    if std::fs::metadata(&path)
        .map_err(|e| error(e.to_string()))?
        .len()
        > p.native_index_max_bytes
    {
        return Err(error("Native index exceeds configured artifact bound"));
    }
    use std::io::Read;
    let mut raw = Vec::new();
    std::fs::File::open(&path)
        .map_err(|e| error(e.to_string()))?
        .take(p.native_index_max_bytes + 1)
        .read_to_end(&mut raw)
        .map_err(|e| error(e.to_string()))?;
    if raw.len() as u64 > p.native_index_max_bytes {
        return Err(error("Native index grew beyond artifact bound"));
    }
    if digest(&raw) != *expected {
        return Err(error("Native index checksum mismatch"));
    }
    let map: Map = serde_json::from_slice(&raw)
        .map_err(|_| error("Legacy/incompatible native index; rebuild from current sources"))?;
    if map.recipe != RECIPE
        || map.settings != settings(p)?
        || map.source.project_root
            != root
                .canonicalize()
                .map_err(|e| error(e.to_string()))?
                .display()
                .to_string()
    {
        return Ok(None);
    }
    Ok(Some(map))
}
pub(in crate::o2::providers) fn build(
    root: &Path,
    p: &crate::o2::policy::CodePolicy,
    previous: Option<&Map>,
) -> Result<Map> {
    let started = std::time::Instant::now();
    let source = source_snapshot(root, p)?;
    let mut stats = Stats::default();
    if let Some(old) = previous {
        stats.removed_files = old
            .entries
            .keys()
            .filter(|name| !source.files.contains_key(*name))
            .count();
    }
    let mut entries = BTreeMap::new();
    let mut truncated = source.truncated;
    let mut output_bytes = 0u64;
    for (path, hash) in &source.files {
        if started.elapsed().as_millis() >= p.native_build_timeout_ms as u128 {
            truncated = true;
            break;
        }
        let Some((language, grammar)) = parse::language(path, p) else {
            stats.unsupported_files += 1;
            continue;
        };
        let (_, data) = paths::source(root, path, p.max_file_bytes)?;
        if digest(&data) != *hash {
            return Err(error("Source changed while building native map"));
        }
        stats.corpus_bytes += data.len() as u64;
        if stats.corpus_bytes > p.native_corpus_max_bytes {
            truncated = true;
            break;
        }
        let reused = previous
            .and_then(|old| old.entries.get(path))
            .filter(|file| {
                file.sha256 == *hash && file.language == language && file.diagnostic.is_none()
            });
        let file = if let Some(file) = reused {
            stats.reused_files += 1;
            file.clone()
        } else {
            stats.parsed_files += 1;
            let source = std::str::from_utf8(&data).map_err(|e| error(e.to_string()))?;
            let (symbols, diagnostic) = match parse::extract(
                grammar,
                source,
                p.native_symbols_per_file as usize,
                p.native_parse_timeout_ms,
            ) {
                Ok(symbols) => (symbols, None),
                Err(e) => {
                    stats.failed_files += 1;
                    (vec![], Some(e.to_string()))
                }
            };
            FileMap {
                sha256: hash.clone(),
                language: language.into(),
                symbols,
                diagnostic,
            }
        };
        output_bytes += serde_json::to_vec(&file)
            .map_err(|e| error(e.to_string()))?
            .len() as u64;
        if output_bytes > p.native_index_max_bytes {
            truncated = true;
            break;
        }
        entries.insert(path.clone(), file);
    }
    stats.elapsed_ms = started.elapsed().as_millis();
    Ok(Map {
        source,
        recipe: RECIPE.into(),
        settings: settings(p)?,
        entries,
        stats,
        symbols_truncated: truncated,
    })
}

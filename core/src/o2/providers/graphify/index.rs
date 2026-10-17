//! Export a verified, restricted snapshot; Graphify never scans the live checkout.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Serialize, Deserialize)]
pub(super) struct Index {
    schema: u32,
    version: String,
    pub source: SourceSnapshot,
    pub graph_path: String,
    graph_sha256: String,
    pub settings_sha256: String,
    pub files: BTreeMap<String, String>,
}
pub(super) fn arguments(corpus: &Path, out: &Path) -> Vec<String> {
    vec![
        "extract".into(),
        corpus.to_string_lossy().into_owned(),
        "--code-only".into(),
        "--no-cluster".into(),
        "--no-dedup".into(),
        "--no-gitignore".into(),
        "--max-workers".into(),
        "1".into(),
        "--out".into(),
        out.to_string_lossy().into_owned(),
    ]
}
pub(super) fn settings(root: &Path, p: &super::super::super::policy::CodePolicy) -> Result<String> {
    let mut ignores = BTreeMap::new();
    for name in [".gitignore", ".graphifyignore"] {
        if root.join(name).exists() {
            let (_, raw) = paths::source(root, name, 1048576)?;
            ignores.insert(name, digest(&raw));
        }
    }
    let names = crate::git::tracked_source_files(root)?;
    let ignored = ignored(root, &names)?;
    Ok(digest(&serde_json::to_vec(&json!({"policy":{"max_files":p.max_files,"max_file_bytes":p.max_file_bytes,"corpus_max_bytes":p.graphify_corpus_max_bytes},"ignores":ignores,"ignored_paths":ignored,"lock":digest(runtime::LOCK.as_bytes()),"launcher":digest(runtime::LAUNCHER.as_bytes())})).map_err(|e|error(e.to_string()))?))
}
fn ignored(root: &Path, names: &[String]) -> Result<std::collections::BTreeSet<String>> {
    super::super::snapshot::ignored_paths(root, names, Some(".graphifyignore"))
}
pub(super) fn build(
    root: &Path,
    generation: &str,
    meta: &Value,
    p: &super::super::super::policy::CodePolicy,
) -> Result<String> {
    let source = source_snapshot(root, p)?;
    if source.truncated {
        return Err(error(
            "Source snapshot exceeds file limit; raise the reviewed limit before Graphify indexing",
        ));
    }
    let base = storage::owned(root, generation)?;
    let corpus = base.join("corpus");
    std::fs::create_dir_all(&corpus).map_err(|e| error(e.to_string()))?;
    let names = source.files.keys().cloned().collect::<Vec<_>>();
    let excluded = ignored(root, &names)?;
    let mut files = BTreeMap::new();
    let mut bytes = 0u64;
    for (path, hash) in &source.files {
        let extension = Path::new(path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if excluded.contains(path)
            || ![
                "py", "rs", "js", "jsx", "ts", "tsx", "go", "java", "c", "h", "cpp", "hpp", "cs",
                "rb", "swift", "kt", "php", "scala", "sh", "lua",
            ]
            .contains(&extension)
        {
            continue;
        }
        let (_, raw) = paths::source(root, path, p.max_file_bytes)?;
        if digest(&raw) != *hash {
            return Err(error("Source changed while exporting Graphify corpus"));
        }
        bytes += raw.len() as u64;
        if bytes > p.graphify_corpus_max_bytes {
            return Err(error("Graphify corpus byte bound exceeded"));
        }
        store::write(root, &corpus.join(path), &raw)?;
        files.insert(path.clone(), hash.clone());
    }
    let out = base.clone();
    runtime::command(root, meta, &arguments(&corpus, &out), &base, p)?;
    let graph_path = format!("{generation}/graphify-out/graph.json");
    let raw = bounded_graph(root, &graph_path, p)?;
    validate(&raw, &files)?;
    let index = Index {
        schema: 1,
        version: runtime::VERSION.into(),
        source,
        graph_path,
        graph_sha256: digest(&raw),
        settings_sha256: settings(root, p)?,
        files,
    };
    let path = format!("{generation}/index.json");
    store::write(
        root,
        &storage::owned(root, &path)?,
        &serde_json::to_vec(&index).map_err(|e| error(e.to_string()))?,
    )?;
    Ok(path)
}
fn bounded_graph(
    root: &Path,
    path: &str,
    p: &super::super::super::policy::CodePolicy,
) -> Result<Vec<u8>> {
    let path = storage::owned(root, path)?;
    if std::fs::metadata(&path)
        .map_err(|e| error(e.to_string()))?
        .len()
        > p.graphify_index_max_bytes
    {
        return Err(error("Graphify graph exceeds byte bound"));
    }
    store::bytes(&path)
}
fn validate(raw: &[u8], files: &BTreeMap<String, String>) -> Result<()> {
    let graph: Value = store::json(raw)?;
    let nodes = graph["nodes"]
        .as_array()
        .ok_or_else(|| error("Unsupported Graphify nodes schema"))?;
    let edges = graph["edges"]
        .as_array()
        .ok_or_else(|| error("Unsupported Graphify edges schema"))?;
    if nodes.is_empty() || nodes.len() > 500000 || edges.len() > 1000000 {
        return Err(error("Graphify graph empty or exceeds schema bounds"));
    }
    for node in nodes {
        if !node["id"].is_string() || !node["label"].is_string() {
            return Err(error("Invalid Graphify node"));
        }
        if let Some(path) = node["source_file"].as_str().filter(|s| !s.is_empty()) {
            if !files.contains_key(path) {
                return Err(error(
                    "Graphify node cites a source outside the exported corpus",
                ));
            }
        }
    }
    for edge in edges {
        if !edge["source"].is_string()
            || !edge["target"].is_string()
            || !matches!(
                edge["confidence"].as_str(),
                Some("EXTRACTED" | "INFERRED" | "AMBIGUOUS")
            )
        {
            return Err(error("Unsupported Graphify relation/provenance schema"));
        }
    }
    Ok(())
}
pub(super) fn load(
    root: &Path,
    path: &str,
    p: &super::super::super::policy::CodePolicy,
) -> Result<Index> {
    let data = store::bytes(&storage::owned(root, path)?)?;
    if data.len() > 16777216 {
        return Err(error("Graphify index manifest exceeds bound"));
    }
    let index: Index = store::json(&data)?;
    if index.schema != 1
        || index.version != runtime::VERSION
        || index.settings_sha256 != settings(root, p)?
    {
        return Err(error(
            "Graphify index settings/version stale; rebuild explicitly",
        ));
    }
    let raw = bounded_graph(root, &index.graph_path, p)?;
    if digest(&raw) != index.graph_sha256 {
        return Err(error("Graphify graph checksum mismatch"));
    }
    validate(&raw, &index.files)?;
    Ok(index)
}

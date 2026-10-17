//! Expendable immutable symbol artifacts; canonical provider receipts own publication.
mod build;
mod parse;
mod query;
#[cfg(test)]
mod tests;
use super::*;
pub(super) use build::{build, load};
pub(super) use query::query;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
const RECIPE: &str =
    "native-symbols-v1:tree-sitter-0.25.10:rust-0.24.0:python-0.25.0:typescript-0.23.2";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Symbol {
    name: String,
    qualified: String,
    kind: String,
    signature: String,
    line: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileMap {
    sha256: String,
    language: String,
    symbols: Vec<Symbol>,
    diagnostic: Option<String>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
struct Stats {
    parsed_files: usize,
    reused_files: usize,
    removed_files: usize,
    unsupported_files: usize,
    failed_files: usize,
    corpus_bytes: u64,
    elapsed_ms: u128,
}
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Map {
    #[serde(flatten)]
    source: SourceSnapshot,
    recipe: String,
    settings: String,
    entries: BTreeMap<String, FileMap>,
    stats: Stats,
    symbols_truncated: bool,
}
impl Map {
    pub(super) fn metadata(&self) -> serde_json::Value {
        serde_json::json!({"native_map":true,"recipe":self.recipe,"stats":self.stats,"truncated":self.symbols_truncated})
    }
}
fn settings(p: &crate::o2::policy::CodePolicy) -> Result<String> {
    Ok(digest(
        &serde_json::to_vec(p).map_err(|e| error(e.to_string()))?,
    ))
}
pub(super) fn details(p: &crate::o2::policy::CodePolicy) -> serde_json::Value {
    serde_json::json!({"native_map_enabled":p.native_map_enabled,"recipe":RECIPE,
        "incremental":p.native_incremental,"languages":{"rust":p.native_rust,"python":p.native_python,
        "typescript":p.native_typescript,"tsx":p.native_tsx},"grammar_origin":"bundled pinned crates; no runtime downloads",
        "semantics":"EXTRACTED declarations/signatures; references remain literal; no graph/type resolution"})
}

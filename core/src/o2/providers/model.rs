use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Descriptor {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    pub origin: String,
    pub operations: Vec<String>,
    pub local_only: bool,
    pub unsupported: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub provider: Descriptor,
    pub status: String,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub provider_id: String,
    pub version: String,
    pub state: String,
    pub index: Option<String>,
    pub index_sha256: Option<String>,
    pub source_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_digest: Option<String>,
    pub owned_paths: Vec<String>,
    pub previous: Option<Box<Receipt>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub provider_id: String,
    pub action: String,
    pub project_id: String,
    pub before_checksum: String,
    pub source_revision: String,
    pub snapshot_digest: String,
    pub affected_paths: Vec<String>,
    pub network: bool,
    pub package_origin: String,
    pub provider_version: String,
    pub diagnostics: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Query {
    pub text: String,
    /// Optional definition-file scope for ambiguous LSP symbol names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub worktree: Option<String>,
    #[serde(default = "default_kind")]
    pub kind: String,
}
fn default_kind() -> String {
    "file_search".into()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeHit {
    pub path: String,
    pub line: u64,
    pub text: String,
    pub file_sha256: String,
    pub provenance: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub provider_id: String,
    pub hits: Vec<CodeHit>,
    pub snapshot_mismatch: bool,
    pub diagnostics: Vec<String>,
    pub estimated_tokens: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub enum Capability<T> {
    Supported(T),
    Unsupported { capability: String, reason: String },
}
#[derive(Debug, Serialize)]
pub struct View {
    pub providers: Vec<Detection>,
    pub receipts: Vec<Receipt>,
    pub plans: Vec<Plan>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_adapter_metadata_preserves_legacy_receipt_checksums() {
        let legacy = serde_json::json!({"provider_id":"native","version":"1","state":"installed","index":null,"index_sha256":null,"source_revision":null,"owned_paths":[],"previous":null});
        let r: Receipt = serde_json::from_value(legacy.clone()).unwrap();
        assert!(r.metadata.is_none());
        assert_eq!(serde_json::to_value(r).unwrap(), legacy);
    }
}

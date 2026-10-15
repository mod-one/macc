//! Adapter-owned declarations, separate from runtime capability observations.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompactionSpec {
    pub schema: String,
    pub version_pattern: String,
    pub documentation: String,
    pub native_auto: Option<NativeAuto>,
    pub context_tokens_path: Option<String>,
    pub context_window_path: Option<String>,
    pub compaction_count_path: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NativeAuto {
    pub format: String,
    pub path: Option<String>,
    pub key: String,
    pub unit: String,
    #[serde(default)]
    pub disabled_env: Vec<String>,
}
impl CompactionSpec {
    pub fn validate(&self) -> crate::Result<()> {
        let invalid =
            || crate::MaccError::Validation("Invalid compaction adapter declaration".into());
        if self.schema.is_empty()
            || self.schema.len() > 100
            || self.version_pattern.len() > 200
            || !self.documentation.starts_with("https://")
        {
            return Err(invalid());
        }
        regex::Regex::new(&self.version_pattern).map_err(|_| invalid())?;
        for path in self
            .context_tokens_path
            .iter()
            .chain(self.context_window_path.iter())
            .chain(self.compaction_count_path.iter())
        {
            if !path.starts_with('/') || path.len() > 200 {
                return Err(invalid());
            }
        }
        if let Some(n) = &self.native_auto {
            if !["env", "json", "toml"].contains(&n.format.as_str())
                || !["percent", "ratio", "tokens"].contains(&n.unit.as_str())
                || n.key.len() > 100
                || !n.key.to_ascii_lowercase().contains("compact")
                    && !n.key.to_ascii_lowercase().contains("compression")
            {
                return Err(invalid());
            }
            if n.format == "env" {
                if n.path.is_some() || !n.key.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') {
                    return Err(invalid());
                }
            } else {
                let path = n.path.as_deref().ok_or_else(invalid)?;
                if !path.starts_with('.')
                    || std::path::Path::new(path)
                        .components()
                        .any(|c| !matches!(c, std::path::Component::Normal(_)))
                    || [".git/", ".macc/", ".env"]
                        .iter()
                        .any(|s| path.starts_with(s))
                {
                    return Err(invalid());
                }
            }
            if n.disabled_env
                .iter()
                .any(|s| s.len() > 100 || !s.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'))
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

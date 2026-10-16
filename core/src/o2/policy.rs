//! One typed policy/catalog and revision-checked transaction for all clients.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub memory: MemoryPolicy,
    pub context_engine: ContextPolicy,
    pub adaptive_compaction: CompactionPolicy,
    pub code_intelligence: CodePolicy,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct MemoryPolicy {
    pub enabled: bool,
    pub max_body_chars: u64,
    pub search_limit: u64,
    pub min_confidence: f64,
    pub provider_note_ttl_days: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ContextPolicy {
    pub enabled: bool,
    pub push_memory_limit_tokens: u64,
    pub pull_limit_tokens: u64,
    pub output_reduction: bool,
    pub max_output_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct CompactionPolicy {
    pub enabled: bool,
    pub configure_native_auto: bool,
    pub native_token_threshold: u64,
    pub warning_ratio: f64,
    pub action_ratio: f64,
    pub guard_ratio: f64,
    pub min_turns: u64,
    pub max_compactions_per_session: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct CodePolicy {
    pub enabled: bool,
    pub routing: String,
    pub max_files: u64,
    pub max_file_bytes: u64,
    pub query_limit: u64,
    pub output_tokens: u64,
    pub graphify_enabled: bool,
    pub graphify_python: String,
    pub graphify_uv: String,
    pub graphify_timeout_seconds: u64,
    pub graphify_install_timeout_seconds: u64,
    pub graphify_memory_mb: u64,
    pub graphify_index_max_bytes: u64,
    pub graphify_corpus_max_bytes: u64,
}
impl Default for MemoryPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_body_chars: 1200,
            search_limit: 20,
            min_confidence: 0.6,
            provider_note_ttl_days: 14,
        }
    }
}
impl Default for ContextPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            push_memory_limit_tokens: 1200,
            pull_limit_tokens: 2000,
            output_reduction: true,
            max_output_bytes: 65536,
        }
    }
}
impl Default for CompactionPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            configure_native_auto: false,
            native_token_threshold: 0,
            warning_ratio: 0.55,
            action_ratio: 0.70,
            guard_ratio: 0.85,
            min_turns: 3,
            max_compactions_per_session: 2,
        }
    }
}
impl Default for CodePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            routing: "native_only".into(),
            max_files: 10000,
            max_file_bytes: 262144,
            query_limit: 20,
            output_tokens: 2000,
            graphify_enabled: false,
            graphify_python: "python3".into(),
            graphify_uv: "uv".into(),
            graphify_timeout_seconds: 120,
            graphify_install_timeout_seconds: 600,
            graphify_memory_mb: 2048,
            graphify_index_max_bytes: 33554432,
            graphify_corpus_max_bytes: 268435456,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Field {
    pub path: String,
    pub label: String,
    pub kind: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub choices: Vec<String>,
    pub default: Value,
    pub activation: String,
}
#[derive(Debug, Serialize)]
pub struct View {
    pub project_root: String,
    pub revision: String,
    pub source: String,
    pub policy: Policy,
    pub fields: Vec<Field>,
    pub sqlite: bool,
    pub ready: bool,
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        let value = serde_json::to_value(self).map_err(|e| error(e.to_string()))?;
        for f in fields() {
            let v = value
                .pointer(&format!("/{}", f.path.replace('.', "/")))
                .unwrap();
            if f.kind == "number"
                && v.as_f64()
                    .is_none_or(|n| !n.is_finite() || n < f.min.unwrap() || n > f.max.unwrap())
            {
                return Err(error(format!("{} is outside its allowed range", f.path)));
            }
            if f.kind == "enum" && !f.choices.iter().any(|s| Some(s.as_str()) == v.as_str()) {
                return Err(error(format!("Invalid {}", f.path)));
            }
            if f.kind == "string"
                && v.as_str()
                    .is_none_or(|s| s.trim().is_empty() || s.len() > 512 || s.contains('\0'))
            {
                return Err(error(format!("Invalid bounded executable path {}", f.path)));
            }
        }
        let c = &self.adaptive_compaction;
        if !(c.warning_ratio < c.action_ratio && c.action_ratio < c.guard_ratio) {
            return Err(error(
                "Compaction thresholds require warning < action < guard",
            ));
        }
        if self.context_engine.enabled && !self.memory.enabled {
            return Err(error("Context Engine requires Memory Ledger enabled"));
        }
        Ok(())
    }
}
pub use super::policy_fields::fields;
pub(crate) fn from_config(c: &crate::config::CanonicalConfig) -> Policy {
    Policy {
        memory: c.memory.clone().unwrap_or_default(),
        context_engine: c.context_engine.clone().unwrap_or_default(),
        adaptive_compaction: c.adaptive_compaction.clone().unwrap_or_default(),
        code_intelligence: c.code_intelligence.clone().unwrap_or_default(),
    }
}
pub fn load(root: &Path) -> Result<Policy> {
    let p = crate::ProjectPaths::from_root(root);
    let c = if p.config_path.exists() {
        crate::load_canonical_config(&p.config_path)?
    } else {
        Default::default()
    };
    let policy = from_config(&c);
    policy.validate()?;
    Ok(policy)
}
pub fn inspect(root: &Path) -> Result<View> {
    let p = crate::ProjectPaths::from_root(root);
    let c = if p.config_path.exists() {
        crate::load_canonical_config(&p.config_path)?
    } else {
        Default::default()
    };
    let policy = from_config(&c);
    policy.validate()?;
    let _guard = store::lock(root)?;
    let sqlite = store::is_sqlite(root)?;
    let ready = if sqlite {
        let (db, _) = store::active_database(root)?;
        super::ready(&db)?
    } else {
        false
    };
    Ok(View {
        project_root: root
            .canonicalize()
            .map_err(|e| error(e.to_string()))?
            .display()
            .to_string(),
        revision: crate::config::transaction::revision(&c)?,
        source: if c.memory.is_some()
            || c.context_engine.is_some()
            || c.adaptive_compaction.is_some()
            || c.code_intelligence.is_some()
        {
            "project"
        } else {
            "defaults"
        }
        .into(),
        policy,
        fields: fields(),
        sqlite,
        ready,
    })
}
pub fn update(root: &Path, revision: &str, p: Option<Policy>) -> Result<View> {
    if let Some(p) = &p {
        p.validate()?;
    }
    crate::config::transaction::update(
        &crate::ProjectPaths::from_root(root),
        Some(revision),
        |c| {
            c.memory = p.as_ref().map(|p| p.memory.clone());
            c.context_engine = p.as_ref().map(|p| p.context_engine.clone());
            c.adaptive_compaction = p.as_ref().map(|p| p.adaptive_compaction.clone());
            c.code_intelligence = p.as_ref().map(|p| p.code_intelligence.clone());
            Ok(())
        },
    )?;
    inspect(root)
}

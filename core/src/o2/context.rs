//! Mandatory checkpoint/PRD data is separate from discretionary memory budgets.
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use store::checkpoints::Checkpoint;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Selection {
    pub id: String,
    pub checksum: String,
    pub source_refs: Vec<String>,
    pub reason: String,
    pub estimated_tokens: u64,
    pub title: String,
    pub body: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub schema_version: u32,
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub phase: String,
    pub role: String,
    pub checkpoint_id: String,
    pub content_fingerprint: String,
    pub cache_hit: bool,
    pub source_revision: String,
    pub worktree_content_digest: String,
    pub mandatory_task: serde_json::Value,
    pub source_hashes: std::collections::BTreeMap<String, String>,
    pub selected: Vec<Selection>,
    pub exclusions: Vec<String>,
    pub optional_estimated_tokens: u64,
    pub optional_limit_tokens: u64,
    pub measurement_quality: String,
}
pub(crate) fn for_checkpoint(
    db: &rusqlite::Connection,
    root: &Path,
    c: &Checkpoint,
) -> Result<String> {
    let policy = super::policy::load(root)?;
    if !policy.context_engine.enabled {
        return Ok(String::new());
    }
    if !super::ready(db)? {
        return Err(error("O2 upgrade required for Context Engine"));
    }
    let query = retrieval_query(&c.mandatory_task);
    let retrieval = memory::search::search_in(
        db,
        &c.project_id,
        root,
        Path::new(&c.worktree),
        &memory::Search {
            query,
            access: memory::Access {
                task_id: Some(c.task_id.clone()),
                worktree: Some(c.worktree.clone()),
                tool_id: Some(c.tool_id.clone()),
                feature: task_feature(&c.mandatory_task),
                ..Default::default()
            },
            include_inactive: false,
        },
        true,
    )?;
    let (content_fingerprint, cache_hit, mut selection) = super::context_cache::select(
        db,
        c,
        retrieval.hits,
        policy.context_engine.push_memory_limit_tokens,
    )?;
    selection.exclusions.extend(retrieval.exclusions);
    let used = selection.used;
    let mut bundle = Bundle {
        schema_version: 1,
        id: String::new(),
        project_id: c.project_id.clone(),
        task_id: c.task_id.clone(),
        phase: c.phase.clone(),
        role: c.role.clone(),
        checkpoint_id: c.id.clone(),
        content_fingerprint,
        cache_hit,
        source_revision: c.source_revision.clone(),
        worktree_content_digest: c.worktree_content_digest.clone(),
        mandatory_task: c.mandatory_task.clone(),
        source_hashes: c.source_hashes.clone(),
        selected: selection.selected,
        exclusions: selection.exclusions,
        optional_estimated_tokens: used,
        optional_limit_tokens: policy.context_engine.push_memory_limit_tokens,
        measurement_quality: "estimated_characters; not provider token usage".into(),
    };
    bundle.id = digest(
        serde_json::to_string(&bundle)
            .map_err(|e| error(e.to_string()))?
            .as_bytes(),
    );
    let raw = serde_json::to_string(&bundle).map_err(|e| error(e.to_string()))?;
    sql(db.execute(
        "INSERT OR IGNORE INTO context_bundles VALUES(?1,?2,?3,?4,?5,?6)",
        rusqlite::params![bundle.id, c.task_id, c.phase, raw, now(), c.id],
    ))?;
    store::write(
        root,
        &root.join(format!(".macc/context/bundles/{}.json", bundle.id)),
        raw.as_bytes(),
    )?;
    super::schema::audit(
        db,
        "context_dispatched",
        &bundle.id,
        json!({"task":c.task_id,"optional_estimated_tokens":used,"memories":bundle.selected.len()}),
    )?;
    super::context_prompt::render(&bundle.selected, &bundle.id)
}
pub(crate) fn task_feature(task: &serde_json::Value) -> Option<String> {
    let selected = &task["tasks"][0];
    selected["feature_id"]
        .as_str()
        .or_else(|| selected["feature"].as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
}
pub fn list(root: &Path) -> Result<Vec<Bundle>> {
    let _guard = store::lock(root)?;
    let (db, _) = database(root)?;
    let mut q = sql(db.prepare(
        "SELECT manifest_json FROM context_bundles ORDER BY created_at DESC,rowid DESC LIMIT 100",
    ))?;
    let result = sql(q.query_map([], |r| r.get::<_, String>(0)))?
        .map(|r| sql(r).and_then(|s| store::json(s.as_bytes())))
        .collect();
    result
}
pub fn get(root: &Path, id: &str) -> Result<Bundle> {
    let _guard = store::lock(root)?;
    let (db, a) = database(root)?;
    let raw: String = sql(db.query_row(
        "SELECT manifest_json FROM context_bundles WHERE id=?1",
        [id],
        |r| r.get(0),
    ))?;
    let b: Bundle = store::json(raw.as_bytes())?;
    let mut unsigned = b.clone();
    unsigned.id.clear();
    if digest(
        serde_json::to_vec(&unsigned)
            .map_err(|e| error(e.to_string()))?
            .as_slice(),
    ) != b.id
    {
        return Err(error("Context manifest checksum mismatch"));
    }
    if b.project_id != a.project_id {
        return Err(error("Foreign context bundle"));
    }
    Ok(b)
}
pub use super::output::{reduce, Reduced};

/// Search lexical task values, not JSON punctuation or structural field names.
fn retrieval_query(task: &serde_json::Value) -> String {
    fn values(value: &serde_json::Value, words: &mut Vec<String>) {
        if words.len() >= 20 {
            return;
        }
        match value {
            serde_json::Value::String(s) => {
                for word in s
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| w.chars().count() > 1)
                {
                    if words.len() >= 20 {
                        break;
                    }
                    words.push(word.chars().take(40).collect());
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    values(item, words);
                }
            }
            serde_json::Value::Object(items) => {
                for item in items.values() {
                    values(item, words);
                }
            }
            _ => {}
        }
    }
    let mut words = Vec::new();
    values(task, &mut words);
    let mut query = String::new();
    for word in words {
        if query.len() + word.len() + 1 > 512 {
            break;
        }
        if !query.is_empty() {
            query.push(' ');
        }
        query.push_str(&word);
    }
    query
}

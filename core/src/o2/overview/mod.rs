//! Consolidated operator read model. No provider execution, inference or lifecycle writes.
mod queries;
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Query {
    pub task_id: Option<String>,
    pub after: Option<String>,
    pub limit: usize,
}
impl Default for Query {
    fn default() -> Self {
        Self {
            task_id: None,
            after: None,
            limit: 25,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRef {
    pub id: String,
    pub phase: String,
    pub optional_estimated_tokens: u64,
    pub optional_limit_tokens: u64,
    pub measurement_quality: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRef {
    pub id: String,
    pub phase: String,
    pub validation_state: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub sessions: u64,
    pub lease_records: u64,
    pub checkpoints: u64,
    pub needs_reconciliation: u64,
    pub contexts: u64,
    pub proposals: u64,
    pub latest_context: Option<ContextRef>,
    pub latest_checkpoint: Option<CheckpointRef>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub state: String,
    pub version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct View {
    pub schema_version: u32,
    pub captured_at: i64,
    pub ready: bool,
    pub notice: String,
    pub memory_statuses: BTreeMap<String, u64>,
    pub providers: Vec<Provider>,
    pub tasks: Vec<Task>,
    pub next_cursor: Option<String>,
}
pub fn inspect(root: &Path, q: &Query) -> Result<View> {
    if !(1..=50).contains(&q.limit) {
        return Err(error("Overview page limit must be 1..50"));
    }
    for v in [&q.task_id, &q.after].into_iter().flatten() {
        if v.is_empty() || v.len() > 256 {
            return Err(error("Overview task identifiers must contain 1..256 bytes"));
        }
        agent::clean(v)?;
    }
    let mut view = View {
        schema_version: 1,
        captured_at: now(),
        ready: false,
        notice:
            "SQLite O2 authority is required; prepare policies or preview migration in Sessions."
                .into(),
        memory_statuses: BTreeMap::new(),
        providers: vec![],
        tasks: vec![],
        next_cursor: None,
    };
    if !store::is_sqlite(root)? {
        return Ok(view);
    }
    let (mut db, authority) = store::readonly_database(root)?;
    if !ready(&db)? {
        view.notice = "Preview the O2 upgrade before inspecting shared state.".into();
        return Ok(view);
    }
    // A short read transaction keeps cross-domain counters and references coherent.
    let tx = sql(db.transaction())?;
    view.ready = true;
    view.notice="Observed records, not task delivery. Memory counts use stored statuses; retrieval revalidates evidence. Lease records may require recovery. Provider receipts are not live health probes. Context budgets are estimates.".into();
    view.memory_statuses = queries::memory_counts(&tx, &authority.project_id)?;
    view.providers = queries::providers(&tx)?;
    view.tasks = queries::tasks(&tx, &authority.project_id, q)?;
    if view.tasks.len() > q.limit {
        view.tasks.truncate(q.limit);
        view.next_cursor = view.tasks.last().map(|t| t.task_id.clone());
    }
    agent::clean_output(&serde_json::to_value(&view).map_err(|e| error(e.to_string()))?)?;
    sql(tx.commit())?;
    Ok(view)
}
#[cfg(test)]
mod tests;

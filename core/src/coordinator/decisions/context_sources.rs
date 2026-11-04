//! Scoped reads of exact immutable sources; prompt references never certify unseen contents.
use super::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const SOURCE_READ: &str = "Read these pinned requirements before relying on them. References do not certify unseen requirements. Use the inherited command: \"$MACC_DECISION_CLI\" --cwd \"$MACC_DECISION_ROOT\" decisions context-source";

pub(super) fn plan_reference(source: &Value) -> String {
    // Keep all plan constraints inline. Compact encoding removes whitespace only;
    // historical keys cannot be guessed to be non-normative by their names.
    format!("Versioned plan requirements and context (exact data; no truncation):\n{}\nPlan source digest: {}. Optional scoped reread using the inherited command: \"$MACC_DECISION_CLI\" --cwd \"$MACC_DECISION_ROOT\" decisions context-source --plan [--pointer /field] [--offset next_offset].\n",serde_json::to_string(source).unwrap(),digest(source))
}
pub(super) fn dependency_references(sources: &BTreeMap<String, Value>) -> String {
    let references:Vec<_> = sources.iter().map(|(id,v)|json!({"id":id,"digest":digest(v),"bytes":serde_json::to_vec_pretty(v).unwrap().len(),"acceptance_criteria":v.get("acceptance_criteria"),"gate":v.get("gate")})).collect();
    format!("Pinned prerequisite contracts (references; complete contracts retained):\n{}\n{SOURCE_READ} --dependency <id> [--pointer /field] [--offset next_offset]. Continue pages until next_offset is null for every selected source. Full contract digests remain mandatory for freshness checks.\n",serde_json::to_string(&references).unwrap())
}

impl Store {
    /// Reads only a dependency or plan source present in the caller's issued context.
    /// JSON pointers scope a read; byte cursors preserve UTF-8 and output envelope bounds.
    pub fn context_source(
        &self,
        plan: &str,
        attempt: &str,
        phase: &str,
        dependency: Option<&str>,
        pointer: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> crate::Result<Value> {
        if !(1024..=65536).contains(&limit) {
            return Err(error("Source page limit must be 1024..65536 bytes"));
        }
        let context = self.read_context(plan, attempt, phase)?;
        let current = self.context(
            &self.plan(plan)?,
            &context.task_id,
            attempt,
            phase,
            &policy(&self.root)?,
        )?;
        if context.context_digest != current.context_digest {
            return Err(error(
                "DM_STALE: pinned source context changed; obtain a fresh task attempt",
            ));
        }
        let source = if let Some(id) = dependency {
            context
                .dependency_contracts
                .get(id)
                .ok_or_else(|| error("Dependency is outside the pinned task context"))?
        } else {
            context
                .plan_context
                .as_ref()
                .ok_or_else(|| error("No plan metadata is pinned in this context"))?
        };
        let selected = if let Some(path) = pointer {
            if !path.is_empty() && !path.starts_with('/') {
                return Err(error("Source pointer must be a JSON pointer"));
            }
            source
                .pointer(path)
                .ok_or_else(|| error("Source pointer is absent from this pinned source"))?
        } else {
            source
        };
        let text = serde_json::to_string_pretty(selected).map_err(|e| error(e.to_string()))?;
        if offset > text.len() || !text.is_char_boundary(offset) {
            return Err(error("Invalid UTF-8 source page offset"));
        }
        let mut end = offset.saturating_add(limit).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        loop {
            let page = json!({"plan_id":plan,"task_id":context.task_id,"context_digest":context.context_digest,"source":dependency.unwrap_or("plan"),"source_digest":digest(source),"selection_digest":digest(selected),"pointer":pointer,"offset":offset,"next_offset":if end<text.len(){Some(end)}else{None},"total_bytes":text.len(),"content":&text[offset..end]});
            let size = serde_json::to_vec_pretty(&page)
                .map_err(|e| error(e.to_string()))?
                .len()
                + 1;
            if end == offset && offset < text.len() {
                return Err(error("Source page limit leaves no room for content"));
            }
            if size <= limit {
                return Ok(page);
            }
            if end == offset {
                return Err(error(
                    "Source page metadata exceeds the selected page limit",
                ));
            }
            // JSON escaping can expand a page beyond twice its raw length.
            // Shrink progressively, never discarding a feasible page outright.
            let shrink = (size - limit).min(((end - offset) / 2).max(1)).max(1);
            end = end.saturating_sub(shrink).max(offset);
            while !text.is_char_boundary(end) {
                end -= 1;
            }
        }
    }
}

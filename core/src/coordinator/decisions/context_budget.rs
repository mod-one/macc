//! Exact serialized prompt accounting, shared by preflight, issuance and clients.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const CONTEXT_BUDGET_CODE: &str = "MACC-DECISION-CONTEXT-BUDGET";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudget {
    pub code: String,
    pub task_id: String,
    pub phase: String,
    pub required_bytes: usize,
    pub budget_bytes: usize,
    pub fits: bool,
    pub sections: BTreeMap<String, usize>,
    pub prerequisite_count: usize,
    pub binding_count: usize,
}
impl ContextBudget {
    pub fn check(&self) -> crate::Result<()> {
        if self.fits {
            return Ok(());
        }
        Err(crate::MaccError::Coordinator {code:"E903",message:format!("{CONTEXT_BUDGET_CODE}: task={} phase={} required_bytes={} budget_bytes={} prerequisites={} bindings={} sections={}; review automation.coordinator.decisions.max_context_bytes or split scope before dispatch; no binding was truncated",self.task_id,self.phase,self.required_bytes,self.budget_bytes,self.prerequisite_count,self.binding_count,serde_json::to_string(&self.sections).unwrap())})
    }
}
impl ContextPackage {
    pub fn budget(&self) -> ContextBudget {
        let sections: BTreeMap<String, usize> = self
            .render_sections()
            .into_iter()
            .map(|(name, text)| (name.into(), text.len()))
            .collect();
        let required_bytes = sections.values().sum();
        ContextBudget {
            code: CONTEXT_BUDGET_CODE.into(),
            task_id: self.task_id.clone(),
            phase: self.phase.clone(),
            required_bytes,
            budget_bytes: self.policy.max_context_bytes,
            fits: required_bytes <= self.policy.max_context_bytes,
            sections,
            prerequisite_count: self.dependency_contracts.len(),
            binding_count: self.bindings.len(),
        }
    }
}
/// Does not claim a task, prepare a worker, rotate a session or issue a context.
pub fn context_budget(
    root: &std::path::Path,
    task: &str,
    phase: &str,
) -> crate::Result<Option<ContextBudget>> {
    let p = policy(root)?;
    if !p.enabled {
        return Ok(None);
    }
    validate_context_identity(task, "budget-preview", phase)?;
    let (store, plan) = active_plan(root)?;
    Ok(Some(
        store
            .build_context(&plan, task, "budget-preview", phase, &p)?
            .budget(),
    ))
}

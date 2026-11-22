use super::{error, observations, Category, Proposal};
use crate::coordinator::{
    decisions as dm,
    model::{ExternalBlockSource, Task},
    phase_diagnostics, worktree_roles,
};
use crate::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub(super) fn candidate(root: &Path, task: &Task) -> Result<PathBuf> {
    let Some(path) = task.worktree_path().map(Path::new) else {
        if task.task_runtime.last_worktree_path.is_some() || task.task_runtime.last_branch.is_some()
        {
            return Err(error("Retained worker assignment must be reconciled first"));
        }
        return Ok(root.to_owned());
    };
    if !worktree_roles::performer(root, path)
        || crate::git::git_common_dir(path)?
            .canonicalize()
            .map_err(error)?
            != crate::git::git_common_dir(root)?
                .canonicalize()
                .map_err(error)?
        || task.branch().is_none()
        || crate::git::current_branch_name(path).ok().as_deref() != task.branch()
    {
        return Err(error("Retained worker no longer owns its branch"));
    }
    Ok(path.to_owned())
}

fn contains_text(value: &Value, text: &str) -> bool {
    match value {
        Value::String(s) => s == text,
        Value::Array(a) => a.iter().any(|v| contains_text(v, text)),
        Value::Object(o) => o.values().any(|v| contains_text(v, text)),
        _ => false,
    }
}

pub(super) fn version(root: &Path, prd: &Path, task: &Task, p: &Proposal) -> Result<Value> {
    let rt = &task.task_runtime;
    if p.task_id != task.id
        || p.phase != crate::coordinator::retained_reconciliation::retained_phase(task)
        || !matches!(
            p.phase.as_str(),
            "dev" | "test" | "review" | "fix" | "merge"
        )
        || p.target.trim().is_empty()
        || p.target.len() > 512
        || p.target.chars().any(char::is_control)
        || p.criterion.trim().is_empty()
        || p.criterion.len() > 4096
        || rt
            .pid
            .is_some_and(crate::coordinator::helpers::is_pid_running)
    {
        return Err(error(
            "Task, target or retained phase does not match the diagnosed block",
        ));
    }
    let contract = read_contract(prd, &task.id)?;
    let technical = p.category == Category::TechnicalCondition;
    if technical != p.verifier.is_some() {
        return Err(error("Only a technical condition takes a verifier"));
    }
    if let Some(v) = &p.verifier {
        observations::validate(&v.source)?;
        if v.script.trim().is_empty() || v.script.len() > 16384 {
            return Err(error("Verifier script must be 1..16384 bytes"));
        }
        if task.state != "blocked"
            || task.gate.is_some()
            || contract.get("gate").is_some_and(|gate| !gate.is_null())
            || rt.extra.contains_key(dm::COMPLIANCE_BLOCK_KEY)
            || !matches!(rt.last_error_code.as_deref(), Some("E903" | "E904"))
            || (p.phase != "dev" && !task.has_worktree_attached())
        {
            return Err(error(
                "This block requires its native review, dependency or recovery path",
            ));
        }
    }
    if let Some(reference) = &p.decision {
        if phase_diagnostics::decision_wait_id(task) != Some(reference.id.as_str()) {
            return Err(error("Task is not waiting on this decision"));
        }
        let store = dm::Store::open(root)?;
        let d = store.decision(&reference.id)?;
        if d.version != reference.version || d.request.task_id != task.id {
            return Err(error("Decision or owner has changed"));
        }
        if technical {
            let plan = store.plan(&d.plan_id)?;
            if root.join(&plan.source).canonicalize().ok()
                != Some(prd.canonicalize().map_err(error)?)
                || d.subject_digest != dm::digest(&contract)
                || rt.unmet_preconditions.len() > 1
            {
                return Err(error("The recorded decision no longer identifies the sole prerequisite of this contract"));
            }
            if rt.last_error_origin.as_deref() == Some("precondition") {
                let recorded = rt.extra.get("decision_wait_reason").and_then(Value::as_str);
                if !rt.unmet_preconditions.first().is_some_and(|condition| {
                    condition == &p.criterion || Some(condition.as_str()) == recorded
                }) {
                    return Err(error(
                        "The performer's independent precondition still applies",
                    ));
                }
            }
            store.check_observable_intervention(&d, &p.criterion)?;
            if !matches!(
                rt.last_error_origin.as_deref(),
                Some("decisions" | "precondition")
            ) || task.blocked_on_external.is_some()
            {
                return Err(error("An independent prerequisite still applies"));
            }
        }
    } else if technical {
        if phase_diagnostics::decision_wait_id(task).is_some()
            || dm::pending_for_task(root, &task.id)?.is_some()
        {
            return Err(error("A native decision must be bound explicitly"));
        }
        if let Some(block) = &task.blocked_on_external {
            if block.source != ExternalBlockSource::Prd
                || contract
                    .pointer("/blocked_on_external/clears_when")
                    .and_then(Value::as_str)
                    != Some(&p.criterion)
                || block.clears_when != p.criterion
                || task.external_block_is_cleared()
                || rt.last_error_code.as_deref() != Some("E904")
            {
                return Err(error(
                    "External PRD condition changed or requires operator authority",
                ));
            }
        } else if rt.last_error_origin.as_deref() != Some("precondition")
            || rt.unmet_preconditions.as_slice() != [p.criterion.as_str()]
            || ![
                "acceptance_criteria",
                "requirements",
                "preconditions",
                "steps",
                "objective",
                "description",
                "result",
                "verification",
            ]
            .iter()
            .any(|key| contains_text(&contract[key], &p.criterion))
        {
            return Err(error(
                "Expected a single unmet criterion from the task contract",
            ));
        }
    }
    let head = if technical {
        Some(crate::git::head_commit(&candidate(root, task)?)?)
    } else {
        None
    };
    anchor(prd, task, &contract, head)
}

pub(super) fn read_contract(prd: &Path, id: &str) -> Result<Value> {
    contract_from(&read_prd(prd)?, id)
}

pub(super) fn read_prd(prd: &Path) -> Result<Value> {
    serde_json::from_slice(&std::fs::read(prd).map_err(error)?).map_err(error)
}

pub(super) fn contract_from(graph: &Value, id: &str) -> Result<Value> {
    let node = graph["tasks"]
        .as_array()
        .and_then(|a| a.iter().find(|n| n["id"] == id))
        .ok_or_else(|| error("Task is absent from the current PRD"))?;
    Ok(dm::task_contract(node))
}

pub(super) fn anchor(
    prd: &Path,
    task: &Task,
    contract: &Value,
    head: Option<String>,
) -> Result<Value> {
    let rt = &task.task_runtime;
    Ok(json!({
        "prd": prd.canonicalize().map_err(error)?, "contract_sha256": dm::digest(&contract),
        "blocked_at": task.state_changed_at, "external":task.blocked_on_external, "state":task.state,
        "claim":rt.claim_id, "run":rt.run_id, "attempt":rt.attempt, "phase_started":rt.phase_started_at,
        "error_code":rt.last_error_code, "origin":rt.last_error_origin,
        "unmet_sha256":dm::digest(&rt.unmet_preconditions),"decision_wait":rt.extra.get("decision_wait"), "phase":crate::coordinator::retained_reconciliation::retained_phase(task),"branch":task.branch(),"worktree":task.worktree_path(),"commit":head
    }))
}

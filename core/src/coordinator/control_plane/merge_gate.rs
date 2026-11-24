//! Retry reconciliation observes delivery on the reference branch; it never writes Git.
use crate::coordinator::model::Task;
use std::path::Path;

#[derive(Debug, PartialEq)]
pub(super) enum MergeGateResult {
    Merged,
    Retained,
    NoBranchProceed,
}

pub(super) fn merge_gate_check(
    task: &Task,
    base: &str,
    root: &Path,
) -> crate::Result<MergeGateResult> {
    let Some(branch) = task.branch() else {
        return Ok(MergeGateResult::NoBranchProceed);
    };
    if crate::coordinator::delivery_evidence::retained_tip_delivered(root, task, branch, base)? {
        return Ok(MergeGateResult::Merged);
    }
    Ok(MergeGateResult::Retained)
}

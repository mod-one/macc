//! Git integration and workflow acceptance are independent facts.
use crate::coordinator::{
    decisions,
    model::{Task, TaskRegistry},
    state, RuntimeStatus, WorkflowState,
};
use crate::{git, ProjectPaths, Result};
use std::collections::BTreeMap;

pub(super) fn reconcile(
    paths: &ProjectPaths,
    registry: &mut TaskRegistry,
    cfg: &crate::config::CoordinatorConfig,
    args: &BTreeMap<String, String>,
) -> Result<Vec<(String, String)>> {
    let settings = crate::config::CoordinatorConfigResolved::resolve(Some(cfg));
    let reference = settings.reference_branch.as_str();
    let mut waits = Vec::new();
    let assignments = registry.clone();
    for task in &mut registry.tasks {
        if task.satisfies_dependants() || task.is_human_approval_gate() || task.state == "abandoned"
        {
            continue;
        }
        let pending = decisions::pending_for_task(&paths.root, &task.id)?;
        let integrated = integrated_commit(&paths.root, task, reference)?;
        if let Some(pending) = pending {
            let prefix = integrated
                .as_ref()
                .map(|sha| format!("Code integrated into {reference} ({sha}). "))
                .unwrap_or_default();
            if pending.owner_task_id == task.id || pending.owner_task_id == "*" {
                waits.push((pending.id.clone(),format!("{prefix}{}: {}. Review decision {} in the Inbox to provide evidence or propose a governed deferral.",task.id,pending.reason(),pending.id)));
            }
            continue;
        }
        let Some(sha) = integrated else { continue };
        let delivery_phase = matches!(
            task.workflow_state(),
            Some(WorkflowState::PrOpen | WorkflowState::Queued)
        ) || (task.state == "blocked"
            && task.task_runtime.current_phase.as_deref() == Some("merge"));
        if !delivery_phase
            || (task.blocked_on_external.is_some() && !task.external_block_is_cleared())
        {
            continue;
        }
        let Some(branch) = task.branch().filter(|b| *b != reference) else {
            continue;
        };
        let Some(worker) = task.worktree_path().map(|p| paths.root.join(p)) else {
            continue;
        };
        if crate::coordinator::scoped_prd::worker_busy(&assignments, &worker)
            || git::resolve_ref(&paths.root, &format!("refs/heads/{branch}")).as_deref()
                != Some(&sha)
            || git::head_commit(&worker)? != sha
            || git::current_branch_name(&worker)? != branch
            || git::is_dirty(&worker)?
            || task
                .task_runtime
                .retained_review
                .as_ref()
                .and_then(|review| review.approved_commit.as_ref())
                .is_some_and(|approved| approved != &sha)
        {
            continue;
        }
        // Consult the same phase planner as normal publication, including a retained merge retry.
        let mut candidate = task.clone();
        if candidate.state == "blocked" {
            candidate.set_workflow_state(WorkflowState::Queued);
        }
        use crate::coordinator::engine::{plan_task, AdvancePlan};
        let plan = plan_task(
            &candidate,
            settings.max_review_cycles,
            &settings.phases,
            cfg.decisions.as_ref().is_some_and(|p| p.enabled),
        );
        if !matches!(
            plan,
            AdvancePlan::Merge | AdvancePlan::MergeAfterReviewLimit
        ) {
            continue;
        }
        let _fence = decisions::publication_lock(&paths.root)?;
        decisions::check_merge_receipt(
            &paths.root,
            &task.id,
            plan == AdvancePlan::MergeAfterReviewLimit,
        )?;
        let original = task.clone();
        task.set_workflow_state(WorkflowState::Merged);
        task.clear_assignment();
        task.task_runtime
            .extra
            .insert("completion_commit_sha".into(), sha.into());
        task.task_runtime.clear_last_error_details();
        task.task_runtime.last_error = None;
        task.task_runtime.set_status(RuntimeStatus::Idle);
        task.task_runtime.current_phase = None;
        task.touch_state_changed(&crate::coordinator::helpers::now_iso_coordinator());
        state::replace_phase_task(paths, args, &original, task)?;
    }
    let previous = registry.clone();
    crate::coordinator::task_selector::reconcile_task_blocks(
        registry,
        &crate::coordinator::helpers::now_iso_coordinator(),
    );
    for (old, next) in previous.tasks.iter().zip(&registry.tasks) {
        if old != next {
            state::replace_phase_task(paths, args, old, next)?;
        }
    }
    Ok(waits)
}

/// A reusable worker branch can carry commits from several tasks. Only the
/// assignment's completion SHA or a delivery message for this task is evidence.
fn integrated_commit(
    root: &std::path::Path,
    task: &Task,
    reference: &str,
) -> Result<Option<String>> {
    let base = format!("refs/heads/{reference}");
    let candidate = if let Some(sha) = task
        .task_runtime
        .extra
        .get("completion_commit_sha")
        .and_then(|v| v.as_str())
    {
        sha.to_owned()
    } else {
        let Some(branch) = task.branch().filter(|b| *b != reference) else {
            return Ok(None);
        };
        let retained = format!("refs/heads/{branch}");
        if git::resolve_ref(root, &retained).is_none() {
            return Ok(None);
        }
        let Some(sha) = git::latest_commit_with_task_trailer(root, &retained, &task.id)? else {
            return Ok(None);
        };
        let message = git::run_git_output_mapped(
            root,
            &["show", "-s", "--format=%B", &sha],
            "read task delivery",
        )?;
        if !message.status.success()
            || crate::commit_completion::task_id(&String::from_utf8_lossy(&message.stdout))
                .as_deref()
                != Some(task.id.as_str())
        {
            return Ok(None);
        }
        sha
    };
    if !matches!(candidate.len(), 40 | 64) || !candidate.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Ok(None);
    }
    Ok(git::merge_base_is_ancestor(root, &candidate, &base)?.then_some(candidate))
}

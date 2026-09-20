use crate::coordinator::integration::{IntegrationWorktree, PublishOutcome};
use std::path::Path;

#[derive(Debug, PartialEq)]
pub(super) enum MergeGateResult {
    Merged,
    ConflictProceed,
    NoBranchProceed,
}

/// Salvage any already-committed work on a task's branches before re-dispatching.
///
/// Runs entirely against the object database and the coordinator's private
/// integration worktree: candidate branches are compared to the base with
/// `git log base..branch` rather than by checking them out, and the merge itself
/// happens in [`IntegrationWorktree`]. The operator's checkout is never read for
/// cleanliness nor written to, so a dirty working tree no longer turns every
/// salvageable task into a conflict.
pub(super) fn merge_gate_check(
    task_id: &str,
    base_branch: &str,
    repo_root: &Path,
) -> MergeGateResult {
    let mut branch_candidates = Vec::new();
    let prefixes = [
        format!("task/{}", task_id.to_ascii_lowercase()),
        format!("task/{}", task_id),
    ];
    for prefix in prefixes {
        if let Ok(branches) = crate::git::list_branches_by_prefix(repo_root, &prefix) {
            branch_candidates.extend(branches);
        }
    }
    branch_candidates.sort();
    branch_candidates.dedup();
    if branch_candidates.is_empty() {
        return MergeGateResult::NoBranchProceed;
    }

    // Which candidates actually carry work the base does not already have?
    // `git log base..branch` answers this without touching a working tree.
    let mut mergeable = Vec::new();
    let mut conflict_or_error = false;
    for branch in branch_candidates {
        if branch == base_branch {
            continue;
        }
        match crate::git::commits_between(repo_root, base_branch, &branch) {
            Ok(commits) if commits.is_empty() => {}
            Ok(_) => mergeable.push(branch),
            Err(_) => conflict_or_error = true,
        }
    }

    if mergeable.is_empty() {
        return if conflict_or_error {
            MergeGateResult::ConflictProceed
        } else {
            MergeGateResult::NoBranchProceed
        };
    }

    let Ok(integration) = IntegrationWorktree::acquire(repo_root, base_branch) else {
        return MergeGateResult::ConflictProceed;
    };

    for branch in mergeable {
        if try_merge_branch(&integration, &branch)
            && matches!(
                integration.publish(),
                Ok(PublishOutcome::Published { .. }) | Ok(PublishOutcome::UpToDate)
            )
        {
            return MergeGateResult::Merged;
        }
        // Discard whatever this attempt left behind before trying the next
        // candidate, so a conflict cannot leak into the following merge.
        if integration.reset().is_err() {
            break;
        }
    }

    // At least one candidate carried work that could not be integrated.
    MergeGateResult::ConflictProceed
}

/// Merge `branch` into the integration worktree's detached HEAD, preferring a
/// fast-forward. Returns `false` when the merge could not be completed.
fn try_merge_branch(integration: &IntegrationWorktree, branch: &str) -> bool {
    let dir = integration.path();
    if crate::git::merge_ff_only(dir, branch).unwrap_or(false) {
        return true;
    }
    crate::git::run_git_output_mapped(dir, &["merge", "--no-edit", branch], "run merge gate merge")
        .map(|out| out.status.success())
        .unwrap_or(false)
}

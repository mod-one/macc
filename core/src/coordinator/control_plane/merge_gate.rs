use std::path::Path;

#[derive(Debug, PartialEq)]
pub(super) enum MergeGateResult {
    Merged,
    ConflictProceed,
    NoBranchProceed,
}

pub(super) fn merge_gate_check(task_id: &str, base_branch: &str, repo_root: &Path) -> MergeGateResult {
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

    let original_branch = crate::git::current_branch_name(repo_root).ok();
    let mut attempted_merge = false;
    let mut conflict_or_error = false;
    let mut merged = false;

    for branch in branch_candidates {
        if branch == base_branch {
            continue;
        }
        if !crate::git::checkout(repo_root, &branch, false).unwrap_or(false) {
            conflict_or_error = true;
            continue;
        }
        let commits_ahead = match crate::git::commits_ahead_of_base(repo_root, base_branch) {
            Ok(commits) => commits,
            Err(_) => {
                conflict_or_error = true;
                continue;
            }
        };
        if commits_ahead.is_empty() {
            continue;
        }
        attempted_merge = true;
        if !crate::git::checkout(repo_root, base_branch, false).unwrap_or(false) {
            conflict_or_error = true;
            continue;
        }
        if crate::git::merge_ff_only(repo_root, &branch).unwrap_or(false) {
            merged = true;
            break;
        }
        let merge_no_edit = crate::git::run_git_output_mapped(
            repo_root,
            &["merge", "--no-edit", &branch],
            "run git merge --no-edit",
        );
        if merge_no_edit
            .map(|out| out.status.success())
            .unwrap_or(false)
        {
            merged = true;
            break;
        }
        let _ = crate::git::run_git_output_mapped(
            repo_root,
            &["merge", "--abort"],
            "abort merge gate conflict",
        );
        conflict_or_error = true;
    }

    if let Some(branch) = original_branch {
        let _ = crate::git::checkout(repo_root, &branch, false);
    }

    if merged {
        return MergeGateResult::Merged;
    }
    if attempted_merge || conflict_or_error {
        MergeGateResult::ConflictProceed
    } else {
        MergeGateResult::NoBranchProceed
    }
}

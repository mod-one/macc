//! Deterministic reconciliation of task state from git commit history.
//!
//! Scans commits on the reference branch for MACC task ID tags (see
//! [`crate::commit_message`]) and transitions matching tasks that are still
//! `todo`, `claimed`, `in_progress`, `pr_open`, `changes_requested`, or
//! `queued` to `merged`.
//!
//! This module is pure business logic — no CLI, no UI.

use crate::commit_message;
use crate::coordinator::model::TaskRegistry;
use crate::coordinator::WorkflowState;
use crate::git;
use crate::{MaccError, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single commit parsed from `git log`.
#[derive(Debug, Clone)]
pub struct GitCommitInfo {
    pub sha: String,
    pub subject: String,
    pub full_message: String,
}

/// Describes one task whose state was (or would be) reconciled.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciledTask {
    pub task_id: String,
    pub previous_state: String,
    pub new_state: String,
    pub matched_commit_sha: String,
    pub matched_commit_subject: String,
}

/// Result of a reconciliation pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReconcileReport {
    /// Tasks that were transitioned to `merged`.
    pub reconciled: Vec<ReconciledTask>,
    /// Task IDs found in commits but already in a terminal state.
    pub already_done: Vec<String>,
    /// Task IDs found in commits that are **not in the current registry** —
    /// typically tasks delivered in earlier PRD lots. The dispatcher consults
    /// the same git history to satisfy cross-lot dependency edges; listing
    /// them here makes that recognition observable in sync output.
    #[serde(default)]
    pub external_committed_ids: Vec<String>,
    /// Number of commits scanned.
    pub commits_scanned: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncBranchStatus {
    Merged,
    MergeFailed,
    SkippedNoTaskTags,
    SkippedTaskState,
}

impl SyncBranchStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            SyncBranchStatus::Merged => "merged",
            SyncBranchStatus::MergeFailed => "merge_failed",
            SyncBranchStatus::SkippedNoTaskTags => "skipped_no_task_tags",
            SyncBranchStatus::SkippedTaskState => "skipped_task_state",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBranchResult {
    pub branch: String,
    pub discovered_task_ids: Vec<String>,
    pub merged_task_ids: Vec<String>,
    pub status: SyncBranchStatus,
    pub detail: Option<String>,
}

// ---------------------------------------------------------------------------
// Git log reader
// ---------------------------------------------------------------------------

/// Read commits in `base..head` (or all commits on `head` if base is None).
///
/// Uses `git log --format` with a NUL-delimited format to handle multi-line
/// commit messages reliably.
pub fn read_commit_range(
    repo_root: &Path,
    base: Option<&str>,
    head: &str,
) -> Result<Vec<GitCommitInfo>> {
    // Format: <sha>\x1f<subject>\x1f<body>\x00
    // \x1f = unit separator, \x00 = record separator
    let range = match base {
        Some(b) => format!("{}..{}", b, head),
        None => head.to_string(),
    };
    let output = git::run_git_output_mapped(
        repo_root,
        &["log", "--format=%H%x1f%s%x1f%b%x00", &range],
        "read git log for commit reconciliation",
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // If the range is invalid (e.g. base doesn't exist), return empty.
        if stderr.contains("unknown revision") || stderr.contains("bad revision") {
            return Ok(Vec::new());
        }
        return Err(MaccError::Validation(format!("git log failed: {}", stderr)));
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let mut commits = Vec::new();
    for record in raw.split('\0') {
        let record = record.trim();
        if record.is_empty() {
            continue;
        }
        let parts: Vec<&str> = record.splitn(3, '\x1f').collect();
        if parts.len() < 2 {
            continue;
        }
        let sha = parts[0].trim().to_string();
        let subject = parts[1].trim().to_string();
        let body = if parts.len() == 3 {
            parts[2].trim().to_string()
        } else {
            String::new()
        };
        let full_message = if body.is_empty() {
            subject.clone()
        } else {
            format!("{}\n\n{}", subject, body)
        };
        commits.push(GitCommitInfo {
            sha,
            subject,
            full_message,
        });
    }
    Ok(commits)
}

// ---------------------------------------------------------------------------
// Reconciliation engine
// ---------------------------------------------------------------------------

/// States eligible for reconciliation (not yet done).
fn is_reconcilable_state(state: &str) -> bool {
    matches!(
        state,
        "todo" | "claimed" | "in_progress" | "pr_open" | "changes_requested" | "queued"
    )
}

/// States that mean the task is already done.
fn is_terminal_state(state: &str) -> bool {
    matches!(state, "merged" | "abandoned")
}

/// Build a map of task_id -> current state from the registry.
fn build_task_state_map(registry: &TaskRegistry) -> BTreeMap<String, String> {
    registry
        .tasks
        .iter()
        .map(|t| (t.id.clone(), t.state.clone()))
        .collect()
}

/// Discover every MACC task ID that appears in the commit history of `head_ref`.
///
/// Scans `git log <head_ref>` for commits whose message carries either the
/// `[macc:task <id>]` trailer or a structured `<type>: <id> - <title>` subject,
/// and returns the resulting set. Empty when the ref is unknown.
///
/// Use this to recognise dependencies on tasks delivered in earlier PRD lots:
/// such tasks are not in the current registry's `tasks` array, but their
/// commits live on the reference branch and should satisfy dependency edges.
pub fn discover_committed_task_ids(repo_root: &Path, head_ref: &str) -> Result<HashSet<String>> {
    let commits = read_commit_range(repo_root, None, head_ref)?;
    let mut ids = HashSet::new();
    for commit in &commits {
        if let Some(task_id) = commit_message::parse(&commit.full_message).task_id {
            ids.insert(task_id);
        }
    }
    Ok(ids)
}

/// Extract all task IDs found in a list of commits.
///
/// Returns a map of task_id -> (first matching commit sha, subject).
fn extract_task_ids_from_commits(commits: &[GitCommitInfo]) -> BTreeMap<String, (String, String)> {
    let mut found: BTreeMap<String, (String, String)> = BTreeMap::new();
    for commit in commits {
        let parsed = commit_message::parse(&commit.full_message);
        if let Some(task_id) = parsed.task_id {
            found
                .entry(task_id)
                .or_insert_with(|| (commit.sha.clone(), commit.subject.clone()));
        }
    }
    found
}

/// Run the reconciliation logic (pure, no side effects).
///
/// Compares task IDs found in commits against the registry and produces
/// a report of tasks to transition.
pub fn reconcile(registry: &TaskRegistry, commits: &[GitCommitInfo]) -> ReconcileReport {
    let task_states = build_task_state_map(registry);
    let commit_tasks = extract_task_ids_from_commits(commits);
    let mut report = ReconcileReport {
        commits_scanned: commits.len(),
        ..Default::default()
    };

    for (task_id, (sha, subject)) in &commit_tasks {
        let Some(current_state) = task_states.get(task_id) else {
            // Task ID is in commit history but not in the current registry.
            // This is the cross-lot case: a prior PRD lot delivered the task
            // and merged it onto the reference branch. Record it so the
            // dispatcher's external dependency-resolution path is visible to
            // operators inspecting sync output.
            report.external_committed_ids.push(task_id.clone());
            continue;
        };
        if is_terminal_state(current_state) {
            report.already_done.push(task_id.clone());
            continue;
        }
        if is_reconcilable_state(current_state) {
            report.reconciled.push(ReconciledTask {
                task_id: task_id.clone(),
                previous_state: current_state.clone(),
                new_state: WorkflowState::Merged.as_str().to_string(),
                matched_commit_sha: sha.clone(),
                matched_commit_subject: subject.clone(),
            });
        }
    }

    report
}

/// Apply the reconciliation report to a mutable task registry.
///
/// Transitions reconciled tasks to `merged` and clears their assignment.
/// Also persists `external_committed_ids` into
/// `registry.external_merged_task_ids` so the dispatcher can satisfy
/// cross-PRD dependency edges on later cycles without re-scanning git.
pub fn apply_reconcile_report(registry: &mut TaskRegistry, report: &ReconcileReport, now: &str) {
    // Merge (do not replace) the persisted external set. Multiple sync runs
    // across the lifetime of a project accumulate prior-lot deliveries.
    registry
        .external_merged_task_ids
        .extend(report.external_committed_ids.iter().cloned());

    let reconciled_ids: BTreeSet<&str> = report
        .reconciled
        .iter()
        .map(|r| r.task_id.as_str())
        .collect();

    for task in &mut registry.tasks {
        if reconciled_ids.contains(task.id.as_str()) {
            task.state = WorkflowState::Merged.as_str().to_string();
            task.updated_at = Some(now.to_string());
            task.state_changed_at = Some(now.to_string());
            task.clear_assignment();
            // Reset runtime to idle
            task.task_runtime.status = Some("idle".to_string());
            task.task_runtime.pid = None;
            task.task_runtime.started_at = None;
            task.task_runtime.current_phase = None;
            task.task_runtime.merge_result_pending = Some(false);
        }
    }
    registry.recompute_resource_locks(now);
    registry.updated_at = Some(now.to_string());
}

fn list_local_branches(repo_root: &Path) -> Result<Vec<String>> {
    let output = git::run_git_output_mapped(
        repo_root,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
        "list local git branches",
    )?;
    if !output.status.success() {
        return Err(MaccError::Validation(format!(
            "failed to list branches: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn branch_matches_known_patterns(branch: &str, task_ids: &[String]) -> bool {
    if branch.starts_with("macc/worker-") {
        return true;
    }
    let branch_upper = branch.to_ascii_uppercase();
    task_ids
        .iter()
        .any(|task_id| branch_upper.contains(&task_id.to_ascii_uppercase()))
}

fn is_unmerged_branch_sync_state(state: &str) -> bool {
    let normalized = state.trim().to_ascii_lowercase();
    normalized == "todo"
        || normalized == "error"
        || normalized == "failed"
        || normalized.contains("error")
}

fn merge_branch_into_base(repo_root: &Path, branch: &str) -> Result<std::process::Output> {
    git::run_git_output_mapped(
        repo_root,
        &["merge", "--no-ff", "--no-edit", branch],
        "merge branch during sync reconciliation",
    )
}

fn abort_merge(repo_root: &Path) {
    let _ = git::run_git_output_mapped(repo_root, &["merge", "--abort"], "abort conflicted merge");
}

pub fn sync_unmerged_branches(
    registry: &mut TaskRegistry,
    repo_root: &Path,
    base_branch: &str,
) -> Result<Vec<SyncBranchResult>> {
    let known_task_ids: Vec<String> = registry.tasks.iter().map(|task| task.id.clone()).collect();
    let mut branches: Vec<String> = list_local_branches(repo_root)?
        .into_iter()
        .filter(|branch| branch != base_branch)
        .filter(|branch| branch_matches_known_patterns(branch, &known_task_ids))
        .collect();
    branches.sort();
    branches.dedup();

    let original_branch = git::current_branch_name(repo_root).unwrap_or_default();
    if original_branch != base_branch {
        let ok = git::checkout(repo_root, base_branch, false)?;
        if !ok {
            return Err(MaccError::Validation(format!(
                "failed to checkout base branch '{}' before sync_unmerged_branches",
                base_branch
            )));
        }
    }

    let mut out = Vec::new();
    for branch in &branches {
        let commits = read_commit_range(repo_root, Some(base_branch), branch)?;
        let task_commits = extract_task_ids_from_commits(&commits);
        let discovered_task_ids: Vec<String> = task_commits.keys().cloned().collect();
        if discovered_task_ids.is_empty() {
            out.push(SyncBranchResult {
                branch: branch.clone(),
                discovered_task_ids,
                merged_task_ids: Vec::new(),
                status: SyncBranchStatus::SkippedNoTaskTags,
                detail: Some("no [macc:task TASK-ID] tags found in branch commits".to_string()),
            });
            continue;
        }

        let mut eligible_task_ids = Vec::new();
        for task_id in &discovered_task_ids {
            if let Some(task) = registry.tasks.iter().find(|task| task.id == *task_id) {
                if is_unmerged_branch_sync_state(&task.state) {
                    eligible_task_ids.push(task_id.clone());
                }
            }
        }
        if eligible_task_ids.is_empty() {
            out.push(SyncBranchResult {
                branch: branch.clone(),
                discovered_task_ids,
                merged_task_ids: Vec::new(),
                status: SyncBranchStatus::SkippedTaskState,
                detail: Some("matching tasks are not in todo/error states".to_string()),
            });
            continue;
        }

        let merge_output = merge_branch_into_base(repo_root, branch)?;
        if merge_output.status.success() {
            let report = ReconcileReport {
                reconciled: eligible_task_ids
                    .iter()
                    .filter_map(|task_id| {
                        let (sha, subject) = task_commits.get(task_id)?;
                        let previous_state = registry
                            .tasks
                            .iter()
                            .find(|task| task.id == *task_id)?
                            .state
                            .clone();
                        Some(ReconciledTask {
                            task_id: task_id.clone(),
                            previous_state,
                            new_state: WorkflowState::Merged.as_str().to_string(),
                            matched_commit_sha: sha.clone(),
                            matched_commit_subject: subject.clone(),
                        })
                    })
                    .collect(),
                already_done: Vec::new(),
                external_committed_ids: Vec::new(),
                commits_scanned: commits.len(),
            };
            let now = crate::coordinator::helpers::now_iso_coordinator();
            apply_reconcile_report(registry, &report, &now);
            out.push(SyncBranchResult {
                branch: branch.clone(),
                discovered_task_ids,
                merged_task_ids: eligible_task_ids,
                status: SyncBranchStatus::Merged,
                detail: None,
            });
        } else {
            abort_merge(repo_root);
            out.push(SyncBranchResult {
                branch: branch.clone(),
                discovered_task_ids,
                merged_task_ids: Vec::new(),
                status: SyncBranchStatus::MergeFailed,
                detail: Some(
                    String::from_utf8_lossy(&merge_output.stderr)
                        .trim()
                        .to_string(),
                ),
            });
        }
    }

    if !original_branch.is_empty() && original_branch != base_branch {
        let _ = git::checkout(repo_root, &original_branch, false);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::model::Task;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::SystemTime;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn make_task(id: &str, state: &str) -> Task {
        Task {
            id: id.to_string(),
            state: state.to_string(),
            title: Some(format!("Task {}", id)),
            ..Task::default()
        }
    }

    fn make_commit(sha: &str, message: &str) -> GitCommitInfo {
        let subject = message.lines().next().unwrap_or("").to_string();
        GitCommitInfo {
            sha: sha.to_string(),
            subject,
            full_message: message.to_string(),
        }
    }

    fn make_registry(tasks: Vec<Task>) -> TaskRegistry {
        TaskRegistry {
            tasks,
            ..TaskRegistry::default()
        }
    }

    fn run_git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("run git command");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn create_commit(repo: &Path, file: &str, content: &str, message: &str) {
        let path = repo.join(file);
        fs::write(&path, content).expect("write file");
        run_git(repo, &["add", file]);
        run_git(repo, &["commit", "-m", message]);
    }

    fn make_test_repo() -> PathBuf {
        let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let repo = std::env::temp_dir().join(format!(
            "macc-sync-unmerged-tests-{}-{}-{}",
            std::process::id(),
            nanos,
            suffix
        ));
        fs::create_dir_all(&repo).expect("create temp repo");
        run_git(&repo, &["init"]);
        run_git(&repo, &["checkout", "-b", "main"]);
        run_git(&repo, &["config", "user.email", "tests@example.com"]);
        run_git(&repo, &["config", "user.name", "MACC Tests"]);
        create_commit(&repo, "base.txt", "base\n", "chore: base");
        repo
    }

    #[test]
    fn reconcile_todo_task_from_tagged_commit() {
        let registry = make_registry(vec![
            make_task("WEB-001", "todo"),
            make_task("WEB-002", "in_progress"),
        ]);
        let commits = vec![make_commit(
            "abc123",
            "feat: WEB-001 - setup\n\n[macc:task WEB-001]",
        )];
        let report = reconcile(&registry, &commits);
        assert_eq!(report.reconciled.len(), 1);
        assert_eq!(report.reconciled[0].task_id, "WEB-001");
        assert_eq!(report.reconciled[0].previous_state, "todo");
        assert_eq!(report.reconciled[0].new_state, "merged");
    }

    #[test]
    fn reconcile_already_merged_task_skipped() {
        let registry = make_registry(vec![make_task("WEB-001", "merged")]);
        let commits = vec![make_commit(
            "abc123",
            "feat: WEB-001\n\n[macc:task WEB-001]",
        )];
        let report = reconcile(&registry, &commits);
        assert_eq!(report.reconciled.len(), 0);
        assert_eq!(report.already_done, vec!["WEB-001"]);
    }

    #[test]
    fn reconcile_legacy_commit_without_tags() {
        let registry = make_registry(vec![make_task("WEB-FRONTEND-006", "in_progress")]);
        let commits = vec![make_commit(
            "def456",
            "feat: WEB-FRONTEND-006 - Integrate Headless UI",
        )];
        let report = reconcile(&registry, &commits);
        assert_eq!(report.reconciled.len(), 1);
        assert_eq!(report.reconciled[0].task_id, "WEB-FRONTEND-006");
    }

    #[test]
    fn apply_report_persists_external_committed_ids_into_registry() {
        // sync-prd must persist cross-PRD task IDs into the registry so a
        // later dispatch (which only loads the registry, not the git log
        // again) recognises them as dependency satisfiers.
        let mut registry = make_registry(vec![make_task("CURRENT", "todo")]);
        assert!(registry.external_merged_task_ids.is_empty());
        let report = ReconcileReport {
            reconciled: vec![],
            already_done: vec![],
            external_committed_ids: vec!["PRIOR-LOT-001".into(), "PRIOR-LOT-002".into()],
            commits_scanned: 2,
        };
        apply_reconcile_report(&mut registry, &report, "2026-05-22T13:00:00Z");
        assert!(registry.external_merged_task_ids.contains("PRIOR-LOT-001"));
        assert!(registry.external_merged_task_ids.contains("PRIOR-LOT-002"));
    }

    #[test]
    fn apply_report_accumulates_external_ids_across_runs() {
        // Each sync run extends the set rather than replacing it — older
        // lot deliveries remain recognised after a subsequent sync.
        let mut registry = make_registry(vec![make_task("CURRENT", "todo")]);
        registry
            .external_merged_task_ids
            .insert("OLDER-LOT-001".into());
        let report = ReconcileReport {
            reconciled: vec![],
            already_done: vec![],
            external_committed_ids: vec!["NEWER-LOT-001".into()],
            commits_scanned: 1,
        };
        apply_reconcile_report(&mut registry, &report, "2026-05-22T13:01:00Z");
        assert!(registry.external_merged_task_ids.contains("OLDER-LOT-001"));
        assert!(registry.external_merged_task_ids.contains("NEWER-LOT-001"));
    }

    #[test]
    fn reconcile_unknown_task_id_recorded_as_external() {
        // Task IDs found in commit history that are not in the current
        // registry are now captured as `external_committed_ids` so the
        // dispatcher can recognise cross-lot dependencies as satisfied.
        let registry = make_registry(vec![make_task("WEB-001", "todo")]);
        let commits = vec![make_commit(
            "aaa111",
            "feat: UNKNOWN-999\n\n[macc:task UNKNOWN-999]",
        )];
        let report = reconcile(&registry, &commits);
        assert_eq!(report.reconciled.len(), 0);
        assert_eq!(report.already_done.len(), 0);
        assert_eq!(report.external_committed_ids, vec!["UNKNOWN-999"]);
    }

    #[test]
    fn reconcile_multiple_tasks_from_multiple_commits() {
        let registry = make_registry(vec![
            make_task("T-1", "todo"),
            make_task("T-2", "in_progress"),
            make_task("T-3", "merged"),
        ]);
        let commits = vec![
            make_commit("a1", "feat: T-1 - first\n\n[macc:task T-1]"),
            make_commit("a2", "feat: T-2 - second\n\n[macc:task T-2]"),
            make_commit("a3", "feat: T-3 - third\n\n[macc:task T-3]"),
        ];
        let report = reconcile(&registry, &commits);
        assert_eq!(report.reconciled.len(), 2);
        assert_eq!(report.already_done, vec!["T-3"]);
        assert_eq!(report.commits_scanned, 3);
    }

    #[test]
    fn apply_report_transitions_tasks() {
        let mut registry = make_registry(vec![
            make_task("T-1", "todo"),
            make_task("T-2", "in_progress"),
        ]);
        let report = ReconcileReport {
            reconciled: vec![ReconciledTask {
                task_id: "T-1".into(),
                previous_state: "todo".into(),
                new_state: "merged".into(),
                matched_commit_sha: "abc".into(),
                matched_commit_subject: "feat: T-1".into(),
            }],
            already_done: vec![],
            external_committed_ids: vec![],
            commits_scanned: 1,
        };
        apply_reconcile_report(&mut registry, &report, "2026-03-17T12:00:00Z");
        assert_eq!(registry.tasks[0].state, "merged");
        assert_eq!(registry.tasks[1].state, "in_progress"); // untouched
    }

    #[test]
    fn no_commits_produces_empty_report() {
        let registry = make_registry(vec![make_task("T-1", "todo")]);
        let report = reconcile(&registry, &[]);
        assert_eq!(report.reconciled.len(), 0);
        assert_eq!(report.commits_scanned, 0);
    }

    #[test]
    fn reconcile_merge_commit() {
        let registry = make_registry(vec![make_task("WEB-001", "queued")]);
        let commits = vec![make_commit(
            "m1",
            "macc: WEB-001 - merge task WEB-001\n\n[macc:task WEB-001]\n[macc:merge true]",
        )];
        let report = reconcile(&registry, &commits);
        assert_eq!(report.reconciled.len(), 1);
        assert_eq!(report.reconciled[0].task_id, "WEB-001");
    }

    #[test]
    fn sync_unmerged_branches_merges_orphaned_branch() {
        let repo = make_test_repo();
        run_git(&repo, &["checkout", "-b", "macc/worker-01"]);
        create_commit(
            &repo,
            "feature.txt",
            "feature\n",
            "feat: L4-SYNC-001\n\n[macc:task L4-SYNC-001]",
        );
        run_git(&repo, &["checkout", "main"]);

        let mut registry = make_registry(vec![make_task("L4-SYNC-001", "todo")]);
        let results = sync_unmerged_branches(&mut registry, &repo, "main").expect("sync branches");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SyncBranchStatus::Merged);
        assert_eq!(results[0].merged_task_ids, vec!["L4-SYNC-001".to_string()]);
        assert_eq!(registry.tasks[0].state, "merged");
    }

    #[test]
    fn sync_unmerged_branches_conflict_is_skipped() {
        let repo = make_test_repo();
        run_git(&repo, &["checkout", "-b", "macc/worker-02"]);
        create_commit(
            &repo,
            "conflict.txt",
            "branch change\n",
            "feat: L4-SYNC-002\n\n[macc:task L4-SYNC-002]",
        );
        run_git(&repo, &["checkout", "main"]);
        create_commit(
            &repo,
            "conflict.txt",
            "main change\n",
            "chore: conflicting main update",
        );

        let mut registry = make_registry(vec![make_task("L4-SYNC-002", "todo")]);
        let results = sync_unmerged_branches(&mut registry, &repo, "main").expect("sync branches");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, SyncBranchStatus::MergeFailed);
        assert_eq!(registry.tasks[0].state, "todo");
    }

    #[test]
    fn sync_unmerged_branches_returns_empty_when_no_candidates() {
        let repo = make_test_repo();
        let mut registry = make_registry(vec![make_task("L4-SYNC-003", "todo")]);
        let results = sync_unmerged_branches(&mut registry, &repo, "main").expect("sync branches");
        assert!(results.is_empty());
        assert_eq!(registry.tasks[0].state, "todo");
    }
}

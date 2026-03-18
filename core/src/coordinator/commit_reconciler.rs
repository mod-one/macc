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
use std::collections::{BTreeMap, BTreeSet};
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
    /// Number of commits scanned.
    pub commits_scanned: usize,
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
            // Task ID in commit but not in registry — ignore.
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
pub fn apply_reconcile_report(registry: &mut TaskRegistry, report: &ReconcileReport, now: &str) {
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::model::{Task, TaskRuntime};

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
    fn reconcile_unknown_task_id_ignored() {
        let registry = make_registry(vec![make_task("WEB-001", "todo")]);
        let commits = vec![make_commit(
            "aaa111",
            "feat: UNKNOWN-999\n\n[macc:task UNKNOWN-999]",
        )];
        let report = reconcile(&registry, &commits);
        assert_eq!(report.reconciled.len(), 0);
        assert_eq!(report.already_done.len(), 0);
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
}

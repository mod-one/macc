//! AI-powered PRD audit: enriches `prd.json` with commit-derived context.
//!
//! Gathers commit history for completed tasks and builds a structured prompt
//! that an LLM can use to:
//! - Update "notes" of completed tasks with what was actually delivered.
//! - Rewrite "todo" task descriptions when integrated code changed the intended
//!   architecture (without deleting original task IDs).
//!
//! This module is pure business logic — no CLI, no UI, no tool invocation.
//! The caller is responsible for feeding the prompt to a tool.

use crate::commit_message;
use crate::coordinator::commit_reconciler::{self, GitCommitInfo};
use crate::coordinator::model::TaskRegistry;
use crate::git;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Commit context gathered for a single completed task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskCommitContext {
    pub task_id: String,
    /// Commits on the reference branch that mention this task.
    pub commits: Vec<CommitSummary>,
    /// Aggregated diff stat for all commits touching this task.
    pub diff_stat: Option<String>,
}

/// Lightweight commit summary (sha prefix + subject).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitSummary {
    pub sha_short: String,
    pub subject: String,
}

/// The full audit context ready to be serialized into a prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditContext {
    /// Current PRD JSON content (potentially truncated).
    pub prd_json: String,
    /// Per-task commit context for completed tasks.
    pub completed_tasks: Vec<TaskCommitContext>,
    /// Task IDs that are still in "todo" state.
    pub todo_task_ids: Vec<String>,
}

/// Result of the audit-prd operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditPrdReport {
    /// Number of completed tasks with commit context.
    pub completed_with_context: usize,
    /// Number of todo tasks included for potential rewrite.
    pub todo_tasks: usize,
    /// Whether the prompt was generated (false if no tasks found).
    pub prompt_generated: bool,
    /// The generated prompt (only when prompt_generated is true).
    pub prompt: Option<String>,
}

// ---------------------------------------------------------------------------
// Context gathering
// ---------------------------------------------------------------------------

/// Gather commit context for tasks marked as "merged" or "abandoned".
///
/// For each terminal task, finds commits on the reference branch that mention
/// its ID and optionally gathers a `git diff --stat` summary.
pub fn gather_task_commit_context(
    repo_root: &Path,
    registry: &TaskRegistry,
    commits: &[GitCommitInfo],
    include_diff_stat: bool,
) -> Result<Vec<TaskCommitContext>> {
    // Build a reverse index: task_id -> Vec<&GitCommitInfo>
    let mut task_commits: BTreeMap<String, Vec<&GitCommitInfo>> = BTreeMap::new();
    for commit in commits {
        let parsed = commit_message::parse(&commit.full_message);
        if let Some(ref task_id) = parsed.task_id {
            task_commits
                .entry(task_id.clone())
                .or_default()
                .push(commit);
        }
    }

    let mut contexts = Vec::new();
    for task in &registry.tasks {
        if !is_completed_state(&task.state) {
            continue;
        }
        let commits_for_task = task_commits.get(&task.id);
        if commits_for_task.is_none() || commits_for_task.unwrap().is_empty() {
            continue;
        }
        let task_commits_list = commits_for_task.unwrap();

        let summaries: Vec<CommitSummary> = task_commits_list
            .iter()
            .map(|c| CommitSummary {
                sha_short: c.sha[..7.min(c.sha.len())].to_string(),
                subject: c.subject.clone(),
            })
            .collect();

        let diff_stat = if include_diff_stat {
            gather_diff_stat_for_commits(repo_root, task_commits_list)
        } else {
            None
        };

        contexts.push(TaskCommitContext {
            task_id: task.id.clone(),
            commits: summaries,
            diff_stat,
        });
    }

    Ok(contexts)
}

/// Collect todo task IDs from the registry.
pub fn collect_todo_task_ids(registry: &TaskRegistry) -> Vec<String> {
    registry
        .tasks
        .iter()
        .filter(|t| t.state == "todo")
        .map(|t| t.id.clone())
        .collect()
}

/// Build the full audit context.
pub fn build_audit_context(
    prd_json: &str,
    registry: &TaskRegistry,
    completed_contexts: Vec<TaskCommitContext>,
) -> AuditContext {
    let todo_task_ids = collect_todo_task_ids(registry);
    AuditContext {
        prd_json: prd_json.to_string(),
        completed_tasks: completed_contexts,
        todo_task_ids,
    }
}

// ---------------------------------------------------------------------------
// Prompt generation
// ---------------------------------------------------------------------------

/// Maximum characters for the PRD JSON in the prompt to avoid context overflow.
const MAX_PRD_CHARS: usize = 80_000;

/// Build the LLM prompt from the audit context.
pub fn build_audit_prompt(context: &AuditContext) -> String {
    let mut prompt = String::with_capacity(4096);

    prompt.push_str("# MACC PRD Audit\n\n");
    prompt.push_str("You are an AI auditor for a project managed by MACC (Multi-Agentic Coding Config).\n");
    prompt.push_str("Your task is to update the PRD (Product Requirements Document) based on what was actually delivered.\n\n");

    prompt.push_str("## Instructions\n\n");
    prompt.push_str("1. For each **completed task** listed below, update its `notes` field to reflect what was actually delivered based on the commit history.\n");
    prompt.push_str("2. For **todo tasks**, review if the integrated code changes have modified the intended architecture. If so, rewrite the task `description` and `steps` to reflect the new reality. **Never delete or rename task IDs.**\n");
    prompt.push_str("3. Output the **complete updated `prd.json`** with your modifications. Preserve all existing fields and structure.\n");
    prompt.push_str("4. Only modify `notes` for completed tasks and `description`/`steps` for todo tasks whose context has changed.\n\n");

    // PRD content
    prompt.push_str("## Current PRD (`prd.json`)\n\n```json\n");
    if context.prd_json.chars().count() > MAX_PRD_CHARS {
        let truncated: String = context.prd_json.chars().take(MAX_PRD_CHARS).collect();
        prompt.push_str(&truncated);
        prompt.push_str("\n... [truncated]\n");
    } else {
        prompt.push_str(&context.prd_json);
    }
    prompt.push_str("\n```\n\n");

    // Completed task context
    if !context.completed_tasks.is_empty() {
        prompt.push_str("## Completed Task Commit Context\n\n");
        for tc in &context.completed_tasks {
            prompt.push_str(&format!("### Task `{}`\n\n", tc.task_id));
            prompt.push_str("Commits:\n");
            for commit in &tc.commits {
                prompt.push_str(&format!("- `{}` {}\n", commit.sha_short, commit.subject));
            }
            if let Some(ref stat) = tc.diff_stat {
                prompt.push_str("\nDiff stat:\n```\n");
                prompt.push_str(stat);
                prompt.push_str("\n```\n");
            }
            prompt.push('\n');
        }
    }

    // Todo tasks
    if !context.todo_task_ids.is_empty() {
        prompt.push_str("## Todo Tasks (review for architectural impact)\n\n");
        prompt.push_str("The following tasks are still `todo`. Based on the commits above, ");
        prompt.push_str("check if any completed work has changed the assumptions or architecture ");
        prompt.push_str("these tasks were designed around:\n\n");
        for id in &context.todo_task_ids {
            prompt.push_str(&format!("- `{}`\n", id));
        }
        prompt.push('\n');
    }

    prompt.push_str("## Output\n\n");
    prompt.push_str("Edit `prd.json` directly in the repository using your file-editing tools.\n");
    prompt.push_str("Do not print the full file content in your response.\n");
    prompt.push_str("At the end, print a short status line confirming the file was updated (e.g. `prd.json updated`).\n");

    prompt
}

/// High-level: gather everything and produce the audit report with prompt.
pub fn prepare_audit(
    repo_root: &Path,
    prd_json: &str,
    registry: &TaskRegistry,
    reference_branch: &str,
    include_diff_stat: bool,
) -> Result<AuditPrdReport> {
    let commits = commit_reconciler::read_commit_range(repo_root, None, reference_branch)?;

    let completed_contexts =
        gather_task_commit_context(repo_root, registry, &commits, include_diff_stat)?;
    let todo_ids = collect_todo_task_ids(registry);

    if completed_contexts.is_empty() && todo_ids.is_empty() {
        return Ok(AuditPrdReport {
            prompt_generated: false,
            ..Default::default()
        });
    }

    let context = build_audit_context(prd_json, registry, completed_contexts.clone());
    let prompt = build_audit_prompt(&context);

    Ok(AuditPrdReport {
        completed_with_context: completed_contexts.len(),
        todo_tasks: todo_ids.len(),
        prompt_generated: true,
        prompt: Some(prompt),
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_completed_state(state: &str) -> bool {
    matches!(state, "merged" | "abandoned")
}

/// Gather a combined `git diff --stat` for a set of commits.
///
/// Uses `git diff --stat <sha>^..<sha>` for each commit and concatenates.
/// Returns None if no stats could be gathered.
fn gather_diff_stat_for_commits(
    repo_root: &Path,
    commits: &[&GitCommitInfo],
) -> Option<String> {
    let mut stats = String::new();
    for commit in commits {
        let output = git::run_git_output_mapped(
            repo_root,
            &["diff", "--stat", &format!("{}^..{}", commit.sha, commit.sha)],
            "gather diff stat for audit",
        );
        if let Ok(out) = output {
            if out.status.success() {
                let stat = String::from_utf8_lossy(&out.stdout);
                let trimmed = stat.trim();
                if !trimmed.is_empty() {
                    if !stats.is_empty() {
                        stats.push('\n');
                    }
                    stats.push_str(trimmed);
                }
            }
        }
    }
    if stats.is_empty() {
        None
    } else {
        Some(stats)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::model::Task;

    fn make_task(id: &str, state: &str) -> Task {
        Task {
            id: id.to_string(),
            state: state.to_string(),
            title: Some(format!("Task {}", id)),
            ..Task::default()
        }
    }

    fn make_registry(tasks: Vec<Task>) -> TaskRegistry {
        TaskRegistry {
            tasks,
            ..TaskRegistry::default()
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

    #[test]
    fn gather_context_for_merged_tasks() {
        let registry = make_registry(vec![
            make_task("T-1", "merged"),
            make_task("T-2", "todo"),
            make_task("T-3", "merged"),
        ]);
        let commits = vec![
            make_commit("abc1234", "feat: T-1 - impl\n\n[macc:task T-1]"),
            make_commit("def5678", "feat: T-3 - setup\n\n[macc:task T-3]"),
            make_commit("ghi9012", "feat: T-2 - wip\n\n[macc:task T-2]"),
        ];
        let contexts =
            gather_task_commit_context(Path::new("."), &registry, &commits, false).unwrap();
        assert_eq!(contexts.len(), 2);
        assert_eq!(contexts[0].task_id, "T-1");
        assert_eq!(contexts[1].task_id, "T-3");
    }

    #[test]
    fn collect_todo_ids() {
        let registry = make_registry(vec![
            make_task("A-1", "todo"),
            make_task("A-2", "merged"),
            make_task("A-3", "todo"),
            make_task("A-4", "in_progress"),
        ]);
        let ids = collect_todo_task_ids(&registry);
        assert_eq!(ids, vec!["A-1", "A-3"]);
    }

    #[test]
    fn build_prompt_contains_prd_and_tasks() {
        let context = AuditContext {
            prd_json: r#"{"lot":"test","tasks":[]}"#.to_string(),
            completed_tasks: vec![TaskCommitContext {
                task_id: "X-1".to_string(),
                commits: vec![CommitSummary {
                    sha_short: "abc1234".to_string(),
                    subject: "feat: X-1 - do thing".to_string(),
                }],
                diff_stat: None,
            }],
            todo_task_ids: vec!["X-2".to_string()],
        };
        let prompt = build_audit_prompt(&context);
        assert!(prompt.contains("MACC PRD Audit"));
        assert!(prompt.contains(r#"{"lot":"test","tasks":[]}"#));
        assert!(prompt.contains("`X-1`"));
        assert!(prompt.contains("abc1234"));
        assert!(prompt.contains("`X-2`"));
    }

    #[test]
    fn empty_registry_produces_no_prompt() {
        let registry = make_registry(vec![]);
        let commits: Vec<GitCommitInfo> = vec![];
        let contexts =
            gather_task_commit_context(Path::new("."), &registry, &commits, false).unwrap();
        let todo = collect_todo_task_ids(&registry);
        assert!(contexts.is_empty());
        assert!(todo.is_empty());
    }

    #[test]
    fn prompt_truncates_large_prd() {
        let large_prd = "x".repeat(MAX_PRD_CHARS + 1000);
        let context = AuditContext {
            prd_json: large_prd,
            completed_tasks: vec![],
            todo_task_ids: vec!["T-1".to_string()],
        };
        let prompt = build_audit_prompt(&context);
        assert!(prompt.contains("[truncated]"));
    }

    #[test]
    fn gather_context_skips_tasks_without_commits() {
        let registry = make_registry(vec![
            make_task("T-1", "merged"),
            make_task("T-2", "merged"),
        ]);
        // Only T-1 has a matching commit
        let commits = vec![
            make_commit("abc1234", "feat: T-1 - impl\n\n[macc:task T-1]"),
        ];
        let contexts =
            gather_task_commit_context(Path::new("."), &registry, &commits, false).unwrap();
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].task_id, "T-1");
    }

    #[test]
    fn multiple_commits_per_task() {
        let registry = make_registry(vec![make_task("T-1", "merged")]);
        let commits = vec![
            make_commit("aaa1111", "feat: T-1 - part 1\n\n[macc:task T-1]"),
            make_commit("bbb2222", "fix: T-1 - patch\n\n[macc:task T-1]"),
        ];
        let contexts =
            gather_task_commit_context(Path::new("."), &registry, &commits, false).unwrap();
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].commits.len(), 2);
        assert_eq!(contexts[0].commits[0].sha_short, "aaa1111");
        assert_eq!(contexts[0].commits[1].sha_short, "bbb2222");
    }
}

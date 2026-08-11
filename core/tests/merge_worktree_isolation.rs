//! Regression tests: coordinator merges must never mutate the operator's
//! working tree.
//!
//! Before the integration worktree existed, `merge_task_with_policy_native`
//! ran `git checkout <base>` in the repository root and merged there. That
//! moved the operator's HEAD, blocked every merge whenever they had
//! uncommitted work, and could leave conflict state in their checkout. These
//! tests pin the new behaviour at the public entry point.

use macc_core::coordinator::engine::MergeTaskContext;
use macc_core::coordinator::runtime::merge_task_with_policy_native;
use std::path::Path;
use std::process::Command;

fn run_git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn make_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = dir.path();
    run_git(repo, &["init", "-q", "-b", "main", "."]);
    run_git(repo, &["config", "user.email", "test@example.com"]);
    run_git(repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("base.txt"), "base\n").expect("write");
    // Mirror what `macc init` does: `.macc/` holds coordinator logs and state
    // and is always ignored (see `BASELINE_IGNORE_ENTRIES`).
    std::fs::write(repo.join(".gitignore"), ".macc/\n").expect("write");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-qm", "init"]);
    dir
}

/// Commit `contents` to `file` on a new `branch` off main, then return to main.
fn add_task_branch(repo: &Path, branch: &str, file: &str, contents: &str) {
    run_git(repo, &["checkout", "-q", "-b", branch]);
    std::fs::write(repo.join(file), contents).expect("write");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-qm", "task work"]);
    run_git(repo, &["checkout", "-q", "main"]);
}

fn merge_context() -> MergeTaskContext {
    MergeTaskContext {
        tool: "test".into(),
        worktree_path: String::new(),
        title: "test task".into(),
        description: String::new(),
        objective: String::new(),
    }
}

fn merge(repo: &Path, task_id: &str, branch: &str) -> std::result::Result<(), String> {
    merge_task_with_policy_native(
        repo,
        task_id,
        branch,
        "main",
        false,
        None,
        &merge_context(),
        |_, _, _, _, _, _| {},
    )
    .expect("merge should not error")
}

#[test]
fn merge_does_not_move_the_operators_checked_out_branch() {
    let dir = make_repo();
    let repo = dir.path();
    add_task_branch(repo, "task/t1", "work.txt", "work\n");
    // Operator is working on their own branch, not the base.
    run_git(repo, &["checkout", "-q", "-b", "operator/wip"]);

    merge(repo, "T1", "task/t1").expect("merge should succeed");

    assert_eq!(
        git_stdout(repo, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "operator/wip",
        "the coordinator must not move the operator's HEAD"
    );
    assert!(
        !repo.join("work.txt").exists(),
        "the operator's branch must not gain the merged files"
    );
    // ...but the base branch really did advance.
    let merged = git_stdout(repo, &["log", "--oneline", "main", "--", "work.txt"]);
    assert!(!merged.is_empty(), "base branch should contain merged work");
}

#[test]
fn uncommitted_operator_work_no_longer_blocks_every_merge() {
    let dir = make_repo();
    let repo = dir.path();
    add_task_branch(repo, "task/t1", "work.txt", "work\n");
    // Operator has WIP on a file untouched by the merge. Under the old
    // `precheck_clean` gate this failed with `step=precheck_clean`.
    std::fs::write(repo.join("base.txt"), "operator wip\n").expect("write");

    merge(repo, "T1", "task/t1").expect("merge should succeed despite unrelated WIP");

    assert_eq!(
        std::fs::read_to_string(repo.join("base.txt")).expect("read"),
        "operator wip\n",
        "operator WIP must be preserved"
    );
    assert!(
        repo.join("work.txt").exists(),
        "base checkout should fast-forward to the merged commit"
    );
}

#[test]
fn overlapping_operator_work_fails_with_an_actionable_reason() {
    let dir = make_repo();
    let repo = dir.path();
    // The task branch edits the very file the operator has uncommitted.
    add_task_branch(repo, "task/t1", "base.txt", "task side\n");
    std::fs::write(repo.join("base.txt"), "operator wip\n").expect("write");
    let main_before = git_stdout(repo, &["rev-parse", "main"]);

    let err = merge(repo, "T1", "task/t1").expect_err("merge should be blocked");

    assert!(
        err.contains("base_checked_out_dirty"),
        "failure must name the real cause, got: {err}"
    );
    assert!(
        err.contains("commit or stash"),
        "failure should tell the operator what to do, got: {err}"
    );
    assert_eq!(
        git_stdout(repo, &["rev-parse", "main"]),
        main_before,
        "base must not move when publishing is blocked"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("base.txt")).expect("read"),
        "operator wip\n",
        "operator WIP must survive"
    );
}

#[test]
fn conflicting_merge_leaves_the_operator_checkout_clean() {
    let dir = make_repo();
    let repo = dir.path();
    add_task_branch(repo, "task/t1", "base.txt", "task side\n");
    // Base diverges, so merging the task branch conflicts.
    std::fs::write(repo.join("base.txt"), "main side\n").expect("write");
    run_git(repo, &["add", "."]);
    run_git(repo, &["commit", "-qm", "main side"]);
    let main_before = git_stdout(repo, &["rev-parse", "main"]);

    let err = merge(repo, "T1", "task/t1").expect_err("merge should conflict");
    assert!(err.contains("failure:local_merge"), "got: {err}");

    assert!(
        git_stdout(repo, &["status", "--porcelain"]).is_empty(),
        "a conflicted merge must not leave conflict state in the operator's tree"
    );
    assert!(
        git_stdout(repo, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_empty(),
        "no merge should be left in progress in the operator's tree"
    );
    assert_eq!(
        git_stdout(repo, &["rev-parse", "main"]),
        main_before,
        "base must not move on a conflicted merge"
    );
}

/// The end-of-run symptom, exercised along the real chain:
/// merge → `apply_merge_result_in_registry` → `counts()` → loop decision.
///
/// With the old `precheck_clean` gate, any uncommitted file in the repo root
/// made the merge fail without ever being attempted. `apply_merge_failure_typed`
/// then set the task to `Blocked`, so the next cycle saw `todo=0, active=0,
/// blocked=1` and ended the run with "finished with blocked tasks" — no active
/// or pending tasks left, and the branch never merged.
#[test]
fn final_task_merges_before_the_run_completes_even_with_a_dirty_tree() {
    use macc_core::coordinator::engine::{
        apply_merge_result_in_registry, ControlPlaneDecision, ControlPlaneLoopConfig,
        CoordinatorCounts, CoordinatorRunController,
    };
    use macc_core::coordinator::model::{Task, TaskRegistry};
    use macc_core::coordinator::WorkflowState;

    let dir = make_repo();
    let repo = dir.path();
    add_task_branch(repo, "task/t1", "work.txt", "work\n");
    // The operator left an untracked file behind — `git status --porcelain`
    // reports `??`, which the old gate treated as "dirty" and refused on.
    std::fs::write(repo.join("notes.md"), "scratch\n").expect("write");

    // The last remaining task is merge-ready.
    let mut task = Task {
        id: "T1".to_string(),
        ..Default::default()
    };
    task.set_workflow_state(WorkflowState::Reviewing);
    let mut registry = TaskRegistry {
        tasks: vec![task],
        ..Default::default()
    }
    .to_value()
    .expect("registry to value");

    // While the merge is in flight the task must still count as active, or the
    // loop would complete and abandon it.
    let (_, todo, active, _, _) = TaskRegistry::from_value(&registry).expect("typed").counts();
    assert_eq!(
        (todo, active),
        (0, 1),
        "a merge-ready task must count as active so the run cannot finish under it"
    );

    let result = merge(repo, "T1", "task/t1");
    assert!(result.is_ok(), "merge should succeed: {result:?}");

    apply_merge_result_in_registry(
        &mut registry,
        "T1",
        result.is_ok(),
        "",
        "2026-01-01T00:00:00Z",
    )
    .expect("apply merge result");

    let typed = TaskRegistry::from_value(&registry).expect("typed");
    assert_eq!(
        typed.tasks[0].workflow_state(),
        Some(WorkflowState::Merged),
        "the task must end merged, not blocked"
    );

    let (total, todo, active, blocked, merged) = typed.counts();
    let mut controller = CoordinatorRunController::new(ControlPlaneLoopConfig {
        timeout: None,
        max_no_progress_cycles: 5,
    });
    let decision = controller
        .on_cycle_counts(
            CoordinatorCounts {
                total,
                todo,
                active,
                blocked,
                merged,
            },
            None,
        )
        .expect("run should complete cleanly, not error with blocked tasks");
    assert_eq!(decision, ControlPlaneDecision::Complete);

    // And the work really is on the base branch.
    assert!(
        !git_stdout(repo, &["log", "--oneline", "main", "--", "work.txt"]).is_empty(),
        "merged work must be reachable from the base branch"
    );
}

/// Documents *why* a failed merge on the last task was so visible: it ends the
/// whole run. This is the mechanism behind "the merge never happened once the
/// run finished" — the old `precheck_clean` gate turned any dirty working tree
/// into this outcome. The chain itself is correct and still in place; only its
/// trigger was wrong.
#[test]
fn a_failed_merge_on_the_last_task_ends_the_run_with_blocked_tasks() {
    use macc_core::coordinator::engine::{
        apply_merge_result_in_registry, ControlPlaneLoopConfig, CoordinatorCounts,
        CoordinatorRunController,
    };
    use macc_core::coordinator::model::{Task, TaskRegistry};
    use macc_core::coordinator::WorkflowState;

    let mut task = Task {
        id: "T1".to_string(),
        ..Default::default()
    };
    task.set_workflow_state(WorkflowState::Reviewing);
    let mut registry = TaskRegistry {
        tasks: vec![task],
        ..Default::default()
    }
    .to_value()
    .expect("registry to value");

    apply_merge_result_in_registry(
        &mut registry,
        "T1",
        false,
        "failure:local_merge step=merge",
        "2026-01-01T00:00:00Z",
    )
    .expect("apply merge result");

    let typed = TaskRegistry::from_value(&registry).expect("typed");
    assert_eq!(
        typed.tasks[0].workflow_state(),
        Some(WorkflowState::Blocked),
        "a failed merge blocks the task"
    );

    let (total, todo, active, blocked, merged) = typed.counts();
    assert_eq!((todo, active, blocked), (0, 0, 1));

    let mut controller = CoordinatorRunController::new(ControlPlaneLoopConfig {
        timeout: None,
        max_no_progress_cycles: 5,
    });
    let err = controller
        .on_cycle_counts(
            CoordinatorCounts {
                total,
                todo,
                active,
                blocked,
                merged,
            },
            None,
        )
        .expect_err("the run must end, not spin");
    assert!(err.to_string().contains("blocked tasks"), "got: {err}");
}

#[test]
fn integration_worktree_is_invisible_to_the_operators_status() {
    let dir = make_repo();
    let repo = dir.path();
    add_task_branch(repo, "task/t1", "work.txt", "work\n");

    merge(repo, "T1", "task/t1").expect("merge should succeed");

    // The integration worktree lives under the git common dir, so it must not
    // appear as untracked content regardless of the project's .gitignore.
    assert!(
        git_stdout(repo, &["status", "--porcelain"]).is_empty(),
        "integration worktree must not show up in git status"
    );
}

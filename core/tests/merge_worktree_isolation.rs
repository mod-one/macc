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

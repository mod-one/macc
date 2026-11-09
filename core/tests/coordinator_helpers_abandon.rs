use macc_core::coordinator::helpers::find_reusable_worktree_native;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

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

fn run_git_capture(repo: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn create_commit(repo: &Path, file: &str, content: &str, message: &str) {
    fs::write(repo.join(file), content).expect("write file");
    run_git(repo, &["add", file]);
    run_git(repo, &["commit", "-m", message]);
}

fn make_clone_with_origin() -> (PathBuf, PathBuf) {
    let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "macc-coordinator-abandon-tests-{}-{}-{}",
        std::process::id(),
        nanos,
        suffix
    ));
    let origin = base.join("origin.git");
    let repo = base.join("repo");

    fs::create_dir_all(&base).expect("create base dir");
    run_git(
        &base,
        &["init", "--bare", origin.to_string_lossy().as_ref()],
    );
    run_git(
        &base,
        &[
            "clone",
            origin.to_string_lossy().as_ref(),
            repo.to_string_lossy().as_ref(),
        ],
    );

    run_git(&repo, &["config", "user.email", "tests@example.com"]);
    run_git(&repo, &["config", "user.name", "MACC Tests"]);
    fs::write(repo.join(".gitignore"), ".macc/\n").expect("write ignore");
    run_git(&repo, &["add", ".gitignore"]);
    create_commit(&repo, "base.txt", "base\n", "chore: base");
    run_git(&repo, &["branch", "-M", "main"]);
    run_git(&repo, &["push", "-u", "origin", "main"]);

    (repo, base)
}

fn make_pool_worktree(repo: &Path, task_id: &str) -> PathBuf {
    let pool_root = repo.join(".macc").join("worktree");
    fs::create_dir_all(&pool_root).expect("create pool root");
    let worktree = pool_root.join("worker-01");
    let branch = format!("ai/codex/{task_id}");
    run_git(
        repo,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            worktree.to_string_lossy().as_ref(),
            "main",
        ],
    );

    run_git(&worktree, &["config", "user.email", "tests@example.com"]);
    run_git(&worktree, &["config", "user.name", "MACC Tests"]);
    fs::create_dir_all(worktree.join(".macc")).unwrap();
    fs::write(
        worktree.join(".macc/worktree.json"),
        serde_json::json!({
            "id":"worker-01","slug":"worker-01","tool":"codex","base":"main","branch":&branch
        })
        .to_string(),
    )
    .unwrap();
    worktree
}

fn abandoned_registry(task_id: &str, worktree_path: &Path) -> serde_json::Value {
    serde_json::json!({
        "tasks": [
            {
                "id": task_id,
                "state": "abandoned",
                "worktree": {
                    "worktree_path": worktree_path.to_string_lossy().to_string()
                }
            }
        ]
    })
}

#[test]
fn abandoned_worktree_with_unmerged_commits_is_preserved_without_recycling() {
    let (repo, _base) = make_clone_with_origin();
    let task_id = "L4-ABANDON-001";
    let worktree = make_pool_worktree(&repo, task_id);

    create_commit(
        &worktree,
        "ahead.txt",
        "ahead\n",
        "feat: work that must be preserved",
    );
    let ahead_head = run_git_capture(&worktree, &["rev-parse", "HEAD"]);

    let registry = abandoned_registry(task_id, &worktree);
    let activity = HashMap::new();
    let (reused, prep_error) =
        find_reusable_worktree_native(&repo, &registry, "codex", "main", 300, &activity)
            .expect("reuse result");

    assert!(reused.is_none(), "unmerged work must remain in its slot");
    assert_eq!(prep_error.unwrap().0, "previous_branch_not_merged");
    assert_eq!(
        run_git_capture(&worktree, &["rev-parse", "HEAD"]),
        ahead_head
    );
    assert_eq!(
        fs::read_to_string(worktree.join("ahead.txt")).unwrap(),
        "ahead\n"
    );
    let tags = run_git_capture(
        &repo,
        &["tag", "--list", &format!("macc/abandoned/{task_id}-*")],
    );
    assert!(
        tags.is_empty(),
        "reuse must not silently abandon retained work"
    );
}

#[test]
fn reused_worktree_without_commits_does_not_create_abandonment_tag() {
    let (repo, _base) = make_clone_with_origin();
    let task_id = "L4-ABANDON-002";
    let worktree = make_pool_worktree(&repo, task_id);

    let registry = abandoned_registry(task_id, &worktree);
    let activity = HashMap::new();
    let (reused, prep_error) =
        find_reusable_worktree_native(&repo, &registry, "codex", "main", 300, &activity)
            .expect("reuse result");

    assert!(
        prep_error.is_none(),
        "unexpected prep error: {prep_error:?}"
    );
    assert!(reused.is_some(), "expected reusable worktree");

    let tags = run_git_capture(
        &repo,
        &["tag", "--list", &format!("macc/abandoned/{task_id}-*")],
    );
    assert!(
        tags.trim().is_empty(),
        "no abandonment tag should be created"
    );
}

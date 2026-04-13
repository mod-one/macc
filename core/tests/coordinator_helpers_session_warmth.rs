use macc_core::coordinator::helpers::find_reusable_worktree_native;
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
        "macc-coordinator-session-tests-{}-{}-{}",
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
    create_commit(&repo, "base.txt", "base\n", "chore: base");
    run_git(&repo, &["branch", "-M", "main"]);
    run_git(&repo, &["push", "-u", "origin", "main"]);

    (repo, base)
}

fn add_pool_worktree(repo: &Path, slot: &str, task_id: &str) -> PathBuf {
    let pool_root = repo.join(".macc").join("worktree");
    fs::create_dir_all(&pool_root).expect("create pool root");
    let worktree = pool_root.join(slot);
    let branch = format!("task/{task_id}");
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
    worktree
}

fn write_tool_sessions(
    repo: &Path,
    tool_id: &str,
    worktree_path: &Path,
    session_id: &str,
    updated_at: &str,
) {
    let state_dir = repo.join(".macc/state");
    fs::create_dir_all(&state_dir).expect("create state dir");
    let payload = serde_json::json!({
        "tools": {
            tool_id: {
                "sessions": {
                    worktree_path.to_string_lossy().to_string(): {
                        "session_id": session_id,
                        "updated_at": updated_at
                    }
                },
                "leases": {
                    session_id: { "status": "active" }
                }
            }
        }
    });
    fs::write(
        state_dir.join("tool-sessions.json"),
        serde_json::to_string_pretty(&payload).expect("serialize"),
    )
    .expect("write tool sessions");
}

#[test]
fn warm_slot_is_preferred_over_cold_slot() {
    let (repo, _base) = make_clone_with_origin();
    let wt1 = add_pool_worktree(&repo, "worker-01", "L4-SES-WARM-001");
    let wt2 = add_pool_worktree(&repo, "worker-02", "L4-SES-WARM-002");
    let fresh = chrono::Utc::now()
        .checked_sub_signed(chrono::TimeDelta::seconds(30))
        .expect("fresh ts")
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    write_tool_sessions(&repo, "codex", &wt2, "sid-warm", &fresh);

    let registry = serde_json::json!({ "tasks": [] });
    let (reused, prep_error) =
        find_reusable_worktree_native(&repo, &registry, "codex", "main", 300)
            .expect("reuse result");

    assert!(
        prep_error.is_none(),
        "unexpected prep error: {prep_error:?}"
    );
    let (picked, _, _, _, _) = reused.expect("expected reusable worktree");
    assert_eq!(picked, wt2, "warm worktree should be selected first");
    assert_ne!(picked, wt1);
}

#[test]
fn expired_session_is_treated_as_cold() {
    let (repo, _base) = make_clone_with_origin();
    let wt1 = add_pool_worktree(&repo, "worker-01", "L4-SES-COLD-001");
    let wt2 = add_pool_worktree(&repo, "worker-02", "L4-SES-COLD-002");
    let expired = chrono::Utc::now()
        .checked_sub_signed(chrono::TimeDelta::seconds(3600))
        .expect("expired ts")
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    write_tool_sessions(&repo, "codex", &wt2, "sid-expired", &expired);

    let registry = serde_json::json!({ "tasks": [] });
    let (reused, prep_error) =
        find_reusable_worktree_native(&repo, &registry, "codex", "main", 300)
            .expect("reuse result");

    assert!(
        prep_error.is_none(),
        "unexpected prep error: {prep_error:?}"
    );
    let (picked, _, _, _, _) = reused.expect("expected reusable worktree");
    assert_eq!(picked, wt1, "expired session must not be treated as warm");
}

#[test]
fn no_sessions_file_falls_back_to_default_slot_order() {
    let (repo, _base) = make_clone_with_origin();
    let wt1 = add_pool_worktree(&repo, "worker-01", "L4-SES-NOSESS-001");
    let _wt2 = add_pool_worktree(&repo, "worker-02", "L4-SES-NOSESS-002");
    let registry = serde_json::json!({ "tasks": [] });

    let (reused, prep_error) =
        find_reusable_worktree_native(&repo, &registry, "codex", "main", 300)
            .expect("reuse result");

    assert!(
        prep_error.is_none(),
        "unexpected prep error: {prep_error:?}"
    );
    let (picked, _, _, _, _) = reused.expect("expected reusable worktree");
    assert_eq!(picked, wt1, "without sessions, selection should fallback");
}

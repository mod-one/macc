use chrono::{SecondsFormat, TimeDelta, Utc};
use macc_core::config::CanonicalConfig;
use macc_core::coordinator::commit_reconciler::{sync_unmerged_branches, SyncBranchStatus};
use macc_core::coordinator::control_plane::{
    dispatch_ready_tasks_native, run_phase_for_task_native,
};
use macc_core::coordinator::engine::{
    apply_job_completion_in_registry, JobCompletionInput, NormalizerInput,
};
use macc_core::coordinator::error_normalizer::NormalizerRegistry;
use macc_core::coordinator::helpers::find_reusable_worktree_native;
use macc_core::coordinator::runtime::CoordinatorRunState;
use macc_core::coordinator::state::{
    coordinator_state_registry_load, coordinator_state_registry_save,
};
use macc_core::coordinator::types::CoordinatorEnvConfig;
use macc_core::coordinator::{model::TaskRegistry, PerformerCompletionKind};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

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

fn make_test_repo(prefix: &str) -> (PathBuf, PathBuf) {
    let suffix = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "macc-coordinator-reliability-{}-{}-{}-{}",
        prefix,
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
    create_commit(&repo, "README.md", "base\n", "chore: base");
    run_git(&repo, &["branch", "-M", "main"]);
    run_git(&repo, &["push", "-u", "origin", "main"]);
    (repo, base)
}

fn add_pool_worktree(repo: &Path, slot: &str, branch: &str) -> PathBuf {
    let pool_root = repo.join(".macc").join("worktree");
    fs::create_dir_all(&pool_root).expect("create pool root");
    let worktree = pool_root.join(slot);
    run_git(
        repo,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            worktree.to_string_lossy().as_ref(),
            "main",
        ],
    );
    run_git(&worktree, &["config", "user.email", "tests@example.com"]);
    run_git(&worktree, &["config", "user.name", "MACC Tests"]);
    worktree
}

fn write_tool_sessions(repo: &Path, tool_id: &str, session_id: &str, updated_at: &str) {
    // Pool format: session keyed by session_id (not by worktree path).
    let state_dir = repo.join(".macc/state");
    fs::create_dir_all(&state_dir).expect("create state dir");
    let payload = json!({
        "tools": {
            tool_id: {
                "sessions": {
                    session_id: {
                        "status": "available",
                        "created_at": updated_at,
                        "updated_at": updated_at,
                        "last_used_at": updated_at
                    }
                }
            }
        }
    });
    fs::write(
        state_dir.join("tool-sessions.json"),
        serde_json::to_string_pretty(&payload).expect("serialize"),
    )
    .expect("write sessions");
}

fn write_registry(repo: &Path, registry: &serde_json::Value) {
    coordinator_state_registry_save(repo, &BTreeMap::new(), registry).expect("save registry");
}

fn read_registry(repo: &Path) -> serde_json::Value {
    coordinator_state_registry_load(repo, &BTreeMap::new()).expect("load registry")
}

#[test]
fn coordinator_reliability_chain_integration() {
    let normalizers = NormalizerRegistry::empty();
    let now = "2026-04-13T00:00:00Z";

    // (1), (2), (6): phase_done override + error_with_changes retry + session carry-over.
    let (repo_a, _base_a) = make_test_repo("chain-a");
    let task_id_a = "L4-INT-001-A";
    let branch_a = format!("task/{task_id_a}");
    let worktree_a = add_pool_worktree(&repo_a, "worker-01", &branch_a);
    create_commit(
        &worktree_a,
        "feature.txt",
        "first pass\n",
        "feat: first pass\n\n[macc:task L4-INT-001-A]",
    );

    let mut registry_a = json!({
        "tasks": [
            {
                "id": task_id_a,
                "state": "claimed",
                "base_branch": "main",
                "tool": "codex",
                "coordinator_tool": "codex",
                "worktree": {
                    "worktree_path": worktree_a.to_string_lossy().to_string(),
                    "branch": branch_a,
                    "base_branch": "main"
                },
                "task_runtime": {
                    "status": "running",
                    "current_phase": "dev",
                    "active_session_id": "sid-chain"
                }
            }
        ]
    });

    let phase_done = apply_job_completion_in_registry(
        &mut registry_a,
        task_id_a,
        &JobCompletionInput {
            success: false,
            attempt: 1,
            max_attempts: 1,
            timed_out: false,
            phase_timeout_seconds: 300,
            elapsed_seconds: 3,
            status_text: "non-zero exit with IPC phase_done".to_string(),
            completion_kind: Some(PerformerCompletionKind::SuccessWithChanges),
            error_code: None,
            error_origin: None,
            error_message: None,
            auto_retry_error_codes: Vec::new(),
            auto_retry_max: 0,
            backoff_base_seconds: 0,
            backoff_max_seconds: 0,
            normalizer_input: None,
        },
        &normalizers,
        now,
    )
    .expect("phase_done completion");
    assert_eq!(phase_done.status_label, "phase_done");
    let typed_phase_done = TaskRegistry::from_value(&registry_a).expect("typed");
    let phase_done_task = typed_phase_done.find_task(task_id_a).expect("task");
    assert_eq!(phase_done_task.state, "in_progress");
    assert_eq!(
        phase_done_task.task_runtime.status.as_deref(),
        Some("phase_done")
    );

    // Reset to a retry-eligible running state for the explicit error_with_changes path.
    registry_a["tasks"][0]["state"] = json!("claimed");
    registry_a["tasks"][0]["task_runtime"]["status"] = json!("running");
    registry_a["tasks"][0]["task_runtime"]["current_phase"] = json!("dev");

    let with_changes = apply_job_completion_in_registry(
        &mut registry_a,
        task_id_a,
        &JobCompletionInput {
            success: false,
            attempt: 1,
            max_attempts: 2,
            timed_out: false,
            phase_timeout_seconds: 300,
            elapsed_seconds: 5,
            status_text: "performer reported error_with_changes".to_string(),
            completion_kind: None,
            error_code: None,
            error_origin: None,
            error_message: None,
            auto_retry_error_codes: Vec::new(),
            auto_retry_max: 0,
            backoff_base_seconds: 0,
            backoff_max_seconds: 0,
            normalizer_input: Some(NormalizerInput {
                exit_code: 1,
                stderr: "simulated error".to_string(),
                stdout: "MACC_TASK_RESULT: error_with_changes".to_string(),
            }),
        },
        &normalizers,
        now,
    )
    .expect("error_with_changes completion");
    assert_eq!(with_changes.status_label, "error_with_changes");

    let typed_a = TaskRegistry::from_value(&registry_a).expect("typed registry");
    let retry_task = typed_a.find_task(task_id_a).expect("retry task");
    assert_eq!(retry_task.state, "todo");
    assert_eq!(
        retry_task.worktree_path(),
        Some(worktree_a.to_string_lossy().as_ref())
    );
    assert_eq!(
        retry_task.task_runtime.last_session_id.as_deref(),
        Some("sid-chain")
    );
    assert_eq!(
        retry_task.task_runtime.last_session_tool.as_deref(),
        Some("codex")
    );

    // Ensure preserved session is injected into the next performer invocation.
    let capture_path = repo_a.join("runner-args.txt");
    let runner_path = repo_a.join("mock.performer.sh");
    fs::write(
        &runner_path,
        format!(
            "#!/usr/bin/env bash\nset -euo pipefail\nprintf '%s\\n' \"$@\" > \"{}\"\necho \"mock runner ok\"\n",
            capture_path.display()
        ),
    )
    .expect("write mock runner");
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&runner_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&runner_path, perms).expect("chmod runner");
    }
    fs::create_dir_all(worktree_a.join(".macc")).expect("create worktree .macc");
    fs::write(
        worktree_a.join(".macc/tool.json"),
        json!({
            "id": "codex",
            "performer": {
                "runner": runner_path.to_string_lossy().to_string()
            }
        })
        .to_string(),
    )
    .expect("write tool.json");
    fs::create_dir_all(repo_a.join(".macc/state")).expect("create coordinator state");
    fs::write(
        repo_a.join(".macc/state/coordinator.ipc.addr"),
        "127.0.0.1:65535\n",
    )
    .expect("write coordinator ipc addr");

    let run_out = run_phase_for_task_native(&repo_a, retry_task, "dev", Some("codex"), 1, None)
        .expect("phase run should execute");
    assert!(run_out.is_ok(), "phase should succeed: {run_out:?}");

    let captured = fs::read_to_string(&capture_path).expect("read runner args");
    assert!(
        captured.contains("--session-id\nsid-chain"),
        "runner args should include preserved session id, got: {}",
        captured
    );

    // (3): abandoned branch gets tagged before recycling.
    let (repo_b, _base_b) = make_test_repo("chain-b");
    let task_id_b = "L4-INT-001-B";
    let worktree_b = add_pool_worktree(&repo_b, "worker-01", &format!("task/{task_id_b}"));
    create_commit(
        &worktree_b,
        "ahead.txt",
        "ahead\n",
        "feat: abandoned work\n\n[macc:task L4-INT-001-B]",
    );
    let abandoned_head = run_git_capture(&worktree_b, &["rev-parse", "HEAD"]);
    let registry_b = json!({
        "tasks": [
            {
                "id": task_id_b,
                "state": "abandoned",
                "worktree": {
                    "worktree_path": worktree_b.to_string_lossy().to_string()
                }
            }
        ]
    });
    let (reused_b, prep_error_b) = find_reusable_worktree_native(
        &repo_b,
        &registry_b,
        "codex",
        "main",
        300,
        &std::collections::HashMap::new(),
    )
    .expect("reuse abandoned worktree");
    assert!(prep_error_b.is_none());
    assert!(reused_b.is_some());
    let tags_b = run_git_capture(
        &repo_b,
        &["tag", "--list", &format!("macc/abandoned/{task_id_b}-*")],
    );
    let abandoned_tag = tags_b
        .lines()
        .find(|line| !line.trim().is_empty())
        .expect("abandoned tag exists");
    let tagged_head = run_git_capture(
        &repo_b,
        &["rev-parse", &format!("refs/tags/{}", abandoned_tag)],
    );
    assert_eq!(tagged_head, abandoned_head);

    // (4): orphaned branch discovered during sync and merged.
    let (repo_c, _base_c) = make_test_repo("chain-c");
    run_git(&repo_c, &["checkout", "-b", "macc/worker-01"]);
    create_commit(
        &repo_c,
        "sync.txt",
        "sync\n",
        "feat: sync\n\n[macc:task L4-INT-001-C]",
    );
    run_git(&repo_c, &["checkout", "main"]);
    let mut registry_c = TaskRegistry::from_value(&json!({
        "tasks": [
            {
                "id": "L4-INT-001-C",
                "state": "todo"
            }
        ]
    }))
    .expect("typed registry");
    let sync_results = sync_unmerged_branches(&mut registry_c, &repo_c, "main").expect("sync");
    assert_eq!(sync_results.len(), 1);
    assert_eq!(sync_results[0].status, SyncBranchStatus::Merged);
    assert_eq!(registry_c.tasks[0].state, "merged");

    // (5): merge-gate merges retry branch before dispatch to prevent duplicate dispatch.
    let (repo_d, _base_d) = make_test_repo("chain-d");
    run_git(&repo_d, &["checkout", "-b", "task/L4-INT-001-D"]);
    create_commit(
        &repo_d,
        "merge-gate.txt",
        "merged by gate\n",
        "feat: retry branch\n\n[macc:task L4-INT-001-D]",
    );
    run_git(&repo_d, &["checkout", "main"]);
    let registry_d = json!({
        "tasks": [
            {
                "id": "L4-INT-001-D",
                "state": "todo",
                "base_branch": "main",
                "coordinator_tool": "codex",
                "task_runtime": {
                    "retries": 1,
                    "metrics": {
                        "retries": 1
                    }
                }
            }
        ]
    });
    write_registry(&repo_d, &registry_d);
    let prd_path_d = repo_d.join("worktree.prd.json");
    fs::write(&prd_path_d, "{\"tasks\":[]}\n").expect("write prd");

    let mut canonical = CanonicalConfig::default();
    canonical.tools.enabled = vec!["codex".to_string()];
    let env_cfg = CoordinatorEnvConfig {
        max_dispatch: Some(1),
        max_parallel: Some(1),
        reference_branch: Some("main".to_string()),
        ..Default::default()
    };
    let mut run_state = CoordinatorRunState::new();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .enable_io()
        .build()
        .expect("tokio runtime");
    let dispatched = runtime
        .block_on(dispatch_ready_tasks_native(
            &repo_d,
            &canonical,
            None,
            &env_cfg,
            &prd_path_d,
            &mut run_state,
            None,
        ))
        .expect("dispatch");
    assert_eq!(dispatched, 0, "merge-gate should cancel dispatch");
    let registry_d_after = TaskRegistry::from_value(&read_registry(&repo_d)).expect("typed");
    let merged_task = registry_d_after
        .find_task("L4-INT-001-D")
        .expect("merged task");
    assert_eq!(merged_task.state, "merged");

    // (7): slot with recent activity preferred when pool has a warm session.
    // In the pool model, session warmth is tool-level; the tiebreak is recency.
    let (repo_e, _base_e) = make_test_repo("chain-e");
    let cold_wt = add_pool_worktree(&repo_e, "worker-01", "task/L4-INT-001-E-COLD");
    let warm_wt = add_pool_worktree(&repo_e, "worker-02", "task/L4-INT-001-E-WARM");
    let fresh = Utc::now()
        .checked_sub_signed(TimeDelta::seconds(15))
        .expect("fresh timestamp")
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    write_tool_sessions(&repo_e, "codex", "sid-warm", &fresh);
    let registry_e = json!({ "tasks": [] });
    // Give warm_wt recent activity so it wins the tiebreak.
    let mut activity_e = std::collections::HashMap::new();
    activity_e.insert(
        warm_wt.to_string_lossy().to_string(),
        chrono::Utc::now().timestamp() - 10,
    );
    let (reused_e, prep_error_e) =
        find_reusable_worktree_native(&repo_e, &registry_e, "codex", "main", 300, &activity_e)
            .expect("reuse scan");
    assert!(prep_error_e.is_none());
    let (picked, _, _, _, _) = reused_e.expect("expected reusable worktree");
    assert_eq!(picked, warm_wt);
    assert_ne!(picked, cold_wt);
}

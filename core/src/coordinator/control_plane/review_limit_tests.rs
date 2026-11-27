//! Count actual reviewer processes across native advance, restart and manual retry.
#![cfg(unix)]

use super::{advance_tasks_native, try_phase_driver};
use crate::coordinator::runtime::CoordinatorRunState;
use crate::coordinator::state::{coordinator_state_registry_load, coordinator_state_registry_save};
use crate::coordinator::types::CoordinatorEnvConfig;
use crate::service::coordinator_workflow::coordinator_retry_phase;
use serde_json::json;
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::Path};

fn git(root: &Path, args: &[&str]) {
    let result = crate::git::run_git_output_mapped(root, args, "review limit fixture").unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[tokio::test]
async fn two_rejections_and_two_fixes_never_launch_a_third_reviewer() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Review fixture"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    fs::write(root.join(".gitignore"), "/.macc/\n").unwrap();
    git(root, &["add", ".gitignore"]);
    git(root, &["commit", "-qm", "baseline"]);
    git(root, &["switch", "-qc", "task"]);
    fs::write(root.join("source"), "candidate\n").unwrap();
    git(root, &["add", "source"]);
    git(root, &["commit", "-qm", "candidate"]);
    fs::create_dir_all(root.join(".macc/state")).unwrap();
    let runner = root.join(".macc/runner.sh");
    fs::write(
        &runner,
        r#"#!/bin/sh
set -eu
echo "$MACC_SESSION_PHASE" >> .macc/invocations
case "$MACC_SESSION_PHASE" in
  review) echo 'REVIEW_VERDICT: CHANGES_REQUESTED' ;;
  fix) echo correction >> source
       git commit -qam correction
       echo 'MACC_TASK_RESULT: success_with_changes' ;;
  *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        root.join(".macc/tool.json"),
        json!({"id":"fixture","performer":{"runner":runner,"session":{"enabled":false}}})
            .to_string(),
    )
    .unwrap();
    fs::write(root.join(".macc/state/coordinator.ipc.addr"), "127.0.0.1:9").unwrap();
    let args = BTreeMap::new();
    coordinator_state_registry_save(root, &args, &json!({"tasks":[{
        "id":"A","state":"testing","tool":"fixture","worktree":{"worktree_path":root,"branch":"task","base_branch":"main"},
        "task_runtime":{"review_refresh_required":"current review required"}
    }]})).unwrap();
    let env = CoordinatorEnvConfig {
        max_review_cycles: Some(2),
        disable_testing: Some(true),
        ..Default::default()
    };
    // Drop each driver after collecting its result, as on a restart.
    for mode in ["review", "fix", "review", "fix"] {
        let mut state = CoordinatorRunState::new();
        advance_tasks_native(root, &env, None, None, 4, &mut state, None)
            .await
            .unwrap();
        let job = state.phase_job.as_mut().expect("expected phase");
        assert_eq!(job.mode, mode);
        assert!(try_phase_driver(root).unwrap().is_none());
        job.wait().await;
        advance_tasks_native(root, &env, None, None, 4, &mut state, None)
            .await
            .unwrap();
        assert!(state.phase_job.is_none());
    }
    let mut registry = coordinator_state_registry_load(root, &args).unwrap();
    let task = &mut registry["tasks"][0];
    assert_eq!(task["task_runtime"]["review_cycles"], 2);
    assert_eq!(
        task["task_runtime"]["review_usage"]["corrections"]["completed"],
        2
    );
    // Reproduce an existing stop written by the previous coordinator.
    task["state"] = json!("blocked");
    task["task_runtime"]["current_phase"] = json!("review");
    task["task_runtime"]["last_error_origin"] = json!("review_budget");
    task["task_runtime"]["review_refresh_required"] =
        json!("Resumed correction requires a fresh independent review");
    coordinator_state_registry_save(root, &args, &registry).unwrap();
    // Explicit reviewer retries still enforce the cap on a blocked review.
    let cli_args = ["--retry-task", "A", "--retry-phase", "review"].map(String::from);
    let error = coordinator_retry_phase(
        &crate::engine::MaccEngine::new(crate::tool::ToolRegistry::new()),
        &crate::ProjectPaths::from_root(root),
        &Default::default(),
        None,
        &env,
        &cli_args,
        None,
    )
    .unwrap_err();
    assert!(
        error.to_string().contains("Review limit reached"),
        "{error}"
    );
    assert_eq!(
        fs::read_to_string(root.join(".macc/invocations")).unwrap(),
        "review\nfix\nreview\nfix\n"
    );
    // Explicitly grant one more slot. Four configured runner attempts must
    // launch exactly once, even when the reviewer process exits unsuccessfully.
    fs::write(
        &runner,
        "#!/bin/sh\necho retry >> .macc/invocations\nexit 7\n",
    )
    .unwrap();
    let raised = CoordinatorEnvConfig {
        max_review_cycles: Some(3),
        phase_runner_max_attempts: Some(4),
        ..env.clone()
    };
    let engine = crate::engine::MaccEngine::new(crate::tool::ToolRegistry::new());
    let paths = crate::ProjectPaths::from_root(root);
    let retry = || {
        coordinator_retry_phase(
            &engine,
            &paths,
            &Default::default(),
            None,
            &raised,
            &cli_args,
            None,
        )
    };
    assert!(retry().unwrap_err().to_string().contains("attempt 1/1"));
    assert!(retry()
        .unwrap_err()
        .to_string()
        .contains("Review limit reached"));
    assert_eq!(
        fs::read_to_string(root.join(".macc/invocations"))
            .unwrap()
            .lines()
            .filter(|line| *line == "retry")
            .count(),
        1
    );
    let after_retry = coordinator_state_registry_load(root, &args).unwrap();
    let runtime = &after_retry["tasks"][0]["task_runtime"];
    assert_eq!(runtime["review_usage"]["started"], 3);
    assert_eq!(
        runtime["review_cycles"], 2,
        "a failed invocation produces no verdict"
    );

    // Restart from the original retained stop and follow the automatic publication path.
    coordinator_state_registry_save(root, &args, &registry).unwrap();
    let mut state = CoordinatorRunState::new();
    advance_tasks_native(root, &env, None, None, 4, &mut state, None)
        .await
        .unwrap();
    assert!(state.active_merge_jobs.contains_key("A"));
    let corrected = crate::git::head_commit(root).unwrap();
    state.merge_join_set.join_next().await.unwrap().unwrap();
    assert_eq!(
        super::monitor_merge_jobs_native(root, &env, None, &mut state, None)
            .await
            .unwrap(),
        None
    );
    assert_eq!(
        coordinator_state_registry_load(root, &args).unwrap()["tasks"][0]["state"],
        "merged"
    );
    git(root, &["merge-base", "--is-ancestor", &corrected, "main"]);
    assert_eq!(
        fs::read_to_string(root.join(".macc/invocations")).unwrap(),
        "review\nfix\nreview\nfix\nretry\n"
    );
}

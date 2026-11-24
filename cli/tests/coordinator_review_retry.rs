//! A refused review retry records an operator event without changing the blocked task.
use macc_core::coordinator::state::{
    coordinator_state_registry_load, coordinator_state_registry_save,
};
use serde_json::json;
use std::{collections::BTreeMap, fs, process::Command};

#[test]
fn unstarted_review_preserves_budget_and_block_reason() {
    for (path, expected) in [
        (None, "missing retained worktree path"),
        (Some("missing-worker"), "inspect retained review worktree"),
        (Some("worker"), "missing coordinator tool"),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let args = BTreeMap::new();
        fs::create_dir(root.join(".macc")).unwrap();
        fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
        if path == Some("worker") {
            for args in [
                vec!["init", "-qb", "main"],
                vec!["config", "user.name", "Fixture"],
                vec!["config", "user.email", "test@example.invalid"],
                vec!["commit", "--allow-empty", "-qm", "base"],
                vec!["worktree", "add", "-qb", "candidate", "worker"],
                vec![
                    "-C",
                    "worker",
                    "commit",
                    "--allow-empty",
                    "-qm",
                    "candidate",
                ],
            ] {
                assert!(Command::new("git")
                    .current_dir(root)
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success());
            }
        }
        let task = json!({"id":"A","state":"blocked","worktree":{"worktree_path":path,"branch":"candidate","base_branch":"main"},
            "task_runtime":{"current_phase":"review","retries":0,"review_cycles":1,"last_error_code":"E103","last_error":"original diagnostic"}});
        coordinator_state_registry_save(root, &args, &json!({"tasks":[task]})).unwrap();
        let tasks = || coordinator_state_registry_load(root, &args).unwrap()["tasks"].clone();
        let before = tasks();
        let out = Command::new(env!("CARGO_BIN_EXE_macc"))
            .current_dir(root)
            .args([
                "coordinator",
                "retry-phase",
                "--retry-task",
                "A",
                "--retry-phase",
                "review",
            ])
            .output()
            .unwrap();
        assert!(!out.status.success());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("Review retry for A did not start"),
            "{stderr}"
        );
        assert!(stderr.contains(expected), "{stderr}");
        assert_eq!(tasks(), before);
        let events = fs::read_to_string(root.join(".macc/log/events.jsonl")).unwrap();
        assert!(events.contains("retry_preflight_failed"));
    }
}

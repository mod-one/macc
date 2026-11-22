//! The CLI owns a real IPC listener and launches the embedded performer in a linked checkout.
#![cfg(unix)]
use macc_core::coordinator::state::{
    coordinator_state_registry_load, coordinator_state_registry_save,
};
use serde_json::json;
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn development_retry_uses_sqlite_assignment_and_launches_once() {
    for stale_mirror in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        git(root, &["init", "-qb", "main"]);
        git(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-qm",
                "base",
            ],
        );
        fs::create_dir_all(root.join(".macc")).unwrap();
        fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
        git(root, &["config", "user.name", "Fixture"]);
        git(root, &["config", "user.email", "fixture@example.invalid"]);
        let worker = root.join(".macc/worktree/worker");
        let args = BTreeMap::new();
        let registry = json!({"tasks": [{
            "id": "A",
            "state": "blocked",
            "tool": "fixture",
            "worktree": {"worktree_path": worker, "branch": "worker", "base_branch": "main"},
            "task_runtime": {
                "current_phase": "dev", "status": "failed",
                "claim_id": "retry-claim", "coordinator_epoch": 1
            }
        }]});
        coordinator_state_registry_save(root, &args, &registry).unwrap();
        let bin = env!("CARGO_BIN_EXE_macc");
        let run = || {
            Command::new(bin)
                .current_dir(root)
                .env("MACC_PROTOCOL_CLI", bin)
                .env("MACC_INTERNAL_INVOCATION", "1")
                .args([
                    "coordinator",
                    "retry-phase",
                    "--retry-task",
                    "A",
                    "--retry-phase",
                    "dev",
                ])
                .output()
                .unwrap()
        };
        let mirror = root.join(".macc/automation/task/task_registry.json");
        if stale_mirror {
            fs::create_dir_all(mirror.parent().unwrap()).unwrap();
            fs::write(&mirror, r#"{"tasks":[]}"#).unwrap();
        } else {
            assert!(!mirror.exists());
        }
        let preflight = run();
        assert!(!preflight.status.success());
        assert!(
            String::from_utf8_lossy(&preflight.stderr).contains("inspect retained worktree"),
            "{}",
            String::from_utf8_lossy(&preflight.stderr)
        );
        let count = || {
            coordinator_state_registry_load(root, &args).unwrap()["tasks"][0]["task_runtime"]
                ["retries"]
                .as_u64()
                .unwrap_or(0)
        };
        assert_eq!(count(), 0);
        git(
            root,
            &[
                "worktree",
                "add",
                "-qb",
                "worker",
                worker.to_str().unwrap(),
                "main",
            ],
        );
        fs::create_dir_all(worker.join(".macc")).unwrap();
        fs::write(
            worker.join(".macc/worktree.json"),
            json!({"id":"worker","slug":"worker","tool":"fixture","branch":"worker","base":"main"})
                .to_string(),
        )
        .unwrap();
        let scoped_prd = json!({"tasks": [{
            "id": "A", "title": "Fixture task", "acceptance_criteria": ["Return the fixture result"]
        }]});
        fs::write(worker.join("worktree.prd.json"), scoped_prd.to_string()).unwrap();
        let script = root.join(".macc/fixture.sh");
        let result_script = if stale_mirror {
            "echo completed > fixture-result.txt\necho 'MACC_TASK_RESULT: success_with_changes'"
        } else {
            "echo 'MACC_TASK_PRECONDITION: fixture device unavailable'\n\
             echo 'MACC_TASK_RESULT_EXP: fixture device unavailable'\n\
             echo 'MACC_TASK_RESULT: precondition_unmet'"
        };
        fs::write(
            &script,
            format!("#!/bin/sh\necho invoked >> .macc/calls\n{result_script}\n"),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            worker.join(".macc/tool.json"),
            json!({"id":"fixture","performer":{"runner":script}}).to_string(),
        )
        .unwrap();
        let result = run();
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert_eq!(
            fs::read_to_string(worker.join(".macc/calls"))
                .ok()
                .as_deref(),
            Some("invoked\n"),
            "{stderr}"
        );
        if stale_mirror {
            assert!(result.status.success(), "{stderr}");
            let persisted = coordinator_state_registry_load(root, &args).unwrap();
            assert_eq!(
                persisted["tasks"][0]["task_runtime"]["status"],
                "phase_done"
            );
        } else {
            assert!(!result.status.success());
            assert!(
                stderr.contains("dev_retry_failed")
                    && stderr.contains("fixture device unavailable"),
                "{stderr}"
            );
        }
        assert_eq!(count(), 1);
    }
}

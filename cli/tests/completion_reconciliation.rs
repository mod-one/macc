//! A manual sync and subsequent queued run must never redispatch published tasks.
use macc_core::coordinator_storage::{CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage};
use macc_core::ProjectPaths;
use std::process::Command;
fn git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn sync_then_startup_reconciles_next_prd_and_survives_state_clear() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join(".macc")).unwrap();
    std::fs::write(
        root.join(".gitignore"),
        ".macc/\nperformer.sh\nworktree.prd.json\n",
    )
    .unwrap();
    std::fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: [codex]\nautomation:\n  coordinator:\n    reference_branch: main\n    sync_unmerged_branches: false\n    prd_files: [a.json, b.json]\n").unwrap();
    for (name, task) in [("a.json", "A"), ("b.json", "B")] {
        std::fs::write(
            root.join(name),
            format!(
                "{{\"tasks\":[{{\"id\":\"{task}\",\"title\":\"Delivered\",\"priority\":\"1\"}}]}}"
            ),
        )
        .unwrap();
    }
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "MACC Evidence Test"]);
    git(root, &["config", "user.email", "evidence@example.invalid"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "Initialize project"]);
    for id in ["A", "B"] {
        git(root, &["commit", "--allow-empty", "-qm", &format!("chore: {id} - Validate delivery\n\n[macc:task {id}]\n[macc:validation true]\n[macc:result already_satisfied]\n\nEvidence: acceptance tests passed")]);
    }
    let sync = Command::new(env!("CARGO_BIN_EXE_macc"))
        .args(["--cwd", root.to_str().unwrap(), "coordinator", "sync"])
        .env("MACC_INTERNAL_INVOCATION", "1")
        .output()
        .unwrap();
    assert!(
        sync.status.success(),
        "{}",
        String::from_utf8_lossy(&sync.stderr)
    );
    let config = macc_core::load_canonical_config(&root.join(".macc/macc.yaml")).unwrap();
    let env = macc_core::coordinator::types::CoordinatorEnvConfig::default();
    // Exercise the exact startup operation for the next queued PRD without
    // binding IPC: the same call runs before the engine's dispatch loop.
    macc_core::coordinator::delivery_evidence::reconcile_before_dispatch(
        root,
        &root.join("b.json"),
        config.automation.coordinator.as_ref(),
        &env,
        None,
    )
    .unwrap();
    let paths = ProjectPaths::from_root(root);
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let snapshot = db.load_snapshot().unwrap();
    assert!(snapshot.registry.tasks.iter().all(|t| t.is_merged()));
    let task = snapshot.registry.find_task("B").unwrap();
    assert!(task
        .task_runtime
        .extra
        .get("completion_commit_sha")
        .is_some());
    assert!(!snapshot
        .events
        .iter()
        .any(|e| e.event_type == "task_dispatched"));
    // Clearing transient coordinator storage must not erase Git delivery proof.
    std::fs::remove_file(&paths.root.join(".macc/state/coordinator.sqlite")).unwrap();
    macc_core::coordinator::delivery_evidence::reconcile_before_dispatch(
        root,
        &root.join("b.json"),
        config.automation.coordinator.as_ref(),
        &env,
        None,
    )
    .unwrap();
    let restored = db.load_snapshot().unwrap();
    assert!(restored.registry.find_task("B").unwrap().is_merged());
    assert_eq!(
        restored.registry.find_task("B").unwrap().task_runtime.extra["completion_commit_sha"],
        task.task_runtime.extra["completion_commit_sha"]
    );
}

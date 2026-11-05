//! Exercise the actual public Sync path; no performer or provider is invoked.
use macc_core::coordinator_storage::{CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage};
use macc_core::{prd_queue, ProjectPaths};
use std::process::Command;
fn git(root: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
#[test]
fn public_sync_accepts_a_completed_series_and_retains_its_git_delivery_proof() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join(".macc")).unwrap();
    std::fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
    for (file, id) in [("old.json", "OLD"), ("new.json", "NEW")] {
        std::fs::write(
            root.join(file),
            format!("{{\"tasks\":[{{\"id\":\"{id}\",\"title\":\"Work\",\"priority\":\"1\"}}]}}"),
        )
        .unwrap();
    }
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
    git(root,&["commit","--allow-empty","-qm","chore: OLD - validate\n\n[macc:task OLD]\n[macc:validation true]\n[macc:result already_satisfied]\n\nEvidence: acceptance passed"]);
    let config = |file: &str| {
        std::fs::write(root.join(".macc/macc.yaml"),format!("tools:\n  enabled: [codex]\nautomation:\n  coordinator:\n    reference_branch: main\n    sync_unmerged_branches: false\n    prd_files: [{file}]\n")).unwrap()
    };
    std::fs::write(
        root.join("new.json"),
        r#"{"tasks":[{"id":"NEW","title":"Work","priority":"1","dependencies":["OLD"]}]}"#,
    )
    .unwrap();
    config("old.json");
    let sync = || {
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .arg("--cwd")
            .arg(root)
            .args(["coordinator", "sync"])
            .env("MACC_INTERNAL_INVOCATION", "1")
            .output()
            .unwrap()
    };
    let out = sync();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let paths = ProjectPaths::from_root(root);
    let store = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let snapshot = store.load_snapshot().unwrap();
    assert!(snapshot.registry.find_task("OLD").unwrap().is_merged());
    std::fs::create_dir_all(root.join(".macc/state")).unwrap();
    std::fs::write(
        root.join(".macc/state/prd-queue-completed-0.json"),
        serde_json::to_vec(&snapshot.registry).unwrap(),
    )
    .unwrap();
    prd_queue::save(
        root,
        &prd_queue::Progress {
            entries: prd_queue::validate(root, &["old.json".into()]).unwrap(),
            next: 1,
            status: "completed".into(),
            reason: None,
        },
    )
    .unwrap();
    config("new.json");
    let out = sync();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let snapshot = store.load_snapshot().unwrap();
    assert!(snapshot.registry.find_task("NEW").is_some());
    assert!(snapshot.registry.external_merged_task_ids.contains("OLD"));
    assert_eq!(prd_queue::load(root).unwrap().next, 0);
    assert_eq!(
        std::fs::read_dir(root.join(".macc/state/prd-queue-history"))
            .unwrap()
            .count(),
        1
    );
    assert!(!snapshot
        .events
        .iter()
        .any(|e| e.event_type == "task_dispatched"));
    // An unfinished replacement is actionable, printed once, and leaves NEW intact.
    config("old.json");
    let failed = sync();
    assert!(!failed.status.success());
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert_eq!(
        stderr.matches(prd_queue::transition::CODE).count(),
        1,
        "{stderr}"
    );
    assert!(store
        .load_snapshot()
        .unwrap()
        .registry
        .find_task("NEW")
        .is_some());
}

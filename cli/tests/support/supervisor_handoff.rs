//! Real supervisor/Git integration with unfinished work retained on its original branch.
use super::*;

fn attach_worker(root: &Path) -> std::path::PathBuf {
    let worker = root.join(".macc/worktree/worker-01");
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "ai/fake/worker-01",
            worker.to_str().unwrap(),
        ],
    );
    std::fs::create_dir_all(worker.join(".macc")).unwrap();
    std::fs::write(
        worker.join("pending.txt"),
        "unfinished task implementation\n",
    )
    .unwrap();
    let paths = ProjectPaths::from_root(root);
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let mut snapshot = db.load_snapshot().unwrap();
    let task = &mut snapshot.registry.tasks[0];
    task.worktree = Some(macc_core::coordinator::model::TaskWorktree {
        branch: Some("ai/fake/worker-01".into()),
        base_branch: Some("master".into()),
        worktree_path: Some(worker.to_string_lossy().into()),
        ..Default::default()
    });
    db.save_snapshot(&snapshot).unwrap();
    worker
}

#[test]
fn supervisor_integrates_only_prerequisite_then_resumes_original_dirty_branch() {
    let tmp = fixture(false);
    let root = tmp.path();
    let worker = attach_worker(root);
    edit_config(
        root,
        "    tool: fake\n",
        "    tool: fake\n    auto_restart: false\n    retain_repair_branches: true\n",
    );
    let supervisor = start(root);
    let r = settled(root, "repair_integrated");
    assert_eq!(
        std::fs::read_to_string(worker.join("pending.txt")).unwrap(),
        "unfinished task implementation\n"
    );
    assert_eq!(
        std::fs::read_to_string(worker.join("defect.txt")).unwrap(),
        "fixed\n"
    );
    assert!(!root.join("pending.txt").exists());
    assert_eq!(
        git_out(&worker, &["branch", "--show-current"]),
        "ai/fake/worker-01"
    );
    assert!(git_out(&worker, &["status", "--porcelain"]).contains("pending.txt"));
    let refs = git_out(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads/macc/supervisor/",
        ],
    );
    assert!(refs.contains("run-fixture-1"));
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(
        &ProjectPaths::from_root(root),
    ));
    let task = db.load_snapshot().unwrap().registry.tasks.remove(0);
    assert!(task.is_awaiting_repair_resume());
    assert!(r["intervention"]["restart_pid"].is_null());
    assert!(r["intervention"]["next_action"]
        .as_str()
        .unwrap()
        .contains("macc coordinator run"));
    let log = Path::new(r["intervention"]["log_dir"].as_str().unwrap());
    wait_for(|| {
        log.join("evidence/.macc/supervisor-staged-diff.txt")
            .exists()
    });
    drop(supervisor);
}

#[test]
fn copying_worker_implementation_into_repair_is_rejected_without_publication() {
    let tmp = fixture(false);
    let root = tmp.path();
    let worker = attach_worker(root);
    let script = root.join(".macc/fake-agent.sh");
    let original = std::fs::read_to_string(&script).unwrap();
    let needle = r"printf 'fixed\n' > defect.txt";
    assert!(original.contains(needle));
    let text = original.replace(
        needle,
        &format!(
            "{}; cp '{}' pending.txt",
            needle,
            worker.join("pending.txt").display()
        ),
    );
    std::fs::write(script, text).unwrap();
    let head = git_out(root, &["rev-parse", "HEAD"]);
    let supervisor = start(root);
    let r = settled(root, "escalated");
    assert!(r["intervention"]["detail"]
        .as_str()
        .unwrap()
        .contains("overlaps unfinished task ROOT"));
    assert_eq!(git_out(root, &["rev-parse", "HEAD"]), head);
    assert!(r["intervention"]["commit"].is_null());
    assert!(worker.join("pending.txt").exists());
    drop(supervisor);
}

#[test]
fn large_incident_reaches_real_arg_mode_tool_with_complete_file_evidence() {
    let tmp = fixture(false);
    let root = tmp.path();
    edit_config(
        root,
        "    tool: fake\n",
        "    tool: fake\n    auto_restart: false\n    retain_repair_branches: true\n",
    );
    let paths = ProjectPaths::from_root(root);
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let mut run = db.get_latest_coordinator_run().unwrap().unwrap();
    run.stop_reason = Some("mandatory incident evidence ".repeat(16000));
    db.upsert_coordinator_run(&run).unwrap();
    let script = root.join(".macc/fake-agent.sh");
    let text = std::fs::read_to_string(&script).unwrap().replace(
        "case \"$1\" in",
        r#"prompt=$1
case "$prompt" in
 *"Read the complete supervisor instructions"*)
   printf '%s' "${#prompt}" > "$(dirname "$0")/argument-length.txt"
   path=$(printf '%s' "$prompt" | sed 's/.*exactly: "\([^"]*\)". The file.*/\1/')
   prompt=$(cat "$path")
   printf '%s' "$prompt" | tail -c 200 > "$(dirname "$0")/complete-evidence.txt" ;;
esac
case "$prompt" in"#,
    );
    std::fs::write(script, text).unwrap();
    let supervisor = start(root);
    let r = settled(root, "repair_integrated");
    assert!(
        std::fs::read_to_string(root.join(".macc/argument-length.txt"))
            .unwrap()
            .parse::<usize>()
            .unwrap()
            < 1024
    );
    let log = Path::new(r["intervention"]["log_dir"].as_str().unwrap());
    assert!(
        std::fs::metadata(log.join("diagnosis-prompt.txt"))
            .unwrap()
            .len()
            > 131072
    );
    assert!(root.join(".macc/complete-evidence.txt").exists());
    drop(supervisor);
}

#[test]
fn supervisor_deletes_merged_repair_branches_by_default() {
    let tmp = fixture(false);
    let root = tmp.path();
    let _worker = attach_worker(root);
    edit_config(
        root,
        "    tool: fake\n",
        "    tool: fake\n    auto_restart: false\n",
    );
    let supervisor = start(root);
    let _r = settled(root, "repair_integrated");
    wait_for(|| !root.join(".macc/worktree/supervisor-01").exists());
    let refs = git_out(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads/macc/supervisor/",
        ],
    );
    assert!(
        refs.trim().is_empty(),
        "Merged supervisor repair branches should be deleted by default: found {refs}"
    );
    drop(supervisor);
}

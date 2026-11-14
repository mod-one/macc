use super::*;
use crate::coordinator::model::TaskWorktree;
use crate::coordinator_storage::{
    CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage,
};
use crate::ProjectPaths;

fn fixture() -> (tempfile::TempDir, Task) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q", "-b", "main"]).unwrap();
    git(root, &["config", "user.name", "Test"]).unwrap();
    git(root, &["config", "user.email", "test@example.invalid"]).unwrap();
    std::fs::write(root.join(".gitignore"), ".macc/\n*.cache\n").unwrap();
    std::fs::write(
        root.join("source.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nseven\n",
    )
    .unwrap();
    std::fs::write(root.join("prerequisite.txt"), "broken\n").unwrap();
    git(root, &["add", "."]).unwrap();
    git(root, &["commit", "-qm", "initial"]).unwrap();
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
    )
    .unwrap();
    std::fs::create_dir_all(worker.join(".macc")).unwrap();
    let task = Task {
        id: "ROOT".into(),
        state: "blocked".into(),
        tool: Some("fake".into()),
        worktree: Some(TaskWorktree {
            branch: Some("ai/fake/worker-01".into()),
            base_branch: Some("main".into()),
            worktree_path: Some(worker.to_string_lossy().into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    (tmp, task)
}

#[test]
fn protocol_unblock_resumes_review_on_the_retained_branch_without_restarting_dev() {
    let (tmp, mut task) = fixture();
    let root = tmp.path();
    let worker = std::path::PathBuf::from(task.worktree_path().unwrap());
    git(
        &worker,
        &["commit", "--allow-empty", "-qm", "retained implementation"],
    )
    .unwrap();
    let head = git(&worker, &["rev-parse", "HEAD"]).unwrap();
    task.task_runtime.current_phase = Some("review".into());
    task.task_runtime.set_last_error_details(
        "E103",
        "output_protocol",
        "Missing exact MACC_DECISION_ACK",
    );
    let paths = ProjectPaths::from_root(root);
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let mut snapshot = CoordinatorSnapshot::empty();
    snapshot.registry.tasks.push(task);
    db.save_snapshot(&snapshot).unwrap();
    let cfg = crate::config::CoordinatorConfig {
        reference_branch: Some("main".into()),
        ..Default::default()
    };
    crate::service::coordinator_workflow::coordinator_unblock_task(
        &paths,
        Some(&cfg),
        &Default::default(),
        "ROOT",
        "Runtime projection repaired; rerun review",
    )
    .unwrap();
    let registry = db.load_snapshot().unwrap().registry;
    let task = registry.find_task("ROOT").unwrap();
    assert_eq!(task.state, "reviewing");
    assert_eq!(task.task_runtime.current_phase.as_deref(), Some("review"));
    assert!(task.task_runtime.last_error_code.is_none());
    assert_eq!(
        git(&worker, &["branch", "--show-current"]).unwrap(),
        "ai/fake/worker-01"
    );
    assert_eq!(git(&worker, &["rev-parse", "HEAD"]).unwrap(), head);
}

#[test]
fn legacy_missing_result_unblock_reattaches_only_the_proven_worker_and_keeps_dirty_work() {
    let (tmp, mut task) = fixture();
    let root = tmp.path();
    let worker = std::path::PathBuf::from(task.worktree_path().unwrap());
    std::fs::write(
        root.join("prd.json"),
        serde_json::json!({"tasks":[{"id":"ROOT","title":"Keep work"}]}).to_string(),
    )
    .unwrap();
    crate::coordinator::helpers::write_worktree_prd_for_task(
        &root.join("prd.json"),
        "ROOT",
        &worker,
    )
    .unwrap();
    std::fs::write(worker.join(".macc/worktree.json"), serde_json::json!({"id":"worker-01","slug":"worker-01","tool":"fake","base":"main","branch":"ai/fake/worker-01"}).to_string()).unwrap();
    std::fs::write(worker.join("new.txt"), "uncommitted implementation").unwrap();
    let head = git(&worker, &["rev-parse", "HEAD"]).unwrap();
    task.task_runtime.last_worktree_path = task.worktree_path().map(str::to_string);
    task.task_runtime.last_branch = task.branch().map(str::to_string);
    task.task_runtime.last_session_tool = Some("fake".into());
    task.task_runtime.extra.insert(
        "decision_context".into(),
        serde_json::json!({"phase":"dev"}),
    );
    task.task_runtime.set_last_error_details(
        "E906",
        "coordinator",
        "Missing explicit MACC_TASK_RESULT; exit status and Git changes cannot prove delivery",
    );
    task.worktree = None;
    task.tool = None;
    let paths = ProjectPaths::from_root(root);
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let mut snapshot = CoordinatorSnapshot::empty();
    snapshot.registry.tasks.push(task);
    db.save_snapshot(&snapshot).unwrap();
    let cfg = crate::config::CoordinatorConfig {
        reference_branch: Some("main".into()),
        prd_file: Some("prd.json".into()),
        ..Default::default()
    };
    // A reused branch must never be adopted, even if its old path still exists.
    git(&worker, &["branch", "-m", "ai/fake/reassigned"]).unwrap();
    assert!(
        crate::service::coordinator_workflow::coordinator_unblock_task(
            &paths,
            Some(&cfg),
            &Default::default(),
            "ROOT",
            "Updated parser"
        )
        .is_err()
    );
    assert_eq!(
        db.load_snapshot().unwrap().registry.tasks[0].state,
        "blocked"
    );
    assert!(db.load_snapshot().unwrap().registry.tasks[0]
        .worktree
        .is_none());
    git(&worker, &["branch", "-m", "ai/fake/worker-01"]).unwrap();
    crate::service::coordinator_workflow::coordinator_unblock_task(
        &paths,
        Some(&cfg),
        &Default::default(),
        "ROOT",
        "Updated parser",
    )
    .unwrap();
    let registry = db.load_snapshot().unwrap().registry;
    let task = registry.find_task("ROOT").unwrap();
    assert_eq!(task.state, "todo");
    assert_eq!(task.worktree_path(), Some(worker.to_str().unwrap()));
    assert_eq!(task.tool.as_deref(), Some("fake"));
    assert!(task.task_runtime.completion_kind.is_none());
    assert_eq!(
        std::fs::read_to_string(worker.join("new.txt")).unwrap(),
        "uncommitted implementation"
    );
    assert_eq!(git(&worker, &["rev-parse", "HEAD"]).unwrap(), head);
    assert!(git(&worker, &["status", "--porcelain"])
        .unwrap()
        .contains("?? new.txt"));
    assert!(task.task_runtime.extra.contains_key(RESUME_KEY));
}

#[test]
fn dirty_worker_unblock_survives_real_startup_sweep_and_is_dispatchable_with_zero_retry_budget() {
    let (tmp, mut task) = fixture();
    let root = tmp.path();
    let worker = std::path::PathBuf::from(task.worktree_path().unwrap());
    std::fs::write(
        worker.join("source.txt"),
        "worker change\ntwo\nthree\nfour\nfive\nsix\nseven\n",
    )
    .unwrap();
    git(&worker, &["add", "source.txt"]).unwrap();
    std::fs::write(
        worker.join("source.txt"),
        "worker change\ntwo\nthree\nfour\nfive\nsix\nunstaged change\n",
    )
    .unwrap();
    std::fs::write(worker.join("new.txt"), "untracked implementation\n").unwrap();
    std::fs::write(root.join("prerequisite.txt"), "fixed\n").unwrap();
    git(root, &["add", "."]).unwrap();
    git(root, &["commit", "-qm", "fix prerequisite"]).unwrap();
    let paths = ProjectPaths::from_root(root);
    task.task_runtime.retries = Some(99);
    let mut snapshot = CoordinatorSnapshot::empty();
    snapshot.registry.tasks.push(task);
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    db.save_snapshot(&snapshot).unwrap();
    let cfg = crate::config::CoordinatorConfig {
        reference_branch: Some("main".into()),
        ..Default::default()
    };
    crate::service::coordinator_workflow::coordinator_unblock_task(
        &paths,
        Some(&cfg),
        &Default::default(),
        "ROOT",
        "prerequisite fixed",
    )
    .unwrap();
    let sweep = crate::coordinator::state_runtime::execute_startup_recovery_sweep(
        root, "main", false, None,
    )
    .unwrap();
    assert!(!sweep
        .iter()
        .any(|entry| entry.classification == "parked_unschedulable_with_changes"));
    let registry = db.load_snapshot().unwrap().registry;
    let selector = crate::coordinator::task_selector::TaskSelectorConfig {
        enabled_tools: vec!["fake".into()],
        default_tool: "fake".into(),
        default_base_branch: "main".into(),
        max_same_worktree_retries: 0,
        ..Default::default()
    };
    let selected =
        crate::coordinator::task_selector::select_next_ready_task_typed(&registry, &selector)
            .unwrap();
    assert_eq!(
        selected.resume_worktree.unwrap().branch,
        "ai/fake/worker-01"
    );
    assert_eq!(
        std::fs::read_to_string(worker.join("prerequisite.txt")).unwrap(),
        "fixed\n"
    );
    assert!(std::fs::read_to_string(worker.join("source.txt"))
        .unwrap()
        .contains("unstaged change"));
    assert_eq!(
        std::fs::read_to_string(worker.join("new.txt")).unwrap(),
        "untracked implementation\n"
    );
    assert!(!git(&worker, &["status", "--porcelain"]).unwrap().is_empty());
    assert_eq!(
        git(&worker, &["branch", "--show-current"]).unwrap(),
        "ai/fake/worker-01"
    );
    let backups = git(
        root,
        &["for-each-ref", "--format=%(refname)", "refs/macc/recovery"],
    )
    .unwrap();
    assert_eq!(backups.lines().count(), 2);
    assert!(!git(root, &["log", "--format=%B", "main"])
        .unwrap()
        .contains("[macc:task"));
    let mut value = registry.to_value().unwrap();
    crate::coordinator::engine::apply_dispatch_claim_in_registry(
        &mut value,
        &crate::coordinator::engine::DispatchClaimUpdate {
            task_id: "ROOT".into(),
            tool: "fake".into(),
            worktree_path: worker.to_string_lossy().into(),
            branch: "ai/fake/worker-01".into(),
            base_branch: "main".into(),
            last_commit: git(&worker, &["rev-parse", "HEAD"]).unwrap(),
            session_id: "claim".into(),
            active_session_id: None,
            pid: None,
            phase: "dev".into(),
            now: "2026-10-19T16:00:00Z".into(),
            run_id: "new".into(),
            coordinator_epoch: 2,
        },
    )
    .unwrap();
    let claimed = crate::coordinator::model::TaskRegistry::from_value(&value).unwrap();
    assert!(!claimed.tasks[0].task_runtime.extra.contains_key(RESUME_KEY));
}

#[test]
fn conflicting_repair_preserves_staged_unstaged_untracked_and_head() {
    let (tmp, task) = fixture();
    let root = tmp.path();
    let worker = std::path::PathBuf::from(task.worktree_path().unwrap());
    std::fs::write(worker.join("prerequisite.txt"), "worker staged\n").unwrap();
    git(&worker, &["add", "."]).unwrap();
    std::fs::write(worker.join("prerequisite.txt"), "worker unstaged\n").unwrap();
    std::fs::write(worker.join("untracked.txt"), "new\n").unwrap();
    let head = git(&worker, &["rev-parse", "HEAD"]).unwrap();
    let index = git(&worker, &["write-tree"]).unwrap();
    let content = crate::git::worktree_content_tree(&worker).unwrap();
    std::fs::write(root.join("prerequisite.txt"), "repair\n").unwrap();
    git(root, &["add", "."]).unwrap();
    git(root, &["commit", "-qm", "repair"]).unwrap();
    let error = reconcile(root, &task, "main").unwrap_err().to_string();
    assert!(error.contains("conflicts with retained task"), "{error}");
    assert_eq!(git(&worker, &["rev-parse", "HEAD"]).unwrap(), head);
    assert_eq!(git(&worker, &["write-tree"]).unwrap(), index);
    assert_eq!(crate::git::worktree_content_tree(&worker).unwrap(), content);
    assert!(error.contains("refs/macc/recovery/"));
}

#[test]
fn already_integrated_untracked_file_and_new_prerequisite_do_not_require_starting_over() {
    let (tmp, task) = fixture();
    let root = tmp.path();
    let worker = std::path::PathBuf::from(task.worktree_path().unwrap());
    for p in [root, worker.as_path()] {
        std::fs::write(p.join("implementation.txt"), "same implementation\n").unwrap();
    }
    std::fs::write(root.join("prerequisite.txt"), "fixed\n").unwrap();
    git(root, &["add", "."]).unwrap();
    git(root, &["commit", "-qm", "previous combined repair"]).unwrap();
    reconcile(root, &task, "main").unwrap();
    assert_eq!(
        std::fs::read_to_string(worker.join("implementation.txt")).unwrap(),
        "same implementation\n"
    );
    assert!(git(&worker, &["status", "--porcelain"]).unwrap().is_empty());
}

#[test]
fn nonoverlapping_edits_in_same_file_are_three_way_merged() {
    let (tmp, task) = fixture();
    let root = tmp.path();
    let worker = std::path::PathBuf::from(task.worktree_path().unwrap());
    std::fs::write(
        worker.join("source.txt"),
        "worker\ntwo\nthree\nfour\nfive\nsix\nseven\n",
    )
    .unwrap();
    std::fs::write(
        root.join("source.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\nrepair\n",
    )
    .unwrap();
    git(root, &["add", "."]).unwrap();
    git(root, &["commit", "-qm", "repair"]).unwrap();
    reconcile(root, &task, "main").unwrap();
    assert_eq!(
        std::fs::read_to_string(worker.join("source.txt")).unwrap(),
        "worker\ntwo\nthree\nfour\nfive\nsix\nrepair\n"
    );
}

#[test]
fn ignored_local_file_cannot_be_overwritten_by_new_tracked_prerequisite() {
    let (tmp, task) = fixture();
    let root = tmp.path();
    let worker = Path::new(task.worktree_path().unwrap());
    std::fs::write(worker.join("local.cache"), "private local content\n").unwrap();
    std::fs::write(root.join("local.cache"), "tracked repair\n").unwrap();
    git(root, &["add", "-f", "local.cache"]).unwrap();
    git(root, &["commit", "-qm", "repair"]).unwrap();
    let head = git(worker, &["rev-parse", "HEAD"]).unwrap();
    assert!(reconcile(root, &task, "main")
        .unwrap_err()
        .to_string()
        .contains("overlaps ignored local files"));
    assert_eq!(
        std::fs::read_to_string(worker.join("local.cache")).unwrap(),
        "private local content\n"
    );
    assert_eq!(git(worker, &["rev-parse", "HEAD"]).unwrap(), head);
}

#[test]
fn divergent_worker_commits_remain_ancestors_after_prerequisite_handoff() {
    let (tmp, task) = fixture();
    let root = tmp.path();
    let worker = Path::new(task.worktree_path().unwrap());
    std::fs::write(worker.join("committed.txt"), "WIP committed earlier\n").unwrap();
    git(worker, &["add", "."]).unwrap();
    git(worker, &["commit", "-qm", "wip: existing implementation"]).unwrap();
    let previous = git(worker, &["rev-parse", "HEAD"]).unwrap();
    std::fs::write(worker.join("pending.txt"), "unfinished\n").unwrap();
    std::fs::write(root.join("prerequisite.txt"), "fixed\n").unwrap();
    git(root, &["add", "."]).unwrap();
    git(root, &["commit", "-qm", "repair"]).unwrap();
    reconcile(root, &task, "main").unwrap();
    git(worker, &["merge-base", "--is-ancestor", &previous, "HEAD"]).unwrap();
    assert!(git(worker, &["status", "--porcelain"])
        .unwrap()
        .contains("pending.txt"));
    assert_eq!(
        std::fs::read_to_string(worker.join("committed.txt")).unwrap(),
        "WIP committed earlier\n"
    );
}

#[test]
fn unchanged_reference_still_pins_pending_content_and_index() {
    let (tmp, task) = fixture();
    let root = tmp.path();
    let worker = Path::new(task.worktree_path().unwrap());
    std::fs::write(
        worker.join("pending.txt"),
        "work before runner-only repair\n",
    )
    .unwrap();
    let content = crate::git::worktree_content_tree(worker).unwrap();
    let head = git(worker, &["rev-parse", "HEAD"]).unwrap();
    assert_eq!(reconcile(root, &task, "main").unwrap(), head);
    assert_eq!(crate::git::worktree_content_tree(worker).unwrap(), content);
    assert_eq!(
        git(
            root,
            &["for-each-ref", "--format=%(refname)", "refs/macc/recovery/"]
        )
        .unwrap()
        .lines()
        .count(),
        2
    );
}

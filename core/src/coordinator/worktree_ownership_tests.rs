use super::*;
use crate::coordinator::{
    integration::IntegrationWorktree,
    model::{Task, TaskWorktree},
    repair_handoff::git,
};
use crate::coordinator_storage::{
    CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage,
};
use std::collections::HashMap;

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]).unwrap();
    git(root, &["config", "user.name", "Test"]).unwrap();
    git(root, &["config", "user.email", "test@example.invalid"]).unwrap();
    std::fs::write(root.join(".gitignore"), ".macc/\n*.cache\n").unwrap();
    std::fs::write(root.join("source.txt"), "base\n").unwrap();
    git(root, &["add", "."]).unwrap();
    git(root, &["commit", "-qm", "initial"]).unwrap();
    dir
}
fn save(root: &Path, task: &Task) {
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(
        &ProjectPaths::from_root(root),
    ));
    let mut snapshot = CoordinatorSnapshot::empty();
    snapshot.registry.tasks.push(task.clone());
    db.save_snapshot(&snapshot).unwrap();
}
fn worker(root: &Path, name: &str) -> Task {
    let path = root.join(".macc/worktree").join(name);
    let branch = format!("ai/future/{name}");
    git(
        root,
        &[
            "worktree",
            "add",
            "-qb",
            &branch,
            path.to_str().unwrap(),
            "main",
        ],
    )
    .unwrap();
    std::fs::create_dir_all(path.join(".macc")).unwrap();
    std::fs::write(
        path.join(".macc/worktree.json"),
        serde_json::json!({"id":name,"slug":name,"tool":"future","base":"main","branch":branch})
            .to_string(),
    )
    .unwrap();
    let mut task = Task {
        id: "ROOT".into(),
        state: "blocked".into(),
        tool: Some("future".into()),
        worktree: Some(TaskWorktree {
            worktree_path: Some(path.to_string_lossy().into()),
            branch: Some(branch),
            base_branch: Some("main".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    task.task_runtime.claim_id = Some("claim".into());
    task.task_runtime.current_phase = Some("merge".into());
    task
}
fn snapshot(path: &Path) -> (String, String, String, String) {
    (
        git(path, &["rev-parse", "HEAD"]).unwrap(),
        git(path, &["branch", "--show-current"]).unwrap(),
        git(path, &["write-tree"]).unwrap(),
        crate::git::worktree_content_tree(path).unwrap(),
    )
}
#[test]
fn integration_scratch_never_becomes_a_performer_even_when_warm_and_clean() {
    let dir = fixture();
    let root = dir.path();
    let scratch = IntegrationWorktree::acquire(root, "main").unwrap();
    let path = scratch.path().to_path_buf();
    drop(scratch);
    let before = snapshot(&path);
    let (candidate, _) = super::super::helpers::find_reusable_worktree_native(
        root,
        &serde_json::json!({"tasks":[]}),
        "future",
        "main",
        3600,
        &HashMap::new(),
    )
    .unwrap();
    assert!(candidate.is_none());
    assert_eq!(before, snapshot(&path));
    let scratch = IntegrationWorktree::acquire(root, "main").unwrap();
    assert_eq!(scratch.path(), path);
}
#[test]
fn performer_assignment_rejects_scratch_cleanup_without_changing_any_work() {
    let dir = fixture();
    let root = dir.path();
    let task = worker(root, "integration");
    let path = Path::new(task.worktree_path().unwrap());
    save(root, &task);
    std::fs::write(path.join("source.txt"), "staged\n").unwrap();
    git(path, &["add", "source.txt"]).unwrap();
    std::fs::write(path.join("source.txt"), "unstaged\n").unwrap();
    std::fs::write(path.join("new.bin"), [0, 255, 0]).unwrap();
    std::fs::write(path.join("private.cache"), "secret fixture").unwrap();
    let before = snapshot(path);
    let error = IntegrationWorktree::acquire(root, "main")
        .err()
        .unwrap()
        .to_string();
    assert!(
        error.contains("ROOT") && error.contains("claim") && error.contains("no cleanup"),
        "{error}"
    );
    assert_eq!(before, snapshot(path));
    assert_eq!(
        std::fs::read_to_string(path.join("private.cache")).unwrap(),
        "secret fixture"
    );
}
#[test]
fn an_attached_branch_or_unknown_detached_commit_is_not_disposable_scratch() {
    for detach in [false, true] {
        let dir = fixture();
        let root = dir.path();
        let task = worker(root, "integration");
        let path = Path::new(task.worktree_path().unwrap());
        std::fs::remove_file(path.join(".macc/worktree.json")).unwrap();
        std::fs::write(path.join("work.txt"), "valuable\n").unwrap();
        git(path, &["add", "work.txt"]).unwrap();
        git(path, &["commit", "-qm", "unpublished work"]).unwrap();
        if detach {
            git(path, &["checkout", "--detach"]).unwrap();
        }
        let before = snapshot(path);
        assert!(IntegrationWorktree::acquire(root, "main").is_err());
        assert_eq!(before, snapshot(path));
    }
}
#[test]
fn unknown_or_nested_slots_are_not_adopted_and_a_proven_worker_is_reusable() {
    let dir = fixture();
    let root = dir.path();
    let task = worker(root, "worker-01");
    let path = Path::new(task.worktree_path().unwrap());
    assert!(performer(root, path));
    assert!(!performer(root, &root.join(".macc/worktree/unknown")));
    let (reuse, _) = super::super::helpers::find_reusable_worktree_native(
        root,
        &serde_json::json!({"tasks":[]}),
        "future",
        "main",
        0,
        &HashMap::new(),
    )
    .unwrap();
    assert_eq!(reuse.unwrap().0, path);
    for phase in ["testing", "reviewing"] {
        let mut reg = TaskRegistry::default();
        let mut owned = task.clone();
        owned.state = phase.into();
        reg.tasks.push(owned);
        assert!(!reg.can_reuse_worktree_slot(path.to_str().unwrap()));
    }
}

#[test]
fn collision_recovery_preserves_branch_staged_unstaged_untracked_and_ignored_work() {
    let dir = fixture();
    let root = dir.path();
    let mut task = worker(root, "integration");
    let old = PathBuf::from(task.worktree_path().unwrap());
    std::fs::write(old.join("implemented.txt"), "delivered part\n").unwrap();
    git(&old, &["add", "implemented.txt"]).unwrap();
    git(&old, &["commit", "-qm", "implementation"]).unwrap();
    let reviewed = git(&old, &["rev-parse", "HEAD"]).unwrap();
    let reference = git(root, &["rev-parse", "main"]).unwrap();
    git(&old, &["checkout", "--detach", "main"]).unwrap();
    let message = crate::commit_message::merge_commit("ROOT").format();
    git(
        &old,
        &["merge", "--no-ff", "-m", &message, task.branch().unwrap()],
    )
    .unwrap();
    let merge = git(&old, &["rev-parse", "HEAD"]).unwrap();
    std::fs::write(old.join("source.txt"), "staged\n").unwrap();
    git(&old, &["add", "source.txt"]).unwrap();
    std::fs::write(old.join("source.txt"), "unstaged\n").unwrap();
    std::fs::write(old.join("new.bin"), [0, 255, 0]).unwrap();
    std::fs::write(old.join("private.cache"), "private fixture").unwrap();
    let before = snapshot(&old);
    save(root, &task);
    let base = super::super::repair_handoff::reconcile_task(root, &mut task, "main").unwrap();
    let new = PathBuf::from(task.worktree_path().unwrap());
    assert_ne!(old, new);
    assert_eq!(new.file_name().unwrap(), "worker-01");
    assert!(!old.exists());
    assert_eq!(base, reviewed);
    assert_eq!(git(&new, &["rev-parse", "HEAD"]).unwrap(), reviewed);
    assert_eq!(git(root, &["rev-parse", "main"]).unwrap(), reference);
    assert_eq!(snapshot(&new).2, before.2);
    assert_eq!(snapshot(&new).3, before.3);
    assert_eq!(std::fs::read(new.join("new.bin")).unwrap(), [0, 255, 0]);
    assert_eq!(
        std::fs::read_to_string(new.join("private.cache")).unwrap(),
        "private fixture"
    );
    assert_eq!(task.task_runtime.current_phase.as_deref(), Some("merge"));
    assert!(performer(root, &new));
    let refs = git(
        root,
        &[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/macc/recovery/",
        ],
    )
    .unwrap();
    assert!(refs.lines().any(|line| line == merge));
    // Replay the pre-publication crash window: files already moved, old registry binding.
    let mut old_binding = task.clone();
    old_binding.ensure_worktree().worktree_path = Some(old.to_string_lossy().into());
    save(root, &old_binding);
    super::super::repair_handoff::reconcile_task(root, &mut old_binding, "main").unwrap();
    assert_eq!(old_binding.worktree_path(), task.worktree_path());
    let scratch = IntegrationWorktree::acquire(root, "main").unwrap();
    assert_ne!(scratch.path(), new);
    assert_eq!(git(&new, &["rev-parse", "HEAD"]).unwrap(), reviewed);
}

#[test]
fn unsupported_collision_is_task_local_and_never_resets_or_requeues_implementation() {
    let dir = fixture();
    let root = dir.path();
    let mut task = worker(root, "integration");
    let path = PathBuf::from(task.worktree_path().unwrap());
    save(root, &task);
    git(&path, &["checkout", "--detach"]).unwrap();
    std::fs::write(path.join("different.txt"), "different content\n").unwrap();
    git(&path, &["add", "different.txt"]).unwrap();
    git(&path, &["commit", "-qm", "unrelated detached change"]).unwrap();
    let before = snapshot(&path);
    assert!(super::super::retained_reconciliation::reconcile(root, &mut task, "main").is_none());
    assert_eq!(task.state, "blocked");
    assert_eq!(task.task_runtime.current_phase.as_deref(), Some("merge"));
    assert_eq!(task.task_runtime.last_error_code.as_deref(), Some("E901"));
    assert_eq!(before, snapshot(&path));
}

#[test]
fn recovery_refuses_a_live_performer_before_recording_or_moving_work() {
    let dir = fixture();
    let root = dir.path();
    let mut task = worker(root, "integration");
    task.task_runtime.pid = Some(std::process::id() as i64);
    save(root, &task);
    let path = PathBuf::from(task.worktree_path().unwrap());
    let before = snapshot(&path);
    assert!(super::super::worktree_recovery::recover_task(root, &mut task).is_err());
    assert_eq!(before, snapshot(&path));
    assert!(!super::super::worktree_recovery::journal_path(root, &task.id).exists());
}

#[test]
fn dedicated_scratch_merge_keeps_the_reviewed_performer_commit_unchanged() {
    let dir = fixture();
    let root = dir.path();
    let task = worker(root, "worker-01");
    let path = Path::new(task.worktree_path().unwrap());
    std::fs::write(path.join("delivered.txt"), "implementation\n").unwrap();
    git(path, &["add", "delivered.txt"]).unwrap();
    git(path, &["commit", "-qm", "implementation"]).unwrap();
    save(root, &task);
    let before = snapshot(path);
    let result = super::super::runtime::merge_task_with_policy_native(
        root,
        &task.id,
        task.branch().unwrap(),
        "main",
        false,
        None,
        &super::super::engine::MergeTaskContext {
            tool: "future".into(),
            worktree_path: path.to_string_lossy().into(),
            title: "test".into(),
            description: String::new(),
            objective: String::new(),
        },
        |_, _, _, _, _, _| {},
    )
    .unwrap();
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(before, snapshot(path));
    assert_eq!(
        git(root, &["show", "main:delivered.txt"]).unwrap(),
        "implementation"
    );
}

#[test]
fn public_performer_creation_rejects_reserved_slots_before_creating_the_pool() {
    let dir = fixture();
    let root = dir.path();
    for (slug, count) in [("integration", 1), ("supervisor", 2), (".hidden", 1)] {
        let spec = crate::WorktreeCreateSpec {
            slug: slug.into(),
            tool: "future".into(),
            count,
            base: "main".into(),
            dir: PathBuf::from(".macc/worktree"),
            scope: None,
            feature: None,
        };
        let error = crate::create_worktrees(root, &spec).unwrap_err();
        assert!(error.to_string().contains("reserved"), "{error}");
        assert!(!root.join(".macc/worktree").exists());
    }
}

#[test]
fn clear_refuses_to_race_a_worktree_role_mutation() {
    let dir = fixture();
    let guard = lock(dir.path()).unwrap();
    assert!(crate::persistent::idle_guards(dir.path()).is_err());
    drop(guard);
    assert!(crate::persistent::idle_guards(dir.path()).is_ok());
}

#[test]
fn a_changed_scratch_identity_refuses_cleanup_and_keeps_stray_files() {
    let dir = fixture();
    let root = dir.path();
    let scratch = IntegrationWorktree::acquire(root, "main").unwrap();
    let path = scratch.path().to_path_buf();
    drop(scratch);
    std::fs::write(path.join(".macc/integration-owner"), "different checkout").unwrap();
    std::fs::write(path.join("retained.txt"), "do not erase").unwrap();
    let before = snapshot(&path);
    assert!(IntegrationWorktree::acquire(root, "main").is_err());
    assert_eq!(before, snapshot(&path));
    assert_eq!(
        std::fs::read_to_string(path.join("retained.txt")).unwrap(),
        "do not erase"
    );
}

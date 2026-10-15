//! Git-backed regressions for checkpoint exclusion during startup and branch sync.
use super::*;
use serde_json::json;

fn git(root: &Path, args: &[&str]) -> String {
    let output =
        crate::git::run_git_output_mapped(root, args, "completion regression fixture").unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().into()
}
fn registry() -> TaskRegistry {
    TaskRegistry::from_value(
        &json!({"tasks":[{"id":"TASK-001","state":"todo"},{"id":"TASK-002","state":"failed"}]}),
    )
    .unwrap()
}
fn repo() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    commit(root, "base.txt", "base", "chore: initial");
    temp
}
fn commit(root: &Path, file: &str, body: &str, message: &str) {
    std::fs::write(root.join(file), body).unwrap();
    git(root, &["add", file]);
    git(root, &["commit", "-qm", message]);
}
#[test]
fn checkpoint_ids_and_gate_tags_do_not_complete_local_or_external_tasks() {
    for message in [
        "wip: TASK-001 implementation",
        "wip: TASK-001 implementation\n\n[macc:task TASK-001]\n[macc:gate_verdict accepted]",
        "feat: TASK-001 partial\n\n[macc:task TASK-001]\n[macc:result precondition_unmet]",
    ] {
        let commits = vec![GitCommitInfo {
            sha: "checkpoint".into(),
            subject: message.lines().next().unwrap().into(),
            full_message: message.into(),
        }];
        let report = reconcile(&registry(), &commits);
        assert!(report.reconciled.is_empty(), "{message}");
        assert!(report.completion_verdicts.is_empty());
        assert!(reconcile(&TaskRegistry::default(), &commits)
            .external_committed_ids
            .is_empty());
    }
}
#[test]
fn wip_only_branch_is_not_merged_and_published_wip_does_not_satisfy_dependencies() {
    let temp = repo();
    let root = temp.path();
    let base = git(root, &["rev-parse", "main"]);
    git(root, &["checkout", "-qb", "ai/test-tool/worker-01"]);
    commit(
        root,
        "partial.txt",
        "unfinished",
        "wip: TASK-001 implementation",
    );
    let mut tasks = registry();
    let results = sync_unmerged_branches(&mut tasks, root, "main").unwrap();
    assert_eq!(tasks.tasks[0].state, "todo");
    assert!(results.iter().all(|r| r.status != SyncBranchStatus::Merged));
    assert_eq!(git(root, &["rev-parse", "main"]), base);
    assert_eq!(git(root, &["show", "HEAD:partial.txt"]), "unfinished");
    // Even when an operator publishes the checkpoint, clearing transient state
    // must not turn it into delivery or an external dependency completion.
    git(root, &["checkout", "main"]);
    git(root, &["merge", "--ff-only", "ai/test-tool/worker-01"]);
    assert!(!discover_committed_task_ids(root, "main")
        .unwrap()
        .contains("TASK-001"));
    let commits = read_commit_range(root, None, "main").unwrap();
    assert!(reconcile(&tasks, &commits).reconciled.is_empty());
}
#[test]
fn mixed_branch_waits_until_its_partial_task_has_a_later_delivery() {
    let temp = repo();
    let root = temp.path();
    let base = git(root, &["rev-parse", "main"]);
    git(root, &["checkout", "-qb", "ai/test-tool/worker-01"]);
    commit(
        root,
        "complete.txt",
        "done",
        "feat: TASK-001 - delivered\n\n[macc:task TASK-001]",
    );
    commit(
        root,
        "partial.txt",
        "unfinished",
        "wip: TASK-002\n\n[macc:task TASK-002]",
    );
    let mut tasks = registry();
    let skipped = sync_unmerged_branches(&mut tasks, root, "main").unwrap();
    assert!(skipped.iter().all(|r| r.status != SyncBranchStatus::Merged));
    assert_eq!(git(root, &["rev-parse", "main"]), base);
    assert_eq!(tasks.tasks[0].state, "todo");
    assert_eq!(tasks.tasks[1].state, "failed");
    commit(
        root,
        "partial.txt",
        "finished",
        "feat: TASK-002 - delivered\n\n[macc:task TASK-002]",
    );
    let completed = sync_unmerged_branches(&mut tasks, root, "main").unwrap();
    assert!(completed
        .iter()
        .any(|r| r.status == SyncBranchStatus::Merged));
    assert!(tasks.tasks.iter().all(|task| task.state == "merged"));
    let ids = discover_committed_task_ids(root, "main").unwrap();
    assert!(ids.contains("TASK-001"));
    assert!(ids.contains("TASK-002"));
}
#[test]
fn a_new_checkpoint_after_a_delivery_does_not_get_merged_implicitly() {
    let temp = repo();
    let root = temp.path();
    let base = git(root, &["rev-parse", "main"]);
    git(root, &["checkout", "-qb", "ai/test-tool/worker-01"]);
    commit(
        root,
        "source.txt",
        "delivered",
        "feat: TASK-001 - delivered",
    );
    commit(
        root,
        "source.txt",
        "new unfinished work",
        "checkpoint: TASK-001",
    );
    let mut tasks = registry();
    sync_unmerged_branches(&mut tasks, root, "main").unwrap();
    assert_eq!(git(root, &["rev-parse", "main"]), base);
    assert_eq!(tasks.tasks[0].state, "todo");
}

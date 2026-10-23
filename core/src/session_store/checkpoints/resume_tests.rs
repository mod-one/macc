use super::tests::{claim, context, fixture};
use super::*;

fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
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
fn quota_resume_on_descendant_branch_keeps_evidence_and_rotates_the_transcript() {
    let temp = fixture();
    let root = temp.path();
    let request = context(root, "A", "performer", "future-tool");
    let prepared = prepare(root, &request).unwrap();
    let lease = claim(root, &request, &prepared);
    std::fs::write(root.join("code.txt"), "work before quota").unwrap();
    let finished = finish(
        root,
        &FinishRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
            result: "quota".into(),
            acknowledged: true,
        },
    )
    .unwrap();
    release(root, &lease.key, true).unwrap();
    git(root, &["add", "code.txt"]);
    git(root, &["commit", "-m", "retain interrupted implementation"]);
    git(root, &["checkout", "-b", "ai/worker-restarted"]);
    let next = context(root, "A", "performer", "future-tool");
    let resumed = prepare(root, &next).unwrap();
    let saved = get(root, resumed.checkpoint_id.as_ref().unwrap()).unwrap();
    assert_eq!(saved.branch, "ai/worker-restarted");
    assert_eq!(saved.mandatory_task, finished.mandatory_task);
    assert_eq!(saved.source_hashes, finished.source_hashes);
    assert!(saved
        .blocking_issues
        .iter()
        .any(|issue| issue.contains("quota")));
    assert!(saved
        .blocking_issues
        .iter()
        .any(|issue| issue.contains("revalidate")));
    assert!(saved.completed_steps.is_empty());
    let grant = claim(root, &next, &resumed);
    assert!(grant.is_new);
    assert_ne!(grant.key.session_id, lease.key.session_id);
    assert_eq!(
        std::fs::read_to_string(root.join("code.txt")).unwrap(),
        "work before quota"
    );
    release(root, &grant.key, true).unwrap();
}

#[test]
fn a_branch_rename_alone_recaptures_evidence_and_starts_a_fresh_transcript() {
    let temp = fixture();
    let root = temp.path();
    let request = context(root, "A", "performer", "future-tool");
    let prepared = prepare(root, &request).unwrap();
    let lease = claim(root, &request, &prepared);
    release(root, &lease.key, true).unwrap();
    git(root, &["branch", "-m", "ai/renamed"]);
    let request = context(root, "A", "performer", "future-tool");
    let resumed = prepare(root, &request).unwrap();
    let saved = get(root, resumed.checkpoint_id.as_ref().unwrap()).unwrap();
    assert_eq!(saved.branch, "ai/renamed");
    assert!(saved
        .blocking_issues
        .iter()
        .any(|issue| issue.contains("revalidate")));
    let next = claim(root, &request, &resumed);
    assert!(next.is_new);
    assert_ne!(lease.key.session_id, next.key.session_id);
    release(root, &next.key, true).unwrap();
}

#[test]
fn changed_specifications_stay_blocked_even_after_branch_change_or_reverting_the_spec() {
    let temp = fixture();
    let root = temp.path();
    let request = context(root, "A", "performer", "future-tool");
    prepare(root, &request).unwrap();
    git(root, &["checkout", "-b", "ai/restarted"]);
    let path = root.join("worktree.prd.json");
    let original = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, original.replace("Validate", "Different contract")).unwrap();
    let request = context(root, "A", "performer", "future-tool");
    let error = prepare(root, &request).unwrap_err().to_string();
    assert!(error.contains("mandatory PRD or specification sources changed"));
    assert!(error.contains("session-store reconcile"));
    std::fs::write(path, original).unwrap();
    assert!(prepare(root, &request)
        .unwrap_err()
        .to_string()
        .contains("prior checkpoint"));
}

#[test]
fn branch_change_cannot_hide_a_rewound_git_history() {
    let temp = fixture();
    let root = temp.path();
    std::fs::write(root.join("code.txt"), "committed task work").unwrap();
    git(root, &["add", "code.txt"]);
    git(root, &["commit", "-m", "task work"]);
    prepare(root, &context(root, "A", "performer", "future-tool")).unwrap();
    git(root, &["checkout", "-b", "ai/rewound", "HEAD~1"]);
    let error = prepare(root, &context(root, "A", "performer", "future-tool"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not descend"));
}

#[test]
fn a_provider_branch_change_during_execution_still_requires_review() {
    let temp = fixture();
    let root = temp.path();
    let request = context(root, "A", "performer", "future-tool");
    let prepared = prepare(root, &request).unwrap();
    let lease = claim(root, &request, &prepared);
    git(root, &["checkout", "-b", "ai/changed-during-execution"]);
    assert!(finish(
        root,
        &FinishRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
            result: "success".into(),
            acknowledged: true,
        }
    )
    .is_err());
    release(root, &lease.key, false).unwrap();
    let error = prepare(root, &context(root, "A", "performer", "future-tool"))
        .unwrap_err()
        .to_string();
    assert!(error.contains("prior checkpoint"));
}

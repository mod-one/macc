use super::super::tests::{claim, context, fixture};
use super::*;
use std::process::Command;
fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn conflicted() -> (tempfile::TempDir, MergeRepair, ContextRequest) {
    let temp = fixture();
    let root = temp.path();
    git(root, &["checkout", "-b", "feature"]);
    std::fs::write(root.join("code.txt"), "feature\n").unwrap();
    git(root, &["commit", "-am", "feature"]);
    git(root, &["checkout", "main"]);
    std::fs::write(root.join("code.txt"), "main\n").unwrap();
    git(root, &["commit", "-am", "main"]);
    let slot = root.join(".git/macc/integration");
    git(
        root,
        &[
            "worktree",
            "add",
            "--detach",
            slot.to_str().unwrap(),
            "main",
        ],
    );
    assert!(!Command::new("git")
        .arg("-C")
        .arg(&slot)
        .args(["merge", "feature"])
        .output()
        .unwrap()
        .status
        .success());
    let c = MergeRepair {
        worktree: slot.clone(),
        source_worktree: root.into(),
        task_id: "A".into(),
        base_revision: "main".into(),
        feature_revision: "feature".into(),
    };
    capture(root, &c).unwrap();
    let mut request = context(root, "A", "merge_fix", "fake-a");
    request.phase = "merge".into();
    request.identity["execution_root"] = json!(slot);
    (temp, c, request)
}
#[test]
fn only_explicit_integration_repair_can_capture_conflicts_and_preserve_interrupted_work() {
    let (temp, c, request) = conflicted();
    let root = temp.path();
    let mut ordinary = request.clone();
    ordinary.identity["role"] = json!("performer");
    ordinary.phase = "dev".into();
    assert!(prepare(root, &ordinary).is_err());
    let prepared = prepare(root, &request).unwrap();
    let lease = claim(root, &request, &prepared);
    assert!(capture(root, &c).is_err());
    std::fs::write(c.worktree.join("kept.txt"), "retained repair").unwrap();
    finish(
        root,
        &FinishRequest {
            key: lease.key.clone(),
            phase: "merge".into(),
            result: "interrupted".into(),
            acknowledged: false,
        },
    )
    .unwrap();
    release(root, &lease.key, false).unwrap();
    let resumed = prepare(root, &request).unwrap();
    assert!(resumed.prompt.contains("kept.txt"));
    assert_eq!(
        std::fs::read_to_string(c.worktree.join("kept.txt")).unwrap(),
        "retained repair"
    );
    assert!(!git(&c.worktree, &["ls-files", "--unmerged"]).is_empty());
}
#[test]
fn merge_source_drift_needs_reviewed_reconciliation_and_pinned_head_is_enforced() {
    let (temp, c, request) = conflicted();
    let root = temp.path();
    let first = prepare(root, &request).unwrap();
    let path = root.join("worktree.prd.json");
    let mut prd: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    prd["tasks"][0]["acceptance_criteria"] = json!(["Revised mandatory contract"]);
    std::fs::write(path, prd.to_string()).unwrap();
    assert!(prepare(root, &request).is_err());
    let preview = reconcile(root, first.checkpoint_id.as_deref().unwrap(), None).unwrap();
    reconcile(
        root,
        first.checkpoint_id.as_deref().unwrap(),
        preview["current_sha256"].as_str(),
    )
    .unwrap();
    assert!(prepare(root, &request)
        .unwrap()
        .prompt
        .contains("Revised mandatory contract"));
    let merge_path = git(
        &c.worktree,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "MERGE_HEAD",
        ],
    );
    std::fs::write(merge_path, git(root, &["rev-parse", "main"])).unwrap();
    assert!(prepare(root, &request).is_err());
}

#[test]
fn repair_rejects_a_branch_attached_to_the_integration_slot() {
    let (temp, c, request) = conflicted();
    git(&c.worktree, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    assert!(capture(temp.path(), &c).is_err());
    assert!(prepare(temp.path(), &request).is_err());
}

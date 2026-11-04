use super::promotion::{promote_prd, PromoteOptions};
use crate::coordinator::{decisions as dm, model::TaskRegistry};
use serde_json::json;
use std::{path::Path, process::Command};
fn git(root: &Path, args: &[&str]) {
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
fn fixture(register: bool) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Fixture"]);
    git(root, &["config", "user.email", "fixture@example.test"]);
    std::fs::create_dir(root.join(".macc")).unwrap();
    std::fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
    let graph = json!({"lot":"old","tasks":[{"id":"A","title":"Original"}]});
    std::fs::write(root.join("prd.json"), graph.to_string()).unwrap();
    git(root, &["add", "prd.json"]);
    git(root, &["commit", "-m", "baseline"]);
    if register {
        let mut s = dm::Store::open(root).unwrap();
        s.ensure_plan(
            root,
            Path::new("prd.json"),
            "main",
            &TaskRegistry::from_value(&json!({"tasks":graph["tasks"]})).unwrap(),
        )
        .unwrap();
    }
    std::fs::write(
        root.join("candidate.json"),
        json!({"lot":"new","tasks":[{"id":"A","title":"Changed"}]}).to_string(),
    )
    .unwrap();
    std::fs::write(root.join("dirty-user-file.txt"), "preserve me").unwrap();
    tmp
}
fn options(root: &Path, dest: &Path) -> PromoteOptions {
    PromoteOptions {
        source_path: root.join("candidate.json"),
        dest_path: dest.to_owned(),
        yes: true,
        backup_dir: Some(root.join("backups")),
    }
}
#[test]
fn registered_prd_cannot_be_promoted_even_when_disabled_or_missing() {
    let tmp = fixture(true);
    let root = tmp.path();
    let original = std::fs::read(root.join("prd.json")).unwrap();
    let error = promote_prd(&options(root, &root.join("prd.json")))
        .unwrap_err()
        .to_string();
    assert!(error.contains("PRD-GEN-PROMOTE-MANAGED"), "{error}");
    assert!(error.contains("graph preview"));
    assert_eq!(std::fs::read(root.join("prd.json")).unwrap(), original);
    assert!(!root.join("backups").exists());
    std::fs::remove_file(root.join("prd.json")).unwrap();
    assert!(promote_prd(&options(root, &root.join("prd.json"))).is_err());
    assert!(!root.join("prd.json").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("dirty-user-file.txt")).unwrap(),
        "preserve me"
    );
    dm::Store::open(root).unwrap().verify().unwrap();
}
#[test]
fn new_draft_can_be_promoted_but_invalid_intake_never_overwrites_or_backs_up() {
    let tmp = fixture(true);
    let root = tmp.path();
    let draft = root.join("drafts/new.json");
    assert!(promote_prd(&options(root, &draft)).unwrap().promoted);
    let before = std::fs::read(&draft).unwrap();
    std::fs::write(
        root.join("candidate.json"),
        r#"{"tasks":[{"id":"A"}],"decision_requests":{}}"#,
    )
    .unwrap();
    assert!(promote_prd(&options(root, &draft))
        .unwrap_err()
        .to_string()
        .contains("PRD-GEN-PROMOTE-INVALID"));
    assert_eq!(std::fs::read(draft).unwrap(), before);
    assert!(!root.join("backups").exists());
}
#[test]
fn ordinary_promotion_keeps_backup_and_file_creation_behavior() {
    let tmp = fixture(false);
    let root = tmp.path();
    let before = std::fs::read(root.join("prd.json")).unwrap();
    let mut opts = options(root, &root.join("prd.json"));
    opts.yes = false;
    assert!(promote_prd(&opts).is_err());
    assert!(!root.join("backups").exists());
    opts.yes = true;
    let result = promote_prd(&opts).unwrap();
    assert_eq!(
        std::fs::read(result.backup_created.unwrap()).unwrap(),
        before
    );
    assert_eq!(
        std::fs::read(root.join("prd.json")).unwrap(),
        std::fs::read(root.join("candidate.json")).unwrap()
    );
}
#[test]
fn linked_worktree_and_other_reference_cannot_bypass_retained_authority() {
    let tmp = fixture(true);
    let root = tmp.path();
    let linked = root.join("linked");
    git(
        root,
        &["worktree", "add", "-b", "other", linked.to_str().unwrap()],
    );
    let before = std::fs::read(linked.join("prd.json")).unwrap();
    assert!(promote_prd(&options(root, &linked.join("prd.json"))).is_err());
    assert_eq!(std::fs::read(linked.join("prd.json")).unwrap(), before);
}
#[cfg(unix)]
#[test]
fn symlink_alias_cannot_overwrite_the_registered_source() {
    let tmp = fixture(true);
    let root = tmp.path();
    let alias = root.join("alias.json");
    std::os::unix::fs::symlink(root.join("prd.json"), &alias).unwrap();
    let before = std::fs::read(root.join("prd.json")).unwrap();
    assert!(promote_prd(&options(root, &alias)).is_err());
    assert_eq!(std::fs::read(root.join("prd.json")).unwrap(), before);
}
#[test]
fn first_registration_waits_for_promotion_before_reading_source_bytes() {
    let tmp = fixture(false);
    let root = tmp.path();
    let destination = dm::promotion_destination(&root.join("prd.json")).unwrap();
    let guard = dm::guard_prd_promotion(&destination).unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let project = root.to_owned();
    let thread = std::thread::spawn(move || {
        let mut store = dm::Store::open(&project).unwrap();
        started_tx.send(()).unwrap();
        let plan = store
            .ensure_plan(
                &project,
                Path::new("prd.json"),
                "main",
                &TaskRegistry::default(),
            )
            .unwrap();
        done_tx.send(plan).unwrap();
    });
    started_rx
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    assert!(done_rx
        .recv_timeout(std::time::Duration::from_millis(100))
        .is_err());
    let bytes = std::fs::read(root.join("candidate.json")).unwrap();
    std::fs::write(&destination, bytes).unwrap();
    drop(guard);
    let plan = done_rx
        .recv_timeout(std::time::Duration::from_secs(3))
        .unwrap();
    assert_eq!(plan.graph["tasks"][0]["title"], "Changed");
    thread.join().unwrap();
}

#[test]
fn existing_plan_verification_does_not_reacquire_the_publication_lock() {
    let tmp = fixture(true);
    let root = tmp.path();
    std::fs::write(root.join(".macc/macc.yaml"),json!({"tools":{"enabled":[]},"automation":{"coordinator":{"prd_file":"prd.json","reference_branch":"main","decisions":{"enabled":true}}}}).to_string()).unwrap();
    let _fence = dm::publication_lock(root).unwrap();
    let error = dm::merge_ready(root, "A").unwrap_err().to_string();
    assert!(
        error.contains("Missing merge task"),
        "Existing-plan checks must reach the merge fence without locking again: {error}"
    );
}
#[test]
fn promotion_returns_disabled_intake_warning_in_structured_result() {
    let tmp = fixture(false);
    let root = tmp.path();
    std::fs::write(root.join("candidate.json"),json!({"lot":"new","tasks":[{"id":"A"}],"decision_requests":[{"idempotency_key":"choice","task_id":"A","question":"Which contract?","investigated":["Read consumers"],"recommendation":"Keep the contract"}]}).to_string()).unwrap();
    let result = promote_prd(&options(root, &root.join("new.json"))).unwrap();
    assert!(result
        .warnings
        .iter()
        .any(|w| w.contains("PRD-GEN-DECISIONS-DISABLED")));
}
#[test]
fn corrupt_native_authority_is_not_treated_as_an_unmanaged_destination() {
    let tmp = fixture(true);
    let root = tmp.path();
    let before = std::fs::read(root.join("prd.json")).unwrap();
    let db = dm::Store::location(root).unwrap();
    std::fs::write(db, "not sqlite").unwrap();
    assert!(promote_prd(&options(root, &root.join("prd.json"))).is_err());
    assert_eq!(std::fs::read(root.join("prd.json")).unwrap(), before);
    assert!(!root.join("backups").exists());
}

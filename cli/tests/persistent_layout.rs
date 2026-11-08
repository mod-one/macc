//! Real public CLI workflow, temporary repositories only; no login/model/network calls.
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
};
fn git(root: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
fn macc(root: &Path, args: &[&str]) -> Output {
    let o = Command::new(env!("CARGO_BIN_EXE_macc"))
        .args(["--cwd", root.to_str().unwrap()])
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    o
}
#[test]
fn clear_preview_is_read_only_and_execution_migrates_and_preserves_portable_state() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    fs::write(root.join("README"), "test").unwrap();
    git(root, &["add", "README"]);
    git(root, &["commit", "-qm", "Initial"]);
    macc(root, &["init"]);
    let configuration = fs::read(root.join(".macc/macc.yaml")).unwrap();
    let store = macc_core::coordinator::decisions::Store::open(root).unwrap();
    let identity = store.project_id.clone();
    drop(store);
    // Reconstruct the pre-layout directory using an actual native authority.
    fs::remove_dir_all(root.join(".git/macc")).unwrap();
    fs::rename(root.join(".macc/persistent"), root.join(".git/macc")).unwrap();
    fs::remove_file(root.join(".git/macc/layout.json")).unwrap();
    fs::write(
        root.join(".macc/catalog/custom.json"),
        "{\"retained\":true}",
    )
    .unwrap();
    let preview = macc(root, &["clear", "--dry-run", "--no-save-prompt", "--force"]);
    assert!(String::from_utf8_lossy(&preview.stdout).contains("Keep: .macc/macc.yaml"));
    assert!(root.join(".git/macc/control.sqlite").exists());
    assert!(!root.join(".macc/persistent").exists());
    macc(root, &["clear", "--no-save-prompt", "--force"]);
    assert_eq!(
        fs::read(root.join(".macc/macc.yaml")).unwrap(),
        configuration
    );
    assert!(root.join(".macc/catalog/custom.json").exists());
    assert!(root.join(".gitignore").exists());
    let store = macc_core::coordinator::decisions::Store::open(root).unwrap();
    assert_eq!(store.project_id, identity);
    assert!(root.join(".macc/persistent/migration-backups").exists());
    assert!(Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "-q", ".macc/persistent/control.sqlite"])
        .status()
        .unwrap()
        .success());
}

use super::*;
use std::{fs, process::Command};
fn git(root: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().into()
}
fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    git(t.path(), &["init", "-q"]);
    git(t.path(), &["config", "user.name", "Test"]);
    git(t.path(), &["config", "user.email", "test@example.invalid"]);
    fs::write(t.path().join("README"), "test").unwrap();
    git(t.path(), &["add", "README"]);
    git(t.path(), &["commit", "-qm", "Initial"]);
    t
}
#[test]
fn legacy_sqlite_is_backed_up_and_shared_from_a_linked_worktree() {
    let t = fixture();
    let root = t.path();
    let legacy = root.join(".git/macc");
    fs::create_dir_all(&legacy).unwrap();
    let db = rusqlite::Connection::open(legacy.join("control.sqlite")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE evidence(value TEXT); INSERT INTO evidence VALUES('retained');").unwrap();
    drop(db);
    fs::create_dir_all(legacy.join("broker")).unwrap();
    fs::write(legacy.join("broker/proof"), "keep").unwrap();
    let side = t.path().join("side");
    git(
        root,
        &[
            "worktree",
            "add",
            "--detach",
            side.to_str().unwrap(),
            "HEAD",
        ],
    );
    let path = prepare(&side).unwrap();
    assert_eq!(path, root.join(".macc/persistent"));
    assert_eq!(prepare(root).unwrap(), path);
    let db = rusqlite::Connection::open(path.join("control.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT value FROM evidence", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "retained"
    );
    assert!(path.join("broker/proof").exists());
    assert_eq!(
        fs::read_dir(path.join("migration-backups"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(
        fs::read_to_string(legacy.join("control.sqlite")).unwrap(),
        FENCE
    );
    assert!(rusqlite::Connection::open(legacy.join("control.sqlite"))
        .unwrap()
        .execute_batch("CREATE TABLE broken(x);")
        .is_err());
}
#[test]
fn independent_authorities_and_held_legacy_lock_are_not_overwritten() {
    let t = fixture();
    let root = t.path();
    let legacy = root.join(".git/macc");
    fs::create_dir_all(root.join(".macc/persistent")).unwrap();
    fs::write(root.join(".macc/persistent/foreign"), "keep").unwrap();
    assert!(prepare(root)
        .unwrap_err()
        .to_string()
        .contains("no automatic merge"));
    fs::remove_file(root.join(".macc/persistent/foreign")).unwrap();
    let _lock = crate::fs_lock::AdvisoryLock::acquire(
        &legacy.join("controller.lock"),
        std::time::Duration::ZERO,
        "test",
    )
    .unwrap();
    assert!(prepare(root).is_err());
    assert!(!root.join(".macc/persistent/layout.json").exists());
}
#[test]
fn integration_move_preserves_git_registration_and_commit() {
    let t = fixture();
    let root = t.path();
    let old = root.join(".git/macc/integration");
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    git(
        root,
        &["worktree", "add", "--detach", old.to_str().unwrap(), "HEAD"],
    );
    let head = git(&old, &["rev-parse", "HEAD"]);
    prepare(root).unwrap();
    let new = integration_path(root).unwrap();
    assert_eq!(git(&new, &["rev-parse", "HEAD"]), head);
    assert!(git(root, &["worktree", "list", "--porcelain"]).contains(new.to_str().unwrap()));
    assert!(old.is_file());
}
#[test]
fn recovery_after_publication_keeps_identity_and_refuses_a_new_legacy_database() {
    let t = fixture();
    let root = t.path();
    let path = prepare(root).unwrap();
    fs::remove_file(root.join(".git/macc/MOVED")).unwrap();
    assert_eq!(prepare(root).unwrap(), path);
    fs::write(
        root.join(".git/macc/control.sqlite"),
        "new legacy authority",
    )
    .unwrap();
    assert!(prepare(root)
        .unwrap_err()
        .to_string()
        .contains("Both legacy"));
}
#[cfg(unix)]
#[test]
fn symlinked_storage_is_rejected() {
    let t = fixture();
    fs::create_dir_all(t.path().join(".macc")).unwrap();
    std::os::unix::fs::symlink("/tmp", t.path().join(".macc/persistent")).unwrap();
    assert!(prepare(t.path()).is_err());
}

#[test]
fn separate_git_directory_still_anchors_authority_in_the_primary_checkout() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("project");
    let metadata = t.path().join("metadata");
    fs::create_dir(&root).unwrap();
    git(
        &root,
        &[
            "init",
            "-q",
            "--separate-git-dir",
            metadata.to_str().unwrap(),
        ],
    );
    assert_eq!(directory(&root).unwrap(), root.join(".macc/persistent"));
    assert_eq!(prepare(&root).unwrap(), root.join(".macc/persistent"));
}
#[test]
fn dirty_integration_is_preserved_and_refuses_migration() {
    let t = fixture();
    let root = t.path();
    let old = root.join(".git/macc/integration");
    fs::create_dir_all(old.parent().unwrap()).unwrap();
    git(
        root,
        &["worktree", "add", "--detach", old.to_str().unwrap(), "HEAD"],
    );
    fs::write(old.join("unfinished"), "work").unwrap();
    assert!(prepare(root)
        .unwrap_err()
        .to_string()
        .contains("uncommitted work"));
    assert!(old.join("unfinished").exists());
    assert!(!root.join(".macc/persistent/layout.json").exists());
}

#[test]
fn live_sqlite_writer_prevents_migration_without_losing_its_transaction() {
    let t = fixture();
    let root = t.path();
    let legacy = root.join(".git/macc");
    fs::create_dir_all(&legacy).unwrap();
    let db = rusqlite::Connection::open(legacy.join("control.sqlite")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE work(value TEXT); BEGIN IMMEDIATE; INSERT INTO work VALUES('unfinished');").unwrap();
    assert!(prepare(root).is_err());
    assert!(legacy.join("control.sqlite").exists());
    assert!(!root.join(".macc/persistent").exists());
    db.execute_batch("COMMIT").unwrap();
    drop(db);
    let path = prepare(root).unwrap();
    let db = rusqlite::Connection::open(path.join("control.sqlite")).unwrap();
    assert_eq!(
        db.query_row("SELECT value FROM work", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "unfinished"
    );
}
#[test]
fn generic_config_copy_never_copies_live_authority_files() {
    let t = tempfile::tempdir().unwrap();
    let paths = crate::ProjectPaths::from_root(t.path());
    fs::create_dir_all(paths.macc_dir.join("persistent")).unwrap();
    fs::create_dir_all(paths.catalog_dir.clone()).unwrap();
    fs::write(
        paths.macc_dir.join("persistent/control.sqlite"),
        "not a snapshot",
    )
    .unwrap();
    fs::write(paths.catalog_dir.join("portable.json"), "{}").unwrap();
    assert_eq!(
        crate::classify_path(paths.macc_dir.join("persistent"), &paths),
        crate::ManagedPathKind::RuntimeState
    );
    let backup = tempfile::tempdir().unwrap();
    crate::save::classifier::copy_dir_all(&paths.macc_dir, backup.path(), &paths).unwrap();
    assert!(!backup.path().join("persistent").exists());
    assert!(backup.path().join("catalog/portable.json").exists());
}

#[test]
fn implicit_runtime_initialization_keeps_legacy_configuration_ignored_and_checkout_clean() {
    let t = fixture();
    let root = t.path();
    fs::create_dir_all(root.join(".macc")).unwrap();
    fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
    fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
    git(root, &["add", ".gitignore"]);
    git(root, &["commit", "-qm", "Ignore local configuration"]);
    crate::service::project::ensure_initialized_paths(root).unwrap();
    assert_eq!(
        fs::read_to_string(root.join(".gitignore")).unwrap(),
        ".macc/\n"
    );
    assert!(git(root, &["status", "--porcelain"]).is_empty());
    // Visibility changes are reserved for explicit Init/Apply.
    crate::init(&crate::ProjectPaths::from_root(root), false).unwrap();
    assert!(fs::read_to_string(root.join(".gitignore"))
        .unwrap()
        .contains("!/.macc/macc.yaml"));
}

#[test]
fn a_foreign_worktree_at_the_integration_location_is_never_cleaned_or_reset() {
    let primary = fixture();
    let other = fixture();
    let root = primary.path();
    prepare(root).unwrap();
    let slot = integration_path(root).unwrap();
    fs::create_dir_all(slot.parent().unwrap()).unwrap();
    git(
        other.path(),
        &[
            "worktree",
            "add",
            "--detach",
            slot.to_str().unwrap(),
            "HEAD",
        ],
    );
    fs::write(slot.join("retained-foreign-work"), "keep").unwrap();
    let base = git(root, &["branch", "--show-current"]);
    assert!(crate::coordinator::integration::IntegrationWorktree::acquire(root, &base).is_err());
    assert_eq!(
        fs::read_to_string(slot.join("retained-foreign-work")).unwrap(),
        "keep"
    );
}

#[test]
fn an_unregistered_nonempty_integration_directory_is_preserved() {
    let t = fixture();
    let root = t.path();
    prepare(root).unwrap();
    let slot = integration_path(root).unwrap();
    fs::create_dir_all(&slot).unwrap();
    fs::write(slot.join("unknown-work"), "keep").unwrap();
    let base = git(root, &["branch", "--show-current"]);
    assert!(crate::coordinator::integration::IntegrationWorktree::acquire(root, &base).is_err());
    assert_eq!(
        fs::read_to_string(slot.join("unknown-work")).unwrap(),
        "keep"
    );
}

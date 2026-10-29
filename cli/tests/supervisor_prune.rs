//! Pruning is an owned mutation and --force never discards an unmerged repair.
use macc_core::process_ownership::{ClientIdentity, ClientKind, ProcessHandle, ProcessKind};
use macc_core::service::process_ownership::{claim_owner, register_process};
use std::{path::Path, process::Command};

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    git(root, &["commit", "--allow-empty", "-qm", "base"]);
    let mut config = macc_core::config::CanonicalConfig::default();
    config.automation.supervisor = Some(macc_core::supervisor::SupervisorConfig {
        retain_repair_branches: true,
        ..Default::default()
    });
    macc_core::save_canonical_config(&macc_core::ProjectPaths::from_root(root), &config).unwrap();
    tmp
}
fn prune(root: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_macc"))
        .current_dir(root)
        .args(["supervisor", "prune", "--force"])
        .env_remove("MACC_INTERNAL_INVOCATION")
        .env("MACC_CLIENT_ID", "review-cli")
        .output()
        .unwrap()
}
#[test]
fn non_owner_cannot_prune_even_with_force() {
    let tmp = fixture();
    let root = tmp.path();
    git(root, &["branch", "macc/supervisor/retained"]);
    let handle = ProcessHandle {
        kind: ProcessKind::Project,
        project_root: root.to_path_buf(),
        pid: None,
    };
    register_process(root, handle.clone()).unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    claim_owner(
        root,
        &handle,
        ClientIdentity {
            client_id: "other-tui".into(),
            kind: ClientKind::Tui,
            connected_at: now.clone(),
            last_heartbeat: now,
        },
    )
    .unwrap();
    let out = prune(root);
    assert!(
        !out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not the owner"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(macc_core::git::resolve_ref(root, "refs/heads/macc/supervisor/retained").is_some());
}
#[test]
fn force_overrides_retention_but_preserves_unmerged_and_in_use_branches() {
    let tmp = fixture();
    let root = tmp.path();
    git(root, &["branch", "macc/supervisor/merged"]);
    git(root, &["checkout", "-qb", "macc/supervisor/unmerged"]);
    git(
        root,
        &["commit", "--allow-empty", "-qm", "unpublished repair"],
    );
    git(root, &["checkout", "-q", "main"]);
    let slot = root.join(".macc/worktree/repair");
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "macc/supervisor/in-use",
            slot.to_str().unwrap(),
            "main",
        ],
    );
    std::fs::write(slot.join("pending.txt"), "keep exact bytes\n").unwrap();
    let out = prune(root);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(macc_core::git::resolve_ref(root, "refs/heads/macc/supervisor/merged").is_none());
    assert!(macc_core::git::resolve_ref(root, "refs/heads/macc/supervisor/unmerged").is_some());
    assert!(macc_core::git::resolve_ref(root, "refs/heads/macc/supervisor/in-use").is_some());
    assert_eq!(
        std::fs::read_to_string(slot.join("pending.txt")).unwrap(),
        "keep exact bytes\n"
    );
}

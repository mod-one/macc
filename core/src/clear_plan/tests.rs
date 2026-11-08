use super::*;
#[test]
fn runtime_reset_archives_unregistered_evidence_and_preserves_setup() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(tmp.path());
    for rel in [
        ".macc/macc.yaml",
        ".macc/catalog/custom.json",
        ".macc/persistent/test",
        ".macc/context/generated",
        ".macc/log/test.log",
        ".macc/cache/decision-draft.json",
        ".macc/state/task-registry.json",
    ] {
        let p = paths.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "fixture").unwrap();
    }
    let preview = preview(&paths).unwrap();
    assert!(preview
        .items
        .iter()
        .any(|i| i.path == ".macc/context" && i.disposition == Disposition::Remove));
    let result = execute(&paths).unwrap();
    assert!(result.failures.is_empty());
    assert!(!paths.macc_dir.join("context").exists());
    assert!(!paths.macc_dir.join("state").exists());
    let archive = result.archive_path.unwrap();
    assert!(archive.join("project/.macc/log/test.log").exists());
    assert!(archive
        .join("project/.macc/cache/decision-draft.json")
        .exists());
    assert!(archive
        .join("project/.macc/state/task-registry.json")
        .exists());
    assert!(paths.config_path.exists());
    assert!(paths.macc_dir.join("persistent/test").exists());
}

fn git(root: &std::path::Path, args: &[&str]) {
    let output = crate::git::run_git_output_mapped(root, args, "clear fixture").unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn repository() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    std::fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
    std::fs::write(root.join("source"), "base").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "base"]);
    tmp
}
fn slot(root: &Path, name: &str) -> std::path::PathBuf {
    let path = root.join(".macc/worktree").join(name);
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            &format!("ai/test/{name}"),
            path.to_str().unwrap(),
            "HEAD",
        ],
    );
    crate::project_ownership::record_branch(root, &format!("ai/test/{name}")).unwrap();
    path
}
#[test]
fn clean_owned_slots_and_only_merged_owned_branches_are_removed() {
    let tmp = repository();
    let root = tmp.path();
    let paths = ProjectPaths::from_root(root);
    let clean = slot(root, "clean");
    let unmerged = slot(root, "unmerged");
    let dirty = slot(root, "dirty");
    let private = slot(root, "private");
    std::fs::write(unmerged.join("change"), "pending").unwrap();
    git(&unmerged, &["add", "change"]);
    git(&unmerged, &["commit", "-qm", "pending"]);
    std::fs::write(dirty.join("source"), "unfinished").unwrap();
    git(
        &private,
        &["config", "--local", "core.excludesFile", "/dev/null"],
    ); // all slots still use .gitignore
    std::fs::write(private.join(".gitignore"), ".macc/\nprivate.key\n").unwrap();
    git(&private, &["add", ".gitignore"]);
    git(&private, &["commit", "-qm", "private exclusion"]);
    git(root, &["merge", "--ff-only", "ai/test/private"]);
    std::fs::write(private.join("private.key"), "private").unwrap();
    // An unrelated user branch at the same integrated tip is never inferred to be owned.
    git(root, &["branch", "user-branch"]);
    // MACC-created, unoccupied merged branch is pruned as well.
    git(root, &["branch", "ai/test/orphan"]);
    crate::project_ownership::record_branch(root, "ai/test/orphan").unwrap();
    let target = clean.join(".codex/config.toml");
    let local = ProjectPaths::from_root(&clean);
    crate::write_if_changed(&local, ".codex/config.toml", &target, b"generated", |_| {
        Ok(())
    })
    .unwrap();
    // Ignore only the generated file, with MACC-owned hash proving it is disposable.
    let common = crate::git::git_common_dir(root).unwrap();
    std::fs::write(common.join("info/exclude"), ".codex/\n").unwrap();
    std::fs::create_dir_all(clean.join(".macc/log")).unwrap();
    std::fs::write(clean.join(".macc/log/evidence.log"), "evidence").unwrap();
    let before = preview(&paths).unwrap();
    assert!(before
        .items
        .iter()
        .any(|i| i.path.ends_with("/clean") && i.disposition == Disposition::RemoveWorktree));
    let result = execute(&paths).unwrap();
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    assert_eq!(result.removed_worktrees, 1);
    assert_eq!(result.removed_branches, 2);
    assert!(!clean.exists());
    assert!(unmerged.exists());
    assert!(dirty.exists());
    assert!(private.exists());
    assert!(crate::git::resolve_ref(root, "refs/heads/user-branch").is_some());
    assert!(result
        .archive_path
        .unwrap()
        .join("worktrees/clean/.macc/log/evidence.log")
        .exists());
}
#[test]
fn legacy_manifest_unknown_data_is_retained_but_runtime_is_reset() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(tmp.path());
    std::fs::write(paths.root.join("CLAUDE.md"), "possibly modified").unwrap();
    crate::record_managed_path(&paths, "CLAUDE.md").unwrap();
    let result = execute(&paths).unwrap();
    assert!(result.failures.is_empty());
    assert!(paths.root.join("CLAUDE.md").exists());
    assert!(preview(&paths)
        .unwrap()
        .items
        .iter()
        .any(|i| i.path == "CLAUDE.md" && i.disposition == Disposition::RetainUser));
}
#[cfg(unix)]
#[test]
fn symlink_runtime_removal_does_not_follow_its_target() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(tmp.path());
    std::fs::create_dir_all(&paths.macc_dir).unwrap();
    std::fs::write(outside.path().join("keep"), "user").unwrap();
    std::os::unix::fs::symlink(outside.path(), paths.macc_dir.join("context")).unwrap();
    execute(&paths).unwrap();
    assert!(outside.path().join("keep").exists());
    assert!(!paths.macc_dir.join("context").exists());
}

#[test]
fn failed_removal_reports_cause_and_keeps_retry_provenance_after_manifest_archival() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(tmp.path());
    let target = paths.root.join("generated.txt");
    crate::write_if_changed(&paths, "generated.txt", &target, b"generated", |_| Ok(())).unwrap();
    crate::record_managed_path(&paths, "generated.txt").unwrap();
    let report = execution::execute_with_remover(&paths, |_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "fixture denied",
        ))
    })
    .unwrap();
    assert_eq!(report.failures.len(), 1);
    assert!(report.failures[0].contains("fixture denied"));
    assert!(target.exists());
    assert!(!paths.managed_paths_state_path().exists());
    assert!(crate::project_ownership::read(&paths)
        .unwrap()
        .files
        .contains_key("generated.txt"));
    assert!(execute(&paths).unwrap().failures.is_empty());
    assert!(!target.exists());
}
#[test]
fn cleanup_uses_configured_reference_instead_of_unrelated_checkout_head() {
    let tmp = repository();
    let root = tmp.path();
    let paths = ProjectPaths::from_root(root);
    git(root, &["branch", "publication"]);
    let path = slot(root, "published");
    std::fs::write(path.join("delivered"), "delivered").unwrap();
    git(&path, &["add", "delivered"]);
    git(&path, &["commit", "-qm", "delivery"]);
    git(root, &["branch", "-f", "publication", "ai/test/published"]);
    let mut config = crate::config::CanonicalConfig::default();
    config.automation.coordinator = Some(crate::config::CoordinatorConfig {
        reference_branch: Some("publication".into()),
        ..Default::default()
    });
    crate::save_canonical_config(&paths, &config).unwrap();
    let report = execute(&paths).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(!path.exists());
    assert!(crate::git::resolve_ref(root, "refs/heads/ai/test/published").is_none());
    assert!(crate::git::resolve_ref(root, "refs/heads/main").is_some());
    assert!(crate::git::resolve_ref(root, "refs/heads/publication").is_some());
}

#[test]
fn registered_worktree_outside_default_pool_is_not_erased_with_runtime_parent() {
    let tmp = repository();
    let root = tmp.path();
    let paths = ProjectPaths::from_root(root);
    let unusual = root.join(".macc/context/custom-worker");
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "user/unmerged",
            unusual.to_str().unwrap(),
            "HEAD",
        ],
    );
    std::fs::write(unusual.join("source"), "important unfinished work").unwrap();
    let report = execute(&paths).unwrap();
    assert!(report.failures.is_empty());
    assert_eq!(
        std::fs::read_to_string(unusual.join("source")).unwrap(),
        "important unfinished work"
    );
    assert!(crate::list_worktrees(root)
        .unwrap()
        .iter()
        .any(|w| w.path == unusual));
}
#[test]
fn formerly_owned_branch_becoming_publication_target_is_never_pruned() {
    let tmp = repository();
    let root = tmp.path();
    let paths = ProjectPaths::from_root(root);
    git(root, &["branch", "ai/test/publication"]);
    crate::project_ownership::record_branch(root, "ai/test/publication").unwrap();
    let mut config = crate::config::CanonicalConfig::default();
    config.automation.coordinator = Some(crate::config::CoordinatorConfig {
        reference_branch: Some("ai/test/publication".into()),
        ..Default::default()
    });
    crate::save_canonical_config(&paths, &config).unwrap();
    let report = execute(&paths).unwrap();
    assert!(report.failures.is_empty());
    assert!(crate::git::resolve_ref(root, "refs/heads/ai/test/publication").is_some());
}

#[test]
fn unknown_user_slot_is_not_adopted_after_clear_or_deleted_by_the_next_clear() {
    let tmp = repository();
    let root = tmp.path();
    let paths = ProjectPaths::from_root(root);
    let path = root.join(".macc/worktree/user-owned");
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "user/owned",
            path.to_str().unwrap(),
            "HEAD",
        ],
    );
    for _ in 0..2 {
        let report = execute(&paths).unwrap();
        assert!(report.failures.is_empty());
        assert!(path.exists());
        assert!(!crate::project_ownership::read(&paths)
            .unwrap()
            .branches
            .contains("user/owned"));
        assert!(crate::git::resolve_ref(root, "refs/heads/user/owned").is_some());
    }
}

#[cfg(unix)]
#[test]
fn symlinked_session_authority_directory_is_rejected_before_runtime_cleanup() {
    let tmp = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(tmp.path());
    std::fs::create_dir_all(paths.macc_dir.join("context")).unwrap();
    std::fs::write(
        paths.macc_dir.join("context/keep-until-reviewed"),
        "runtime",
    )
    .unwrap();
    std::os::unix::fs::symlink(external.path(), paths.macc_dir.join("state")).unwrap();
    assert!(preview(&paths).is_err());
    assert!(paths.macc_dir.join("context/keep-until-reviewed").exists());
}

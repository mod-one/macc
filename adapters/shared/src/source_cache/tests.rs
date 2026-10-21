use super::*;
fn run(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}
fn fixture() -> (tempfile::TempDir, ProjectPaths, Source) {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("origin");
    fs::create_dir(&repo).unwrap();
    run(&repo, &["init", "-b", "main"]);
    run(&repo, &["config", "user.name", "Test"]);
    run(&repo, &["config", "user.email", "test@example.invalid"]);
    run(&repo, &["config", "commit.gpgsign", "false"]);
    fs::write(repo.join("SKILL.md"), "one").unwrap();
    run(&repo, &["add", "."]);
    run(&repo, &["commit", "-m", "one"]);
    let paths = ProjectPaths::from_root(&dir.path().join("project"));
    fs::create_dir_all(paths.cache_dir.join("sources-v2")).unwrap();
    let source = Source {
        kind: SourceKind::Git,
        url: repo.display().to_string(),
        reference: "main".into(),
        checksum: None,
        subpaths: vec![],
    };
    (dir, paths, source)
}
fn get(
    paths: &ProjectPaths,
    source: &Source,
    refresh: bool,
) -> Result<(PathBuf, SourceProvenance)> {
    materialize_at(paths, source, true, false, refresh, &paths.cache_dir)
}
#[test]
fn snapshots_are_frozen_updates_are_explicit_and_cached_sha_needs_no_remote() {
    let (dir, paths, source) = fixture();
    let (one, p1) = get(&paths, &source, false).unwrap();
    let repo = Path::new(&source.url);
    fs::write(repo.join("SKILL.md"), "two").unwrap();
    run(repo, &["add", "."]);
    run(repo, &["commit", "-m", "two"]);
    assert_eq!(get(&paths, &source, false).unwrap().0, one);
    let (two, p2) = get(&paths, &source, true).unwrap();
    assert_ne!(one, two);
    assert_ne!(p1.resolved.reference, p2.resolved.reference);
    assert_eq!(fs::read_to_string(one.join("SKILL.md")).unwrap(), "one");
    fs::rename(repo, dir.path().join("unreachable-origin")).unwrap();
    assert_eq!(get(&paths, &p1.resolved, false).unwrap().0, one);
    assert_eq!(get(&paths, &source, false).unwrap().0, two);
}
#[test]
fn concurrent_publishers_share_one_snapshot_and_tampering_fails_closed() {
    let (_dir, paths, source) = fixture();
    let roots = std::thread::scope(|scope| {
        let a = scope.spawn(|| get(&paths, &source, false).unwrap().0);
        let b = scope.spawn(|| get(&paths, &source, false).unwrap().0);
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(roots.0, roots.1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(roots.0.join("SKILL.md"), fs::Permissions::from_mode(0o644)).unwrap();
    }
    fs::write(roots.0.join("SKILL.md"), "tampered").unwrap();
    assert!(get(&paths, &source, false)
        .unwrap_err()
        .to_string()
        .contains("integrity"));
}

#[test]
fn legacy_cached_git_can_be_migrated_offline_without_mutating_its_checkout() {
    let (dir, paths, source) = fixture();
    let legacy = paths.source_cache_path(&source.cache_key()).join("repo");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    run(
        dir.path(),
        &["clone", &source.url, legacy.to_str().unwrap()],
    );
    let sha = run(&legacy, &["rev-parse", "HEAD"]);
    fs::write(legacy.join("SKILL.md"), "old checkout local edit").unwrap();
    fs::rename(&source.url, dir.path().join("unreachable-origin")).unwrap();
    let (tree, provenance) =
        materialize_at(&paths, &source, true, true, false, &paths.cache_dir).unwrap();
    assert_eq!(provenance.resolved.reference, sha);
    assert_eq!(fs::read_to_string(tree.join("SKILL.md")).unwrap(), "one");
    assert_eq!(
        fs::read_to_string(legacy.join("SKILL.md")).unwrap(),
        "old checkout local edit"
    );
}

#[test]
fn annotated_tag_updates_record_the_commit_instead_of_the_tag_object() {
    let (_dir, paths, mut source) = fixture();
    let repo = Path::new(&source.url);
    run(repo, &["tag", "-a", "v1", "-m", "version one"]);
    let commit = run(repo, &["rev-parse", "HEAD"]);
    source.reference = "refs/tags/v1".into();
    let (_, provenance) = get(&paths, &source, true).unwrap();
    assert_eq!(provenance.resolved.reference, commit);
}

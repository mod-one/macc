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

#[test]
fn clear_resets_runtime_and_dry_run_has_no_side_effects() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    fs::write(root.join("README"), "fixture").unwrap();
    git(root, &["add", "README"]);
    git(root, &["commit", "-qm", "fixture"]);
    fs::create_dir_all(root.join(".claude")).unwrap();
    fs::write(root.join(".claude/settings.json"), "user").unwrap();
    fs::write(root.join("CLAUDE.md"), "user").unwrap();
    macc(root, &["init", "--fresh", "--no-restore-prompt"]);
    let ignore = fs::read_to_string(root.join(".gitignore")).unwrap();
    let block = ignore
        .split("# BEGIN MACC project visibility")
        .nth(1)
        .unwrap();
    assert!(!block
        .lines()
        .any(|line| line == "/.claude/" || line == "/CLAUDE.md"));
    for relative in [
        ".macc/context/old.json",
        ".macc/log/old.log",
        ".macc/state/old-run.json",
        ".macc/cache/decision-draft.json",
    ] {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "fixture").unwrap();
    }
    let preview = macc(root, &["clear", "--dry-run", "--force", "--no-save-prompt"]);
    let output = String::from_utf8_lossy(&preview.stdout);
    assert!(output.contains("Archive:"));
    assert!(output.contains("Remove: .macc/context"));
    assert!(root.join(".macc/state/old-run.json").exists());
    assert!(!root.join(".macc/persistent/clear-history").exists());
    let cleared = macc(root, &["clear", "--force", "--no-save-prompt"]);
    assert!(String::from_utf8_lossy(&cleared.stdout).contains("Recovery archive:"));
    assert!(!root.join(".macc/context").exists());
    assert!(!root.join(".macc/log").exists());
    assert!(!root.join(".macc/state/old-run.json").exists());
    assert_eq!(fs::read_to_string(root.join("CLAUDE.md")).unwrap(), "user");
    assert_eq!(
        fs::read_to_string(root.join(".claude/settings.json")).unwrap(),
        "user"
    );
    assert!(root.join(".macc/persistent/clear-history").exists());
}

#[test]
fn portable_local_catalog_source_materializes_from_a_different_clone_root() {
    use macc_core::{
        catalog::{Source, SourceKind},
        ProjectPaths,
    };
    let original = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    for root in [original.path(), checkout.path()] {
        fs::create_dir_all(root.join("docs/skill")).unwrap();
        fs::write(root.join("docs/skill/SKILL.md"), "# Portable skill").unwrap();
    }
    // A catalog saved by any client normalizes a project-owned absolute path.
    let paths = ProjectPaths::from_root(original.path());
    let mut entry = macc_core::catalog::SkillsCatalog::default();
    let skill: macc_core::catalog::SkillEntry=serde_json::from_value(serde_json::json!({"id":"portable","name":"Portable","description":"Fixture","tags":[],"selector":{"subpath":"."},"source":{"kind":"local","url":original.path().join("docs/skill"),"ref":""}})).unwrap();
    entry.entries.push(skill);
    entry
        .save_atomically(&paths, &paths.project_skills_catalog_path())
        .unwrap();
    let saved =
        macc_core::catalog::SkillsCatalog::load(&paths.project_skills_catalog_path()).unwrap();
    assert_eq!(saved.entries[0].source.url, "docs/skill");
    let source = Source {
        kind: SourceKind::Local,
        url: saved.entries[0].source.url.clone(),
        reference: String::new(),
        checksum: None,
        subpaths: vec![],
    };
    let resolved = macc_adapter_shared::fetch::materialize_source(
        &ProjectPaths::from_root(checkout.path()),
        &source,
        true,
        true,
    )
    .unwrap();
    assert_eq!(resolved, checkout.path().join("docs/skill"));
    assert!(resolved.join("SKILL.md").is_file());
    let checkout_paths = ProjectPaths::from_root(checkout.path());
    let mut install_entry = saved.entries[0].clone();
    macc_core::catalog::availability::check_install(&checkout_paths, &mut install_entry).unwrap();
    let mut plan = macc_core::plan::ActionPlan::new();
    macc_core::plan::builders::plan_skill_install(&mut plan, "fixture", "portable", &resolved, ".")
        .unwrap();
    let operations = macc_core::plan::collect_plan_operations(&checkout_paths, &plan);
    macc_core::apply_operations(&checkout_paths, &operations, false, |_, _, _| {}).unwrap();
    assert_eq!(
        fs::read_to_string(checkout.path().join(".fixture/skills/portable/SKILL.md")).unwrap(),
        "# Portable skill"
    );
}

#[test]
fn clear_refuses_an_uninitialized_directory_with_only_shared_cache_data() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".macc/cache")).unwrap();
    fs::write(root.join(".macc/cache/keep"), "shared cache").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_macc"))
        .args([
            "--cwd",
            root.to_str().unwrap(),
            "clear",
            "--force",
            "--no-save-prompt",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        fs::read_to_string(root.join(".macc/cache/keep")).unwrap(),
        "shared cache"
    );
    assert!(!root.join(".macc/persistent/clear-history").exists());
}

use super::*;
use crate::plan::{Action, ActionPlan, Scope};

fn git(root: &Path, args: &[&str]) -> String {
    let output = matcher::git(root).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn fixture() -> (tempfile::TempDir, ProjectPaths) {
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init", "-q"]);
    git(tmp.path(), &["config", "user.name", "test"]);
    git(
        tmp.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    git(tmp.path(), &["config", "core.excludesFile", "/dev/null"]);
    let paths = ProjectPaths::from_root(tmp.path());
    (tmp, paths)
}
fn deploy(paths: &ProjectPaths, files: &[&str]) -> crate::ApplyReport {
    let mut plan = ActionPlan::new();
    for pattern in crate::gitignore_policy::BASELINE {
        plan.add_action(Action::EnsureGitignore {
            pattern: pattern.to_string(),
            scope: Scope::Project,
        });
    }
    for path in files {
        plan.add_action(Action::WriteFile {
            path: path.to_string(),
            content: b"generated".to_vec(),
            scope: Scope::Project,
        });
    }
    let ops = crate::plan::collect_plan_operations(paths, &plan);
    crate::apply_operations(paths, &ops, false, |_, _, _| {}).unwrap()
}
fn excludes(paths: &ProjectPaths) -> String {
    fs::read_to_string(
        crate::git::git_common_dir(&paths.root)
            .unwrap()
            .join("info/exclude"),
    )
    .unwrap_or_default()
}
fn ignored(paths: &ProjectPaths, path: &str) -> bool {
    matcher::git(&paths.root)
        .args(["check-ignore", "-q", "--no-index", "--", path])
        .status()
        .unwrap()
        .success()
}

#[test]
fn new_directory_gets_one_local_rule_and_shared_policy_stays_stable() {
    let (_tmp, paths) = fixture();
    deploy(
        &paths,
        &[".tool/settings.json", ".tool/skills/new/SKILL.md"],
    );
    let local = excludes(&paths);
    assert!(local.contains("/.tool/\n"), "{local}");
    assert!(!local.contains("/.tool/settings"));
    let shared = fs::read_to_string(paths.root.join(".gitignore")).unwrap();
    assert!(!shared.contains(".tool"));
    assert!(shared.contains("!/.macc/skills.lock.json"));
    assert!(ignored(&paths, ".tool/skills/new/SKILL.md"));
    deploy(
        &paths,
        &[".tool/settings.json", ".tool/skills/new/SKILL.md"],
    );
    assert_eq!(local, excludes(&paths));
}

#[test]
fn existing_directory_excludes_only_new_subdirectories_and_files() {
    let (_tmp, paths) = fixture();
    fs::create_dir(paths.root.join(".tool")).unwrap();
    fs::write(paths.root.join(".tool/user.json"), "user").unwrap();
    deploy(
        &paths,
        &[".tool/settings.json", ".tool/skills/new/SKILL.md"],
    );
    let local = excludes(&paths);
    assert!(!local.lines().any(|line| line == "/.tool/"));
    assert!(local.contains("/.tool/skills/\n"));
    assert!(local.contains("/.tool/settings.json\n"));
    assert!(!local.contains("SKILL.md"));
    assert!(!ignored(&paths, ".tool/user.json"));
}

#[test]
fn existing_user_agents_exclusion_is_not_duplicated() {
    let (_tmp, paths) = fixture();
    let user = "# own rules\n.agents/\n*.private\n";
    fs::write(paths.root.join(".gitignore"), user).unwrap();
    deploy(
        &paths,
        &[".agents/settings.json", ".agents/skills/new/SKILL.md"],
    );
    assert!(!excludes(&paths).contains(".agents"));
    assert!(fs::read_to_string(paths.root.join(".gitignore"))
        .unwrap()
        .starts_with(user));
    assert!(ignored(&paths, ".agents/settings.json"));
}

#[test]
fn nested_rules_and_local_user_rules_are_respected_without_being_changed() {
    let (_tmp, paths) = fixture();
    fs::create_dir(paths.root.join(".tool")).unwrap();
    fs::write(paths.root.join(".tool/.gitignore"), "skills/\n").unwrap();
    let path = paths.root.join(".git/info/exclude");
    let user = "# exact user bytes\r\n/settings.json\r\n";
    fs::write(&path, user).unwrap();
    deploy(&paths, &[".tool/skills/new/SKILL.md", "settings.json"]);
    let local = excludes(&paths);
    assert!(local.ends_with(user));
    assert!(!local.contains("/.tool/"));
    assert_eq!(local.matches("/settings.json").count(), 1);
}

#[test]
fn user_negation_cannot_be_silently_overridden_by_info_exclude() {
    let (_tmp, paths) = fixture();
    fs::create_dir(paths.root.join(".tool")).unwrap();
    fs::write(paths.root.join(".gitignore"), ".tool/*\n!.tool/skills/\n").unwrap();
    let report = deploy(&paths, &[".tool/skills/new/SKILL.md"]);
    assert!(!ignored(&paths, ".tool/skills/new/SKILL.md"));
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("MACC-EXCLUDE-002")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn clear_prunes_owned_rules_and_preserves_user_local_rules() {
    let (_tmp, paths) = fixture();
    fs::write(paths.root.join(".git/info/exclude"), "# user\n*.private\n").unwrap();
    deploy(&paths, &[".tool/settings.json", "AGENTS.md"]);
    let report = crate::clear(&paths).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    let proof = fs::read_to_string(
        report
            .archive_path
            .as_ref()
            .unwrap()
            .join("generated-project-files-before.json"),
    )
    .unwrap();
    assert!(proof.contains(".tool/settings.json"));
    assert!(proof.contains("AGENTS.md"));
    let local = excludes(&paths);
    assert!(local.ends_with("# user\n*.private\n"));
    assert!(!local.contains(".tool"));
    assert!(!local.contains("AGENTS.md"));
}

#[test]
fn migration_removes_old_managed_file_rules_but_preserves_user_rules() {
    let (_tmp, paths) = fixture();
    fs::write(
        paths.root.join(".gitignore"),
        format!(
            "# user\n.claude/\n{}\n/.macc/*\n/.tool/settings.json\n{}\n",
            crate::gitignore_policy::BEGIN,
            crate::gitignore_policy::END
        ),
    )
    .unwrap();
    deploy(&paths, &[".tool/settings.json"]);
    let shared = fs::read_to_string(paths.root.join(".gitignore")).unwrap();
    assert!(shared.starts_with("# user\n.claude/\n"));
    assert!(!shared.contains(".tool"));
    assert!(excludes(&paths).contains("/.tool/\n"));
}

#[test]
fn common_rules_never_hide_user_content_in_another_worktree() {
    let (_tmp, paths) = fixture();
    git(&paths.root, &["commit", "--allow-empty", "-qm", "initial"]);
    let worker_tmp = tempfile::tempdir().unwrap();
    let worker = worker_tmp.path().join("worker");
    git(
        &paths.root,
        &["worktree", "add", "--detach", worker.to_str().unwrap()],
    );
    fs::create_dir(worker.join(".tool")).unwrap();
    fs::write(worker.join(".tool/user.txt"), "user").unwrap();
    fs::write(worker.join("AGENTS.md"), "user").unwrap();
    let report = deploy(&paths, &[".tool/skills/new/SKILL.md", "AGENTS.md"]);
    let local = excludes(&paths);
    assert!(!local.lines().any(|line| line == "/.tool/"));
    assert!(local.contains("/.tool/skills/\n"));
    assert!(!local.contains("/AGENTS.md\n"));
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("MACC-EXCLUDE-001")));
    assert!(!ignored(
        &ProjectPaths::from_root(&worker),
        ".tool/user.txt"
    ));
    // Applying from a linked checkout reaches the same shared file and preserves rules.
    reconcile(&ProjectPaths::from_root(&worker)).unwrap();
    assert_eq!(local, excludes(&paths));
}

#[test]
fn apply_after_clone_reconstructs_exclusions_from_new_local_creation() {
    let (_tmp, paths) = fixture();
    deploy(&paths, &[".tool/settings.json"]);
    git(&paths.root, &["add", ".gitignore"]);
    git(&paths.root, &["commit", "-qm", "portable setup"]);
    let clone_tmp = tempfile::tempdir().unwrap();
    let clone = clone_tmp.path().join("clone");
    git(
        clone_tmp.path(),
        &[
            "clone",
            "-q",
            "--no-local",
            paths.root.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    let clone = ProjectPaths::from_root(&clone);
    assert!(!excludes(&clone).contains(".tool"));
    deploy(&clone, &[".tool/settings.json"]);
    assert!(excludes(&clone).contains("/.tool/\n"));
    assert!(ignored(&clone, ".tool/settings.json"));
}

#[test]
fn malformed_blocks_and_literal_metacharacters_are_safe() {
    let before = format!("# user\n{BEGIN}\n*.private\n");
    let rules = BTreeSet::from(["/.tool/".into()]);
    let after = rewrite(&before, &rules, &[]);
    assert!(after.ends_with(&before));
    assert_eq!(rewrite(&after, &rules, &[]), after);
    let (_tmp, paths) = fixture();
    deploy(&paths, &["tool[1]/generated", "assets/a*b.txt"]);
    assert!(ignored(&paths, "tool[1]/generated"));
    assert!(!ignored(&paths, "tool1/user.txt"));
}

#[test]
fn bootstrap_never_adopts_rules_from_an_unterminated_user_block() {
    let (_tmp, paths) = fixture();
    let before = format!("# user\n{BEGIN}\n/visible\n!/visible\n");
    fs::write(paths.root.join(".git/info/exclude"), &before).unwrap();
    bootstrap(&paths.root).unwrap();
    assert!(excludes(&paths).ends_with(&before));
    assert_eq!(
        excludes(&paths)
            .lines()
            .filter(|line| *line == "/visible")
            .count(),
        1
    );
    assert!(!ignored(&paths, "visible"));
}

#[test]
fn partial_apply_failure_keeps_successfully_generated_assets_excluded() {
    let (_tmp, paths) = fixture();
    fs::write(paths.root.join("z-file"), "blocks parent creation").unwrap();
    let mut plan = ActionPlan::new();
    for file in ["a-tool/settings.json", "z-file/impossible"] {
        plan.add_action(Action::WriteFile {
            path: file.into(),
            content: b"generated".to_vec(),
            scope: Scope::Project,
        });
    }
    let operations = crate::plan::collect_plan_operations(&paths, &plan);
    assert!(crate::apply_operations(&paths, &operations, false, |_, _, _| {}).is_err());
    assert!(paths.root.join("a-tool/settings.json").exists());
    assert!(ignored(&paths, "a-tool/settings.json"));
}

#[test]
fn global_rules_are_used_without_changing_user_git_configuration() {
    let (_tmp, paths) = fixture();
    let global = tempfile::NamedTempFile::new().unwrap();
    fs::write(global.path(), ".tool/\n").unwrap();
    git(
        &paths.root,
        &[
            "config",
            "core.excludesFile",
            global.path().to_str().unwrap(),
        ],
    );
    deploy(&paths, &[".tool/settings.json"]);
    assert!(!excludes(&paths).contains(".tool"));
    assert_eq!(
        git(&paths.root, &["config", "--get", "core.excludesFile"]).trim(),
        global.path().to_str().unwrap()
    );
}

#[test]
fn tracked_assets_are_reported_without_untracking_anything() {
    let (_tmp, paths) = fixture();
    deploy(&paths, &[".tool/settings.json"]);
    git(&paths.root, &["add", "-f", ".tool/settings.json"]);
    let report = deploy(&paths, &[".tool/settings.json"]);
    assert!(report
        .warnings
        .iter()
        .any(|w| w.contains("MACC-EXCLUDE-003")));
    assert!(git(&paths.root, &["ls-files"]).contains(".tool/settings.json"));
}

#[cfg(unix)]
#[test]
fn symlinked_exclude_file_is_refused_before_generating_assets() {
    let (_tmp, paths) = fixture();
    let user_file = tempfile::NamedTempFile::new().unwrap();
    fs::write(user_file.path(), "user rules").unwrap();
    let exclude = paths.root.join(".git/info/exclude");
    fs::remove_file(&exclude).unwrap();
    std::os::unix::fs::symlink(user_file.path(), &exclude).unwrap();
    let mut plan = ActionPlan::new();
    plan.add_action(Action::WriteFile {
        path: ".tool/settings.json".into(),
        content: b"generated".to_vec(),
        scope: Scope::Project,
    });
    assert!(crate::apply_operations(
        &paths,
        &crate::plan::collect_plan_operations(&paths, &plan),
        false,
        |_, _, _| {}
    )
    .is_err());
    assert!(!paths.root.join(".tool/settings.json").exists());
    assert_eq!(fs::read_to_string(user_file.path()).unwrap(), "user rules");
}

#[test]
fn standalone_worker_writes_install_and_repair_shared_local_exclusions() {
    let (_tmp, root) = fixture();
    let project_rules = "# project policy\n*.private\n\n";
    fs::write(root.root.join(".gitignore"), project_rules).unwrap();
    git(&root.root, &["add", ".gitignore"]);
    git(&root.root, &["commit", "-qm", "initial"]);
    let tmp = tempfile::tempdir().unwrap();
    let slot = tmp.path().join("slot");
    git(
        &root.root,
        &["worktree", "add", "--detach", slot.to_str().unwrap()],
    );
    let worker = ProjectPaths::from_root(&slot);
    let prd = root.root.join("prd.json");
    fs::write(&prd, r#"{"tasks":[{"id":"A"}]}"#).unwrap();
    let scope =
        || crate::coordinator::helpers::write_worktree_prd_for_task(&prd, "A", &slot).unwrap();
    scope();
    assert!(ignored(&worker, "worktree.prd.json"));
    crate::ensure_performer(&slot).unwrap();
    let local = root.root.join(".git/info/exclude");
    fs::write(&local, "# custom\n*.mine\n").unwrap();
    crate::ensure_performer(&slot).unwrap();
    assert!(ignored(&worker, "performer.sh"));
    fs::write(&local, "# custom\n*.mine\n").unwrap();
    scope();
    assert!(ignored(&worker, "worktree.prd.json"));
    for checkout in [&worker, &root] {
        assert_eq!(
            fs::read_to_string(checkout.root.join(".gitignore")).unwrap(),
            project_rules
        );
        assert!(ignored(checkout, "performer.sh"));
        assert!(ignored(checkout, "worktree.prd.json"));
        assert!(!ignored(checkout, "scripts/performer.sh"));
    }
    deploy(
        &worker,
        &[".codex/config.toml", ".codex/skills/sample/SKILL.md"],
    );
    for checkout in [&worker, &root] {
        for path in [
            ".macc/state/tool-sessions.json",
            ".macc/log/run",
            ".macc/persistent/generated-project-files.json",
            ".macc/memory.db-wal",
            ".codex/config.toml",
            ".codex/skills/sample/SKILL.md",
        ] {
            assert!(ignored(checkout, path), "{path}");
        }
        for path in [
            ".macc/macc.yaml",
            ".macc/catalog/nested/item.json",
            ".macc/skills.lock.json",
        ] {
            assert!(!ignored(checkout, path), "{path}");
        }
    }
    let local = fs::read_to_string(local).unwrap();
    assert_eq!(local.lines().filter(|l| *l == "/.codex/").count(), 1);
    assert!(!local.contains("/.codex/config.toml"));
    assert!(local.contains("*.mine\n"));

    // A user can introduce a conflicting path after a common rule was installed.
    fs::write(root.root.join("performer.sh"), "my launcher").unwrap();
    assert_ne!(
        crate::ensure_performer(&root.root).unwrap(),
        root.root.join("performer.sh")
    );
    assert!(!ignored(&root, "performer.sh"));
    assert_eq!(
        fs::read_to_string(root.root.join("performer.sh")).unwrap(),
        "my launcher"
    );
}

#[test]
fn existing_user_runner_files_stay_visible_and_intact() {
    let (_tmp, paths) = fixture();
    for name in ["performer.sh", "worktree.prd.json"] {
        fs::write(paths.root.join(name), "user content").unwrap();
    }
    let prd = paths.root.join("prd.json");
    fs::write(&prd, r#"{"tasks":[{"id":"A"}]}"#).unwrap();
    assert_ne!(
        crate::ensure_performer(&paths.root).unwrap(),
        paths.root.join("performer.sh")
    );
    assert!(
        crate::coordinator::helpers::write_worktree_prd_for_task(&prd, "A", &paths.root).is_err()
    );
    deploy(&paths, &[".codex/settings.json"]);
    for name in ["performer.sh", "worktree.prd.json"] {
        assert_eq!(
            fs::read_to_string(paths.root.join(name)).unwrap(),
            "user content"
        );
        assert!(!ignored(&paths, name));
    }
}

#[test]
fn runtime_bootstrap_respects_personal_ignores_before_project_setup() {
    let (_tmp, paths) = fixture();
    let global = tempfile::NamedTempFile::new().unwrap();
    fs::write(global.path(), "*.yaml\n").unwrap();
    git(
        &paths.root,
        &[
            "config",
            "core.excludesFile",
            global.path().to_str().unwrap(),
        ],
    );
    for prepare in [true, false] {
        if prepare {
            bootstrap(&paths.root).unwrap();
        } else {
            reconcile(&paths).unwrap();
        }
        assert!(ignored(&paths, ".macc/macc.yaml"));
        assert!(ignored(&paths, ".macc/supervisor-owner.json"));
        assert!(!ignored(&paths, ".macc/catalog/nested/file.json"));
        assert!(!ignored(&paths, ".macc/skills.lock.json"));
    }
}

#[test]
fn task_delivery_preserves_user_files_and_excludes_only_proven_additions() {
    use crate::coordinator::integration::{IntegrationWorktree, PublishOutcome};
    let (_tmp, base) = fixture();
    let holder = tempfile::tempdir().unwrap();
    let worker = ProjectPaths::from_root(holder.path().join("worker"));
    git(&base.root, &["checkout", "-b", "reference"]);
    fs::create_dir(base.root.join(".tool")).unwrap();
    fs::write(base.root.join(".tool/tracked"), "original").unwrap();
    git(&base.root, &["add", ".tool/tracked"]);
    git(&base.root, &["commit", "-qm", "base"]);
    let original = git(&base.root, &["rev-parse", "HEAD"]);
    fs::write(base.root.join(".tool/config"), "personal config").unwrap();
    git(
        &base.root,
        &[
            "worktree",
            "add",
            "-b",
            "candidate",
            worker.root.to_str().unwrap(),
        ],
    );
    deploy(&worker, &[".tool/config"]);
    fs::write(worker.root.join(".tool/tracked"), "updated").unwrap();
    fs::write(worker.root.join(".tool/user"), "new user file").unwrap();
    git(&worker.root, &["add", "-f", ".tool/config"]);
    let rules = excludes(&base);
    assert!(crate::git::stage_task_changes(&worker.root).unwrap());
    let staged = git(&worker.root, &["diff", "--cached", "--name-only"]);
    assert!(staged.lines().any(|p| p == ".tool/tracked"));
    assert!(staged.lines().any(|p| p == ".tool/user"));
    assert!(!staged.lines().any(|p| p == ".tool/config"));
    assert!(!ignored(&base, ".tool/config"));
    assert_eq!(rules, excludes(&base));
    // A direct agent commit bypasses staging. Check the integrated tree again.
    git(&worker.root, &["add", "-f", ".tool/config"]);
    git(&worker.root, &["commit", "-qm", "candidate"]);
    let scratch = IntegrationWorktree::acquire(&base.root, "reference").unwrap();
    git(scratch.path(), &["merge", "--ff-only", "candidate"]);
    assert_eq!(
        scratch.publish("candidate").unwrap(),
        PublishOutcome::PrivateAdditions(vec![".tool/config".into()])
    );
    assert_eq!(original, git(&base.root, &["rev-parse", "HEAD"]));
    assert_eq!(
        fs::read(base.root.join(".tool/config")).unwrap(),
        b"personal config"
    );
    assert_eq!(
        fs::read(worker.root.join(".tool/config")).unwrap(),
        b"generated"
    );
}

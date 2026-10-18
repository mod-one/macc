//! Full native coordinator → real hook → common SQLite runner regression.
use super::*;
use macc_core::{
    coordinator::{engine::MergeTaskContext, runtime::merge_task_with_policy_native},
    session_store as store,
};
fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn exercise_merge_repair(source_drift: bool) {
    let (temp, lib) = setup();
    let root = temp.path();
    std::fs::write(root.join("code.txt"), "initial\n").unwrap();
    git(root, &["add", "code.txt"]);
    git(root, &["commit", "-m", "base"]);
    git(root, &["checkout", "-b", "feature"]);
    std::fs::write(root.join("code.txt"), "feature\n").unwrap();
    git(root, &["commit", "-am", "feature"]);
    let feature = git(root, &["rev-parse", "HEAD"]);
    git(root, &["checkout", "main"]);
    std::fs::write(root.join("code.txt"), "main\n").unwrap();
    git(root, &["commit", "-am", "main"]);
    let base = git(root, &["rev-parse", "HEAD"]);
    // Fake provider still flows through checkpoint ACK and actual Git resolution.
    let fake = root.join("fake-ai.sh");
    std::fs::write(
        &fake,
        r#"#!/bin/bash
set -eu
printf '%s\n' "${@: -1}" | sed -n '/^MACC_CHECKPOINT_ACK: /p'
printf 'main\nfeature\n' > code.txt
git add code.txt
git -c user.name=Test -c user.email=test@example.invalid commit --no-edit >/dev/null
echo "session id: ${2:-merge-provider}"
for ((i=0;i<5000;i++)); do echo verbose-merge-evidence; done
echo 'MACC_TASK_RESULT: success_without_changes' 
"#,
    )
    .unwrap();
    if source_drift {
        let script = std::fs::read_to_string(&fake).unwrap();
        let changed = r#"printf '%s' '{"tasks":[{"id":"A","acceptance_criteria":["Changed source"]}]}' > "$(dirname "$0")/worktree.prd.json"
"#;
        std::fs::write(
            &fake,
            script.replace(
                "for ((i=0;i<5000;i++));",
                &format!("{changed}for ((i=0;i<5000;i++));"),
            ),
        )
        .unwrap();
    }
    let wrapper = root.join(".macc/runner.sh");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/bash\nTOOL_ID=future-ai\nTOOL_LOG_PREFIX=test\nsource '{}' \"$@\"\n",
            lib.display()
        ),
    )
    .unwrap();
    let mut spec: Value =
        serde_json::from_slice(&std::fs::read(root.join(".macc/tool.json")).unwrap()).unwrap();
    spec["performer"]["runner"] = json!(wrapper);
    std::fs::write(root.join(".macc/tool.json"), spec.to_string()).unwrap();
    let hook = macc_core::ProjectPaths::from_root(root).automation_merge_fix_hook_path();
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    let actual = Path::new(env!("CARGO_MANIFEST_DIR")).join("../automat/hooks/ai-merge-fix.sh");
    std::fs::write(&hook,format!("#!/bin/bash\nexport MACC_SESSION_SERVICE_BIN='{}'\nexport MACC_INTERNAL_INVOCATION=1\nexec bash '{}' \"$@\"\n",env!("CARGO_BIN_EXE_macc"),actual.display())).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [&wrapper, &hook] {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    let context = MergeTaskContext {
        tool: "future-ai".into(),
        worktree_path: root.display().to_string(),
        ..Default::default()
    };
    let result = merge_task_with_policy_native(
        root,
        "A",
        "feature",
        "main",
        true,
        Some(30),
        &context,
        |_, _, _, _, _, _| {},
    )
    .unwrap();
    if source_drift {
        assert!(
            result.is_err(),
            "A source-changing hook must not publish its commit"
        );
        assert_eq!(git(root, &["rev-parse", "HEAD"]), base);
    } else {
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            std::fs::read_to_string(root.join("code.txt")).unwrap(),
            "main\nfeature\n"
        );
        assert_ne!(git(root, &["rev-parse", "HEAD"]), base);
        assert!(Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["merge-base", "--is-ancestor", &feature, "HEAD"])
            .status()
            .unwrap()
            .success());
    }
    let state = store::inspect(root).unwrap();
    assert!(state.sessions.iter().all(|s| s.role == "merge_fix"));
    assert!(state.sessions.iter().all(|s| s.status != "active"));
    assert!(state
        .checkpoints
        .iter()
        .any(|c| c.phase == "merge" && c.reason == "invocation_finished"));
    // A normal performer may not adopt the detached integration slot.
    let slot = root.join(".git/macc/integration");
    assert!(slot.join(".macc/merge-repair.json").exists());
    let req = json!({"worktree":root,"source_worktree":root,"task_id":"A","base_revision":base,"feature_revision":feature});
    assert!(!cli(root, &["merge-repair-prepare"], Some(&req))
        .status
        .success());
}

#[test]
fn sqlite_merge_repair_resolves_conflict_without_reusing_performer_history() {
    exercise_merge_repair(false);
}
#[test]
fn completed_merge_commit_is_not_published_when_checkpoint_sources_change() {
    exercise_merge_repair(true);
}

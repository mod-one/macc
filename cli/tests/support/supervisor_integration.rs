//! Baseline-aware validation, reference-branch publishing and retry regressions.
use super::*;

fn git_out(root: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_owned()
}
fn settled(root: &Path, status: &str) -> Value {
    wait_for(|| report(root).is_some_and(|v| v["intervention"]["status"] == status));
    report(root).unwrap()
}
fn edit_config(root: &Path, from: &str, to: &str) {
    let config = root.join(".macc/macc.yaml");
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(text.contains(from));
    std::fs::write(config, text.replace(from, to)).unwrap();
}

#[test]
fn validation_already_failing_on_the_base_does_not_block_the_repair() {
    let temp = fixture(false);
    let root = temp.path();
    // An environment failure unrelated to the repair, like a missing system library.
    edit_config(
        root,
        "    validation_commands:\n",
        "    validation_commands:\n      - \"echo environment-broken; exit 1\"\n",
    );
    let supervisor = start(root);
    let r = settled(root, "recovered");
    assert_eq!(
        r["intervention"]["preexisting_failures"],
        json!(["echo environment-broken; exit 1"])
    );
    assert!(r["intervention"]["detail"]
        .as_str()
        .unwrap()
        .contains("already failing on the unmodified base"));
    let dir = Path::new(r["intervention"]["log_dir"].as_str().unwrap());
    assert!(dir.join("baseline-0.log").exists());
    assert!(!dir.join("baseline-1.log").exists());
    // The independent verifier is told to compare both logs.
    assert!(std::fs::read_to_string(dir.join("verification-prompt.txt"))
        .unwrap()
        .contains("baseline-N.log"));
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "fixed\n"
    );
    assert!(r["intervention"]["next_action"].is_null());
    drop(supervisor);
}

#[test]
fn repair_is_published_to_the_reference_branch_not_the_checked_out_branch() {
    let temp = fixture(false);
    let root = temp.path();
    git(root, &["checkout", "-q", "-b", "feature"]);
    let feature = git_out(root, &["rev-parse", "feature"]);
    let supervisor = start(root);
    let r = settled(root, "recovered");
    assert_eq!(r["intervention"]["target_branch"], "master");
    assert_eq!(git_out(root, &["show", "master:defect.txt"]), "fixed");
    assert_eq!(
        git_out(root, &["rev-parse", "master"]),
        r["intervention"]["commit"].as_str().unwrap()
    );
    // The operator's checkout is left exactly where it was.
    assert_eq!(git_out(root, &["rev-parse", "feature"]), feature);
    assert_eq!(git_out(root, &["branch", "--show-current"]), "feature");
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "broken\n"
    );
    assert!(!Path::new(r["intervention"]["worktree"].as_str().unwrap()).exists());
    drop(supervisor);
}

#[test]
fn unrelated_local_changes_no_longer_block_and_overlapping_ones_are_preserved() {
    let temp = fixture(false);
    let root = temp.path();
    // The coordinator's own preflight decides whether a dirty reference
    // checkout may run; the supervisor must not be stricter than it.
    edit_config(
        root,
        "    reference_branch: master\n",
        "    reference_branch: master\n    require_clean_reference_branch: false\n",
    );
    std::fs::write(root.join("notes.txt"), "scratch\n").unwrap();
    std::fs::write(root.join("defect.txt"), "operator edit\n").unwrap();
    let supervisor = start(root);
    let r = settled(root, "escalated");
    let detail = r["intervention"]["detail"].as_str().unwrap();
    assert!(detail.contains("integration deferred"), "{detail}");
    assert!(r["intervention"]["next_action"]
        .as_str()
        .unwrap()
        .contains("Commit or stash"));
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "operator edit\n"
    );
    assert_eq!(git_out(root, &["show", "master:defect.txt"]), "broken");
    // The verified repair stays staged on its base for the next attempt.
    let slot = Path::new(r["intervention"]["worktree"].as_str().unwrap());
    assert_eq!(
        git_out(slot, &["rev-parse", "HEAD"]),
        git_out(root, &["rev-parse", "master"])
    );
    assert_eq!(
        git_out(slot, &["diff", "--cached", "--name-only"]),
        "defect.txt"
    );
    // Removing the overlap is new evidence: the same slot is integrated.
    git(root, &["checkout", "--", "defect.txt"]);
    let r = settled(root, "recovered");
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "fixed\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).unwrap(),
        "scratch\n"
    );
    assert!(r["intervention"]["restart_pid"].as_i64().is_some());
    drop(supervisor);
}

#[test]
fn explicit_retry_rediagnoses_in_a_slot_that_still_holds_the_repair() {
    let temp = fixture(true);
    let root = temp.path();
    let supervisor = start(root);
    let r = settled(root, "escalated");
    let slot = Path::new(r["intervention"]["worktree"].as_str().unwrap()).to_path_buf();
    assert!(!git_out(&slot, &["status", "--porcelain"]).is_empty());
    drop(supervisor);
    let script = root.join(".macc/fake-agent.sh");
    std::fs::write(
        &script,
        std::fs::read_to_string(&script)
            .unwrap()
            .replace("\"verified\":false", "\"verified\":true"),
    )
    .unwrap();
    let supervisor = Supervisor(
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .args([
                "--cwd",
                root.to_str().unwrap(),
                "supervisor",
                "start",
                "--retry",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let r = settled(root, "recovered");
    assert_eq!(r["intervention"]["attempt"], 2);
    assert!(root
        .join(".macc/log/supervisor/run-fixture-1-attempt-2/diagnosis-prompt.txt")
        .exists());
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "fixed\n"
    );
    drop(supervisor);
}

#[test]
fn missing_git_identity_escalates_before_any_ai_call() {
    let temp = fixture(false);
    let root = temp.path();
    git(root, &["config", "--unset", "user.name"]);
    git(root, &["config", "--unset", "user.email"]);
    let supervisor = Supervisor(
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .args(["--cwd", root.to_str().unwrap(), "supervisor", "start"])
            .env("HOME", root.join(".macc/empty-home"))
            .env("XDG_CONFIG_HOME", root.join(".macc/empty-home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let r = settled(root, "escalated");
    assert!(r["intervention"]["detail"]
        .as_str()
        .unwrap()
        .contains("Git identity is missing"));
    assert!(r["intervention"]["next_action"]
        .as_str()
        .unwrap()
        .contains("macc doctor --fix"));
    let dir = Path::new(r["intervention"]["log_dir"].as_str().unwrap());
    assert!(!dir.join("diagnosis-prompt.txt").exists());
    drop(supervisor);
}

#[test]
fn report_command_prints_reason_next_step_and_macc_findings() {
    let temp = fixture(true);
    let root = temp.path();
    let supervisor = start(root);
    settled(root, "escalated");
    for command in ["report", "status"] {
        let output = Command::new(env!("CARGO_BIN_EXE_macc"))
            .args(["--cwd", root.to_str().unwrap(), "supervisor", command])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(output.status.success(), "{text}");
        assert!(
            text.contains("reason: Validation error: Repair verification rejected"),
            "{text}"
        );
        assert!(
            text.contains("next: Resolve the reason above, then run `macc supervisor stop"),
            "{text}"
        );
        assert!(text.contains("MACC findings (1):"), "{text}");
        assert!(
            text.contains("1. Normalized logs masked the precondition"),
            "{text}"
        );
    }
    drop(supervisor);
}

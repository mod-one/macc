//! Phase switches and settings-only recovery use the production incident loop.
use super::*;

#[test]
fn checks_can_be_disabled_and_repair_integrated_without_restarting() {
    let temp = fixture(true); // verifier would reject if invoked
    let root = temp.path();
    edit_config(
        root,
        "    validation_commands:\n      - \"test $(cat defect.txt) = fixed\"",
        "    tests_enabled: false\n    ai_verification_enabled: false\n    auto_restart: false",
    );
    let supervisor = start(root);
    let r = settled(root, "repair_integrated");
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "fixed\n"
    );
    assert!(r["intervention"]["restart_pid"].is_null());
    assert!(r["intervention"]["validation"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(r["intervention"]["detail"]
        .as_str()
        .unwrap()
        .contains("skipped by configuration"));
    let dir = Path::new(r["intervention"]["log_dir"].as_str().unwrap());
    assert!(!dir.join("verification-prompt.txt").exists());
    assert!(!dir.join("validation-0.log").exists());
    drop(supervisor);
}

#[test]
fn missing_checks_wait_then_reuse_diagnosis_after_configuration() {
    let temp = fixture(false);
    let root = temp.path();
    edit_config(
        root,
        "    validation_commands:\n      - \"test $(cat defect.txt) = fixed\"",
        "    auto_restart: false",
    );
    let supervisor = start(root);
    let first = settled(root, "escalated");
    assert!(first["intervention"]["detail"]
        .as_str()
        .unwrap()
        .contains("requires validation_commands"));
    assert!(first["intervention"]["next_action"]
        .as_str()
        .unwrap()
        .contains("supervisor config check"));
    let first_dir = first["intervention"]["log_dir"].as_str().unwrap();
    assert!(!Path::new(first_dir).join("repair-prompt.txt").exists());
    edit_config(root, "    auto_restart: false", "    auto_restart: false\n    validation_commands:\n      - \"test $(cat defect.txt) = fixed\"");
    let r = settled(root, "repair_integrated");
    assert!(r["intervention"]["attempt"].as_u64().unwrap() > 1);
    let dir = Path::new(r["intervention"]["log_dir"].as_str().unwrap());
    assert!(dir.join("repair-prompt.txt").exists());
    assert!(
        !dir.join("diagnosis-prompt.txt").exists(),
        "settings-only retry must reuse the diagnostic"
    );
    assert!(dir.join("verification-prompt.txt").exists());
    drop(supervisor);
}

#[test]
fn diagnosis_only_never_repairs_or_commits() {
    let temp = fixture(false);
    let root = temp.path();
    edit_config(
        root,
        "    tool: fake",
        "    tool: fake\n    repair_enabled: false",
    );
    // Diagnosis does not create a commit and must work without author identity.
    git(root, &["config", "user.name", ""]);
    git(root, &["config", "user.email", ""]);
    let head = git_out(root, &["rev-parse", "HEAD"]);
    let supervisor = start(root);
    let r = settled(root, "escalated");
    assert!(r["intervention"]["detail"]
        .as_str()
        .unwrap()
        .contains("Automatic repair is disabled"));
    assert_eq!(git_out(root, &["rev-parse", "HEAD"]), head);
    assert_eq!(r["intervention"]["repair_rounds"], 0);
    drop(supervisor);
}

#[test]
fn human_gate_wins_over_contradictory_verification_acceptance() {
    let temp = fixture(false);
    let root = temp.path();
    let script = root.join(".macc/fake-agent.sh");
    let text = std::fs::read_to_string(&script).unwrap().replace(
        "\"verified\":true",
        "\"verified\":true,\"requires_human\":true",
    );
    std::fs::write(script, text).unwrap();
    let head = git_out(root, &["rev-parse", "HEAD"]);
    let supervisor = start(root);
    let r = settled(root, "escalated");
    assert!(r["intervention"]["detail"]
        .as_str()
        .unwrap()
        .contains("human decision"));
    assert_eq!(git_out(root, &["rev-parse", "HEAD"]), head);
    assert!(r["intervention"]["restart_pid"].is_null());
    drop(supervisor);
}

#[test]
fn cli_settings_roundtrip_rejects_invalid_edits_atomically() {
    let temp = fixture(false);
    let root = temp.path();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .args(["--cwd", root.to_str().unwrap(), "supervisor", "config"])
            .args(args)
            .output()
            .unwrap()
    };
    assert!(run(&["set", "tests_enabled", "false"]).status.success());
    let path = root.join(".macc/macc.yaml");
    let before = std::fs::read_to_string(&path).unwrap();
    assert!(!run(&["set", "max_repair_rounds", "0"]).status.success());
    assert!(!run(&["set", "typo", "true"]).status.success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    let config = macc_core::load_canonical_config(&path).unwrap();
    assert!(!config.automation.supervisor.unwrap().tests_enabled);
    assert_eq!(
        config
            .automation
            .coordinator
            .unwrap()
            .reference_branch
            .as_deref(),
        Some("master")
    );
    assert!(run(&["check"]).status.success());
    let show = run(&["show"]);
    assert!(show.status.success());
    assert!(String::from_utf8_lossy(&show.stdout).contains("ai_verification_enabled"));
}

#[test]
fn tests_can_be_disabled_while_ai_verification_still_runs() {
    let temp = fixture(false);
    let root = temp.path();
    edit_config(
        root,
        "    tool: fake",
        "    tool: fake\n    tests_enabled: false\n    auto_restart: false",
    );
    let supervisor = start(root);
    let r = settled(root, "repair_integrated");
    let dir = Path::new(r["intervention"]["log_dir"].as_str().unwrap());
    assert!(!dir.join("validation-0.log").exists());
    assert!(dir.join("verification-prompt.txt").exists());
    assert!(std::fs::read_to_string(dir.join("verification-prompt.txt"))
        .unwrap()
        .contains("validation was explicitly disabled"));
    drop(supervisor);
}

#[test]
fn ai_verification_can_be_disabled_while_checks_still_run() {
    let temp = fixture(true);
    let root = temp.path();
    edit_config(
        root,
        "    tool: fake",
        "    tool: fake\n    ai_verification_enabled: false\n    auto_restart: false",
    );
    let supervisor = start(root);
    let r = settled(root, "repair_integrated");
    let dir = Path::new(r["intervention"]["log_dir"].as_str().unwrap());
    assert!(dir.join("validation-0.log").exists());
    assert!(!dir.join("verification-prompt.txt").exists());
    assert!(r["intervention"]["detail"]
        .as_str()
        .unwrap()
        .contains("AI verification=skipped"));
    drop(supervisor);
}

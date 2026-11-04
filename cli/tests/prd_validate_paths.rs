//! Reproduce validation from a launcher directory different from --cwd.
use std::{path::Path, process::Command};
fn validate(launcher: &Path, cwd: &Path, file: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_macc"))
        .current_dir(launcher)
        .arg("--cwd")
        .arg(cwd)
        .args(["prd", "validate"])
        .arg(file)
        .arg("--json")
        .output()
        .unwrap()
}
#[test]
fn validation_uses_effective_cwd_and_preserves_absolute_paths() {
    let launcher = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    std::fs::create_dir_all(root.join(".macc/scenarios")).unwrap();
    std::fs::create_dir(root.join("nested")).unwrap();
    std::fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
    let file = root.join(".macc/scenarios/context.prd.json");
    std::fs::write(
        &file,
        r#"{"lot":"Path regression","tasks":[{"id":"PATH-001","title":"Validate"}]}"#,
    )
    .unwrap();
    // A same-named invalid launcher file must never shadow the selected project.
    std::fs::create_dir_all(launcher.path().join(".macc/scenarios")).unwrap();
    std::fs::write(
        launcher.path().join(".macc/scenarios/context.prd.json"),
        "invalid",
    )
    .unwrap();
    for (cwd, path) in [
        (root, Path::new(".macc/scenarios/context.prd.json")),
        (root, file.as_path()),
        (
            root.join("nested").as_path(),
            Path::new("../.macc/scenarios/context.prd.json"),
        ),
    ] {
        let out = validate(launcher.path(), cwd, path);
        assert!(
            out.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(json["ok"], true);
    }
    let out = validate(launcher.path(), root, Path::new("missing.prd.json"));
    assert!(!out.status.success());
    // The CLI also emits its normal terminal diagnostic after failed validation.
    let json: serde_json::Value = serde_json::Deserializer::from_slice(&out.stdout)
        .into_iter()
        .next()
        .unwrap()
        .unwrap();
    assert!(json["errors"][0]
        .as_str()
        .unwrap()
        .contains("PRD-GEN-OUTPUT-MISSING"));
}

#[test]
fn native_validation_rejects_malformed_decisions_and_reports_disabled_intake() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    std::fs::create_dir(root.join(".macc")).unwrap();
    std::fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
    let file = root.join("prd.json");
    std::fs::write(
        &file,
        r#"{"lot":"test","tasks":[{"id":"A"}],"decision_requests":{}}"#,
    )
    .unwrap();
    let out = validate(root, root, &file);
    assert!(!out.status.success());
    let result: serde_json::Value = serde_json::Deserializer::from_slice(&out.stdout)
        .into_iter()
        .next()
        .unwrap()
        .unwrap();
    assert!(result["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e.as_str().unwrap().contains("PRD-GEN-DECISION-REQUESTS")));
    let bytes = r#"{"lot":"test","tasks":[{"id":"A"}],"decision_requests":[{"idempotency_key":"device","task_id":"A","kind":"intervention","question":"Connect the phone?","investigated":["No physical access"],"recommendation":"Connect and return actual status","intervention":{"action":"Connect and unlock the phone","inaccessible_reason":"No physical access","expected_result":"Actual device status"}}]}"#;
    std::fs::write(&file, bytes).unwrap();
    let out = validate(root, root, &file);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let result: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(result["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|w| w.as_str().unwrap().contains("PRD-GEN-DECISIONS-DISABLED")));
    assert_eq!(std::fs::read_to_string(file).unwrap(), bytes);
}

#[test]
fn cli_promotion_cannot_overwrite_a_native_plan_with_yes() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.name", "Fixture"],
        vec!["config", "user.email", "fixture@example.test"],
    ] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
    std::fs::create_dir(root.join(".macc")).unwrap();
    std::fs::write(root.join(".macc/macc.yaml"),r#"{"tools":{"enabled":[]},"automation":{"coordinator":{"prd_file":"prd.json","reference_branch":"main","decisions":{"enabled":true}}}}"#).unwrap();
    let original = r#"{"lot":"original","tasks":[{"id":"A","title":"Keep"}]}"#;
    std::fs::write(root.join("prd.json"), original).unwrap();
    std::fs::write(
        root.join("candidate.json"),
        r#"{"lot":"new","tasks":[{"id":"A","title":"Replace"}]}"#,
    )
    .unwrap();
    std::fs::write(root.join("user-work.txt"), "Retain dirty user work").unwrap();
    let list = Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("--cwd")
        .arg(root)
        .args(["decisions", "list"])
        .output()
        .unwrap();
    assert!(
        list.status.success(),
        "{}",
        String::from_utf8_lossy(&list.stdout)
    );
    let out = Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("--cwd")
        .arg(root)
        .args(["prd", "promote", "candidate.json", "--yes", "--json"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("PRD-GEN-PROMOTE-MANAGED"),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("prd.json")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read_to_string(root.join("user-work.txt")).unwrap(),
        "Retain dirty user work"
    );
    assert!(!root.join(".macc/backups").exists());
    let verify = Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("--cwd")
        .arg(root)
        .args(["decisions", "verify"])
        .output()
        .unwrap();
    assert!(
        verify.status.success(),
        "{}",
        String::from_utf8_lossy(&verify.stdout)
    );
}

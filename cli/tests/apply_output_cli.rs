//! User-facing output contracts; fixtures never log in, invoke AI, or save user profiles.
use std::process::{Command, Output, Stdio};
fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join(".macc/catalog")).unwrap();
    std::fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
    let mut catalog: serde_json::Value =
        serde_json::from_str(include_str!("../../catalog/skills.catalog.json")).unwrap();
    for entry in catalog["entries"].as_array_mut().unwrap() {
        entry["mandatory"] = serde_json::json!(false);
    }
    std::fs::write(
        root.join(".macc/catalog/skills.catalog.json"),
        catalog.to_string(),
    )
    .unwrap();
    temp
}
fn invoke(root: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_macc"))
        .args(["--cwd", root.to_str().unwrap(), "apply"])
        .args(args)
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}
#[test]
fn json_preview_is_one_document_without_terminal_sequences_or_progress() {
    let temp = fixture();
    let result = invoke(temp.path(), &["--dry-run", "--json"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&result.stdout)
        .expect("stdout must contain exactly one JSON document");
    assert!(value.is_object());
    assert!(!result.stdout.contains(&27));
    assert!(!temp.path().join(".macc/memory.db").exists());
}
#[test]
fn json_without_consent_fails_without_apply_and_reports_the_reason() {
    let temp = fixture();
    let result = invoke(temp.path(), &["--json"]);
    assert!(!result.status.success());
    let value: serde_json::Value =
        serde_json::from_slice(&result.stdout).expect("errors remain machine readable");
    assert_eq!(value["status"], "failed");
    assert!(value["error"]
        .as_str()
        .unwrap()
        .contains("explicit consent"));
    assert!(!temp.path().join(".gitignore").exists());
    assert!(!result.stderr.contains(&27));
}
#[test]
fn malformed_configuration_still_returns_a_json_error() {
    let temp = fixture();
    std::fs::write(temp.path().join(".macc/macc.yaml"), "{broken").unwrap();
    let result = invoke(temp.path(), &["--json"]);
    assert!(!result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["status"], "failed");
}

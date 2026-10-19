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

//! Real public CLI lifecycle; no model calls or network access.
use serde_json::{json, Value};
use std::{
    path::Path,
    process::{Command, Output},
};
fn cli(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("--cwd")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}
fn value(out: Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn public_cli_manages_offline_snapshots_required_claims_and_stale_revisions() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(".macc")).unwrap();
    std::fs::write(
        root.path().join(".macc/macc.yaml"),
        "tools:\n  enabled: []\nsettings:\n  offline: true\n",
    )
    .unwrap();
    let before = value(cli(root.path(), &["hooks", "list", "--json"]));
    assert_eq!(before["hooks"].as_array().unwrap().len(), 7);
    let blocked = cli(root.path(), &["hooks", "disable", "performer-log-summary"]);
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("O2 context engine"));
    let manifest = root.path().join("hook.json");
    std::fs::write(&manifest,json!({"id":"external-check","name":"External check","description":"Keep warnings","version":"1.0.0","event":"tool_output","default_enabled":false,"handler":{"kind":"rules","steps":[{"kind":"retain_lines","pattern":"warning"}]},"skills":["fixture-check"],"source":{"repository":"local","subpath":"hooks/external","distribution":"snapshot"}}).to_string()).unwrap();
    let reviewed = value(cli(
        root.path(),
        &["hooks", "inspect", manifest.to_str().unwrap()],
    ));
    let installed = cli(
        root.path(),
        &[
            "hooks",
            "import",
            manifest.to_str().unwrap(),
            "--sha256",
            reviewed["sha256"].as_str().unwrap(),
        ],
    );
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    // Changing/deleting the original checkout does not affect the installed snapshot.
    std::fs::remove_file(&manifest).unwrap();
    assert!(cli(root.path(), &["hooks", "enable", "external-check"])
        .status
        .success());
    let current = value(cli(root.path(), &["hooks", "list", "--json"]));
    assert!(current["hooks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h["hook"]["id"] == "external-check" && h["enabled"] == true));
    let stale = cli(
        root.path(),
        &[
            "hooks",
            "disable",
            "external-check",
            "--revision",
            before["revision"].as_str().unwrap(),
        ],
    );
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("revision conflict"));
    assert!(cli(root.path(), &["hooks", "remove", "external-check"])
        .status
        .success());
    assert_eq!(
        value(cli(root.path(), &["hooks", "list", "--json"]))["hooks"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
}

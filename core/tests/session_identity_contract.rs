//! Ensure Rust hints and every shell adapter agree on the ownership contract.
use macc_core::coordinator::session_identity::{identity, SessionRouting};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

#[test]
fn shell_and_rust_session_identity_contract_match() {
    if Command::new("bash")
        .args(["-c", "command -v jq"])
        .output()
        .map(|out| !out.status.success())
        .unwrap_or(true)
    {
        eprintln!("Skipping shell identity contract: bash/jq unavailable");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../adapters/shared/session_identity.sh");
    let spec_path = repo.join("tool.json");
    for scope in ["task_role_tool", "task", "worktree", "project"] {
        let spec = json!({"id":"future-ai", "model_tiers":{"heavy":{"model":"example-v2"}},
            "performer":{"command":"future-ai", "args":["--model","example-v1"],
                "session":{"enabled":true, "scope":scope}}});
        std::fs::write(&spec_path, spec.to_string()).unwrap();
        let expected = identity(
            repo,
            repo,
            "TASK-1",
            "reviewer",
            "future-ai",
            &spec,
            &SessionRouting {
                tier: "heavy".into(),
                mode: "auto".into(),
            },
        )
        .unwrap();
        let output = Command::new("bash")
            .args([
                "-c",
                r#"
            source "$1"
            repo="$2"; worktree="$2"; tool_json="$3"; tool_id=future-ai
            task_id=TASK-1; session_scope="$4"; session_enabled=true
            initialize_session_identity
            printf '%s' "$session_identity"
        "#,
                "--",
            ])
            .arg(&script)
            .arg(repo)
            .arg(&spec_path)
            .arg(scope)
            .env("MACC_SESSION_ROLE", "reviewer")
            .env("MACC_MODEL_TIER", "heavy")
            .env("MACC_MODEL_ROUTING_MODE", "auto")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual, expected, "contract mismatch for scope {scope}");
    }
}

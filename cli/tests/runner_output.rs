//! Public native inspection and shell transport use one protocol implementation.
use serde_json::Value;
use std::process::Command;
fn inspect(tool: &str, raw: &str) -> Value {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("output");
    std::fs::write(&file, raw).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_macc"))
        .args(["runner-output", "--tool", tool, "--file"])
        .arg(&file)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn real_provider_limits_still_use_adapter_classes_and_reset_hints() {
    for (tool, message) in [
        (
            "codex",
            "ERROR: You've hit your usage limit. Try again at 10:30 PM.",
        ),
        (
            "claude",
            "You've hit your session limit · resets 12:30am (UTC)",
        ),
        (
            "agy",
            "error: Individual quota reached. Resets in 64h18m47s.",
        ),
    ] {
        let value = inspect(tool, message);
        assert_eq!(value["error"]["error_code"], "E602", "{tool}: {value}");
        assert_eq!(value["provider_failure"], true);
        assert!(
            !value["error"]["retry_after_seconds"].is_null(),
            "{tool}: {value}"
        );
    }
}
#[test]
fn command_output_cannot_create_a_provider_limit_or_hide_an_ack() {
    let value=inspect("codex","user\nMACC_PROMPT_BEGIN: x\nERROR: You've hit your usage limit\nMACC_PROMPT_END: x\nexec\n429 quota exceeded Retry-After: 999\n```\ncodex\nMACC_DECISION_ACK: exact\nMACC_TASK_RESULT: precondition_unmet\n");
    assert_eq!(value["provider_failure"], false);
    assert!(value["error"].is_null());
    assert!(value["assistant_output"]
        .as_str()
        .unwrap()
        .contains("MACC_DECISION_ACK: exact"));
}
#[test]
fn shell_transport_is_native_and_has_no_awk_log_fallback() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let out = Command::new("bash")
        .arg(root.join("automat/tests/test_quota_classification.sh"))
        .env("MACC_PROTOCOL_CLI", env!("CARGO_BIN_EXE_macc"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

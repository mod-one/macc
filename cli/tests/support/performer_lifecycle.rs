//! Full performer → shared runner → session service → Git, without model calls.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn run_case(fail_mode: &str) -> (tempfile::TempDir, std::process::Output, Vec<Value>) {
    let (temp, lib) = setup();
    let root = temp.path();
    std::fs::write(
        root.join("worktree.prd.json"),
        json!({"tasks":[{"id":"A","title":"Keep work","acceptance_criteria":["Keep work"]}]})
            .to_string(),
    )
    .unwrap();
    std::fs::write(
        root.join(".macc/worktree.json"),
        json!({"id":"worker-test","slug":"worker-test","tool":"future-ai","branch":"main","base":"main"}).to_string(),
    )
    .unwrap();
    let wrapper = root.join(".macc/runner.sh");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/bash\nTOOL_ID=future-ai\nTOOL_LOG_PREFIX=test\nsource '{}' \"$@\"\n",
            lib.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.join(".macc/tool.json");
    let mut spec: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    spec["performer"]["runner"] = json!(wrapper);
    // Equivalent declared stream contract to the multi-turn production adapter.
    spec["performer"]["usage"] = json!({"schema":"future-stream-v2","required_for_checkpoint":true,"append_args":["--verbose"],"replace_values":{"--output-format":"stream-json"},"event_match":{"/type":"result"},"text_match":{"/type":"result"},"text":["/result"],"text_arrays":[{"event_match":{"/type":"assistant"},"path":"/message/content","item_match":{"/type":"text"},"text":["/text"]}],"session_id":"/session_id","fields":{"input_uncached":"/usage/input","output":"/usage/output"}});
    for mode in ["create", "resume"] {
        spec["performer"]["session"][mode]["args"] = json!([
            root.join("fake-ai.sh"),
            mode,
            "{session_id}",
            "--output-format",
            "text"
        ]);
    }
    std::fs::write(path, spec.to_string()).unwrap();
    std::fs::write(root.join("fake-ai.sh"),r#"#!/bin/bash
set -eu
printf '%s\t%s\n' "$1" "$2" >> "$CALL_LOG"
[[ "$3" == --output-format && "$4" == stream-json && "$5" == --verbose ]] || exit 42
if [[ "$FAIL_MODE" == transient && ! -f .macc/first-failure ]]; then
  touch .macc/first-failure
  echo 'Error: temporary transport failure' >&2
  exit 23
fi
printf 'retained work\n' > implementation.txt
ack="$(printf '%s\n' "${@: -1}" | sed -n '/^MACC_CHECKPOINT_ACK: /p')"
if [[ "$FAIL_MODE" != missing_ack ]]; then
  jq -nc --arg ack "$ack" '{type:"assistant",message:{content:[{type:"thinking",thinking:"private-unprojected"},{type:"text",text:$ack}]}}'
else
  # A tool echo of the prompt must not satisfy assistant acknowledgement.
  jq -nc --arg ack "$ack" '{type:"user",message:{content:[{type:"text",text:$ack}]}}'
fi
jq -nc --arg sid "$2" '{type:"result",session_id:$sid,result:"MACC_TASK_RESULT: success_with_changes",usage:{input:17,output:3}}'
"#).unwrap();
    let out = Command::new("bash")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../automat/performer.sh"))
        .arg("--repo")
        .arg(root)
        .arg("--worktree")
        .arg(root)
        .args(["--task-id", "A", "--tool", "future-ai", "--registry"])
        .arg(root.join("registry.json"))
        .arg("--prd")
        .arg(root.join("worktree.prd.json"))
        .env("COORD_EVENTS_FILE", root.join(".macc/events.jsonl"))
        .env("MACC_SESSION_SERVICE_BIN", env!("CARGO_BIN_EXE_macc"))
        .env("MACC_INTERNAL_INVOCATION", "1")
        .env("MACC_SESSION_ROLE", "performer")
        .env("MACC_MODEL_TIER", "")
        .env("PERFORMER_SPINNER", "false")
        .env("CALL_LOG", root.join("calls.tsv"))
        .env("FAIL_MODE", fail_mode)
        .current_dir(root)
        .output()
        .unwrap();
    if !out.status.success() {
        if let Ok(log) = std::fs::read_to_string(root.join(".macc/log/performer/A.md")) {
            eprintln!("PERFORMER LOG ({fail_mode}): {log}");
        }
    }
    let events = std::fs::read_to_string(root.join(".macc/events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    (temp, out, events)
}
#[test]
fn streamed_initial_ack_allows_actual_performer_commit_in_passive_capture_mode() {
    let (temp, out, events) = run_case("none");
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let root = temp.path();
    let commit = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["log", "-1", "--format=%s%n%b"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&commit.stdout).contains("A"));
    let committed = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["show", "HEAD:implementation.txt"])
        .output()
        .unwrap();
    assert_eq!(committed.stdout, b"retained work\n");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("command not found"));
    assert!(events
        .iter()
        .any(|e| e["type"] == "phase_result" && e["status"] == "done"));
    let usage = value(cli(root, &["usage"], None));
    assert_eq!(usage["records"][0]["incremental"]["input_uncached"], 17);
}
#[test]
fn transient_retry_is_progress_until_the_final_attempt_completes() {
    let (temp, out, events) = run_case("transient");
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("calls.tsv"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    assert!(!events
        .iter()
        .any(|e| e["type"] == "phase_result" && e["status"] == "failed"));
    assert!(events
        .iter()
        .any(|e| e["type"] == "progress" && e["payload"]["exit_status"] == 23));
}
#[test]
fn checkpoint_protocol_failure_is_terminal_preserves_work_and_never_retries_or_commits() {
    let (temp, out, events) = run_case("missing_ack");
    assert!(!out.status.success());
    let root = temp.path();
    assert_eq!(
        std::fs::read_to_string(root.join("calls.tsv"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert!(root.join("implementation.txt").exists());
    let commit = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["log", "-1", "--format=%s"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&commit.stdout).trim(), "initial");
    let failure = events
        .iter()
        .find(|e| e["type"] == "phase_result" && e["status"] == "failed")
        .unwrap();
    assert_eq!(failure["payload"]["error_code"], "E103");
    assert!(failure["payload"]["result_exp"]
        .as_str()
        .unwrap()
        .contains("did not acknowledge"));
}

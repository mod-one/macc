//! Exercise the actual adapter library and CLI session transport without AI calls.
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
fn cli(root: &Path, args: &[&str], input: Option<&Value>) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("--cwd")
        .arg(root)
        .arg("session-store")
        .args(args)
        .env("MACC_INTERNAL_INVOCATION", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(body) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(body.to_string().as_bytes())
            .unwrap();
    } else {
        drop(child.stdin.take());
    }
    child.wait_with_output().unwrap()
}
fn value(out: std::process::Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn setup() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join(".macc/state")).unwrap();
    std::fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: []\n").unwrap();
    let fake = root.join("fake-ai.sh");
    std::fs::write(
        &fake,
        r#"#!/bin/bash
printf '%s\t%s\n' "$1" "${2:-}" >> "$CALL_LOG"
if [[ "${FAKE_MODE:-}" == sleep ]]; then echo "$$" > "$READY_FILE"; exec sleep 60; fi
if [[ "${FAKE_MODE:-}" == quota ]]; then echo "You've hit your usage limit"; exit 0; fi
if [[ "${FAKE_MODE:-}" == fail ]]; then echo 'transport failed' >&2; exit 23; fi
echo "session id: ${2:-fresh-provider-id}"
echo 'MACC_TASK_RESULT: success_without_changes'
"#,
    )
    .unwrap();
    let spec = json!({"id":"future-ai","performer":{"command":"bash","args":[fake.to_str().unwrap(),"new"],"prompt":{"mode":"arg"},"session":{
        "enabled":true,"scope":"task_role_tool","id_strategy":"generated","create":{"command":"bash","args":[fake.to_str().unwrap(),"create","{session_id}"]},
        "resume":{"command":"bash","args":[fake.to_str().unwrap(),"resume","{session_id}"]}}}});
    std::fs::write(root.join(".macc/tool.json"), spec.to_string()).unwrap();
    std::fs::write(root.join("prompt.txt"), "Implement task\n").unwrap();
    let plan = value(cli(root, &["migrate"], None));
    value(cli(
        root,
        &[
            "migrate",
            "--apply",
            "--expected-sha256",
            plan["source_sha256"].as_str().unwrap(),
        ],
        None,
    ));
    (
        temp,
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../adapters/shared/performer_lib.sh"),
    )
}
fn runner(root: &Path, lib: &Path, task: &str, role: &str) -> Command {
    let mut c = Command::new("bash");
    c.args([
        "-c",
        r#"TOOL_ID=future-ai; TOOL_LOG_PREFIX=test; source "$1" "${@:2}""#,
        "--",
    ])
    .arg(lib)
    .args(["--repo"])
    .arg(root)
    .arg("--worktree")
    .arg(root)
    .arg("--tool-json")
    .arg(root.join(".macc/tool.json"))
    .arg("--prompt-file")
    .arg(root.join("prompt.txt"))
    .args(["--task-id", task])
    .env("MACC_SESSION_SERVICE_BIN", env!("CARGO_BIN_EXE_macc"))
    .env("MACC_SESSION_ROLE", role)
    .env("MACC_MODEL_TIER", "")
    .env("MACC_MODEL_ROUTING_MODE", "auto")
    .env("CALL_LOG", root.join("calls.tsv"))
    .current_dir(root);
    c
}
#[test]
fn real_runner_reuses_same_task_and_keeps_review_independent() {
    let (temp, lib) = setup();
    let root = temp.path();
    for (task, role) in [
        ("A", "performer"),
        ("A", "performer"),
        ("A", "reviewer"),
        ("A", "performer"),
        ("B", "performer"),
    ] {
        let out = runner(root, &lib, task, role).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let calls = std::fs::read_to_string(root.join("calls.tsv")).unwrap();
    let rows = calls
        .lines()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .collect::<Vec<_>>();
    assert_eq!(
        rows.iter().map(|r| r[0]).collect::<Vec<_>>(),
        ["create", "resume", "create", "resume", "create"]
    );
    assert_eq!(rows[0][1], rows[1][1]);
    assert_eq!(rows[0][1], rows[3][1]);
    assert_ne!(rows[0][1], rows[2][1]);
    assert_ne!(rows[0][1], rows[4][1]);
    let status = value(cli(root, &["status"], None));
    assert_eq!(status["sessions"].as_array().unwrap().len(), 3);
    assert!(status["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["owner_pid"].is_null()));
    let frozen: Value = serde_json::from_slice(
        &std::fs::read(root.join(".macc/state/tool-sessions.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(frozen["session_store"]["mode"], "sqlite");
    assert!(frozen["tools"].is_array());
}
#[test]
fn failed_or_uncreated_sessions_do_not_enter_a_resume_loop() {
    let (temp, lib) = setup();
    let root = temp.path();
    let out = runner(root, &lib, "A", "performer")
        .env("FAKE_MODE", "quota")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let out = runner(root, &lib, "A", "performer")
        .env("FAKE_MODE", "fail")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(23));
    let out = runner(root, &lib, "A", "performer").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let calls = std::fs::read_to_string(root.join("calls.tsv")).unwrap();
    assert!(calls.lines().all(|l| l.starts_with("create\t")));
}
#[cfg(unix)]
#[test]
fn termination_quarantines_lease_and_stops_the_provider() {
    let (temp, lib) = setup();
    let root = temp.path();
    let ready = root.join("provider.pid");
    let mut child = runner(root, &lib, "A", "performer")
        .env("FAKE_MODE", "sleep")
        .env("READY_FILE", &ready)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while !ready.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(ready.exists());
    let pid: i32 = std::fs::read_to_string(&ready)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    while child.try_wait().unwrap().is_none() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
        panic!("Runner failed to terminate promptly");
    }
    assert!(!macc_core::coordinator::helpers::is_pid_running(pid as i64));
    let status = value(cli(root, &["status"], None));
    assert_eq!(status["sessions"][0]["status"], "abandoned");
    assert!(status["sessions"][0]["owner_pid"].is_null());
}

//! Real Linux detached/double-forked writers; no model or live project involved.
use super::*;
use std::time::{Duration, Instant};

fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(path.exists(), "{} was not created", path.display());
}

fn writer(root: &Path) {
    // A new session, then another fork: neither provider PGID nor its direct
    // child PID identifies the final writer. It deliberately ignores TERM.
    std::fs::write(
        root.join(".macc/writer.py"),
        r#"
import os,signal,time
if os.fork(): os._exit(0)
os.setsid()
if os.fork(): os._exit(0)
signal.signal(signal.SIGTERM,signal.SIG_IGN)
open('.macc/writer.pid','w').write(str(os.getpid()))
while True:
    with open('capture.txt','a') as f: f.write('capture\n')
    time.sleep(.02)
"#,
    )
    .unwrap();
}

fn stopped(root: &Path) {
    let pid: i64 = std::fs::read_to_string(root.join(".macc/writer.pid"))
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        !macc_core::coordinator::helpers::is_pid_running(pid),
        "detached writer survived"
    );
    let before = std::fs::read(root.join("capture.txt")).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(
        before,
        std::fs::read(root.join("capture.txt")).unwrap(),
        "late writes after completion"
    );
}

#[test]
fn provider_exit_reaps_detached_writers_preserves_status_and_unrelated_processes() {
    let (temp, _) = setup();
    let root = temp.path();
    writer(root);
    let mut unrelated = Command::new("sleep").arg("60").spawn().unwrap();
    let receipt = root.join(".macc/quiescent");
    let out = Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("runner-process").arg("--completion-file").arg(&receipt)
        .args(["--", "bash", "-c", "python3 .macc/writer.py; while [ ! -s .macc/writer.pid ]; do sleep .02; done; echo provider-output; exit 23"])
        .current_dir(root).output().unwrap();
    let unrelated_alive = unrelated.try_wait().unwrap().is_none();
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
    assert_eq!(
        out.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"provider-output\n");
    assert_eq!(std::fs::read_to_string(receipt).unwrap(), "quiescent-v1\n");
    assert!(unrelated_alive);
    stopped(root);
}

#[test]
fn stale_decision_stops_detached_writer_before_final_checkpoint_and_allows_refresh() {
    use macc_core::{
        coordinator::{decisions as dm, model::TaskRegistry},
        session_store as ss,
    };
    let (temp, lib) = setup();
    let root = temp.path();
    writer(root);
    std::fs::write(
        root.join("fake-ai.sh"),
        r#"#!/bin/bash
printf '%s\n' "${@: -1}" | sed -n '/^MACC_CHECKPOINT_ACK: /p'
python3 .macc/writer.py
while [[ ! -s .macc/writer.pid ]]; do sleep .02; done
echo ready > .macc/provider-ready
exec sleep 60
"#,
    )
    .unwrap();
    let paths = macc_core::ProjectPaths::from_root(root);
    let mut cfg = macc_core::load_canonical_config(&paths.config_path).unwrap();
    let p = dm::Policy {
        enabled: true,
        human_question_min_risk: dm::Consultation::NeverAsk,
        ..Default::default()
    };
    let c = cfg
        .automation
        .coordinator
        .get_or_insert_with(Default::default);
    c.reference_branch = Some("main".into());
    c.prd_file = Some("worktree.prd.json".into());
    c.decisions = Some(p.clone());
    macc_core::save_canonical_config(&paths, &cfg).unwrap();
    let graph: Value =
        serde_json::from_slice(&std::fs::read(root.join("worktree.prd.json")).unwrap()).unwrap();
    let mut store = dm::Store::open(root).unwrap();
    let plan = store
        .ensure_plan(
            root,
            Path::new("worktree.prd.json"),
            "main",
            &TaskRegistry::from_value(&graph).unwrap(),
        )
        .unwrap();
    let view = ss::inspect(root).unwrap();
    let mut policy = view.policy;
    policy.lease.heartbeat_seconds = 1;
    ss::update_policy(root, &view.revision, &policy).unwrap();
    let mut child = runner(root, &lib, "A", "performer")
        .env("MACC_EVENT_SOURCE", "stale-with-writer")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_file(&root.join(".macc/provider-ready"));
    store
        .feedback(
            &plan,
            "A",
            "Preserve the payment permission boundary",
            "Human",
            false,
            &p,
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
        panic!("stale runner did not stop");
    }
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success());
    let wait = ss::decision_wait::waiting_for_invocation(root, "A", "stale-with-writer")
        .unwrap()
        .unwrap();
    assert!(wait.reason.starts_with("DM_STALE:"), "{wait:?}");
    stopped(root);
    let view = ss::inspect(root).unwrap();
    assert!(view.sessions.iter().all(|s| s.owner_pid.is_none()));
    assert!(view
        .checkpoints
        .iter()
        .any(|c| c.reason == "invocation_finished"));
    // Exact-state recovery now succeeds without ignoring dirty implementation
    // evidence, source checks or the native decision binding.
    ss::checkpoints::recovery::ensure_decision_context_ready(root, "A").unwrap();
    assert!(!String::from_utf8_lossy(&out.stderr).contains("cleanup was not confirmed"));
}

#[test]
fn missing_lifetime_receipt_refuses_checkpoint_finalization() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../adapters/shared/checkpoint.sh");
    let out = Command::new("bash")
        .arg("-c")
        .arg(
            r#"
source "$1"
checkpoint_enabled=true
sqlite_lease_key='{}'
provider_quiesced=false
session_service() { echo unsafe-finalization; }
finish_checkpoint interrupted false
"#,
        )
        .arg("--")
        .arg(source)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("unsafe-finalization"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("checkpoint finalization refused"));
}

#[test]
fn stdin_prompt_and_json_backend_use_the_same_lifetime_boundary() {
    let (temp, lib) = setup();
    let root = temp.path();
    // Exercise the original lightweight store too, with a stdin-based tool.
    std::fs::remove_file(root.join(".macc/state/tool-sessions.json")).unwrap();
    std::fs::remove_file(root.join(".macc/memory.db")).unwrap();
    std::fs::write(root.join(".macc/tool.json"), json!({"id":"future-ai","performer":{"command":"bash","args":[root.join("fake-ai.sh")],"prompt":{"mode":"stdin"}}}).to_string()).unwrap();
    writer(root);
    std::fs::write(
        root.join("fake-ai.sh"),
        r#"#!/bin/bash
cat > .macc/received-prompt
python3 .macc/writer.py
while [[ ! -s .macc/writer.pid ]]; do sleep .02; done
echo 'MACC_TASK_RESULT: success_without_changes'
"#,
    )
    .unwrap();
    let out = runner(root, &lib, "A", "performer").output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(std::fs::read_to_string(root.join(".macc/received-prompt"))
        .unwrap()
        .contains("Implement task"));
    stopped(root);
}

#[test]
fn killed_runner_parent_does_not_abandon_detached_tools() {
    let (temp, _) = setup();
    let root = temp.path();
    writer(root);
    let mut parent = Command::new("bash").args(["-c", r#"
"$1" runner-process --completion-file .macc/quiescent -- bash -c 'python3 .macc/writer.py; while [ ! -s .macc/writer.pid ]; do sleep .02; done; exec sleep 60' &
wait
"#, "--"]).arg(env!("CARGO_BIN_EXE_macc"))
        .current_dir(root).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    wait_file(&root.join(".macc/writer.pid"));
    parent.kill().unwrap();
    parent.wait().unwrap();
    wait_file(&root.join(".macc/quiescent"));
    stopped(root);
}

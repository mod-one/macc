//! Native waiting receipt is durable before the real shell stops its provider.
use super::*;
#[test]
fn heartbeat_stops_for_human_decision_without_losing_the_native_wait_receipt() {
    // Exercise the signal/EXIT-trap ordering, not only the durable DB receipt.
    for _ in 0..3 {
        human_wait_fixture();
    }
}
fn human_wait_fixture() {
    use macc_core::{
        coordinator::{decisions as dm, model::TaskRegistry},
        session_store as ss,
    };
    let (temp, lib) = setup();
    let root = temp.path();
    let paths = macc_core::ProjectPaths::from_root(root);
    let mut cfg = macc_core::load_canonical_config(&paths.config_path).unwrap();
    let c = cfg
        .automation
        .coordinator
        .get_or_insert_with(Default::default);
    c.reference_branch = Some("main".into());
    c.prd_file = Some("worktree.prd.json".into());
    c.decisions = Some(dm::Policy {
        enabled: true,
        ..Default::default()
    });
    std::fs::write(&paths.config_path, cfg.to_yaml().unwrap()).unwrap();
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
    let ready = root.join("ready-provider");
    let mut child = runner(root, &lib, "A", "performer")
        .env("FAKE_MODE", "sleep")
        .env("READY_FILE", &ready)
        .env("MACC_EVENT_SOURCE", "invocation-current")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !ready.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if !ready.exists() {
        let _ = child.kill();
        let out = child.wait_with_output().unwrap();
        panic!(
            "fake provider did not start: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let request:dm::Request=serde_json::from_value(json!({"idempotency_key":"human-wait","task_id":"A","kind":"clarification","question":"Which isolated device should be used?","context":"No default device","recommendation":"Use an isolated fixture","investigated":["Read configuration"],"risk":"high","impact":{"shared_contract":true,"evidence":["Verification depends on the selected fixture"]}})).unwrap();
    let d = store
        .submit(
            &plan,
            request,
            "agent:A",
            "agent",
            &dm::policy(root).unwrap(),
        )
        .unwrap();
    assert_eq!(d.status, "pending_human");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let native = ss::decision_wait::waiting_for_invocation(root, "A", "invocation-current");
            let sessions = ss::inspect(root);
            let processes = Command::new("ps")
                .args(["-eo", "pid,ppid,pgid,stat,wchan:25,args"])
                .output()
                .unwrap();
            let processes = String::from_utf8_lossy(&processes.stdout)
                .lines()
                .filter(|l| {
                    l.contains(root.to_str().unwrap())
                        || l.contains(&child.id().to_string())
                        || l.contains("sleep 60")
                })
                .collect::<Vec<_>>()
                .join("\n");
            if let Ok(pid) = std::fs::read_to_string(&ready) {
                let _ = Command::new("kill").args(["-TERM", pid.trim()]).status();
            }
            let _ = child.kill();
            let out = child.wait_with_output().unwrap();
            panic!("runner did not stop: native={native:?} sessions={sessions:?} stdout={} stderr={} processes={processes}",String::from_utf8_lossy(&out.stdout),String::from_utf8_lossy(&out.stderr));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let out = child.wait_with_output().unwrap();
    assert!(!status.success());
    let provider: i64 = std::fs::read_to_string(&ready)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(!macc_core::coordinator::helpers::is_pid_running(provider));
    let view = ss::inspect(root).unwrap();
    assert_eq!(view.sessions[0].status, "abandoned");
    assert!(view.sessions[0].owner_pid.is_none());
    assert!(String::from_utf8_lossy(&out.stdout).contains("MACC_TASK_RESULT: precondition_unmet"));
    let receipt = ss::decision_wait::waiting_for_invocation(root, "A", "invocation-current")
        .unwrap()
        .expect("native receipt must precede termination");
    assert_eq!(receipt.decision.unwrap().id, d.id);
    let interrupted = root.join("interrupted-output.txt");
    std::fs::write(
        &interrupted,
        "exec\nsource containing quota exceeded\n```\n",
    )
    .unwrap();
    for (source, expected) in [("invocation-current", true), ("unrelated", false)] {
        let result = Command::new(env!("CARGO_BIN_EXE_macc"))
            .args(["runner-output", "--tool", "future-ai", "--file"])
            .arg(&interrupted)
            .arg("--project-root")
            .arg(root)
            .args(["--task-id", "A", "--source", source, "--exit-code", "143"])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let result: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(result["native_wait"].is_object(), expected);
        assert_eq!(
            result["terminal_output"]
                .as_str()
                .unwrap()
                .contains("MACC_TASK_RESULT: precondition_unmet"),
            expected
        );
        assert_eq!(result["provider_failure"], false);
    }
    assert!(
        ss::decision_wait::waiting_for_invocation(root, "A", "another-invocation")
            .unwrap()
            .is_none()
    );
    assert!(!String::from_utf8_lossy(&out.stderr).contains("session lease heartbeat failed"));
}

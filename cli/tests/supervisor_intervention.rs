//! End-to-end production supervisor wiring, using a deterministic AI tool double.
#![cfg(unix)]
use macc_core::coordinator::model::Task;
use macc_core::coordinator_storage::{
    CoordinatorRun, CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage,
};
use macc_core::ProjectPaths;
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
fn git(root: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
fn wait_for(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(25);
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("supervisor condition timed out");
}
fn fixture(reject: bool) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join(".macc/tools.d")).unwrap();
    std::fs::write(root.join(".gitignore"),".macc/\nperformer.sh\nworktree.prd.json\n.antigravitycli/\n.agents/\nGEMINI.md\n.claude/\nCLAUDE.md\n.codex/\nAGENTS.md\n.gemini/\n.geminiignore\n.vibe/\n").unwrap();
    std::fs::write(root.join("defect.txt"), "broken\n").unwrap();
    std::fs::write(root.join("prd.json"), r#"{"tasks":[]}"#).unwrap();
    let script = root.join(".macc/fake-agent.sh");
    std::fs::write(&script,format!(r#"#!/bin/sh
set -eu
case "$1" in
 *"Diagnose first"*) printf '%s' '{{"summary":"defect.txt is broken; repair it","repairable":true,"task_ids":["ROOT"],"macc_findings":["Normalized logs masked the precondition"]}}' > .macc/supervisor-diagnosis.json ;;
 *"authorized MACC supervisor repair agent"*) printf 'fixed\n' > defect.txt ;;
 *"Independently verify"*) printf '%s' '{{"verified":{},"explanation":"fixture root corrected and regression command passed"}}' > .macc/supervisor-verification.json ;;
 *) exit 9 ;;
esac
"#,if reject{"false"}else{"true"})).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.join(".macc/tools.d/fake.tool.yaml"),format!("api_version: v1\nid: fake\ndisplay_name: Fake\nfields: []\nperformer:\n  command: {}\n  runner: unused\n  args: []\n  prompt:\n    mode: arg\n",script.display())).unwrap();
    std::fs::write(root.join(".macc/macc.yaml"),"tools:\n  enabled: [fake]\nautomation:\n  coordinator:\n    reference_branch: master\n  supervisor:\n    tool: fake\n    watchdog_interval_seconds: 1\n    max_restart_attempts: 1\n    intervention_timeout_seconds: 10\n    validation_commands:\n      - \"test $(cat defect.txt) = fixed\"\n").unwrap();
    git(root, &["init", "-q", "-b", "master"]);
    git(root, &["config", "user.name", "Supervisor Test"]);
    git(
        root,
        &["config", "user.email", "supervisor@example.invalid"],
    );
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "Initialize fixture"]);
    let paths = ProjectPaths::from_root(root);
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let mut snapshot = CoordinatorSnapshot::empty();
    snapshot.registry.tasks = vec![Task {
        id: "ROOT".into(),
        state: "blocked".into(),
        ..Task::default()
    }];
    snapshot.registry.tasks[0].task_runtime.unmet_preconditions =
        vec!["defect.txt must be fixed".into()];
    snapshot.registry.tasks[0].task_runtime.last_error_code = Some("E903".into());
    db.save_snapshot(&snapshot).unwrap();
    db.upsert_coordinator_run(&CoordinatorRun {
        run_id: "run-fixture".into(),
        pid: 99999999,
        hostname: "test".into(),
        started_at: "2026-10-03T00:00:00Z".into(),
        last_tick_at: None,
        stopped_at: None,
        status: "blocked".into(),
        epoch: 1,
        version: "test".into(),
        stop_reason: Some("unmet precondition".into()),
    })
    .unwrap();
    temp
}
struct Supervisor(std::process::Child);
impl Drop for Supervisor {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.0.id() as i32, libc::SIGTERM);
        }
        let _ = self.0.wait();
    }
}
fn start(root: &Path) -> Supervisor {
    Supervisor(
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .args(["--cwd", root.to_str().unwrap(), "supervisor", "start"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    )
}
fn report(root: &Path) -> Option<Value> {
    std::fs::read_to_string(root.join(".macc/log/supervisor/latest.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}
#[test]
fn sqlite_incident_is_diagnosed_repaired_validated_integrated_and_restarted() {
    let temp = fixture(false);
    let root = temp.path();
    let supervisor = start(root);
    wait_for(|| {
        report(root).is_some_and(|v| {
            v.pointer("/intervention/status").and_then(Value::as_str) == Some("recovered")
        })
    });
    let r = report(root).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "fixed\n"
    );
    assert!(r
        .pointer("/intervention/restart_pid")
        .and_then(Value::as_i64)
        .is_some());
    assert_eq!(
        r.pointer("/intervention/diagnosis/task_ids"),
        Some(&json!(["ROOT"]))
    );
    assert!(r
        .pointer("/intervention/diagnosis/macc_findings/0")
        .is_some());
    let paths = ProjectPaths::from_root(root);
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    wait_for(|| {
        db.get_latest_coordinator_run()
            .unwrap()
            .is_some_and(|r| r.run_id != "run-fixture" && r.status == "success")
    });
    let status = Command::new(env!("CARGO_BIN_EXE_macc"))
        .args(["--cwd", root.to_str().unwrap(), "supervisor", "status"])
        .output()
        .unwrap();
    assert!(status.status.success());
    let output = Command::new(env!("CARGO_BIN_EXE_macc"))
        .args(["--cwd", root.to_str().unwrap(), "supervisor", "report"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("recovered"));
    drop(supervisor);
    let ledger: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(".macc/state/supervisor-interventions.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(ledger["completed"], json!(["run-fixture-1"]));
}
#[test]
fn rejected_verification_retains_report_and_does_not_integrate_or_restart() {
    let temp = fixture(true);
    let root = temp.path();
    let supervisor = start(root);
    wait_for(|| {
        report(root).is_some_and(|v| {
            v.pointer("/intervention/status").and_then(Value::as_str) == Some("escalated")
        })
    });
    let r = report(root).unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "broken\n"
    );
    assert!(r.pointer("/intervention/restart_pid").unwrap().is_null());
    assert!(r
        .pointer("/intervention/detail")
        .and_then(Value::as_str)
        .unwrap()
        .contains("verification rejected"));
    assert!(Path::new(
        r.pointer("/intervention/worktree")
            .and_then(Value::as_str)
            .unwrap()
    )
    .exists());
    drop(supervisor);
}

#[test]
fn tool_timeout_kills_children_and_keeps_failure_evidence() {
    let temp = fixture(false);
    let root = temp.path();
    let script = root.join(".macc/fake-agent.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nsleep 60 &\necho $! > .macc/tool-child.pid\nwait\n",
    )
    .unwrap();
    let config = root.join(".macc/macc.yaml");
    let text = std::fs::read_to_string(&config).unwrap().replace(
        "intervention_timeout_seconds: 10",
        "intervention_timeout_seconds: 1",
    );
    std::fs::write(config, text).unwrap();
    let supervisor = start(root);
    wait_for(|| {
        report(root).is_some_and(|v| {
            v.pointer("/intervention/status").and_then(Value::as_str) == Some("escalated")
        })
    });
    let r = report(root).unwrap();
    assert!(r
        .pointer("/intervention/detail")
        .and_then(Value::as_str)
        .unwrap()
        .contains("timed out"));
    let tree = Path::new(
        r.pointer("/intervention/worktree")
            .and_then(Value::as_str)
            .unwrap(),
    );
    let pid = std::fs::read_to_string(tree.join(".macc/tool-child.pid")).unwrap();
    wait_for(|| {
        std::fs::read_to_string(format!("/proc/{}/stat", pid.trim()))
            .map(|s| s.rsplit_once(") ").unwrap().1.starts_with('Z'))
            .unwrap_or(true)
    });
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "broken\n"
    );
    drop(supervisor);
}
#[test]
fn validation_failure_blocks_integration() {
    let temp = fixture(false);
    let root = temp.path();
    let config = root.join(".macc/macc.yaml");
    let text = std::fs::read_to_string(&config)
        .unwrap()
        .replace("test $(cat defect.txt) = fixed", "false");
    std::fs::write(config, text).unwrap();
    let supervisor = start(root);
    wait_for(|| {
        report(root).is_some_and(|v| {
            v.pointer("/intervention/status").and_then(Value::as_str) == Some("escalated")
        })
    });
    let r = report(root).unwrap();
    assert!(r
        .pointer("/intervention/detail")
        .and_then(Value::as_str)
        .unwrap()
        .contains("Validation failed"));
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "broken\n"
    );
    drop(supervisor);
}
#[test]
fn shutdown_marks_an_in_flight_intervention_interrupted() {
    let temp = fixture(false);
    let root = temp.path();
    std::fs::write(root.join(".macc/fake-agent.sh"), "#!/bin/sh\nsleep 60\n").unwrap();
    let supervisor = start(root);
    wait_for(|| {
        report(root).is_some_and(|v| {
            v.pointer("/intervention/status").and_then(Value::as_str) == Some("diagnosing")
        })
    });
    drop(supervisor);
    assert_eq!(
        report(root)
            .unwrap()
            .pointer("/intervention/status")
            .and_then(Value::as_str),
        Some("interrupted")
    );
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "broken\n"
    );
}

#[test]
fn verifier_cannot_modify_source_after_validation() {
    let temp = fixture(false);
    let root = temp.path();
    let script = root.join(".macc/fake-agent.sh");
    let text = std::fs::read_to_string(&script).unwrap();
    std::fs::write(
        &script,
        text.replace(
            "*\"Independently verify\"*)",
            "*\"Independently verify\"*) printf 'untested\\n' > defect.txt; ",
        ),
    )
    .unwrap();
    let supervisor = start(root);
    wait_for(|| {
        report(root).is_some_and(|v| {
            v.pointer("/intervention/status").and_then(Value::as_str) == Some("escalated")
        })
    });
    let evidence = report(root).unwrap();
    assert!(evidence["intervention"]["detail"]
        .as_str()
        .unwrap()
        .contains("modified validated source"));
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "broken\n"
    );
    assert!(evidence["intervention"]["restart_pid"].is_null());
    drop(supervisor);
}

#[test]
fn monitoring_error_is_reported_and_recovers_without_restarting_supervisor() {
    let temp = fixture(false);
    let root = temp.path();
    let ledger = root.join(".macc/state/supervisor-interventions.json");
    std::fs::write(&ledger, "invalid ledger").unwrap();
    let supervisor = start(root);
    wait_for(|| {
        root.join(".macc/log/supervisor/runtime-error.json")
            .exists()
    });
    assert!(macc_core::coordinator::helpers::is_pid_running(
        supervisor.0.id() as i64
    ));
    std::fs::remove_file(ledger).unwrap();
    wait_for(|| {
        report(root).is_some_and(|v| {
            v.pointer("/intervention/status").and_then(Value::as_str) == Some("recovered")
        })
    });
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "fixed\n"
    );
    drop(supervisor);
}

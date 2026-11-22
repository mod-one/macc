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
fn wait_for_recovered(root: &Path) {
    let result = std::panic::catch_unwind(|| {
        wait_for(|| report(root).is_some_and(|v| v["intervention"]["status"] == "recovered"))
    });
    if let Err(error) = result {
        if let Some(report) = report(root) {
            eprintln!("Latest intervention: {}", report["intervention"]);
        }
        for name in ["daemon-stderr.log", "daemon-stdout.log"] {
            if let Ok(text) = std::fs::read_to_string(root.join(".macc/log/coordinator").join(name))
            {
                eprintln!("{name}: {text}");
            }
        }
        std::panic::resume_unwind(error);
    }
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
    // These scenarios exercise supervisor recovery, not optional provider setup.
    // Real O2 preparation is covered by the dedicated O2 CLI/runner fixtures.
    let config_path = root.join(".macc/macc.yaml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    std::fs::write(config_path,format!("{config}settings:\n  offline: true\ncontext_engine:\n  enabled: false\ncode_intelligence:\n  enabled: false\n")).unwrap();
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
        let signal_result = unsafe { libc::kill(self.0.id() as i32, libc::SIGTERM) };
        if signal_result != 0 {
            eprintln!(
                "Supervisor fixture SIGTERM failed: {}",
                std::io::Error::last_os_error()
            );
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn start(root: &Path) -> Supervisor {
    Supervisor(
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .args(["--cwd", root.to_str().unwrap(), "supervisor", "start"])
            .env("TOKIO_WORKER_THREADS", "2")
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
    wait_for_recovered(root);
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
    let snapshot = Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("--cwd")
        .arg(root)
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(snapshot.status.success());
    let snapshot: Value = serde_json::from_slice(&snapshot.stdout).unwrap();
    let summary = &snapshot["coordinator"]["last_run_summary"];
    assert_ne!(summary["run_id"], "run-fixture");
    assert!(summary["supervisor"].is_null());
    let prior = summary["recent_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["run_id"] == "run-fixture")
        .unwrap();
    assert_eq!(prior["supervisor"]["status"], "recovered");
    assert_eq!(
        prior["supervisor"]["diagnosis"],
        r["intervention"]["diagnosis"]["summary"]
    );
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
    let evidence = Path::new(
        r.pointer("/intervention/log_dir")
            .and_then(Value::as_str)
            .unwrap(),
    );
    let pid = std::fs::read_to_string(evidence.join("evidence/.macc/tool-child.pid")).unwrap();
    assert!(
        !tree.exists(),
        "clean exhausted slot should be removed after archiving"
    );
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
    // Passes on the unmodified base and fails with the repair: a regression.
    let text = std::fs::read_to_string(&config).unwrap().replace(
        "test $(cat defect.txt) = fixed",
        "test $(cat defect.txt) = broken",
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
        .contains("Validation failed"));
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "broken\n"
    );
    // The baseline run must hand the slot back with the repair intact.
    let slot = Path::new(r["intervention"]["worktree"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(slot.join("defect.txt")).unwrap(),
        "fixed\n"
    );
    assert!(!slot.join(".macc/supervisor-restore-tree").exists());
    assert!(r["intervention"]["next_action"]
        .as_str()
        .unwrap()
        .contains("macc supervisor start --daemon --retry"));
    drop(supervisor);
}
#[test]
fn stop_command_marks_an_in_flight_intervention_interrupted() {
    let temp = fixture(false);
    let root = temp.path();
    std::fs::write(root.join(".macc/fake-agent.sh"), "#!/bin/sh\nsleep 60\n").unwrap();
    let supervisor = start(root);
    wait_for(|| {
        report(root).is_some_and(|v| {
            v.pointer("/intervention/status").and_then(Value::as_str) == Some("diagnosing")
        })
    });
    // Live clients read the phase, its start and the step in progress.
    let live = report(root).unwrap();
    assert!(live["intervention"]["activity"]
        .as_str()
        .unwrap()
        .starts_with("AI diagnosis"));
    assert_eq!(live["intervention"]["tool"], "fake");
    assert!(live["intervention"]["phase_started_at"].is_string());
    assert!(live["intervention"]["started_at"].is_string());
    let output = Command::new(env!("CARGO_BIN_EXE_macc"))
        .args(["--cwd", root.to_str().unwrap(), "supervisor", "stop"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
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
    wait_for_recovered(root);
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "fixed\n"
    );
    drop(supervisor);
}

#[path = "support/supervisor_lifecycle.rs"]
mod supervisor_lifecycle;

#[path = "support/supervisor_integration.rs"]
mod supervisor_integration;

#[path = "support/supervisor_feedback.rs"]
mod supervisor_feedback;

/// The engine stopped on an internal error while the root task was between
/// phases (left in `testing` with its review verdict pending). Nothing needs
/// repairing: the supervisor restarts the coordinator without any AI call (the
/// fixture tool fails every call) and records the restart against the crash.
#[test]
fn an_engine_stop_between_phases_is_recovered_by_a_restart_without_repair() {
    let temp = fixture(false);
    let root = temp.path();
    let paths = ProjectPaths::from_root(root);
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let mut snapshot = CoordinatorSnapshot::empty();
    snapshot.registry.tasks = vec![Task {
        id: "L4L-BASELINE-001".into(),
        state: "testing".into(),
        ..Task::default()
    }];
    db.save_snapshot(&snapshot).unwrap();
    db.upsert_coordinator_run(&CoordinatorRun {
        run_id: "run-fixture".into(),
        pid: 99999999,
        hostname: "test".into(),
        started_at: "2026-10-14T13:49:00Z".into(),
        last_tick_at: None,
        stopped_at: Some("2026-10-14T14:30:10Z".into()),
        status: "failed".into(),
        epoch: 1,
        version: "test".into(),
        stop_reason: Some(
            "Coordinator error [invalid_transition]: Invalid coordinator FSM transition: from=testing event=ReviewChangesRequested".into(),
        ),
    })
    .unwrap();
    // Any AI call would fail: the restart path must not make one.
    std::fs::write(root.join(".macc/fake-agent.sh"), "#!/bin/sh\nexit 9\n").unwrap();

    let supervisor = start(root);
    wait_for_recovered(root);
    let r = report(root).unwrap();
    assert_eq!(r.pointer("/intervention/restart_only"), Some(&json!(true)));
    assert!(r
        .pointer("/intervention/diagnosis")
        .is_none_or(Value::is_null));
    assert!(r
        .pointer("/intervention/restart_pid")
        .and_then(Value::as_i64)
        .is_some());
    let detail = r
        .pointer("/intervention/detail")
        .and_then(Value::as_str)
        .unwrap();
    assert!(detail.contains("L4L-BASELINE-001 (testing)"), "{detail}");
    drop(supervisor);
    let ledger: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(".macc/state/supervisor-interventions.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        ledger["restarts"]
            .as_object()
            .map(|restarts| restarts.values().cloned().collect::<Vec<_>>()),
        Some(vec![json!(1)])
    );
}

#[test]
fn historical_scoped_prd_is_recovered_without_ai_with_manual_restart_policy() {
    use macc_core::coordinator::{
        helpers::write_worktree_prd_for_task, model::TaskWorktree, scoped_prd,
    };
    let temp = fixture(false);
    let root = temp.path();
    let paths = ProjectPaths::from_root(root);
    let mut cfg = macc_core::load_canonical_config(&paths.config_path).unwrap();
    cfg.automation.supervisor.as_mut().unwrap().auto_restart = false;
    macc_core::save_canonical_config(&paths, &cfg).unwrap();
    let prd = root.join("prd.json");
    std::fs::write(&prd, r#"{"tasks":[{"id":"PREVIOUS"},{"id":"ROOT"}]}"#).unwrap();
    git(root, &["add", "prd.json"]);
    git(root, &["commit", "-qm", "record scoped PRD fixture"]);
    let worker = root.join(".macc/worktree/worker-01");
    std::fs::create_dir_all(worker.parent().unwrap()).unwrap();
    let branch = "ai/fake/worker-01";
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            branch,
            worker.to_str().unwrap(),
            "master",
        ],
    );
    std::fs::create_dir_all(worker.join(".macc")).unwrap();
    std::fs::write(
        worker.join(".macc/worktree.json"),
        json!({"id":"worker-01","slug":"worker-01","tool":"fake","base":"master","branch":branch})
            .to_string(),
    )
    .unwrap();
    write_worktree_prd_for_task(&prd, "PREVIOUS", &worker).unwrap();
    let path = worker.join(scoped_prd::NAME);
    let mut old: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    old["tasks"][0]["passes"] = true.into();
    std::fs::write(&path, old.to_string()).unwrap();
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let mut snapshot = db.load_snapshot().unwrap();
    let task = snapshot.registry.find_task_mut("ROOT").unwrap();
    task.worktree = Some(TaskWorktree {
        worktree_path: Some(worker.display().to_string()),
        branch: Some(branch.into()),
        base_branch: Some("master".into()),
        last_commit: Some(macc_core::git::head_commit(&worker).unwrap()),
        ..Default::default()
    });
    let rt = task.ensure_runtime();
    rt.current_phase = Some("dev".into());
    rt.claim_id = Some("unlaunched".into());
    rt.run_id = Some("run-fixture".into());
    rt.coordinator_epoch = Some(1);
    let cause=format!("Validation error: Cannot replace {}: preserve or move the existing scoped PRD before dispatch",path.display());
    rt.set_last_error_details("E901", "dispatch_preparation", &cause);
    rt.extra.insert(
        "dispatch_preparation_failure".into(),
        json!({"cause":cause,"claim_id":"unlaunched","phase":"dev"}),
    );
    snapshot.registry.tasks.push(Task {
        id: "INDEPENDENT".into(),
        state: "changes_requested".into(),
        ..Default::default()
    });
    db.save_snapshot(&snapshot).unwrap();
    // Simulate upgrading a project that already has a conclusive legacy
    // escalation. The previous AI diagnosis must not silence native recovery.
    let previous = macc_core::supervisor::incident::inspect(&paths)
        .unwrap()
        .unwrap();
    let dir = root.join(".macc/log/supervisor");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("latest.json"), json!({"intervention":{
        "incident":previous, "attempt":1, "status":"escalated", "tool":"fake",
        "validation":[], "detail":"Worker dispatch failed.",
        "diagnosis":{"summary":"Inspect worker manually", "repairable":false, "requires_human":true}
    }}).to_string()).unwrap();
    let _supervisor = start(root);
    wait_for(|| report(root).is_some_and(|v| v["intervention"]["status"] == "ready_to_resume"));
    let report = report(root).unwrap();
    for args in [vec!["status"], vec!["coordinator", "status"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_macc"))
            .arg("--cwd")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let status = String::from_utf8(out.stdout).unwrap();
        assert!(
            status.contains("ready_to_resume") && status.contains("PREVIOUS"),
            "{status}"
        );
    }
    let incident_dir = root.join(".macc/log/coordinator/incidents");
    let files: Vec<_> = std::fs::read_dir(incident_dir).unwrap().collect();
    assert_eq!(files.len(), 1);
    let incident: Value =
        serde_json::from_slice(&std::fs::read(files[0].as_ref().unwrap().path()).unwrap()).unwrap();
    assert_eq!(incident["supervisor"]["status"], "ready_to_resume");
    assert_eq!(incident["run_id"], "run-fixture");
    assert!(report["intervention"]["tool"].is_null());
    assert!(report["intervention"]["restart_pid"].is_null());
    assert!(report["intervention"]["diagnosis"]["summary"]
        .as_str()
        .unwrap()
        .contains("PREVIOUS"));
    let task = db
        .load_registry()
        .unwrap()
        .find_task("ROOT")
        .unwrap()
        .clone();
    assert!(scoped_prd::requeued(&task));
    assert_eq!(task.worktree_path(), worker.to_str());
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(path).unwrap()).unwrap()["tasks"][0]["id"],
        "ROOT"
    );
}

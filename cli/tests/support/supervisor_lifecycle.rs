//! Lifecycle regressions sharing the deterministic supervisor fixture.
use super::*;

#[test]
fn related_task_context_retains_external_diagnosis_without_repair() {
    let temp = fixture(false);
    let root = temp.path();
    std::fs::write(root.join(".macc/fake-agent.sh"), r#"#!/bin/sh
set -eu
printf '%s' '{"summary":"Native host unavailable; related deployment has a quota","repairable":false,"requires_human":true,"task_ids":["ROOT","DEPLOYMENT"],"external_prerequisites":["iOS host"]}' > .macc/supervisor-diagnosis.json
"#).unwrap();
    let supervisor = start(root);
    wait_for(|| report(root).is_some_and(|r| r["intervention"]["status"] == "escalated"));
    let r = report(root).unwrap();
    assert_eq!(
        r["intervention"]["diagnosis"]["task_ids"],
        serde_json::json!(["ROOT"])
    );
    assert_eq!(
        r["intervention"]["diagnosis"]["related_task_ids"],
        serde_json::json!(["DEPLOYMENT"])
    );
    assert_eq!(
        r["intervention"]["diagnosis"]["external_prerequisites"],
        serde_json::json!(["iOS host"])
    );
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt"))
            .unwrap()
            .trim(),
        "broken"
    );
    drop(supervisor);
}

#[test]
fn external_diagnosis_waits_durably_and_rechecks_only_after_configuration_changes() {
    let temp = fixture(false);
    let root = temp.path();
    let config = root.join(".macc/macc.yaml");
    std::fs::write(
        &config,
        std::fs::read_to_string(&config)
            .unwrap()
            .replace("max_restart_attempts: 1", "max_restart_attempts: 3"),
    )
    .unwrap();
    std::fs::write(root.join(".macc/fake-agent.sh"),r#"#!/bin/sh
set -eu
printf 'diagnosis\n' >> "$(dirname "$0")/calls.log"
printf '%s' '{"summary":"external service unavailable","repairable":false,"requires_human":false}' > .macc/supervisor-diagnosis.json
printf 'diagnostic evidence\n' > .macc/evidence.txt
"#).unwrap();
    let supervisor = start(root);
    wait_for(|| report(root).is_some_and(|v| v["intervention"]["status"] == "escalated"));
    let r = report(root).unwrap();
    let worktree = Path::new(r["intervention"]["worktree"].as_str().unwrap());
    assert!(worktree.starts_with(root.join(".macc/worktree")));
    assert!(!worktree.exists());
    let dir = Path::new(r["intervention"]["log_dir"].as_str().unwrap());
    assert!(dir.join("diagnosis-prompt.txt").exists());
    assert!(dir.join("diagnosis-stdout.log").exists());
    assert!(dir
        .join("evidence/.macc/supervisor-diagnosis.json")
        .exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("evidence/.macc/evidence.txt")).unwrap(),
        "diagnostic evidence\n"
    );
    std::thread::sleep(Duration::from_millis(2200));
    assert_eq!(
        std::fs::read_to_string(root.join(".macc/calls.log"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    drop(supervisor);
    let supervisor = start(root);
    std::thread::sleep(Duration::from_millis(2200));
    assert_eq!(
        std::fs::read_to_string(root.join(".macc/calls.log"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let text = std::fs::read_to_string(&config)
        .unwrap()
        .replace("tool: fake", "tool: fake\n    model: changed-context");
    std::fs::write(&config, text).unwrap();
    wait_for(|| {
        report(root).is_some_and(|v| {
            v["intervention"]["attempt"] == 2 && v["intervention"]["status"] == "escalated"
        })
    });
    assert_eq!(
        std::fs::read_to_string(root.join(".macc/calls.log"))
            .unwrap()
            .lines()
            .count(),
        2
    );
    assert!(dir.join("report.json").exists());
    drop(supervisor);
}

#[test]
fn transient_repair_resumes_same_slot_without_rediagnosis_or_losing_edits() {
    let temp = fixture(false);
    let root = temp.path();
    let config = root.join(".macc/macc.yaml");
    std::fs::write(
        &config,
        std::fs::read_to_string(&config)
            .unwrap()
            .replace("max_restart_attempts: 1", "max_restart_attempts: 3")
            .replace(
                "intervention_timeout_seconds: 10",
                "intervention_timeout_seconds: 1",
            ),
    )
    .unwrap();
    std::fs::write(root.join(".macc/fake-agent.sh"),r#"#!/bin/sh
set -eu
case "$1" in
 *"Diagnose first"*) printf 'diagnosis\n' >> "$(dirname "$0")/calls.log"; printf '%s' '{"summary":"repair defect.txt","repairable":true,"task_ids":["ROOT"]}' > .macc/supervisor-diagnosis.json ;;
 *"authorized MACC supervisor repair agent"*)
   if ! test -f .macc/retry-seen; then
     printf 'partial\n' > defect.txt
     printf 'untracked\n' > continuation.txt
     git add defect.txt
     printf 'checkpoint\n' > .macc/retry-seen
     sleep 60
   fi
   test "$(cat defect.txt)" = partial
   test "$(cat continuation.txt)" = untracked
   printf 'fixed\n' > defect.txt ;;
 *"Independently verify"*) printf '%s' '{"verified":true,"explanation":"continued partial repair validated"}' > .macc/supervisor-verification.json ;;
 *) exit 9 ;;
esac
"#).unwrap();
    let supervisor = start(root);
    wait_for(|| report(root).is_some_and(|v| v["intervention"]["status"] == "recovered"));
    let final_report = report(root).unwrap();
    let first: Value = serde_json::from_str(
        &std::fs::read_to_string(
            root.join(".macc/log/supervisor/run-fixture-1-attempt-1/report.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        first["intervention"]["worktree"],
        final_report["intervention"]["worktree"]
    );
    assert_eq!(final_report["intervention"]["attempt"], 2);
    assert_eq!(
        std::fs::read_to_string(root.join(".macc/calls.log"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(root.join("continuation.txt")).unwrap(),
        "untracked\n"
    );
    assert!(!Path::new(final_report["intervention"]["worktree"].as_str().unwrap()).exists());
    drop(supervisor);
}

#[test]
fn attached_supervisor_cancels_an_active_diagnosis_on_operator_force_stop() {
    let temp = fixture(false);
    let root = temp.path();
    std::fs::write(
        root.join(".macc/fake-agent.sh"),
        "#!/bin/sh\nsleep 60 &\necho $! > .macc/tool-child.pid\nwait\n",
    )
    .unwrap();
    let mut supervisor = Supervisor(
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .args([
                "--cwd",
                root.to_str().unwrap(),
                "supervisor",
                "start",
                "--attach",
                "--coordinator-pid",
                "99999999",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(|| report(root).is_some_and(|v| v["intervention"]["status"] == "diagnosing"));
    let paths = ProjectPaths::from_root(root);
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    db.set_coordinator_control(&macc_core::coordinator_storage::CoordinatorControl {
        mode: "force_stopping".into(),
        requested_at: Some(chrono::Utc::now().to_rfc3339()),
        requested_by: Some("tui".into()),
        drain_snapshot_json: None,
        force_grace_seconds: Some(0),
        cleanup_after_force: Some(false),
        reason: Some("tui force stop".into()),
    })
    .unwrap();
    wait_for(|| supervisor.0.try_wait().unwrap().is_some());
    let r = report(root).unwrap();
    assert_eq!(r["intervention"]["status"], "interrupted");
    assert!(r["intervention"]["restart_pid"].is_null());
    assert!(!root.join(".macc/state/supervisor.pid").exists());
    let dir = Path::new(r["intervention"]["log_dir"].as_str().unwrap());
    assert!(dir.join("report.json").exists());
    assert!(dir.join("diagnosis-prompt.txt").exists());
}

#[test]
fn supervisor_registers_new_script_without_repair_or_operator_approval() {
    let temp = fixture(false);
    let root = temp.path();
    let block =
        json!({"reason":"CI delivery is pending","clears_when":"Candidate passes staging CI"});
    let prd = json!({"tasks":[{"id":"ROOT","blocked_on_external":block}]});
    std::fs::write(root.join("prd.json"), prd.to_string()).unwrap();
    git(root, &["add", "prd.json"]);
    git(root, &["commit", "-qm", "declare CI prerequisite"]);
    let db = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(
        &ProjectPaths::from_root(root),
    ));
    let mut snapshot = db.load_snapshot().unwrap();
    let task = snapshot.registry.find_task_mut("ROOT").unwrap();
    task.blocked_on_external = Some(serde_json::from_value(block).unwrap());
    task.task_runtime.set_last_error_details(
        "E904",
        "external_precondition",
        "CI delivery is pending",
    );
    db.save_snapshot(&snapshot).unwrap();
    let config = macc_core::load_canonical_config(root.join(".macc/macc.yaml")).unwrap();
    let startup = || {
        macc_core::coordinator::delivery_evidence::reconcile_before_dispatch(
            root,
            &root.join("prd.json"),
            config.automation.coordinator.as_ref(),
            &Default::default(),
            None,
        )
        .unwrap();
    };
    startup();
    let response = json!({"summary":"Waiting for the staging CI report","repairable":false,"task_ids":["ROOT"],"resume_checks":[{
        "task_id":"ROOT","category":"technical_condition","target":"staging","criterion":"Candidate passes staging CI","phase":"dev",
        "verifier":{"source":{"kind":"json_file","path":"reports/ci.json"},"script":include_str!("../../../docs/resume-checks/ci.sql")}
    }]});
    std::fs::write(root.join(".macc/fake-agent.sh"),format!("#!/bin/sh\nset -eu\ncat > .macc/supervisor-diagnosis.json <<'RESPONSE'\n{response}\nRESPONSE\n")).unwrap();
    let supervisor = start(root);
    wait_for(|| {
        report(root).is_some_and(|r| {
            matches!(
                r["intervention"]["status"].as_str(),
                Some("waiting_condition" | "escalated")
            )
        })
    });
    let r = report(root).unwrap();
    assert_eq!(
        r["intervention"]["status"], "waiting_condition",
        "{}",
        r["intervention"]["detail"]
    );
    assert_eq!(r["intervention"]["diagnosis"]["requires_human"], false);
    let registry = db.load_registry().unwrap();
    let task = registry.find_task("ROOT").unwrap();
    let check = task.task_runtime.resume_check.as_ref().unwrap();
    assert_eq!(
        r["intervention"]["diagnosis"]["resume_check_ids"]["ROOT"],
        check.id
    );
    assert_eq!(task.state, "blocked");
    assert!(check.checked_at.is_none());
    assert_eq!(
        std::fs::read_to_string(root.join("defect.txt")).unwrap(),
        "broken\n"
    );
    drop(supervisor);
    startup();
    let waiting = db.load_registry().unwrap();
    let task = waiting.find_task("ROOT").unwrap();
    assert_eq!(task.state, "blocked");
    assert_eq!(
        task.task_runtime
            .resume_check
            .as_ref()
            .unwrap()
            .last
            .as_ref()
            .unwrap()
            .status,
        macc_core::coordinator::resume_check::Status::Waiting
    );
    std::fs::create_dir_all(root.join("reports")).unwrap();
    std::fs::write(
        root.join("reports/ci.json"),
        json!({
            "commit":macc_core::git::head_commit(root).unwrap(), "environment":"staging",
            "finished_at":chrono::Utc::now().timestamp(), "result":"passed",
        })
        .to_string(),
    )
    .unwrap();
    startup();
    let released = db.load_registry().unwrap();
    let task = released.find_task("ROOT").unwrap();
    assert_eq!(task.state, "todo", "{:?}", task.task_runtime.resume_check);
    assert!(task.external_block_is_cleared());
}

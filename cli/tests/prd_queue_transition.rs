//! Exercise the actual public Sync path; no performer or provider is invoked.
use macc_core::coordinator_storage::{CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage};
use macc_core::{prd_queue, ProjectPaths};
use std::process::Command;
fn git(root: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
#[test]
fn public_sync_accepts_a_completed_series_and_retains_its_git_delivery_proof() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join(".macc")).unwrap();
    std::fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
    for (file, id) in [("old.json", "OLD"), ("new.json", "NEW")] {
        std::fs::write(
            root.join(file),
            format!("{{\"tasks\":[{{\"id\":\"{id}\",\"title\":\"Work\",\"priority\":\"1\"}}]}}"),
        )
        .unwrap();
    }
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
    git(root,&["commit","--allow-empty","-qm","chore: OLD - validate\n\n[macc:task OLD]\n[macc:validation true]\n[macc:result already_satisfied]\n\nEvidence: acceptance passed"]);
    let config = |file: &str| {
        std::fs::write(root.join(".macc/macc.yaml"),format!("tools:\n  enabled: [codex]\nautomation:\n  coordinator:\n    reference_branch: main\n    sync_unmerged_branches: false\n    prd_files: [{file}]\n")).unwrap()
    };
    std::fs::write(
        root.join("new.json"),
        r#"{"tasks":[{"id":"NEW","title":"Work","priority":"1","dependencies":["OLD"]}]}"#,
    )
    .unwrap();
    config("old.json");
    let sync = || {
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .arg("--cwd")
            .arg(root)
            .args(["coordinator", "sync"])
            .env("MACC_INTERNAL_INVOCATION", "1")
            .output()
            .unwrap()
    };
    let out = sync();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let paths = ProjectPaths::from_root(root);
    let store = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let snapshot = store.load_snapshot().unwrap();
    assert!(snapshot.registry.find_task("OLD").unwrap().is_merged());
    std::fs::create_dir_all(root.join(".macc/state")).unwrap();
    std::fs::write(
        root.join(".macc/state/prd-queue-completed-0.json"),
        serde_json::to_vec(&snapshot.registry).unwrap(),
    )
    .unwrap();
    prd_queue::save(
        root,
        &prd_queue::Progress {
            entries: prd_queue::validate(root, &["old.json".into()]).unwrap(),
            next: 1,
            status: "completed".into(),
            reason: None,
        },
    )
    .unwrap();
    config("new.json");
    let out = sync();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let snapshot = store.load_snapshot().unwrap();
    assert!(snapshot.registry.find_task("NEW").is_some());
    assert!(snapshot.registry.external_merged_task_ids.contains("OLD"));
    assert_eq!(prd_queue::load(root).unwrap().next, 0);
    assert_eq!(
        std::fs::read_dir(root.join(".macc/state/prd-queue-history"))
            .unwrap()
            .count(),
        1
    );
    assert!(!snapshot
        .events
        .iter()
        .any(|e| e.event_type == "task_dispatched"));
    // An unfinished replacement is actionable, printed once, and leaves NEW intact.
    config("old.json");
    let failed = sync();
    assert!(!failed.status.success());
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert_eq!(
        stderr.matches(prd_queue::transition::CODE).count(),
        1,
        "{stderr}"
    );
    assert!(store
        .load_snapshot()
        .unwrap()
        .registry
        .find_task("NEW")
        .is_some());
}

#[test]
fn removing_last_prd_clears_legacy_selection_and_reports_default() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    std::fs::create_dir(root.join(".macc")).unwrap();
    std::fs::create_dir_all(root.join("docs/prd")).unwrap();
    let legacy = "docs/prd/a.json";
    let content = r#"{"tasks":[{"id":"A"}]}"#;
    std::fs::write(root.join(legacy), content).unwrap();
    std::fs::write(root.join("docs/prd/other.json"), content).unwrap();
    let paths = ProjectPaths::from_root(root);
    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_macc"))
            .current_dir(root)
            .args(["coordinator", "prds"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    for explicit_queue in ["", "    prd_files: [docs/prd/other.json]\n"] {
        let yaml = format!(
            "tools:\n  enabled: []\nautomation:\n  coordinator:\n    prd_file: {legacy}\n{explicit_queue}"
        );
        std::fs::write(&paths.config_path, yaml).unwrap();
        assert!(run(&["list"]).contains(if explicit_queue.is_empty() {
            legacy
        } else {
            "docs/prd/other.json"
        }));
        assert!(run(&["remove", "1"]).contains("prd.json"));
        let listed = run(&["list"]);
        assert!(listed.contains("prd.json"));
        assert!(!listed.contains("docs/prd/"));
        let config = macc_core::load_canonical_config(&paths.config_path)
            .unwrap()
            .automation
            .coordinator
            .unwrap();
        assert!(config.prd_file.is_none() && config.prd_files.is_empty());
        assert_eq!(
            prd_queue::active_path(root, Some(&config), None).unwrap(),
            root.join("prd.json")
        );
        assert_eq!(std::fs::read_to_string(root.join(legacy)).unwrap(), content);
    }
}

#[test]
fn public_resume_restores_the_decision_owner_without_dispatching_past_the_gate() {
    use macc_core::coordinator::{decisions, model::TaskRegistry};
    use serde_json::json;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Tester"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    let paths = ProjectPaths::from_root(root);
    std::fs::create_dir_all(&paths.macc_dir).unwrap();
    let files = vec!["first.json".to_string(), "second.json".to_string()];
    for (file, id) in files.iter().zip(["FIRST", "CHECK"]) {
        std::fs::write(root.join(file), json!({"tasks":[{"id":id}]}).to_string()).unwrap();
    }
    std::fs::write(&paths.config_path,"tools:\n  enabled: []\nautomation:\n  coordinator:\n    prd_files: [first.json, second.json]\n    reference_branch: main\n    decisions:\n      enabled: true\n  supervisor:\n    auto_restart: false\n").unwrap();
    git(
        root,
        &[
            "commit",
            "--allow-empty",
            "-qm",
            "feat: delivered\n\n[macc:task CHECK]",
        ],
    );
    git(root, &["branch", "worker"]);
    let reg = TaskRegistry::from_value(
        &json!({"tasks":[{"id":"CHECK","state":"blocked","worktree":{"branch":"worker"}}]}),
    )
    .unwrap();
    let sqlite = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let mut snapshot = sqlite.load_snapshot().unwrap();
    snapshot.registry = reg.clone();
    sqlite.save_snapshot(&snapshot).unwrap();
    let mut store = decisions::Store::open(root).unwrap();
    let plan = store
        .ensure_plan(root, std::path::Path::new("second.json"), "main", &reg)
        .unwrap();
    let request=serde_json::from_value(json!({"idempotency_key":"check","task_id":"CHECK","question":"Windows NVDA result?","topic":"verification","context":"The workstation is external","investigated":["Checked local devices"],"recommendation":"Supply the observed result","risk":"high","impact":{"shared_contract":true,"evidence":["external check"]}})).unwrap();
    let decision = store
        .submit(
            &plan,
            request,
            "agent:CHECK",
            "agent",
            &decisions::policy(root).unwrap(),
        )
        .unwrap();
    let mut progress = prd_queue::Progress {
        entries: prd_queue::validate(root, &files).unwrap(),
        next: 1,
        status: "blocked".into(),
        reason: None,
    };
    let history = root.join(".macc/state/prd-queue-history/reset");
    std::fs::create_dir_all(&history).unwrap();
    for (name, body) in [
        ("transition.json", json!({})),
        ("progress.json", json!(progress)),
        ("registry.json", json!(reg)),
        (
            "prd-queue-completed-0.json",
            json!({"tasks":[{"id":"FIRST","state":"merged"}]}),
        ),
    ] {
        std::fs::write(history.join(name), body.to_string()).unwrap();
    }
    progress.next = 0;
    prd_queue::save(root, &progress).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("--cwd")
        .arg(root)
        .args(["coordinator", "resume"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.contains("Code integrated into main") && text.contains(&decision.id),
        "{text}"
    );
    assert_eq!(prd_queue::load(root).unwrap().next, 1);
    assert_eq!(
        sqlite
            .load_snapshot()
            .unwrap()
            .registry
            .find_task("CHECK")
            .unwrap()
            .state,
        "blocked"
    );
    assert!(!root.join(".macc/state/resume-after-decision.json").exists());
}

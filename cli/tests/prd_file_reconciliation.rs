//! Operator recovery after a manual edit, through the public coordinator and decision commands.
use macc_core::coordinator::decisions::Store;
use serde_json::{json, Value};
use std::{fs, process::Command};

#[test]
fn sync_identifies_drift_then_accepts_only_the_reviewed_file() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let git = |args: &[&str]| {
        assert!(Command::new("git")
            .current_dir(root)
            .args(args)
            .status()
            .unwrap()
            .success())
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Test operator"]);
    git(&["config", "user.email", "operator@example.invalid"]);
    fs::create_dir(root.join(".macc")).unwrap();
    fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
    fs::write(root.join(".macc/macc.yaml"), "tools:\n  enabled: []\nautomation:\n  coordinator:\n    prd_files: [plan.json]\n    reference_branch: main\n    decisions:\n      enabled: true\n").unwrap();
    let file = root.join("plan.json");
    let graph = |step: &str| json!({"tasks":[{"id":"A","title":"Check","steps":[step]}]});
    fs::write(&file, graph("old").to_string()).unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "fixture"]);
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .current_dir(root)
            .args(args)
            .env("MACC_INTERNAL_INVOCATION", "1")
            .output()
            .unwrap()
    };
    let ok = |args: &[&str]| {
        let out = run(args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };
    let sync = ["coordinator", "sync"];
    ok(&sync);
    let plan = Store::inspect_registered(root)
        .unwrap()
        .unwrap()
        .plans()
        .unwrap()
        .pop()
        .unwrap();
    fs::write(&file, graph("first edit").to_string()).unwrap();
    ok(&["coordinator", "prds", "reset", "--confirm"]);
    let rejected = run(&sync);
    assert!(!rejected.status.success());
    let message = String::from_utf8_lossy(&rejected.stderr);
    for clue in [
        "plan.json",
        "A updated",
        "steps",
        "prds reset",
        "reconcile-file",
        &plan.id,
    ] {
        assert!(message.contains(clue), "{message}");
    }
    let command = ["decisions", "graph", "reconcile-file", "--plan", &plan.id];
    let preview = || serde_json::from_str::<Value>(&ok(&command)).unwrap();
    let mut invalid = graph("first edit");
    invalid["tasks_done"] = json!([{"id":"X"}]);
    fs::write(&file, invalid.to_string()).unwrap();
    let denied = run(&command);
    assert!(!denied.status.success());
    assert!(String::from_utf8_lossy(&denied.stderr).contains("Archived tasks are immutable"));
    assert_eq!(fs::read_to_string(&file).unwrap(), invalid.to_string());
    fs::write(&file, graph("first edit").to_string()).unwrap();
    let first = preview();
    let edited = format!("{}\n", graph("second edit"));
    fs::write(&file, &edited).unwrap();
    let confirm = |token: &str| run(&[command.as_slice(), &["--confirm", token]].concat());
    let stale = confirm(first["confirmation"].as_str().unwrap());
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("Preview expired"));
    let current = preview();
    assert_eq!(current["graph_version"], plan.graph_version);
    let accepted = confirm(current["confirmation"].as_str().unwrap());
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert_eq!(fs::read_to_string(&file).unwrap(), edited);
    ok(&sync);
    assert_eq!(preview()["status"], "already in sync");
    let saved = Store::inspect_registered(root)
        .unwrap()
        .unwrap()
        .plan(&plan.id)
        .unwrap();
    assert_eq!(saved.graph_version, plan.graph_version + 1);
    assert_eq!(saved.graph, graph("second edit"));

    assert_eq!(message.matches("PRD edits awaiting approval").count(), 1);
    fs::write(&file, graph("interactive edit").to_string()).unwrap();
    let interactive = |answer: &[u8]| {
        use std::{io::Write, process::Stdio};
        let mut child = Command::new(env!("CARGO_BIN_EXE_macc"))
            .current_dir(root)
            .args(["coordinator", "sync", "--interactive"])
            .env("MACC_INTERNAL_INVOCATION", "1")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(answer).unwrap();
        child.wait_with_output().unwrap()
    };
    let cancelled = interactive(b"\n");
    assert!(!cancelled.status.success());
    assert!(
        String::from_utf8_lossy(&cancelled.stderr).contains("Sync cancelled"),
        "{cancelled:?}"
    );
    let approved = interactive(b"yes\n");
    assert!(approved.status.success(), "{approved:?}");
    assert_eq!(preview()["status"], "already in sync");
}

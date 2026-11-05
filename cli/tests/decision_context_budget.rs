//! Public command coverage with immutable local decision sources; no AI tools.
use macc_core::{
    config::CanonicalConfig,
    coordinator::{decisions as dm, model::TaskRegistry},
};
use serde_json::{json, Value};
use std::{collections::BTreeMap, path::Path, process::Command};
fn git(root: &Path, args: &[&str]) {
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
fn public_budget_preview_and_scoped_source_commands_share_the_issued_context() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".macc")).unwrap();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    std::fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
    let graph = json!({"global_constraints":["Retain exact global requirements"],"tasks":[{"id":"A","description":"Complete prerequisite éè 日本語 ".repeat(800),"dependencies":[]},{"id":"B","dependencies":["A"]},{"id":"C","dependencies":[]}]});
    std::fs::write(root.join("prd.json"), graph.to_string()).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "fixture"]);
    let mut config = CanonicalConfig::default();
    let cfg = config
        .automation
        .coordinator
        .get_or_insert_with(Default::default);
    cfg.reference_branch = Some("main".into());
    cfg.prd_file = Some("prd.json".into());
    cfg.decisions = Some(dm::Policy {
        enabled: true,
        max_context_bytes: 32768,
        ..Default::default()
    });
    std::fs::write(root.join(".macc/macc.yaml"), config.to_yaml().unwrap()).unwrap();
    let registry = TaskRegistry::from_value(&graph).unwrap();
    macc_core::coordinator::state::coordinator_state_registry_save(
        root,
        &BTreeMap::new(),
        &registry.to_value().unwrap(),
    )
    .unwrap();
    let mut store = dm::Store::open(root).unwrap();
    let plan = store
        .ensure_plan(root, Path::new("prd.json"), "main", &registry)
        .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_macc"))
        .arg("--cwd")
        .arg(root)
        .args(["decisions", "context-budget", "--task", "B"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["fits"], true);
    assert_eq!(report["prerequisite_count"], 1);
    assert_eq!(report["budget_bytes"], 32768);
    let (context, _) = store
        .issue(
            &plan,
            "B",
            "cli-source",
            "dev",
            root,
            &dm::policy(root).unwrap(),
        )
        .unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_macc"))
            .arg("--cwd")
            .arg(root)
            .arg("decisions")
            .args(args)
            .env("MACC_DECISION_PLAN", &plan.id)
            .env("MACC_DECISION_ATTEMPT", "cli-source")
            .env("MACC_DECISION_PHASE", "dev")
            .output()
            .unwrap()
    };
    let out = run(&[
        "context-source",
        "--dependency",
        "A",
        "--pointer",
        "/description",
        "--limit",
        "2048",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.len() <= 2048);
    let page: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(page["context_digest"], context.context_digest);
    assert!(page["next_offset"].as_u64().unwrap() > 0);
    let out = run(&[
        "context-source",
        "--plan",
        "--pointer",
        "/global_constraints",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let page: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(page["content"].as_str().unwrap()).unwrap(),
        graph["global_constraints"]
    );
    assert!(!run(&["context-source", "--dependency", "C"])
        .status
        .success());
    assert!(!run(&["context-source", "--dependency", "A", "--plan"])
        .status
        .success());
}

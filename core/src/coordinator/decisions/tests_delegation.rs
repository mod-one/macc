use super::*;
use crate::coordinator::{approval::reconcile_human_gates_for_project, model::TaskRegistry};
use crate::coordinator_storage::{
    CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage,
};
use std::path::Path;
fn git(root: &Path, args: &[&str]) -> String {
    let out = crate::git::run_git_output_mapped(root, args, "delegation regression").unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}
fn save(root: &Path, registry: &TaskRegistry) {
    let mut snapshot = CoordinatorSnapshot::empty();
    snapshot.registry = registry.clone();
    SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(
        &crate::ProjectPaths::from_root(root),
    ))
    .save_snapshot(&snapshot)
    .unwrap();
}
fn fixture() -> (tempfile::TempDir, TaskRegistry) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    std::fs::create_dir_all(root.join(".macc")).unwrap();
    std::fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
    let mut cfg = crate::config::CanonicalConfig::default();
    let c = cfg
        .automation
        .coordinator
        .get_or_insert_with(Default::default);
    c.reference_branch = Some("main".into());
    c.prd_file = Some("prd.json".into());
    c.decisions = Some(Policy {
        enabled: true,
        human_question_min_risk: Consultation::NeverAsk,
        ..Default::default()
    });
    std::fs::write(root.join(".macc/macc.yaml"), cfg.to_yaml().unwrap()).unwrap();
    let graph = serde_json::json!({"tasks":[{"id":"A","state":"merged","title":"Delivered source"},{"id":"G","state":"waiting_approval","dependencies":["A"],"gate":{"kind":"human_approval","subject_task":"A","required_approvers":[{"role":"OWNER","count":1}]}},{"id":"C","state":"todo"}]});
    std::fs::write(
        root.join("prd.json"),
        serde_json::to_vec_pretty(&graph).unwrap(),
    )
    .unwrap();
    git(root, &["add", "."]);
    let msg = crate::commit_message::task_commit(
        crate::commit_message::CommitType::Feat,
        "A",
        Some("Delivered source"),
        None,
    )
    .format();
    git(root, &["commit", "-qm", &msg]);
    let registry = TaskRegistry::from_value(&serde_json::json!({"tasks":graph["tasks"]})).unwrap();
    save(root, &registry);
    (dir, registry)
}

#[test]
fn delegated_gate_is_stable_across_native_and_ledger_sync_without_fake_human_votes() {
    let (dir, mut registry) = fixture();
    let root = dir.path();
    for _ in 0..4 {
        sync(root, &root.join("prd.json"), &mut registry).unwrap();
        save(root, &registry);
        let events =
            reconcile_human_gates_for_project(root, &mut registry, "2026-11-09T12:00:00Z").unwrap();
        assert!(!events
            .iter()
            .any(|event| event.event == "approval_unverified"));
        let gate = registry.find_task("G").unwrap();
        assert_eq!(gate.state, "approved");
        assert!(gate.task_runtime.approval.is_none());
        assert_eq!(
            gate.task_runtime.extra["delegated_gate_resolution"]["origin"],
            "delegated_autonomous"
        );
        save(root, &registry);
    }
    let ledger = approval_ledger_for_project(root, &registry).unwrap();
    assert!(ledger.get("G").is_none_or(|votes| votes.is_empty()));
    // A forged projection with no corresponding durable resolution must not approve a gate.
    registry
        .find_task_mut("G")
        .unwrap()
        .task_runtime
        .extra
        .get_mut("delegated_gate_resolution")
        .unwrap()["decision_id"] = serde_json::json!("forged");
    reconcile_human_gates_for_project(root, &mut registry, "2026-11-09T12:01:00Z").unwrap();
    assert_ne!(registry.find_task("G").unwrap().state, "approved");
}

#[test]
fn delegation_is_invalidated_by_policy_subject_revision_and_durable_revocation() {
    for change in ["policy", "revision", "revocation"] {
        let (dir, mut registry) = fixture();
        let root = dir.path();
        sync(root, &root.join("prd.json"), &mut registry).unwrap();
        save(root, &registry);
        match change {
            "policy" => {
                let path = root.join(".macc/macc.yaml");
                let mut cfg = crate::load_canonical_config(&path).unwrap();
                cfg.automation
                    .coordinator
                    .as_mut()
                    .unwrap()
                    .decisions
                    .as_mut()
                    .unwrap()
                    .human_question_min_risk = Consultation::High;
                std::fs::write(path, cfg.to_yaml().unwrap()).unwrap();
            }
            "revision" => {
                let msg = crate::commit_message::task_commit(
                    crate::commit_message::CommitType::Feat,
                    "A",
                    Some("Changed source"),
                    None,
                )
                .format();
                git(root, &["commit", "--allow-empty", "-qm", &msg]);
            }
            _ => {
                let (mut store, _) = active_plan(root).unwrap();
                let id = registry.find_task("G").unwrap().task_runtime.extra
                    ["delegated_gate_resolution"]["decision_id"]
                    .as_str()
                    .unwrap();
                let d = store.decision(id).unwrap();
                store
                    .revoke(id, d.version, "Human", "Withdraw delegated authority")
                    .unwrap();
            }
        }
        reconcile_human_gates_for_project(root, &mut registry, "2026-11-09T12:01:00Z").unwrap();
        assert_ne!(
            registry.find_task("G").unwrap().state,
            "approved",
            "{change}"
        );
    }
}

#[test]
fn ownership_failure_is_local_and_later_unrelated_gates_still_reconcile() {
    let (dir, mut registry) = fixture();
    let root = dir.path();
    let path = root.join(".macc/worktree/worker-01");
    git(
        root,
        &[
            "worktree",
            "add",
            "--detach",
            path.to_str().unwrap(),
            "main",
        ],
    );
    let task = registry.find_task_mut("C").unwrap();
    task.ensure_worktree().worktree_path = Some(path.to_string_lossy().into());
    task.ensure_worktree().branch = Some("ai/future/missing".into());
    task.task_runtime.current_phase = Some("merge".into());
    task.task_runtime
        .extra
        .insert("decision_wait".into(), serde_json::json!("context_refresh"));
    registry.tasks.reverse();
    save(root, &registry);
    sync(root, &root.join("prd.json"), &mut registry).unwrap();
    let task = registry.find_task("C").unwrap();
    assert_eq!(task.state, "blocked");
    assert_eq!(task.task_runtime.last_error_code.as_deref(), Some("E901"));
    assert_eq!(task.task_runtime.current_phase.as_deref(), Some("merge"));
    assert!(task
        .task_runtime
        .last_error_message
        .as_deref()
        .unwrap()
        .contains("expected_branch"));
    assert_eq!(registry.find_task("G").unwrap().state, "approved");
}

#[test]
fn an_explicit_live_human_veto_is_never_overridden_by_standing_delegation() {
    use crate::coordinator::model::{ApprovalDecision, ApprovalRecord};
    let (dir, mut registry) = fixture();
    let root = dir.path();
    sync(root, &root.join("prd.json"), &mut registry).unwrap();
    save(root, &registry);
    let (mut store, plan) = active_plan(root).unwrap();
    let record = ApprovalRecord {
        decision: ApprovalDecision::Rejected,
        role: "OWNER".into(),
        actor: "Human".into(),
        revision: Some(git(root, &["rev-parse", "HEAD"])),
        evidence: Some("Independent veto of exact source".into()),
        reason: Some("Do not proceed".into()),
        recorded_at: chrono::Utc::now().to_rfc3339(),
    };
    store.record_approval(root, &plan, "G", &record).unwrap();
    sync(root, &root.join("prd.json"), &mut registry).unwrap();
    assert_eq!(registry.find_task("G").unwrap().state, "rejected");
    reconcile_human_gates_for_project(root, &mut registry, &chrono::Utc::now().to_rfc3339())
        .unwrap();
    assert_eq!(registry.find_task("G").unwrap().state, "rejected");
}

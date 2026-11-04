use super::*;
use crate::coordinator::{decisions, model::TaskRegistry};
use serde_json::json;

fn git(root: &Path, args: &[&str]) {
    let out = std::process::Command::new("git")
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

fn fixture(task_id: &str) -> (tempfile::TempDir, DispatchCandidate, AcquiredWorktree) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".macc")).unwrap();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@example.invalid"]);
    std::fs::write(root.join(".gitignore"), ".macc/\nworktree.prd.json\n").unwrap();
    std::fs::write(root.join("code.txt"), "initial").unwrap();
    let graph = json!({"tasks":[{"id":task_id,"title":"Task","dependencies":[],"exclusive_resources":["fixture-resource"]}]});
    std::fs::write(root.join("prd.json"), graph.to_string()).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "fixture"]);
    let mut config = crate::config::CanonicalConfig::default();
    let cfg = config
        .automation
        .coordinator
        .get_or_insert_with(Default::default);
    cfg.prd_file = Some("prd.json".into());
    cfg.reference_branch = Some("main".into());
    cfg.decisions = Some(decisions::Policy {
        enabled: true,
        ..Default::default()
    });
    std::fs::write(root.join(".macc/macc.yaml"), config.to_yaml().unwrap()).unwrap();
    let path = root.join(".macc/worktree/worker-01");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "ai/fake/worker-01",
            path.to_str().unwrap(),
            "main",
        ],
    );
    write_worktree_prd_for_task(&root.join("prd.json"), task_id, &path).unwrap();
    let registry = TaskRegistry::from_value(&graph).unwrap();
    crate::coordinator::state::coordinator_state_registry_save(
        root,
        &BTreeMap::new(),
        &registry.to_value().unwrap(),
    )
    .unwrap();
    let candidate = DispatchCandidate {
        task: crate::coordinator::task_selector::SelectedTask {
            id: task_id.into(),
            title: "Task".into(),
            tool: "fake".into(),
            base_branch: "main".into(),
            is_fallback: false,
            resume_worktree: None,
        },
        worktree_slot: WorktreeSlot::Auto,
    };
    let worktree = AcquiredWorktree {
        path,
        branch: "ai/fake/worker-01".into(),
        last_commit: crate::git::head_commit(root).unwrap(),
        active_session_id: Some("retained-provider-session".into()),
    };
    (tmp, candidate, worktree)
}

fn claim(root: &Path, candidate: &DispatchCandidate, worktree: &AcquiredWorktree) -> DispatchClaim {
    let mut registry =
        crate::coordinator::state::coordinator_state_registry_load(root, &BTreeMap::new()).unwrap();
    claim_task_in_registry(root, candidate, worktree, &mut registry, None).unwrap()
}

#[test]
fn native_dispatch_claim_issues_decision_context_and_matches_runner_receipts() {
    let (tmp, candidate, worktree) = fixture("L4P-ASYNC-STATES-001");
    let root = tmp.path();
    let first = claim(root, &candidate, &worktree);
    let env = launch::decision_environment(root, &first, Path::new("/fixture/macc")).unwrap();
    let env: HashMap<_, _> = env.into_iter().collect();
    assert_eq!(env["MACC_DECISION_ATTEMPT"], first.claim_id);
    let file = Path::new(&env["MACC_DECISION_CONTEXT_FILE"]);
    let context: decisions::ContextPackage =
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(context.attempt_id, first.claim_id);
    assert_eq!(context.phase, "dev");
    assert!(file.starts_with(&worktree.path));
    let mut store = decisions::Store::open(root).unwrap();
    store
        .issued(
            &context.plan_id,
            &first.claim_id,
            "dev",
            &context.render(),
            &decisions::policy(root).unwrap(),
        )
        .unwrap();
    let saved =
        crate::coordinator::state::coordinator_state_registry_load(root, &BTreeMap::new()).unwrap();
    assert_eq!(
        saved["tasks"][0]["task_runtime"]["claim_id"],
        first.claim_id
    );
    assert_eq!(
        saved["tasks"][0]["task_runtime"]["decision_context"]["attempt_id"],
        first.claim_id
    );
    let next = claim(root, &candidate, &worktree);
    assert_ne!(next.claim_id, first.claim_id);
    let next_env = launch::decision_environment(root, &next, Path::new("/fixture/macc")).unwrap();
    assert!(next_env
        .iter()
        .any(|(k, v)| *k == "MACC_DECISION_ATTEMPT" && v == &next.claim_id));
    assert!(store
        .read_context(&context.plan_id, &first.claim_id, "dev")
        .is_ok());
}

#[test]
fn claim_identifiers_are_bounded_even_for_long_task_names() {
    let id = format!("TASK-{}", "X".repeat(150));
    let (tmp, candidate, worktree) = fixture(&id);
    let claim = claim(tmp.path(), &candidate, &worktree);
    assert!(claim.claim_id.len() < 128);
    decisions::validate_context_identity(&id, &claim.claim_id, "dev").unwrap();
}

#[test]
fn failed_preparation_blocks_only_its_claim_and_retains_work() {
    let (tmp, candidate, worktree) = fixture("A");
    let root = tmp.path();
    let first = claim(root, &candidate, &worktree);
    let code = worktree.path.join("code.txt");
    std::fs::write(&code, "staged work").unwrap();
    git(&worktree.path, &["add", "code.txt"]);
    std::fs::write(&code, "staged and unstaged work").unwrap();
    let before = crate::git::run_git_output_mapped(&worktree.path, &["diff", "--cached"], "test")
        .unwrap()
        .stdout;
    let error = failure::record(
        root,
        &first,
        decisions::error("Invalid context attempt"),
        None,
    )
    .unwrap();
    assert!(error.to_string().contains("task=A phase=dev"));
    let saved =
        crate::coordinator::state::coordinator_state_registry_load(root, &BTreeMap::new()).unwrap();
    let task = &saved["tasks"][0];
    assert_eq!(task["state"], "blocked");
    assert_eq!(task["task_runtime"]["status"], "failed");
    assert_eq!(task["task_runtime"]["last_error_code"], "E901");
    assert!(task["task_runtime"]["pid"].is_null());
    assert_eq!(task["worktree"]["branch"], worktree.branch);
    assert_eq!(
        std::fs::read_to_string(code).unwrap(),
        "staged and unstaged work"
    );
    assert_eq!(
        crate::git::run_git_output_mapped(&worktree.path, &["diff", "--cached"], "test")
            .unwrap()
            .stdout,
        before
    );
    assert!(saved["resource_locks"]
        .as_object()
        .is_none_or(|locks| locks.is_empty()));
    let next = claim(root, &candidate, &worktree);
    assert!(failure::record(root, &first, decisions::error("late failure"), None).is_err());
    let saved =
        crate::coordinator::state::coordinator_state_registry_load(root, &BTreeMap::new()).unwrap();
    assert_eq!(saved["tasks"][0]["task_runtime"]["claim_id"], next.claim_id);
    assert_eq!(saved["tasks"][0]["state"], "claimed");
}

#[test]
fn invalid_attempt_is_rejected_before_any_decision_context_is_issued() {
    let (tmp, candidate, worktree) = fixture("A");
    let mut claim = claim(tmp.path(), &candidate, &worktree);
    claim.claim_id = "coordinator-A-2026-11-04T04:28:30Z".into();
    let error =
        launch::decision_environment(tmp.path(), &claim, Path::new("/fixture/macc")).unwrap_err();
    assert!(error.to_string().contains("Invalid context attempt"));
    assert!(error.to_string().contains("task \"A\""));
    assert!(!worktree.path.join(".macc/runtime/decisions").exists());
    let error = decisions::validate_context_identity("A", "valid-attempt", "testing").unwrap_err();
    assert!(error.to_string().contains("Invalid context phase"));
}

#[tokio::test]
async fn preparation_exception_is_handled_by_the_actual_launch_boundary() {
    let (tmp, candidate, worktree) = fixture("A");
    let root = tmp.path();
    let claim = claim(root, &candidate, &worktree);
    let canonical = crate::load_canonical_config(root.join(".macc/macc.yaml")).unwrap();
    let mut state = CoordinatorRunState::new();
    let env = CoordinatorEnvConfig::default();
    let error = launch_performer(
        LaunchPerformerContext {
            repo_root: root,
            prd_file: &root.join("missing.json"),
            canonical: &canonical,
            coordinator: canonical.automation.coordinator.as_ref(),
            env_cfg: &env,
            state: &mut state,
            logger: None,
        },
        &claim,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("task=A phase=dev"));
    let saved =
        crate::coordinator::state::coordinator_state_registry_load(root, &BTreeMap::new()).unwrap();
    assert_eq!(saved["tasks"][0]["state"], "blocked");
    assert_eq!(
        saved["tasks"][0]["task_runtime"]["last_error_origin"],
        "dispatch_preparation"
    );
    assert!(state.join_set.is_empty());
    assert!(state.active_jobs.is_empty());
}

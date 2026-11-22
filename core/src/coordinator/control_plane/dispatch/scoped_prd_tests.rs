use super::*;
fn legacy_scoped_prd_incident() -> (tempfile::TempDir, AcquiredWorktree, DispatchClaim, Vec<u8>) {
    let (temp, candidate, worker) = fixture("B");
    let root = temp.path();
    crate::worktree::write_worktree_metadata(
        &worker.path,
        crate::WorktreeMetadata {
            id: "worker-01".into(),
            slug: "worker-01".into(),
            tool: "fake".into(),
            scope: None,
            feature: None,
            base: "main".into(),
            branch: worker.branch.clone(),
            released_head: None,
        },
    )
    .unwrap();
    let prd = root.join("prd.json");
    let mut graph: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&prd).unwrap()).unwrap();
    graph["tasks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"A","title":"Earlier task"}));
    std::fs::write(&prd, graph.to_string()).unwrap();
    write_worktree_prd_for_task(&prd, "A", &worker.path).unwrap();
    let path = worker.path.join("worktree.prd.json");
    let mut doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    doc["tasks"][0]["passes"] = true.into();
    let bytes = format!("{}\n", serde_json::to_string_pretty(&doc).unwrap()).into_bytes();
    std::fs::write(&path, &bytes).unwrap();
    let claim = claim(root, &candidate, &worker);
    failure::record(
        root,
        &claim,
        MaccError::Validation(format!(
            "Cannot replace {}: preserve or move the existing scoped PRD before dispatch",
            path.display()
        )),
        None,
    )
    .unwrap();
    (temp, worker, claim, bytes)
}

#[test]
fn scoped_prd_legacy_before_launch_recovers_and_requeues_the_retained_worker() {
    use crate::coordinator::scoped_prd::{self, Cause};
    let (temp, worker, claim, original) = legacy_scoped_prd_incident();
    let paths = crate::ProjectPaths::from_root(temp.path());
    let registry = || {
        TaskRegistry::from_value(
            &crate::coordinator::state::coordinator_state_registry_load(
                &paths.root,
                &BTreeMap::new(),
            )
            .unwrap(),
        )
        .unwrap()
    };
    let registry_before = registry();
    let task = registry_before.find_task("B").unwrap();
    assert!(scoped_prd::is_dispatch_failure(task));
    let details = scoped_prd::describe(&paths.root, task).unwrap();
    assert_eq!(details.cause, Cause::LegacyPerformerMarker);
    assert_eq!(details.previous_task.as_deref(), Some("A"));
    let request = crate::coordinator::control::Request::new(task, "recover proven marker").unwrap();
    let config = crate::load_canonical_config(&paths.config_path).unwrap();
    for _ in 0..2 {
        crate::service::coordinator_workflow::submit_unblock_request(
            &paths,
            config.automation.coordinator.as_ref(),
            &Default::default(),
            &request,
        )
        .unwrap();
    }
    let after = registry();
    let task = after.find_task("B").unwrap();
    assert_eq!(task.state, "todo");
    assert!(task.is_awaiting_same_worktree_retry());
    assert_eq!(task.worktree_path(), worker.path.to_str());
    assert_eq!(
        task.task_runtime.claim_id.as_deref(),
        Some(claim.claim_id.as_str())
    );
    let archives: Vec<_> = paths
        .root
        .join(".macc/persistent/scoped-prd-archive")
        .read_dir()
        .unwrap()
        .collect();
    assert_eq!(archives.len(), 1);
    assert_eq!(
        std::fs::read(archives.into_iter().next().unwrap().unwrap().path()).unwrap(),
        original
    );
    let expected =
        crate::coordinator::helpers::scoped_prd_for_task(&paths.root.join("prd.json"), "B")
            .unwrap();
    scoped_prd::verify_dispatch(&expected, "B", &worker.path).unwrap();
    git(&worker.path, &["add", "-f", scoped_prd::NAME]);
    assert!(matches!(
        scoped_prd::verify_dispatch(&expected, "B", &worker.path),
        Err(MaccError::ScopedPrd(_))
    ));
}

#[test]
fn scoped_prd_unknown_edits_preserve_contract_and_worker_work() {
    use crate::coordinator::scoped_prd;
    let (temp, worker, _, _) = legacy_scoped_prd_incident();
    let root = temp.path();
    let path = worker.path.join(scoped_prd::NAME);
    std::fs::write(&path, b"user file").unwrap();
    std::fs::write(worker.path.join("code.txt"), b"staged work").unwrap();
    git(&worker.path, &["add", "code.txt"]);
    let registry = TaskRegistry::from_value(
        &crate::coordinator::state::coordinator_state_registry_load(root, &BTreeMap::new())
            .unwrap(),
    )
    .unwrap();
    let task = registry.find_task("B").unwrap();
    let error = scoped_prd::recover_dispatch(root, &registry, task, &root.join("prd.json"), "main")
        .unwrap_err();
    assert!(
        matches!(error,MaccError::ScopedPrd(i) if i.cause == scoped_prd::Cause::UnverifiedFile)
    );
    assert_eq!(std::fs::read(path).unwrap(), b"user file");
    assert!(!root.join(".macc/persistent/scoped-prd-archive").exists());
    assert_eq!(
        crate::git::status_porcelain_v1(&worker.path, true).unwrap()[0].index_status,
        'M'
    );
}

#[test]
fn scoped_prd_reuse_skips_an_unverified_file_before_switching_branches() {
    let (temp, worker, _, _) = legacy_scoped_prd_incident();
    let root = temp.path();
    let path = worker.path.join("worktree.prd.json");
    std::fs::write(&path, b"operator notes").unwrap();
    let worker2 = root.join(".macc/worktree/worker-02");
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            "ai/fake/worker-02",
            worker2.to_str().unwrap(),
            "main",
        ],
    );
    let mut metadata = crate::read_worktree_metadata(&worker.path)
        .unwrap()
        .unwrap();
    metadata.id = "worker-02".into();
    metadata.slug = metadata.id.clone();
    metadata.branch = "ai/fake/worker-02".into();
    crate::worktree::write_worktree_metadata(&worker2, metadata).unwrap();
    let registry = json!({"tasks":[]});
    let (reused, _) = crate::coordinator::helpers::find_reusable_worktree_for_task(
        root,
        &registry,
        "fake",
        "main",
        0,
        &Default::default(),
        "B",
    )
    .unwrap();
    assert_eq!(reused.unwrap().0, worker2);
    assert_eq!(std::fs::read(path).unwrap(), b"operator notes");
    assert_eq!(
        crate::git::current_branch_name(&worker.path).unwrap(),
        worker.branch
    );
}

#[test]
fn scoped_prd_recovery_checks_the_original_false_value_and_refuses_other_changes() {
    use crate::coordinator::scoped_prd;
    let (temp, worker, _, _) = legacy_scoped_prd_incident();
    let prd = temp.path().join("prd.json");
    let slot = worker.path.join(scoped_prd::NAME);
    // First recover the historical missing-field variant. Generate a new
    // contract with an explicit false baseline through the real writer.
    scoped_prd::prepare(&worker.path, "B").unwrap();
    std::fs::write(
        &prd,
        r#"{"tasks":[{"id":"B","passes":false,"title":"Keep"}]}"#,
    )
    .unwrap();
    write_worktree_prd_for_task(&prd, "B", &worker.path).unwrap();
    let baseline = std::fs::read(&slot).unwrap();
    let mut doc: serde_json::Value = serde_json::from_slice(&baseline).unwrap();
    doc["tasks"][0]["passes"] = true.into();
    doc["tasks"][0]["title"] = "changed by user".into();
    let edits = doc.to_string();
    std::fs::write(&slot, &edits).unwrap();
    assert!(scoped_prd::prepare(&worker.path, "B").is_err());
    assert_eq!(std::fs::read_to_string(&slot).unwrap(), edits);
    doc["tasks"][0]["title"] = "Keep".into();
    let ambiguous = doc.to_string().replace(
        r#""title":"Keep""#,
        r#""title":"operator edit","title":"Keep""#,
    );
    std::fs::write(&slot, &ambiguous).unwrap();
    assert!(scoped_prd::prepare(&worker.path, "B").is_err());
    assert_eq!(std::fs::read_to_string(&slot).unwrap(), ambiguous);
    std::fs::write(&slot, doc.to_string()).unwrap();
    assert_eq!(
        scoped_prd::prepare(&worker.path, "B")
            .unwrap()
            .unwrap()
            .cause,
        scoped_prd::Cause::LegacyPerformerMarker
    );
    assert_eq!(std::fs::read(slot).unwrap(), baseline);
}

#[tokio::test]
async fn scoped_prd_conflict_waits_for_an_independent_phase_without_claiming() {
    let (temp, _, _) = fixture("INCOMING");
    let root = temp.path();
    let prd = root.join("prd.json");
    let occupied = root.join(crate::coordinator::scoped_prd::NAME);
    std::fs::write(&occupied, "operator file tracked on the base").unwrap();
    git(root, &["add", "-f", "worktree.prd.json"]);
    git(root, &["commit", "-qm", "operator file"]);
    let mut canonical = crate::load_canonical_config(root.join(".macc/macc.yaml")).unwrap();
    canonical.tools.enabled = vec!["fake".into()];
    let coordinator = canonical.automation.coordinator.as_ref();
    let cfg = CoordinatorConfigResolved::resolve(coordinator);
    let env = CoordinatorEnvConfig::default();
    let mut state = CoordinatorRunState::new();
    state.effective_max_parallel = 2;
    let args = BTreeMap::new();
    let mut value =
        crate::coordinator::state::coordinator_state_registry_load(root, &args).unwrap();
    value["tasks"][0]["state"] = "todo".into();
    value["tasks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"INDEPENDENT","state":"testing"}));
    for independent_state in ["testing", "merged"] {
        value["tasks"][1]["state"] = independent_state.into();
        crate::coordinator::state::coordinator_state_registry_save(root, &args, &value).unwrap();
        let count = run_dispatch_pipeline(
            DispatchPipelineContext {
                repo_root: root,
                canonical: &canonical,
                coordinator,
                env_cfg: &env,
                prd_file: &prd,
                state: &mut state,
                logger: None,
                cfg: &cfg,
            },
            1,
        )
        .await
        .unwrap();
        assert_eq!(count, 0);
        value = crate::coordinator::state::coordinator_state_registry_load(root, &args).unwrap();
        let task = &value["tasks"][0];
        assert!(task["task_runtime"]["claim_id"].is_null());
        if independent_state == "testing" {
            assert_eq!(task["state"], "todo");
        } else {
            assert_eq!(task["state"], "blocked");
            let runtime = &task["task_runtime"];
            assert_eq!(runtime["last_error_code"], "E901");
            assert_eq!(runtime["scoped_prd_incident"]["cause"], "unverified_file");
        }
    }
    assert_eq!(
        std::fs::read_to_string(occupied).unwrap(),
        "operator file tracked on the base"
    );
}

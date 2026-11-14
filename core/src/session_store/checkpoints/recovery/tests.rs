use super::*;
use crate::coordinator_storage::{
    CoordinatorSnapshot, CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage,
};

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

pub(crate) fn setup() -> (tempfile::TempDir, String, ContextRequest) {
    let tmp = super::super::tests::fixture();
    let root = tmp.path();
    let prd = root.join("prd.json");
    std::fs::copy(root.join("worktree.prd.json"), &prd).unwrap();
    crate::save_canonical_config(
        &crate::ProjectPaths::from_root(root),
        &crate::config::CanonicalConfig::default(),
    )
    .unwrap();
    for n in ["worker-01", "worker-02"] {
        let wt = root.join(".macc/worktree").join(n);
        std::fs::create_dir_all(wt.parent().unwrap()).unwrap();
        git(
            root,
            &["worktree", "add", "-b", n, wt.to_str().unwrap(), "main"],
        );
        crate::coordinator::helpers::write_worktree_prd_for_task(&prd, "A", &wt).unwrap();
    }
    let old = root.join(".macc/worktree/worker-02");
    let current = root.join(".macc/worktree/worker-01");
    let request = request_for(root, &old);
    let prepared = prepare(root, &request).unwrap();
    let mut task = crate::coordinator::model::Task {
        id: "A".into(),
        state: "blocked".into(),
        ..Default::default()
    };
    task.ensure_worktree().worktree_path = Some(current.display().to_string());
    task.ensure_worktree().branch = Some("worker-01".into());
    task.task_runtime
        .set_last_error_details("E903", "checkpoint", CODE);
    let storage = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(
        &crate::ProjectPaths::from_root(root),
    ));
    let mut snapshot = CoordinatorSnapshot::empty();
    snapshot.registry.tasks.push(task);
    storage.save_snapshot(&snapshot).unwrap();
    (tmp, prepared.checkpoint_id.unwrap(), request)
}

fn request_for(root: &Path, worker: &Path) -> ContextRequest {
    let spec = json!({"id":"future-tool","performer":{"command":"fake","session":{"enabled":true,"scope":"task_role_tool","create":{"command":"fake","args":["new","{session_id}"]}}}});
    ContextRequest {
        identity: crate::coordinator::session_identity::identity(
            root,
            worker,
            "A",
            "performer",
            "future-tool",
            &spec,
            &crate::coordinator::session_identity::SessionRouting {
                tier: "".into(),
                mode: "auto".into(),
            },
        )
        .unwrap(),
        phase: "dev".into(),
    }
}

#[test]
fn reconciliation_uses_the_actual_worker_and_changed_authoritative_contract() {
    let (tmp, id, _) = setup();
    let root = tmp.path();
    let worker = root.join(".macc/worktree/worker-01");
    let old = root.join(".macc/worktree/worker-02/worktree.prd.json");
    let retained = std::fs::read(&old).unwrap();
    let prd = root.join("prd.json");
    let text = std::fs::read_to_string(&prd)
        .unwrap()
        .replace("Validate", "New reviewed scope");
    std::fs::write(&prd, text).unwrap();
    let local_before = std::fs::read(worker.join("worktree.prd.json")).unwrap();
    let preview = reconcile(root, &id, None).unwrap();
    assert_eq!(preview["diagnostic"]["execution_worktree"], json!(worker));
    assert_eq!(preview["diagnostic"]["requires_contract_review"], true);
    assert_eq!(
        std::fs::read(worker.join("worktree.prd.json")).unwrap(),
        local_before
    );
    assert!(ensure_task_ready(root, "A")
        .unwrap_err()
        .to_string()
        .contains(CODE));
    let applied = reconcile(root, &id, preview["current_sha256"].as_str()).unwrap();
    let resumed = prepare(root, &request_for(root, &worker)).unwrap();
    assert!(resumed.prompt.contains("New reviewed scope"));
    assert_eq!(std::fs::read(old).unwrap(), retained);
    let lease = super::super::tests::claim(root, &request_for(root, &worker), &resumed);
    release(root, &lease.key, true).unwrap();
    assert!(reconcile(root, &id, preview["current_sha256"].as_str()).is_err());
    assert!(applied["checkpoint"].is_string());
    let (db, _) = db::active(root).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM session_events WHERE event_type='checkpoint_reconciled'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(count > 0);
}

#[test]
fn reviewed_plan_is_invalidated_by_prd_worker_assignment_or_file_changes() {
    let (tmp, id, _) = setup();
    let root = tmp.path();
    let preview = reconcile(root, &id, None).unwrap();
    let worker = root.join(".macc/worktree/worker-01");
    std::fs::write(worker.join("unfinished.txt"), "new work").unwrap();
    assert!(reconcile(root, &id, preview["current_sha256"].as_str()).is_err());
    let preview = reconcile(root, &id, None).unwrap();
    std::fs::write(
        root.join("prd.json"),
        std::fs::read_to_string(root.join("prd.json"))
            .unwrap()
            .replace("Validate", "Changed again"),
    )
    .unwrap();
    assert!(reconcile(root, &id, preview["current_sha256"].as_str()).is_err());
    assert_eq!(
        std::fs::read_to_string(worker.join("unfinished.txt")).unwrap(),
        "new work"
    );
}

#[test]
fn automatic_rebinding_preserves_requirements_but_never_discards_old_work() {
    let (tmp, id, _) = setup();
    let root = tmp.path();
    let old = root.join(".macc/worktree/worker-02");
    std::fs::write(old.join("unfinished.txt"), "retain me").unwrap();
    assert!(reconcile(root, &id, None)
        .unwrap_err()
        .to_string()
        .contains("not integrated"));
    std::fs::remove_file(old.join("unfinished.txt")).unwrap();
    ensure_task_ready(root, "A").unwrap();
    let worker = root.join(".macc/worktree/worker-01");
    assert!(prepare(root, &request_for(root, &worker)).is_ok());
}

#[test]
fn active_lease_prevents_reconciliation() {
    let (tmp, id, _) = setup();
    let root = tmp.path();
    let worker = root.join(".macc/worktree/worker-01");
    ensure_task_ready(root, "A").unwrap();
    let req = request_for(root, &worker);
    let p = prepare(root, &req).unwrap();
    let lease = super::super::tests::claim(root, &req, &p);
    assert!(reconcile(root, p.checkpoint_id.as_deref().unwrap(), None).is_err());
    release(root, &lease.key, true).unwrap();
    assert!(reconcile(root, &id, None).is_err());
}

#[test]
fn unblock_rejects_unreviewed_contract_without_clearing_the_task() {
    let (tmp, _, _) = setup();
    let root = tmp.path();
    let prd = root.join("prd.json");
    std::fs::write(
        &prd,
        std::fs::read_to_string(&prd)
            .unwrap()
            .replace("Validate", "Unreviewed contract"),
    )
    .unwrap();
    let paths = crate::ProjectPaths::from_root(root);
    let storage = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let before = storage.load_registry().unwrap();
    let request =
        crate::coordinator::control::Request::new(&before.tasks[0], "attempted recovery").unwrap();
    let error = crate::service::coordinator_workflow::unblocking::apply(
        &paths,
        None,
        &crate::coordinator::types::CoordinatorEnvConfig::default(),
        &request,
    )
    .unwrap_err();
    assert!(error.to_string().contains(CODE));
    assert_eq!(storage.load_registry().unwrap(), before);
}

#[test]
fn a_preview_is_bound_to_the_registry_assignment() {
    let (tmp, id, _) = setup();
    let root = tmp.path();
    let plan = reconcile(root, &id, None).unwrap();
    let storage = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(
        &crate::ProjectPaths::from_root(root),
    ));
    let mut snapshot = storage.load_snapshot().unwrap();
    snapshot.registry.tasks[0].ensure_worktree().worktree_path =
        Some(root.join(".macc/worktree/worker-02").display().to_string());
    snapshot.registry.tasks[0].ensure_worktree().branch = Some("worker-02".into());
    storage.save_snapshot(&snapshot).unwrap();
    assert!(reconcile(root, &id, plan["current_sha256"].as_str())
        .unwrap_err()
        .to_string()
        .contains("source changed"));
}

#[test]
fn supervisor_revision_is_stable_and_changes_after_reviewed_reconciliation() {
    let (tmp, id, _) = setup();
    let root = tmp.path();
    let ids = vec!["A".into()];
    let before = revision(root, &ids).unwrap();
    assert_eq!(before, revision(root, &ids).unwrap());
    let preview = reconcile(root, &id, None).unwrap();
    reconcile(root, &id, preview["current_sha256"].as_str()).unwrap();
    assert_ne!(before, revision(root, &ids).unwrap());
}

#[test]
fn physical_relocation_guard_refuses_a_live_session_on_either_checkout() {
    let (tmp, id, request) = setup();
    let root = tmp.path();
    let old = Path::new(request.identity["execution_root"].as_str().unwrap());
    let current = root.join(".macc/worktree/worker-01");
    let grant = crate::session_store::acquire(
        root,
        &crate::session_store::AcquireRequest {
            identity: request.identity.clone(),
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: true,
            checkpoint_id: Some(id),
        },
    )
    .unwrap();
    assert!(idle_worktrees(root, &[&current, old]).is_err());
    crate::session_store::release(root, &grant.key, false).unwrap();
    assert!(idle_worktrees(root, &[&current, old]).unwrap().is_some());
}

#[test]
fn public_unblock_relocates_a_reserved_worker_before_checkpoint_branch_validation() {
    let (tmp, _, _) = setup();
    let root = tmp.path();
    let paths = crate::ProjectPaths::from_root(root);
    let from = root.join(".macc/worktree/worker-01");
    let reserved = root.join(".macc/worktree/integration");
    let branch = "ai/future-tool/legacy";
    git(&from, &["branch", "-m", branch]);
    git(
        root,
        &[
            "worktree",
            "move",
            from.to_str().unwrap(),
            reserved.to_str().unwrap(),
        ],
    );
    std::fs::create_dir_all(reserved.join(".macc")).unwrap();
    std::fs::write(reserved.join(".macc/worktree.json"), json!({
        "id":"integration","slug":"integration","tool":"future-tool","base":"main","branch":branch
    }).to_string()).unwrap();
    git(&reserved, &["checkout", "--detach"]);
    let storage = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths));
    let mut snapshot = storage.load_snapshot().unwrap();
    let task = &mut snapshot.registry.tasks[0];
    task.tool = Some("future-tool".into());
    task.ensure_worktree().worktree_path = Some(reserved.to_string_lossy().into());
    task.ensure_worktree().branch = Some(branch.into());
    task.task_runtime.current_phase = Some("review".into());
    task.task_runtime.set_last_error_details(
        "E901",
        "worktree_ownership",
        "Legacy reserved collision",
    );
    let request = crate::coordinator::control::Request::new(
        task,
        "Restore retained ownership and revalidate",
    )
    .unwrap();
    storage.save_snapshot(&snapshot).unwrap();
    let config = crate::config::CoordinatorConfig {
        reference_branch: Some("main".into()),
        ..Default::default()
    };
    crate::service::coordinator_workflow::unblocking::apply(
        &paths,
        Some(&config),
        &crate::coordinator::types::CoordinatorEnvConfig::default(),
        &request,
    )
    .unwrap();
    let after = storage.load_registry().unwrap();
    let task = &after.tasks[0];
    assert_ne!(task.worktree_path(), Some(reserved.to_str().unwrap()));
    assert_eq!(task.branch(), Some(branch));
    assert_eq!(task.state, "testing");
    assert!(task
        .task_runtime
        .extra
        .contains_key("review_refresh_required"));
    ensure_task_ready(root, "A").unwrap();
}

#[test]
fn supervisor_preserves_missing_evidence_across_a_published_prerequisite_repair() {
    let (tmp, id, _) = setup();
    let root = tmp.path();
    let old = root.join(".macc/worktree/worker-02");
    let target = root.join(".macc/worktree/worker-01");
    // The destination includes the supervisor's source repair, so whole-tree
    // hashes differ even after the retained evidence has been copied correctly.
    std::fs::write(target.join("prerequisite.rs"), "pub fn repaired() {}\n").unwrap();
    git(&target, &["add", "prerequisite.rs"]);
    git(&target, &["commit", "-m", "supervisor prerequisite"]);
    std::fs::create_dir_all(old.join("evidence")).unwrap();
    std::fs::write(old.join("evidence/result.json"), "{\"verified\":false}").unwrap();
    assert!(reconcile(root, &id, None).is_err());
    let diagnostics = diagnostics(root).unwrap();
    assert!(diagnostics
        .iter()
        .any(|d| d["task_id"] == "A" && d["error"].is_string()));
    let receipt = preserve_task_work(root, "A").unwrap();
    assert_eq!(receipt["copied"].as_array().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(old.join("evidence/result.json")).unwrap(),
        std::fs::read(target.join("evidence/result.json")).unwrap()
    );
    assert!(
        git_state::text(&target, &["ls-files", "--", "evidence/result.json"])
            .unwrap()
            .is_empty()
    );
    assert_eq!(preserve_task_work(root, "A").unwrap()["copied"], json!([]));
    ensure_task_ready(root, "A").unwrap();
    assert!(prepare(root, &request_for(root, &target)).is_ok());
    let (db, _) = db::active(root).unwrap();
    let events:i64=db.query_row("SELECT count(*) FROM session_events WHERE event_type='checkpoint_retained_file_copied'",[],|r|r.get(0)).unwrap();
    assert_eq!(events, 1);
}

#[test]
fn retained_transfer_preflights_conflicts_and_never_overwrites_or_copies_partially() {
    let (tmp, _, _) = setup();
    let root = tmp.path();
    let old = root.join(".macc/worktree/worker-02");
    let target = root.join(".macc/worktree/worker-01");
    std::fs::write(old.join("a-missing.txt"), "retain").unwrap();
    std::fs::write(old.join("z-conflict.txt"), "old").unwrap();
    std::fs::write(target.join("z-conflict.txt"), "new").unwrap();
    assert!(preserve_task_work(root, "A")
        .unwrap_err()
        .to_string()
        .contains("conflicts"));
    assert!(!target.join("a-missing.txt").exists());
    assert_eq!(
        std::fs::read_to_string(target.join("z-conflict.txt")).unwrap(),
        "new"
    );
    assert_eq!(
        std::fs::read_to_string(old.join("a-missing.txt")).unwrap(),
        "retain"
    );
}

#[test]
fn retained_transfer_refuses_changed_requirements_and_active_source_leases() {
    let (tmp, id, request) = setup();
    let root = tmp.path();
    let old = root.join(".macc/worktree/worker-02");
    let target = root.join(".macc/worktree/worker-01");
    let grant = crate::session_store::acquire(
        root,
        &crate::session_store::AcquireRequest {
            identity: request.identity.clone(),
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: true,
            checkpoint_id: Some(id),
        },
    )
    .unwrap();
    std::fs::write(old.join("retained.txt"), "retain").unwrap();
    assert!(preserve_task_work(root, "A").is_err());
    assert!(!target.join("retained.txt").exists());
    crate::session_store::release(root, &grant.key, false).unwrap();
    let prd = root.join("prd.json");
    let before = std::fs::read_to_string(&prd).unwrap();
    std::fs::write(&prd, before.replace("Validate", "New requirements")).unwrap();
    assert!(preserve_task_work(root, "A")
        .unwrap_err()
        .to_string()
        .contains("changed task requirements"));
    assert!(!target.join("retained.txt").exists());
}

#[test]
fn retained_transfer_refuses_tracked_edits_and_symlinks() {
    let (tmp, _, _) = setup();
    let root = tmp.path();
    let old = root.join(".macc/worktree/worker-02");
    let target = root.join(".macc/worktree/worker-01");
    let tracked = git_state::text(&old, &["ls-files"])
        .unwrap()
        .lines()
        .find(|p| *p != "worktree.prd.json" && !p.starts_with(".macc/"))
        .unwrap()
        .to_owned();
    let before = std::fs::read(target.join(&tracked)).unwrap();
    std::fs::write(old.join(&tracked), "unfinished edit").unwrap();
    assert!(preserve_task_work(root, "A").is_err());
    assert_eq!(std::fs::read(target.join(&tracked)).unwrap(), before);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target.join(&tracked), old.join("linked-file")).unwrap();
        assert!(preserve_task_work(root, "A").is_err());
        assert!(!target.join("linked-file").exists());
    }
}

#[test]
fn retained_copy_does_not_undo_a_tracked_deletion_in_the_destination() {
    let (tmp, _, _) = setup();
    let root = tmp.path();
    let old = root.join(".macc/worktree/worker-02");
    let target = root.join(".macc/worktree/worker-01");
    std::fs::write(old.join("later.txt"), "retained").unwrap();
    std::fs::write(target.join("later.txt"), "new implementation").unwrap();
    git(&target, &["add", "later.txt"]);
    git(&target, &["commit", "-m", "new prerequisite file"]);
    std::fs::remove_file(target.join("later.txt")).unwrap();
    assert!(preserve_task_work(root, "A").is_err());
    assert!(!target.join("later.txt").exists());
    assert_eq!(
        std::fs::read_to_string(old.join("later.txt")).unwrap(),
        "retained"
    );
}

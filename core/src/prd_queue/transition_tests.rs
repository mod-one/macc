use super::transition::*;
use super::*;
use crate::config::CoordinatorConfig;
use serde_json::json;
use std::collections::BTreeMap;

fn fixture() -> (tempfile::TempDir, CoordinatorConfig) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join(".macc/state")).unwrap();
    std::fs::write(root.join("old.json"), r#"{"tasks":[{"id":"OLD"}]}"#).unwrap();
    std::fs::write(
        root.join("new.json"),
        r#"{"tasks":[{"id":"NEW","dependencies":["OLD"]}]}"#,
    )
    .unwrap();
    let cfg = CoordinatorConfig {
        prd_files: vec!["old.json".into()],
        storage_mode: Some("json".into()),
        ..Default::default()
    };
    save(
        root,
        &Progress {
            entries: validate(root, &cfg.prd_files).unwrap(),
            next: 1,
            status: "completed".into(),
            reason: None,
        },
    )
    .unwrap();
    let delivered = json!({"tasks":[{"id":"OLD","state":"merged"}]});
    std::fs::write(
        root.join(".macc/state/prd-queue-completed-0.json"),
        delivered.to_string(),
    )
    .unwrap();
    crate::coordinator::state::coordinator_state_registry_save(
        root,
        &BTreeMap::from([("storage-mode".into(), "json".into())]),
        &delivered,
    )
    .unwrap();
    (tmp, cfg)
}
#[test]
fn replacement_archives_complete_series_and_keeps_delivered_dependencies() {
    let (tmp, mut cfg) = fixture();
    let root = tmp.path();
    cfg.prd_files = vec!["new.json".into()];
    // Users may remove old source files after completing them.
    std::fs::remove_file(root.join("old.json")).unwrap();
    assert_eq!(
        active_path(root, Some(&cfg), None).unwrap(),
        root.join("new.json")
    );
    assert_eq!(load(root).unwrap().next, 1); // preview is not an acceptance
    reconcile(root, &cfg).unwrap();
    assert_eq!(load(root).unwrap().next, 0);
    assert_eq!(load(root).unwrap().status, "ready");
    let dirs: Vec<_> = std::fs::read_dir(root.join(".macc/state/prd-queue-history"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(dirs.len(), 1);
    assert!(dirs[0].join("progress.json").exists());
    assert!(dirs[0].join("registry.json").exists());
    assert!(dirs[0].join("prd-queue-completed-0.json").exists());
    let registry = crate::coordinator::state::coordinator_state_registry_load(
        root,
        &BTreeMap::from([("storage-mode".into(), "json".into())]),
    )
    .unwrap();
    assert!(registry["external_merged_task_ids"]
        .as_array()
        .unwrap()
        .contains(&json!("OLD")));
    reconcile(root, &cfg).unwrap();
    assert_eq!(
        std::fs::read_dir(root.join(".macc/state/prd-queue-history"))
            .unwrap()
            .count(),
        1
    );
}
#[test]
fn append_keeps_cursor_both_completed_and_incomplete() {
    for (status, next) in [("completed", 1), ("blocked", 0)] {
        let (tmp, mut cfg) = fixture();
        let root = tmp.path();
        let mut p = load(root).unwrap();
        p.status = status.into();
        p.next = next;
        save(root, &p).unwrap();
        cfg.prd_files.push("new.json".into());
        reconcile(root, &cfg).unwrap();
        let p = load(root).unwrap();
        assert_eq!(p.next, next);
        assert_eq!(p.entries.len(), 2);
        assert_eq!(
            p.status,
            if status == "completed" {
                "ready"
            } else {
                status
            }
        );
    }
}
#[test]
fn incomplete_replacement_rejected_without_mutating_progress() {
    let (tmp, mut cfg) = fixture();
    let root = tmp.path();
    let mut p = load(root).unwrap();
    p.next = 0;
    p.status = "blocked".into();
    save(root, &p).unwrap();
    let before = read(&state_path(root)).unwrap();
    cfg.prd_files = vec!["new.json".into()];
    assert!(reconcile(root, &cfg)
        .unwrap_err()
        .to_string()
        .contains(CODE));
    assert_eq!(read(&state_path(root)).unwrap(), before);
    assert!(!root.join(".macc/state/prd-queue-history").exists());
}
#[test]
fn missing_or_negative_delivery_evidence_prevents_automatic_replacement() {
    for evidence in [
        None,
        Some(
            json!({"tasks":[{"id":"OLD","state":"merged","gate":{},"task_runtime":{"gate_verdict":"rejected"}}]}),
        ),
    ] {
        let (tmp, mut cfg) = fixture();
        let root = tmp.path();
        let file = root.join(".macc/state/prd-queue-completed-0.json");
        if let Some(v) = evidence {
            std::fs::write(&file, v.to_string()).unwrap()
        } else {
            std::fs::remove_file(file).unwrap()
        }
        cfg.prd_files = vec!["new.json".into()];
        assert!(reconcile(root, &cfg).is_err());
        assert_eq!(load(root).unwrap().next, 1);
    }
}
#[test]
fn changed_contract_or_reused_task_id_never_inherits_prior_delivery() {
    let (tmp, cfg) = fixture();
    let root = tmp.path();
    std::fs::write(
        root.join("old.json"),
        r#"{"tasks":[{"id":"OLD","description":"new contract"}]}"#,
    )
    .unwrap();
    let error = reconcile(root, &cfg).unwrap_err().to_string();
    assert!(error.contains("reuses recorded task OLD"));
    assert_eq!(load(root).unwrap().next, 1);
}
#[test]
fn live_coordinator_blocks_a_queue_transition() {
    let (tmp, mut cfg) = fixture();
    let root = tmp.path();
    cfg.prd_files = vec!["new.json".into()];
    // Parent shell remains alive during this unit test.
    #[cfg(unix)]
    {
        std::fs::write(
            root.join(".macc/state/coordinator.pid"),
            unsafe { libc::getppid() }.to_string(),
        )
        .unwrap();
        assert!(reconcile(root, &cfg)
            .unwrap_err()
            .to_string()
            .contains("still running"));
    }
}
#[test]
fn explicit_reset_archives_and_unchanged_completed_queue_does_not_restart() {
    let (tmp, cfg) = fixture();
    let root = tmp.path();
    assert!(active_path(root, Some(&cfg), None)
        .unwrap_err()
        .to_string()
        .contains("completed"));
    reconcile(root, &cfg).unwrap();
    assert_eq!(load(root).unwrap().next, 1);
    reset_locked(root, &cfg).unwrap();
    assert_eq!(load(root).unwrap().entries.len(), 1);
    assert_eq!(load(root).unwrap().next, 0);
    assert_eq!(
        std::fs::read_dir(root.join(".macc/state/prd-queue-history"))
            .unwrap()
            .count(),
        1
    );
}
#[test]
fn preview_follows_new_series_in_doctor_and_launch_without_saving() {
    let (tmp, mut cfg) = fixture();
    cfg.prd_files = vec!["new.json".into()];
    let paths = crate::ProjectPaths::from_root(tmp.path());
    let r = readiness::inspect(&paths, Some(&cfg)).unwrap();
    assert_eq!(r.current.name, "new.json");
    assert!(!r.current.completed);
    assert_eq!(r.ready_tasks, 1);
    assert_eq!(load(tmp.path()).unwrap().next, 1);
}
#[test]
fn explicit_review_rejects_missing_confirmation_and_stale_sources() {
    let (tmp, mut cfg) = fixture();
    let root = tmp.path();
    cfg.prd_files = vec!["new.json".into()];
    let review = super::review::inspect(root, &cfg).unwrap();
    assert!(review.automatic);
    assert_eq!(review.changes.len(), 2);
    assert!(super::review::accept(root, &cfg, &review.revision, false).is_err());
    std::fs::write(root.join("new.json"), r#"{"tasks":[{"id":"OTHER"}]}"#).unwrap();
    assert!(super::review::accept(root, &cfg, &review.revision, true)
        .unwrap_err()
        .to_string()
        .contains("refresh"));
    assert_eq!(load(root).unwrap().next, 1);
}
#[test]
fn approved_gate_delivery_allows_replacement_but_unfinished_registry_does_not() {
    let (tmp, mut cfg) = fixture();
    let root = tmp.path();
    let approved =
        json!({"tasks":[{"id":"OLD","state":"approved","gate":{"kind":"human_approval"}}]});
    std::fs::write(
        root.join(".macc/state/prd-queue-completed-0.json"),
        approved.to_string(),
    )
    .unwrap();
    let args = BTreeMap::from([("storage-mode".into(), "json".into())]);
    crate::coordinator::state::coordinator_state_registry_save(root, &args, &approved).unwrap();
    cfg.prd_files = vec!["new.json".into()];
    assert!(
        super::transition::preview(root, &cfg, &validate(root, &cfg.prd_files).unwrap()).is_ok()
    );
    crate::coordinator::state::coordinator_state_registry_save(
        root,
        &args,
        &json!({"tasks":[{"id":"OLD","state":"blocked"}]}),
    )
    .unwrap();
    assert!(reconcile(root, &cfg)
        .unwrap_err()
        .to_string()
        .contains("unfinished"));
}
#[test]
fn dirty_or_unmerged_worktree_blocks_transition_without_discarding_work() {
    let (tmp, mut cfg) = fixture();
    let root = tmp.path();
    cfg.reference_branch = Some("main".into());
    let git = |args: &[&str]| {
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
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Test"]);
    git(&["config", "user.email", "test@example.invalid"]);
    std::fs::write(root.join(".gitignore"), ".macc/\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "fixture"]);
    let slot = root.join(".macc/worktree/supervisor-01");
    git(&["worktree", "add", "-b", "repair", slot.to_str().unwrap()]);
    std::fs::write(slot.join("partial.txt"), "preserve this work").unwrap();
    cfg.prd_files = vec!["new.json".into()];
    assert!(reconcile(root, &cfg)
        .unwrap_err()
        .to_string()
        .contains("unintegrated files"));
    let in_slot = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&slot)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success());
    };
    in_slot(&["add", "partial.txt"]);
    in_slot(&["commit", "-qm", "unfinished repair"]);
    assert!(reconcile(root, &cfg)
        .unwrap_err()
        .to_string()
        .contains("commits not verified"));
    assert_eq!(
        std::fs::read_to_string(slot.join("partial.txt")).unwrap(),
        "preserve this work"
    );
    assert_eq!(load(root).unwrap().next, 1);
}
#[test]
fn explicit_reset_cannot_bypass_a_delivered_task_identity_conflict() {
    let (tmp, cfg) = fixture();
    let root = tmp.path();
    std::fs::write(
        root.join("old.json"),
        r#"{"tasks":[{"id":"OLD","description":"different contract"}]}"#,
    )
    .unwrap();
    assert!(reset_locked(root, &cfg)
        .unwrap_err()
        .to_string()
        .contains("reuses recorded task"));
    assert_eq!(load(root).unwrap().next, 1);
}
#[test]
fn sqlite_transition_preserves_approved_dependency_in_the_sync_compatibility_snapshot() {
    use crate::coordinator_storage::{CoordinatorStorage, CoordinatorStoragePaths, SqliteStorage};
    let (tmp, mut cfg) = fixture();
    let root = tmp.path();
    cfg.storage_mode = Some("sqlite".into());
    let approved =
        json!({"tasks":[{"id":"OLD","state":"approved","gate":{"kind":"human_approval"}}]});
    std::fs::write(
        root.join(".macc/state/prd-queue-completed-0.json"),
        approved.to_string(),
    )
    .unwrap();
    crate::coordinator::state::coordinator_state_registry_save(root, &BTreeMap::new(), &approved)
        .unwrap();
    cfg.prd_files = vec!["new.json".into()];
    reconcile(root, &cfg).unwrap();
    let paths = crate::ProjectPaths::from_root(root);
    crate::coordinator_storage::coordinator_storage_import_json_to_sqlite(&paths).unwrap();
    let snapshot = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&paths))
        .load_snapshot()
        .unwrap();
    assert!(snapshot.registry.external_merged_task_ids.contains("OLD"));
    assert_eq!(
        snapshot.registry.find_task("OLD").unwrap().state,
        "approved"
    );
}
#[test]
fn launch_preview_keeps_unfinished_old_work_visible_after_explicit_reset() {
    let (tmp, mut cfg) = fixture();
    let root = tmp.path();
    crate::coordinator::state::coordinator_state_registry_save(
        root,
        &BTreeMap::from([("storage-mode".into(), "json".into())]),
        &json!({"tasks":[{"id":"OLD","state":"blocked"}]}),
    )
    .unwrap();
    cfg.prd_files = vec!["new.json".into()];
    reset_locked(root, &cfg).unwrap();
    let error = readiness::inspect(&crate::ProjectPaths::from_root(root), Some(&cfg))
        .unwrap_err()
        .to_string();
    assert!(error.contains("unfinished work outside new.json"));
}
#[test]
fn an_undelivered_task_can_keep_its_id_after_explicit_contract_review() {
    let (tmp, cfg) = fixture();
    let root = tmp.path();
    let mut progress = load(root).unwrap();
    progress.next = 0;
    progress.status = "blocked".into();
    save(root, &progress).unwrap();
    crate::coordinator::state::coordinator_state_registry_save(
        root,
        &BTreeMap::from([("storage-mode".into(), "json".into())]),
        &json!({"tasks":[{"id":"OLD","state":"todo"}]}),
    )
    .unwrap();
    std::fs::write(
        root.join("old.json"),
        r#"{"tasks":[{"id":"OLD","description":"Reviewed refinement before delivery"}]}"#,
    )
    .unwrap();
    assert!(reconcile(root, &cfg).is_err());
    reset_locked(root, &cfg).unwrap();
    assert_eq!(
        load(root).unwrap().entries,
        validate(root, &cfg.prd_files).unwrap()
    );
    assert_eq!(load(root).unwrap().next, 0);
}

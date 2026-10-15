use super::*;
use std::process::Command;
fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let r = temp.path();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(r)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{:?}", out);
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.email", "test@example.invalid"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(r.join(".gitignore"), ".macc/\nworktree.prd.json\n").unwrap();
    std::fs::write(r.join("code.txt"), "initial").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "initial"]);
    std::fs::write(r.join("worktree.prd.json"),json!({"global_constraints":["Keep API"],"tasks":[{"id":"A","acceptance_criteria":[{"id":"AC1","text":"Validate"}]},{"id":"B","acceptance_criteria":["Isolated"]}]}).to_string()).unwrap();
    std::fs::create_dir_all(r.join(".macc/state")).unwrap();
    let plan = migrate(r, false, None).unwrap();
    migrate(r, true, Some(&plan.source_sha256)).unwrap();
    temp
}
fn context(root: &Path, task: &str, role: &str, tool: &str) -> ContextRequest {
    let spec = json!({"id":tool,"performer":{"command":"fake","session":{"enabled":true,"scope":"task_role_tool","create":{"command":"fake","args":["new","{session_id}"]}}}});
    ContextRequest {
        identity: crate::coordinator::session_identity::identity(
            root,
            root,
            task,
            role,
            tool,
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
fn claim(root: &Path, c: &ContextRequest, p: &PreparedContext) -> LeaseGrant {
    acquire(
        root,
        &AcquireRequest {
            identity: c.identity.clone(),
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: true,
            checkpoint_id: p.checkpoint_id.clone(),
        },
    )
    .unwrap()
}
#[test]
fn checkpoint_is_scoped_checksummed_and_never_claims_completion() {
    let t = fixture();
    let r = t.path();
    let c = context(r, "A", "performer", "fake-a");
    let p = prepare(r, &c).unwrap();
    let saved = get(r, p.checkpoint_id.as_ref().unwrap()).unwrap();
    assert_eq!(saved.acceptance_criteria_ids, ["AC1"]);
    assert!(saved.completed_steps.is_empty());
    assert!(p.prompt.contains("Keep API"));
    let lease = claim(r, &c, &p);
    assert!(prepare(r, &c).is_err());
    std::fs::write(r.join("new.txt"), "work before quota").unwrap();
    finish(
        r,
        &FinishRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
            result: "quota".into(),
            acknowledged: true,
        },
    )
    .unwrap();
    release(r, &lease.key, false).unwrap();
    let next = prepare(r, &context(r, "A", "performer", "fake-b")).unwrap();
    assert!(next.prompt.contains("quota"));
    assert!(next.prompt.contains("new.txt"));
    let resumed = claim(r, &context(r, "A", "performer", "fake-b"), &next);
    assert!(resumed.is_new);
    assert_eq!(
        std::fs::read_to_string(r.join("new.txt")).unwrap(),
        "work before quota"
    );
    let (db, _) = db::active(r).unwrap();
    db.execute(
        "UPDATE task_checkpoints SET content_json='{}' WHERE id=?1",
        [p.checkpoint_id.unwrap()],
    )
    .unwrap();
    assert!(get(r, &saved.id).is_err());
}
#[test]
fn source_drift_requires_reviewed_reconciliation_and_rejects_stale_plan() {
    let t = fixture();
    let r = t.path();
    let c = context(r, "A", "performer", "fake-a");
    let p = prepare(r, &c).unwrap();
    let id = p.checkpoint_id.unwrap();
    let path = r.join("worktree.prd.json");
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Validate", "Validate precisely");
    std::fs::write(&path, text).unwrap();
    assert!(prepare(r, &c).is_err());
    let plan = reconcile(r, &id, None).unwrap();
    std::fs::write(r.join("code.txt"), "changed").unwrap();
    assert!(reconcile(r, &id, plan["current_sha256"].as_str()).is_err());
    let plan = reconcile(r, &id, None).unwrap();
    reconcile(r, &id, plan["current_sha256"].as_str()).unwrap();
    assert!(prepare(r, &c).is_ok());
}
#[test]
fn rotation_budget_and_tool_switch_back_open_fresh_transcripts() {
    let t = fixture();
    let r = t.path();
    let view = inspect(r).unwrap();
    let mut policy = view.policy;
    policy.checkpoints.rotate_after_uses = 2;
    update_policy(r, &view.revision, &policy).unwrap();
    let mut ids = vec![];
    for tool in ["fake-a", "fake-a", "fake-a", "fake-b", "fake-a"] {
        let c = context(r, "A", "performer", tool);
        let p = prepare(r, &c).unwrap();
        let lease = claim(r, &c, &p);
        ids.push(lease.key.session_id.clone());
        finish(
            r,
            &FinishRequest {
                key: lease.key.clone(),
                phase: "dev".into(),
                result: "success".into(),
                acknowledged: true,
            },
        )
        .unwrap();
        release(r, &lease.key, true).unwrap();
    }
    assert_eq!(ids[0], ids[1]);
    assert_ne!(ids[1], ids[2]);
    assert_ne!(ids[2], ids[4]);
}
#[test]
fn staged_unstaged_and_untracked_evidence_survives_restart_and_reconciliation() {
    let t = fixture();
    let r = t.path();
    let c = context(r, "A", "performer", "fake-a");
    let p = prepare(r, &c).unwrap();
    std::fs::write(r.join("code.txt"), "staged").unwrap();
    Command::new("git")
        .arg("-C")
        .arg(r)
        .args(["add", "code.txt"])
        .output()
        .unwrap();
    std::fs::write(r.join("code.txt"), "unstaged").unwrap();
    std::fs::write(r.join("other.txt"), "untracked").unwrap();
    let after = prepare(r, &c).unwrap();
    assert_ne!(p.checkpoint_id, after.checkpoint_id);
    assert_eq!(
        Command::new("git")
            .arg("-C")
            .arg(r)
            .args(["show", ":code.txt"])
            .output()
            .unwrap()
            .stdout,
        b"staged"
    );
    assert_eq!(std::fs::read(r.join("code.txt")).unwrap(), b"unstaged");
    assert!(r.join("other.txt").exists());
}
#[test]
fn oversized_mandatory_context_is_rejected_without_truncation() {
    let t = fixture();
    let r = t.path();
    let path = r.join("worktree.prd.json");
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Keep API", &"x".repeat(70_000));
    std::fs::write(path, text).unwrap();
    assert!(prepare(r, &context(r, "A", "performer", "fake-a")).is_err());
}
#[test]
fn existing_sqlite_requires_reviewed_feature_upgrade_and_backup() {
    let t = fixture();
    let r = t.path();
    let (db, _) = db::active(r).unwrap();
    db.execute_batch("DROP TABLE task_checkpoints;DELETE FROM session_meta WHERE key='checkpoint_schema_version'").unwrap();
    drop(db);
    assert!(!inspect(r).unwrap().checkpoints_ready);
    assert!(
        !prepare(r, &context(r, "A", "performer", "fake-a"))
            .unwrap()
            .enabled
    );
    let plan = upgrade(r, false, None).unwrap();
    assert!(upgrade(r, true, Some("stale")).is_err());
    let report = upgrade(r, true, Some(&plan.source_sha256)).unwrap();
    let old = rusqlite::Connection::open(report.backup.unwrap()).unwrap();
    let count: i64 = old
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='task_checkpoints'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    assert!(inspect(r).unwrap().checkpoints_ready);
    assert!(
        prepare(r, &context(r, "A", "performer", "fake-a"))
            .unwrap()
            .enabled
    );
}
#[test]
fn coordinator_commit_and_phase_change_keep_same_performer_session() {
    let t = fixture();
    let r = t.path();
    let mut c = context(r, "A", "performer", "fake-a");
    let p = prepare(r, &c).unwrap();
    let first = claim(r, &c, &p);
    std::fs::write(r.join("code.txt"), "implementation").unwrap();
    finish(
        r,
        &FinishRequest {
            key: first.key.clone(),
            phase: "dev".into(),
            result: "success".into(),
            acknowledged: true,
        },
    )
    .unwrap();
    release(r, &first.key, true).unwrap();
    for args in [
        ["add", "code.txt"].as_slice(),
        ["commit", "-m", "[macc:task A] Work"].as_slice(),
    ] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(r)
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
    c.phase = "fix".into();
    let p = prepare(r, &c).unwrap();
    let next = claim(r, &c, &p);
    assert_eq!(next.key.session_id, first.key.session_id);
    assert!(!next.is_new);
}
#[test]
fn source_redaction_and_task_role_isolation_apply_on_restore() {
    let t = fixture();
    let r = t.path();
    let path = r.join("worktree.prd.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Keep API", "Keep API sk-abcdefghijklmnopqrstuvwxyz");
    std::fs::write(path, raw).unwrap();
    let p = prepare(r, &context(r, "A", "performer", "fake-a")).unwrap();
    assert!(!p.prompt.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    assert!(p.prompt.contains("REDACTED"));
    let review = prepare(r, &context(r, "A", "reviewer", "fake-a")).unwrap();
    assert!(!review.prompt.contains("role\\\":\\\"performer"));
    let other = prepare(r, &context(r, "B", "performer", "fake-a")).unwrap();
    assert!(!other.prompt.contains("AC1"));
}
#[test]
fn provider_cannot_silently_replace_mandatory_sources_during_invocation() {
    let t = fixture();
    let r = t.path();
    let c = context(r, "A", "performer", "fake-a");
    let p = prepare(r, &c).unwrap();
    let lease = claim(r, &c, &p);
    let path = r.join("worktree.prd.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Validate", "Skip validation");
    std::fs::write(path, raw).unwrap();
    assert!(finish(
        r,
        &FinishRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
            result: "success".into(),
            acknowledged: true
        }
    )
    .is_err());
    release(r, &lease.key, false).unwrap();
    assert!(prepare(r, &c).is_err());
    assert_eq!(
        inspect(r).unwrap().checkpoints[0].validation_state,
        "needs_reconciliation"
    );
}
#[test]
fn checkpoint_limit_changes_apply_to_the_next_invocation() {
    let t = fixture();
    let r = t.path();
    let path = r.join("worktree.prd.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap()
        .replace("Keep API", &"x".repeat(5000));
    std::fs::write(path, raw).unwrap();
    let c = context(r, "A", "performer", "fake-a");
    let p = prepare(r, &c).unwrap();
    let lease = claim(r, &c, &p);
    let view = inspect(r).unwrap();
    let mut policy = view.policy;
    policy.checkpoints.max_content_bytes = 2048;
    update_policy(r, &view.revision, &policy).unwrap();
    finish(
        r,
        &FinishRequest {
            key: lease.key.clone(),
            phase: "dev".into(),
            result: "success".into(),
            acknowledged: true,
        },
    )
    .unwrap();
    release(r, &lease.key, true).unwrap();
    assert!(prepare(r, &c).is_err());
}
#[test]
fn native_phases_preserve_the_last_explicit_precondition_marker() {
    let reason=precondition_reason("MACC_TASK_RESULT: success_with_changes\nMACC_TASK_PRECONDITION: sources changed\nMACC_TASK_RESULT: precondition_unmet\n").unwrap();
    assert_eq!(reason, "E903: sources changed");
    assert!(precondition_reason(
        "MACC_TASK_RESULT: precondition_unmet\nMACC_TASK_RESULT: success_with_changes\n"
    )
    .is_none());
}

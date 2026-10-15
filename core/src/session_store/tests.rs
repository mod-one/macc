use super::*;
use serde_json::json;
use std::path::Path;
fn root() -> tempfile::TempDir {
    let r = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(r.path().join(".macc/state")).unwrap();
    r
}
fn request(root: &Path, task: &str, role: &str, tool: &str) -> AcquireRequest {
    let spec = json!({"id":tool,"performer":{"command":"future-command","session":{"enabled":true,"scope":"task_role_tool","create":{"command":"future-command","args":["create","{session_id}"]}}}});
    let identity = crate::coordinator::session_identity::identity(
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
    .unwrap();
    AcquireRequest {
        identity,
        request_id: uuid::Uuid::new_v4().to_string(),
        owner_pid: std::process::id() as i64,
        preferred_provider_id: None,
        generate_provider_id: true,
    }
}
fn activate(root: &Path) {
    let plan = migrate(root, false, None).unwrap();
    let applied = migrate(root, true, Some(&plan.source_sha256)).unwrap();
    assert!(applied.applied);
    assert!(Path::new(applied.backup.as_ref().unwrap()).exists());
}
#[test]
fn claim_resume_and_fencing_are_durable_and_idempotent() {
    let r = root();
    activate(r.path());
    let first_request = request(r.path(), "A", "performer", "future-tool");
    let first = acquire(r.path(), &first_request).unwrap();
    let replay = acquire(r.path(), &first_request).unwrap();
    assert_eq!(first.key.lease_token, replay.key.lease_token);
    assert_eq!(first.key.generation, 1);
    heartbeat(r.path(), &first.key).unwrap();
    release(r.path(), &first.key, true).unwrap();
    release(r.path(), &first.key, true).unwrap();
    assert!(acquire(r.path(), &first_request).is_err());
    let mut next = first_request.clone();
    next.request_id = uuid::Uuid::new_v4().to_string();
    let second = acquire(r.path(), &next).unwrap();
    assert!(!second.is_new);
    assert_eq!(first.key.session_id, second.key.session_id);
    assert_eq!(second.key.generation, 2);
    assert_ne!(first.key.lease_token, second.key.lease_token);
    assert!(heartbeat(r.path(), &first.key).is_err());
    assert!(bind(r.path(), &first.key, "foreign").is_err());
    // A replayed release cannot clear a later lease.
    release(r.path(), &first.key, true).unwrap();
    heartbeat(r.path(), &second.key).unwrap();
    assert_eq!(inspect(r.path()).unwrap().sessions[0].use_count, 2);
}
#[test]
fn task_role_tool_and_configuration_isolation() {
    let r = root();
    activate(r.path());
    let original = request(r.path(), "A", "performer", "future-tool");
    let lease = acquire(r.path(), &original).unwrap();
    release(r.path(), &lease.key, true).unwrap();
    for req in [
        request(r.path(), "B", "performer", "future-tool"),
        request(r.path(), "A", "reviewer", "future-tool"),
        request(r.path(), "A", "performer", "another-tool"),
    ] {
        let new = acquire(r.path(), &req).unwrap();
        assert!(new.is_new);
        assert_ne!(lease.provider_session_id, new.provider_session_id);
        release(r.path(), &new.key, true).unwrap();
    }
    let mut changed = original.clone();
    changed.request_id = uuid::Uuid::new_v4().to_string();
    changed.identity["execution_config"]["tier"] = json!("heavy");
    assert!(acquire(r.path(), &changed).unwrap().is_new);
}
#[test]
fn concurrent_workers_never_share_a_provider_lease() {
    let r = root();
    activate(r.path());
    let initial = acquire(
        r.path(),
        &request(r.path(), "A", "performer", "future-tool"),
    )
    .unwrap();
    release(r.path(), &initial.key, true).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let jobs = (0..8)
        .map(|_| {
            let path = r.path().to_path_buf();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let req = request(&path, "A", "performer", "future-tool");
                barrier.wait();
                acquire(&path, &req).unwrap()
            })
        })
        .collect::<Vec<_>>();
    let grants = jobs
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect::<Vec<_>>();
    let ids = grants
        .iter()
        .map(|g| g.provider_session_id.clone())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), 8);
    for grant in grants {
        release(r.path(), &grant.key, true).unwrap();
    }
}
#[test]
fn live_owner_cannot_be_evicted_even_after_expiry() {
    let r = root();
    activate(r.path());
    let req = request(r.path(), "A", "performer", "future-tool");
    let first = acquire(r.path(), &req).unwrap();
    let db = db::open(r.path(), false).unwrap();
    db.execute("UPDATE session_leases SET expires_at=0", [])
        .unwrap();
    assert_eq!(recover(r.path()).unwrap(), 0);
    let another = acquire(
        r.path(),
        &request(r.path(), "A", "performer", "future-tool"),
    )
    .unwrap();
    assert_ne!(first.provider_session_id, another.provider_session_id);
    assert!(heartbeat(r.path(), &first.key).is_err());
    assert!(close(r.path(), &first.key.session_id).is_err());
}
#[test]
fn expired_dead_owner_is_quarantined_not_resumed() {
    let r = root();
    activate(r.path());
    let first = acquire(
        r.path(),
        &request(r.path(), "A", "performer", "future-tool"),
    )
    .unwrap();
    let db = db::open(r.path(), false).unwrap();
    db.execute(
        "UPDATE session_leases SET owner_process_id=99999999,expires_at=0",
        [],
    )
    .unwrap();
    assert_eq!(recover(r.path()).unwrap(), 1);
    let view = inspect(r.path()).unwrap();
    assert_eq!(view.sessions[0].status, "abandoned");
    let next = acquire(
        r.path(),
        &request(r.path(), "A", "performer", "future-tool"),
    )
    .unwrap();
    assert_ne!(first.provider_session_id, next.provider_session_id);
}
#[test]
fn identity_root_and_process_must_be_verified() {
    let r = root();
    activate(r.path());
    let mut req = request(r.path(), "A", "performer", "future-tool");
    req.identity["project_root"] = json!("/unrelated");
    assert!(acquire(r.path(), &req).is_err());
    let mut req = request(r.path(), "A", "performer", "future-tool");
    req.owner_pid = 99999999;
    assert!(acquire(r.path(), &req).is_err());
    let mut req = request(r.path(), "A", "performer", "future-tool");
    req.identity["role"] = json!("");
    assert!(acquire(r.path(), &req).is_err());
}
#[test]
fn binding_is_unique_and_cannot_change_transcripts() {
    let r = root();
    activate(r.path());
    let mut a = request(r.path(), "A", "performer", "future-tool");
    a.generate_provider_id = false;
    let lease = acquire(r.path(), &a).unwrap();
    assert!(lease.provider_session_id.is_none());
    bind(r.path(), &lease.key, "provider-id").unwrap();
    bind(r.path(), &lease.key, "provider-id").unwrap();
    assert!(bind(r.path(), &lease.key, "other-id").is_err());
    let mut b = request(r.path(), "B", "performer", "future-tool");
    b.generate_provider_id = false;
    let second = acquire(r.path(), &b).unwrap();
    assert!(bind(r.path(), &second.key, "provider-id").is_err());
    release(r.path(), &second.key, true).unwrap();
    assert!(inspect(r.path())
        .unwrap()
        .sessions
        .iter()
        .any(|s| s.id == second.key.session_id && s.status == "abandoned"));
}
#[test]
fn migration_preserves_evidence_and_rejects_stale_preview() {
    let r = root();
    let req = request(r.path(), "A", "performer", "future-tool");
    let original = json!({"tools":{"future-tool":{"sessions":{
        "known":{"session_identity":req.identity,"status":"available","updated_at":chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()},
        "legacy":{"status":"available","last_task_id":"A"},
        "/old/worktree":{"session_id":"old-provider-id"}
    }}}});
    let raw = original.to_string();
    std::fs::write(legacy_path(r.path()), &raw).unwrap();
    let plan = migrate(r.path(), false, None).unwrap();
    assert_eq!(plan.imported, 3);
    assert_eq!(plan.quarantined, 2);
    assert!(migrate(r.path(), true, Some("stale")).is_err());
    assert!(!is_sqlite(r.path()).unwrap());
    let report = migrate(r.path(), true, Some(&plan.source_sha256)).unwrap();
    assert_eq!(
        std::fs::read_to_string(report.backup.unwrap()).unwrap(),
        raw
    );
    let frozen: serde_json::Value = json(&bytes(&legacy_path(r.path())).unwrap()).unwrap();
    assert_eq!(frozen["session_store"]["mode"], "sqlite");
    assert!(frozen["tools"].is_array());
    let grant = acquire(r.path(), &req).unwrap();
    assert!(!grant.is_new);
    assert_eq!(grant.provider_session_id.as_deref(), Some("known"));
}
#[test]
fn rollback_requires_no_leases_and_exports_safe_metadata() {
    let r = root();
    activate(r.path());
    let req = request(r.path(), "A", "performer", "future-tool");
    let lease = acquire(r.path(), &req).unwrap();
    assert!(rollback(r.path(), false, None).is_err());
    release(r.path(), &lease.key, true).unwrap();
    let plan = rollback(r.path(), false, None).unwrap();
    let report = rollback(r.path(), true, Some(&plan.source_sha256)).unwrap();
    assert!(!is_sqlite(r.path()).unwrap());
    assert!(Path::new(report.backup.as_ref().unwrap()).exists());
    let v: serde_json::Value = json(&bytes(&legacy_path(r.path())).unwrap()).unwrap();
    let entry = &v["tools"]["future-tool"]["sessions"][lease.provider_session_id.as_ref().unwrap()];
    assert_eq!(entry["session_identity"], req.identity);
    assert_eq!(entry["status"], "available");
    assert!(entry.get("owner_pid").is_none());
    activate(r.path());
    let next = acquire(
        r.path(),
        &request(r.path(), "A", "performer", "future-tool"),
    )
    .unwrap();
    assert_eq!(next.key.session_id, lease.key.session_id);
    assert!(next.key.generation > lease.key.generation);
}
#[test]
fn missing_or_corrupt_authority_never_falls_back_to_json() {
    let r = root();
    activate(r.path());
    std::fs::remove_file(marker_path(r.path())).unwrap();
    assert!(is_sqlite(r.path()).is_err());
    assert!(inspect(r.path()).is_err());
    // Explicit migration repairs a crash after DB commit and before marker publication.
    let plan = migrate(r.path(), false, None).unwrap();
    migrate(r.path(), true, Some(&plan.source_sha256)).unwrap();
    assert!(is_sqlite(r.path()).unwrap());
    std::fs::write(marker_path(r.path()), "broken").unwrap();
    assert!(inspect(r.path()).is_err());
}
#[test]
fn policy_validation_revision_conflicts_and_catalog_coverage() {
    let r = root();
    let first = inspect(r.path()).unwrap();
    let mut p = first.policy.clone();
    p.lease.ttl_seconds = 60;
    p.lease.heartbeat_seconds = 10;
    let saved = update_policy(r.path(), &first.revision, &p).unwrap();
    assert_eq!(saved.policy, p);
    assert!(update_policy(r.path(), &first.revision, &p).is_err());
    p.lease.heartbeat_seconds = 30;
    assert!(update_policy(r.path(), &saved.revision, &p).is_err());
    let value = serde_json::to_value(SessionPolicy::default()).unwrap();
    fn leaves(v: &serde_json::Value, prefix: &str, out: &mut std::collections::BTreeSet<String>) {
        if let Some(object) = v.as_object() {
            for (key, value) in object {
                let p = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                leaves(value, &p, out);
            }
        } else {
            assert!(v.is_u64(), "New field types need typed client editors");
            out.insert(prefix.to_string());
        }
    }
    let mut schema_fields = std::collections::BTreeSet::new();
    leaves(&value, "", &mut schema_fields);
    assert_eq!(
        schema_fields,
        fields().into_iter().map(|f| f.path).collect()
    );
    let saved = inspect(r.path()).unwrap();
    let reset = remove_policy_override(r.path(), &saved.revision).unwrap();
    assert_eq!(reset.policy_source, "defaults");
}
#[test]
fn consistent_backup_includes_wal_and_has_private_permissions() {
    let r = root();
    activate(r.path());
    let lease = acquire(
        r.path(),
        &request(r.path(), "A", "performer", "future-tool"),
    )
    .unwrap();
    let path = backup(r.path()).unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM logical_sessions WHERE id=?1",
            [lease.key.session_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
#[test]
fn active_legacy_owner_prevents_cutover() {
    let r = root();
    std::fs::write(legacy_path(r.path()),json!({"tools":{"future-tool":{"sessions":{"sid":{"status":"active","owner_pid":std::process::id()}}}}}).to_string()).unwrap();
    assert!(migrate(r.path(), false, None).is_err());
    assert!(!is_sqlite(r.path()).unwrap());
}
#[test]
fn stale_json_recovery_and_restore_cannot_write_after_cutover() {
    let r = root();
    activate(r.path());
    assert!(
        crate::coordinator::session_manager::save_sessions(r.path(), Some("should-not-write"))
            .is_err()
    );
    assert!(
        crate::coordinator::session_manager::restore_sessions(r.path(), "none", false).is_err()
    );
    assert_eq!(
        crate::coordinator::session_manager::reset_stale_active_sessions(r.path(), 0).unwrap(),
        0
    );
}

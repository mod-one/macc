//! End-to-end bundles use explicit temporary save roots, never the user's HOME.
use super::*;
#[path = "p3_roundtrip_tests.rs"]
mod p3_roundtrip_tests;
use crate::session_store as store;
use serde_json::json;
use std::path::Path;
fn setup() -> (tempfile::TempDir, ProjectPaths, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    fs::create_dir_all(root.join(".macc/state")).unwrap();
    let paths = ProjectPaths::from_root(&root);
    fs::write(&paths.config_path,"tools:\n  enabled: []\nsession_management:\n  usage:\n    capture_mode: 1\n    max_output_bytes: 8192\n").unwrap();
    let plan = store::migrate(&root, false, None).unwrap();
    store::migrate(&root, true, Some(&plan.source_sha256)).unwrap();
    let saves = tmp.path().join("saves");
    (tmp, paths, saves)
}
fn options() -> SaveOptions {
    SaveOptions {
        description: None,
        overwrite: false,
        only: None,
        no_sessions: false,
        include_logs: false,
        log_max_size: "10MB".into(),
        log_since: "7d".into(),
        redact_logs: true,
        dry_run: false,
        include_prd: false,
        include_state: false,
        handle_secrets: None,
    }
}
fn restore_options() -> RestoreOptions {
    RestoreOptions {
        config_only: false,
        sessions: false,
        no_sessions: false,
        include_logs: false,
        dry_run: false,
        yes: true,
    }
}
fn claim(root: &Path) -> store::LeaseGrant {
    let spec = json!({"id":"future-tool","performer":{"command":"fake","usage":{"schema":"fake-usage-v1","session_id":"/session_id","event_match":{"/type":"result"},"fields":{"input_uncached":"/input","cost":"/cost"},"currency":"USD","cost_quality":"estimated","cost_scope":"session"},"session":{"enabled":true,"scope":"task_role_tool","create":{"command":"fake","args":["new","{session_id}"]}}}});
    let identity = crate::coordinator::session_identity::identity(
        root,
        root,
        "TASK-A",
        "performer",
        "future-tool",
        &spec,
        &crate::coordinator::session_identity::SessionRouting {
            tier: "".into(),
            mode: "auto".into(),
        },
    )
    .unwrap();
    store::acquire(
        root,
        &store::AcquireRequest {
            identity,
            request_id: uuid::Uuid::new_v4().to_string(),
            owner_pid: std::process::id() as i64,
            preferred_provider_id: None,
            generate_provider_id: true,
            checkpoint_id: None,
        },
    )
    .unwrap()
}
fn record(root: &Path, lease: &store::LeaseGrant) {
    store::usage::begin(
        root,
        &store::usage::BeginRequest {
            invocation: None,
            key: lease.key.clone(),
            phase: "dev".into(),
        },
    )
    .unwrap();
    store::usage::end(root,&store::usage::EndRequest{key:lease.key.clone(),outcome:"success".into(),raw:json!({"type":"result","session_id":lease.provider_session_id,"input":123,"cost":0.25}).to_string(),omitted:false}).unwrap();
}
fn save(paths: &ProjectPaths, saves: &Path) -> SaveBundleManifest {
    bundle::create_in(paths, "full", &options(), saves).unwrap()
}
#[test]
fn sqlite_bundle_round_trip_retains_usage_checkpoints_and_policy() {
    let (_tmp, paths, saves) = setup();
    let lease = claim(&paths.root);
    record(&paths.root, &lease);
    store::release(&paths.root, &lease.key, true).unwrap();
    // Checkpoint storage is part of the whole database, independently of UI views.
    let db = rusqlite::Connection::open(paths.macc_dir.join("memory.db")).unwrap();
    db.execute("INSERT INTO task_checkpoints VALUES('cp-test',?1,'TASK-A','performer','future-tool','dev','worker-01','{}','test','verified','invocation_finished',1)",[store::authority(&paths.root).unwrap().unwrap().project_id]).unwrap();
    drop(db);
    let manifest = save(&paths, &saves);
    let snapshot = manifest.session_store.as_ref().unwrap();
    assert_eq!(manifest.version, 2);
    assert!(manifest.includes.coordinator_sessions);
    assert!(manifest.paths.coordinator_sessions.is_none());
    assert_eq!(
        snapshot.state_sha256,
        store::fingerprint::current(&paths.root).unwrap()
    );
    let db = rusqlite::Connection::open(paths.macc_dir.join("memory.db")).unwrap();
    db.execute("DELETE FROM invocation_usage", []).unwrap();
    db.execute("DELETE FROM task_checkpoints", []).unwrap();
    drop(db);
    fs::write(&paths.config_path, "tools:\n  enabled: []\n").unwrap();
    restore::restore_from(&paths, "full", &restore_options(), &saves).unwrap();
    assert!(fs::read_to_string(&paths.config_path)
        .unwrap()
        .contains("capture_mode: 1"));
    assert_eq!(store::usage::inspect(&paths.root).unwrap().records.len(), 1);
    assert_eq!(store::inspect(&paths.root).unwrap().checkpoints.len(), 1);
    assert_eq!(
        store::inspect(&paths.root).unwrap().sessions[0].status,
        "abandoned"
    );
    assert!(store::heartbeat(&paths.root, &lease.key).is_err());
    assert!(fs::read_dir(paths.macc_dir.join("state"))
        .unwrap()
        .any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("pre-restore-")));
    let db = rusqlite::Connection::open(paths.macc_dir.join("memory.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM usage_watermarks", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn wal_snapshot_can_capture_active_work_but_restore_rejects_live_leases() {
    let (_tmp, paths, saves) = setup();
    let lease = claim(&paths.root);
    record(&paths.root, &lease);
    let db = rusqlite::Connection::open(paths.macc_dir.join("memory.db")).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
        .unwrap();
    db.execute(
        "INSERT INTO session_meta VALUES('wal-only-proof','present')",
        [],
    )
    .unwrap();
    let manifest = save(&paths, &saves);
    let snapshot = rusqlite::Connection::open(
        saves
            .join("full")
            .join(&manifest.session_store.unwrap().database_path),
    )
    .unwrap();
    assert_eq!(
        snapshot
            .query_row(
                "SELECT value FROM session_meta WHERE key='wal-only-proof'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "present"
    );
    drop(snapshot);
    drop(db);
    let before = store::fingerprint::current(&paths.root).unwrap();
    fs::write(&paths.config_path, "live configuration\n").unwrap();
    assert!(
        restore::restore_from(&paths, "full", &restore_options(), &saves)
            .unwrap_err()
            .to_string()
            .contains("outstanding leases")
    );
    assert_eq!(
        fs::read_to_string(&paths.config_path).unwrap(),
        "live configuration\n"
    );
    assert_eq!(before, store::fingerprint::current(&paths.root).unwrap());
    store::release(&paths.root, &lease.key, true).unwrap();
    restore::restore_from(&paths, "full", &restore_options(), &saves).unwrap();
    assert_eq!(
        store::inspect(&paths.root).unwrap().sessions[0].status,
        "abandoned"
    );
}
#[test]
fn no_sessions_and_config_only_leave_the_database_unchanged() {
    let (_tmp, paths, saves) = setup();
    let lease = claim(&paths.root);
    record(&paths.root, &lease);
    let mut opts = options();
    opts.no_sessions = true;
    let config = bundle::create_in(&paths, "config", &opts, &saves).unwrap();
    assert_eq!(config.version, 1);
    assert!(config.session_store.is_none());
    assert!(!config.includes.coordinator_sessions);
    save(&paths, &saves);
    let before = store::fingerprint::current(&paths.root).unwrap();
    let mut opts = restore_options();
    opts.config_only = true;
    restore::restore_from(&paths, "full", &opts, &saves).unwrap();
    assert_eq!(before, store::fingerprint::current(&paths.root).unwrap());
    opts.config_only = false;
    opts.no_sessions = true;
    restore::restore_from(&paths, "full", &opts, &saves).unwrap();
    assert_eq!(before, store::fingerprint::current(&paths.root).unwrap());
    store::release(&paths.root, &lease.key, true).unwrap();
}
#[test]
fn corrupt_database_is_rejected_before_configuration_changes() {
    let (_tmp, paths, saves) = setup();
    save(&paths, &saves);
    fs::write(saves.join("full/state/memory.sqlite"), b"not SQLite").unwrap();
    fs::write(&paths.config_path, "keep me\n").unwrap();
    assert!(
        restore::restore_from(&paths, "full", &restore_options(), &saves)
            .unwrap_err()
            .to_string()
            .contains("checksum")
    );
    assert_eq!(fs::read_to_string(&paths.config_path).unwrap(), "keep me\n");
}
#[test]
fn sqlite_sessions_are_not_transplanted_to_another_project() {
    let (tmp, paths, saves) = setup();
    save(&paths, &saves);
    let root = tmp.path().join("other");
    fs::create_dir_all(root.join(".macc")).unwrap();
    let other = ProjectPaths::from_root(&root);
    fs::write(&other.config_path, "keep other\n").unwrap();
    assert!(restore::restore_from(&other, "full", &restore_options(), &saves).is_err());
    assert_eq!(
        fs::read_to_string(&other.config_path).unwrap(),
        "keep other\n"
    );
    let mut opts = restore_options();
    opts.config_only = true;
    restore::restore_from(&other, "full", &opts, &saves).unwrap();
    assert!(!other.macc_dir.join("memory.db").exists());
}
#[test]
fn dry_run_preserves_sqlite_authority_and_all_payloads() {
    let (_tmp, paths, saves) = setup();
    save(&paths, &saves);
    let before = store::fingerprint::current(&paths.root).unwrap();
    let marker = fs::read(paths.macc_dir.join("state/tool-sessions.json")).unwrap();
    let mut opts = restore_options();
    opts.dry_run = true;
    restore::restore_from(&paths, "full", &opts, &saves).unwrap();
    assert_eq!(before, store::fingerprint::current(&paths.root).unwrap());
    assert_eq!(
        marker,
        fs::read(paths.macc_dir.join("state/tool-sessions.json")).unwrap()
    );
}
#[test]
fn legacy_json_bundle_remains_readable_and_cannot_replace_sqlite_authority() {
    let (_tmp, paths, saves) = setup();
    let rollback = store::rollback(&paths.root, false, None).unwrap();
    store::rollback(&paths.root, true, Some(&rollback.source_sha256)).unwrap();
    let manifest = save(&paths, &saves);
    assert_eq!(manifest.version, 1);
    assert!(manifest.session_store.is_none());
    restore::restore_from(&paths, "full", &restore_options(), &saves).unwrap();
    let p = store::migrate(&paths.root, false, None).unwrap();
    store::migrate(&paths.root, true, Some(&p.source_sha256)).unwrap();
    assert!(restore::restore_from(&paths, "full", &restore_options(), &saves).is_err());
    let mut opts = restore_options();
    opts.config_only = true;
    restore::restore_from(&paths, "full", &opts, &saves).unwrap();
    assert!(store::is_sqlite(&paths.root).unwrap());
}

#[test]
fn sqlite_secret_scan_never_rewrites_binary_pages_as_text() {
    let (_tmp, paths, saves) = setup();
    let db = rusqlite::Connection::open(paths.macc_dir.join("memory.db")).unwrap();
    db.execute("INSERT INTO session_meta VALUES('credential-test','sk-abcdefghijklmnopqrstuvwxyz1234567890')",[]).unwrap();
    drop(db);
    assert!(bundle::create_in(&paths, "abort", &options(), &saves)
        .unwrap_err()
        .to_string()
        .contains("MACC-SAVE-1003"));
    let mut opts = options();
    opts.handle_secrets = Some(SecretHandlingChoice::Redact);
    assert!(bundle::create_in(&paths, "redact", &opts, &saves)
        .unwrap_err()
        .to_string()
        .contains("cannot be text-redacted"));
    opts.handle_secrets = Some(SecretHandlingChoice::Exclude);
    let manifest = bundle::create_in(&paths, "exclude", &opts, &saves).unwrap();
    assert!(manifest.session_store.is_none());
    assert!(!manifest.includes.coordinator_sessions);
    assert!(!saves.join("exclude/state/memory.sqlite").exists());
    assert!(store::is_sqlite(&paths.root).unwrap());
}
#[test]
fn restore_to_cleared_store_reinstalls_single_sqlite_authority() {
    let (_tmp, paths, saves) = setup();
    let lease = claim(&paths.root);
    record(&paths.root, &lease);
    store::release(&paths.root, &lease.key, true).unwrap();
    let manifest = save(&paths, &saves);
    fs::remove_file(paths.macc_dir.join("memory.db")).unwrap();
    fs::remove_file(paths.macc_dir.join("state/tool-sessions.json")).unwrap();
    restore::restore_from(&paths, "full", &restore_options(), &saves).unwrap();
    assert_eq!(
        store::authority(&paths.root).unwrap().unwrap().project_id,
        manifest.session_store.unwrap().project_id
    );
    assert_eq!(store::usage::inspect(&paths.root).unwrap().records.len(), 1);
    assert!(store::is_sqlite(&paths.root).unwrap());
}
#[test]
fn bundle_payload_rejects_traversal_and_symlinks() {
    let (_tmp, paths, saves) = setup();
    save(&paths, &saves);
    assert!(sessions::payload(&saves.join("full"), "../outside").is_err());
    assert!(sessions::payload(&saves.join("full"), "/etc/passwd").is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&paths.config_path, saves.join("full/linked")).unwrap();
        assert!(sessions::payload(&saves.join("full"), "linked").is_err());
    }
}
#[test]
fn logical_saved_state_ignores_wal_layout_and_detects_new_receipts() {
    let (_tmp, paths, saves) = setup();
    let lease = claim(&paths.root);
    record(&paths.root, &lease);
    store::release(&paths.root, &lease.key, true).unwrap();
    let manifest = save(&paths, &saves);
    let before = manifest.session_store.unwrap().state_sha256;
    let db = rusqlite::Connection::open(paths.macc_dir.join("memory.db")).unwrap();
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    drop(db);
    assert_eq!(before, store::fingerprint::current(&paths.root).unwrap());
    let next = claim(&paths.root);
    record(&paths.root, &next);
    store::release(&paths.root, &next.key, true).unwrap();
    assert_ne!(before, store::fingerprint::current(&paths.root).unwrap());
}

#[test]
fn missing_config_does_not_hide_unsaved_sqlite_state() {
    let (_tmp, paths, _saves) = setup();
    fs::remove_file(&paths.config_path).unwrap();
    assert!(is_macc_state_unsaved(&paths).unwrap());
    assert!(get_unsaved_state_report(&paths).unwrap().sessions_changed);
}

#[test]
fn restoring_an_older_bundle_advances_fencing_beyond_current_generations() {
    let (_tmp, paths, saves) = setup();
    let first = claim(&paths.root);
    record(&paths.root, &first);
    store::release(&paths.root, &first.key, true).unwrap();
    save(&paths, &saves);
    let mut latest = first;
    for _ in 0..3 {
        latest = claim(&paths.root);
        record(&paths.root, &latest);
        store::release(&paths.root, &latest.key, true).unwrap();
    }
    restore::restore_from(&paths, "full", &restore_options(), &saves).unwrap();
    let restored = &store::inspect(&paths.root).unwrap().sessions[0];
    assert!(restored.generation > latest.key.generation);
    assert!(store::heartbeat(&paths.root, &latest.key).is_err());
    let next = claim(&paths.root);
    assert!(next.is_new);
    assert_ne!(next.key.session_id, latest.key.session_id);
    store::release(&paths.root, &next.key, true).unwrap();
}
#[test]
fn attached_coordinator_prevents_restore_before_any_config_write() {
    use crate::process_ownership::{ProcessHandle, ProcessKind};
    let (_tmp, paths, saves) = setup();
    save(&paths, &saves);
    crate::service::process_ownership::register_process(
        &paths.root,
        ProcessHandle {
            kind: ProcessKind::Coordinator,
            project_root: paths.root.clone(),
            pid: Some(std::process::id() as i32),
        },
    )
    .unwrap();
    fs::write(&paths.config_path, "preserve live configuration\n").unwrap();
    let err = restore::restore_from(&paths, "full", &restore_options(), &saves).unwrap_err();
    assert!(err.to_string().contains("Stop coordinator and supervisor"));
    assert_eq!(
        fs::read_to_string(&paths.config_path).unwrap(),
        "preserve live configuration\n"
    );
}

#[test]
fn malformed_catalog_destination_prevents_restoring_sessions_and_configuration() {
    let (_tmp, paths, saves) = setup();
    let catalog = paths.project_catalog_dir().join("items");
    fs::create_dir_all(&catalog).unwrap();
    fs::write(catalog.join("entry"), "saved").unwrap();
    let manifest = save(&paths, &saves);
    let lease = claim(&paths.root);
    store::release(&paths.root, &lease.key, true).unwrap();
    let current = store::fingerprint::current(&paths.root).unwrap();
    assert_ne!(manifest.session_store.unwrap().state_sha256, current);
    fs::remove_dir_all(&catalog).unwrap();
    fs::write(&catalog, "occupied").unwrap();
    fs::write(&paths.config_path, "tools:\n  enabled: []\n").unwrap();

    let err = restore::restore_from(&paths, "full", &restore_options(), &saves)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("catalogs") && err.contains(catalog.to_str().unwrap()),
        "{err}"
    );
    assert_eq!(
        fs::read_to_string(&paths.config_path).unwrap(),
        "tools:\n  enabled: []\n"
    );
    assert_eq!(store::fingerprint::current(&paths.root).unwrap(), current);
}

#[test]
fn deletion_only_removes_the_named_bundle_in_the_save_root() {
    let (tmp, paths, saves) = setup();
    let manifest = save(&paths, &saves);
    let neighbor = tmp.path().join("neighbor");
    let ordinary = saves.join("ordinary");
    for dir in [&neighbor, &ordinary] {
        fs::create_dir(dir).unwrap();
        fs::write(dir.join("notes"), "keep").unwrap();
    }
    for name in [
        "",
        ".",
        "..",
        "../neighbor",
        neighbor.to_str().unwrap(),
        "ordinary",
        "nested/full",
        r"..\neighbor",
    ] {
        assert!(delete_from(name, &saves).is_err(), "{name:?}");
    }
    let manifest_path = ordinary.join("manifest.yaml");
    for text in [
        "unrelated: metadata".into(),
        serde_yaml::to_string(&manifest).unwrap(),
    ] {
        fs::write(&manifest_path, text).unwrap();
        assert!(delete_from("ordinary", &saves).is_err());
    }
    let mut wrong_kind = manifest;
    wrong_kind.name = "ordinary".into();
    wrong_kind.kind = "other".into();
    fs::write(&manifest_path, serde_yaml::to_string(&wrong_kind).unwrap()).unwrap();
    assert!(delete_from("ordinary", &saves).is_err());
    let mut overwrite = options();
    overwrite.overwrite = true;
    assert!(bundle::create_in(&paths, "ordinary", &overwrite, &saves).is_err());
    assert_eq!(fs::read(ordinary.join("notes")).unwrap(), b"keep");
    assert_eq!(fs::read(neighbor.join("notes")).unwrap(), b"keep");
    bundle::create_in(&paths, "full", &overwrite, &saves).unwrap();
    delete_from("full", &saves).unwrap();
    assert!(!saves.join("full").exists());
}

#[cfg(unix)]
#[test]
fn deletion_refuses_linked_bundle_and_linked_manifest() {
    use std::os::unix::fs::symlink;
    let (tmp, paths, saves) = setup();
    save(&paths, &saves);
    let bundle = saves.join("full");
    let outside = tmp.path().join("outside");
    fs::rename(&bundle, &outside).unwrap();
    symlink(&outside, &bundle).unwrap();
    assert!(delete_from("full", &saves).is_err());
    assert!(bundle.is_symlink());
    fs::remove_file(&bundle).unwrap();
    fs::create_dir(&bundle).unwrap();
    symlink(outside.join("manifest.yaml"), bundle.join("manifest.yaml")).unwrap();
    assert!(delete_from("full", &saves).is_err());
    assert!(bundle.join("manifest.yaml").is_symlink());
    assert!(outside.join("manifest.yaml").is_file());
}

#[test]
fn state_only_save_requires_a_readable_coordinator_registry() {
    let (_tmp, paths, saves) = setup();
    let mut opts = options();
    opts.only = Some("automation_state".into());
    opts.no_sessions = true;
    let storage = crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&paths);
    // A stale mirror cannot stand in for a missing authoritative SQLite database.
    fs::create_dir_all(storage.registry_json_path.parent().unwrap()).unwrap();
    fs::write(&storage.registry_json_path, r#"{"tasks":[]}"#).unwrap();
    assert!(bundle::create_in(&paths, "missing", &opts, &saves).is_err());
    assert!(!storage.sqlite_path.exists());
    assert!(!saves.join("missing").exists());

    crate::coordinator_storage::coordinator_storage_import_json_to_sqlite(&paths).unwrap();
    let db = rusqlite::Connection::open(storage.sqlite_path).unwrap();
    db.execute("INSERT INTO tasks (task_id,state,payload_json,updated_at) VALUES ('A','merged','bad','now')", []).unwrap();
    assert!(bundle::create_in(&paths, "broken", &opts, &saves).is_err());
    assert!(!saves.join("broken").exists());
}

#[test]
fn legacy_state_bundle_requires_an_actual_registry_before_restore() {
    let (_tmp, paths, saves) = setup();
    let mut manifest = save(&paths, &saves);
    manifest.includes.automation_state = true;
    manifest.paths.task_registry = Some("automation/task".into());
    let dir = saves.join("full");
    fs::create_dir_all(dir.join("automation/task")).unwrap();
    manifest.hashes.manifest_payload =
        compute_manifest_payload_hash(&serde_yaml::to_string(&manifest).unwrap());
    fs::write(
        dir.join("manifest.yaml"),
        serde_yaml::to_string(&manifest).unwrap(),
    )
    .unwrap();

    super::integrity::write_checksums_file(&dir).unwrap();
    let opts = RestoreOptions {
        no_sessions: true,
        ..restore_options()
    };
    let initial = "tools:\n  enabled: []\n# retain until preflight succeeds\n";
    fs::write(&paths.config_path, initial).unwrap();
    let err = restore::restore_from(&paths, "full", &opts, &saves).unwrap_err();
    assert!(err.to_string().contains("Missing bundle payload"), "{err}");
    assert_eq!(fs::read_to_string(&paths.config_path).unwrap(), initial);
    fs::write(
        dir.join("automation/task/task_registry.json"),
        r#"{"tasks":[{"id":"legacy","state":"merged"}]}"#,
    )
    .unwrap();
    super::integrity::write_checksums_file(&dir).unwrap();
    restore::restore_from(&paths, "full", &opts, &saves).unwrap();
    let registry = crate::coordinator::state::coordinator_state_registry_load(
        &paths.root,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(registry["tasks"][0]["state"], "merged");
}

#[test]
fn redaction_uses_detector_ranges_and_hashes_the_archived_bytes() {
    let (_tmp, paths, saves) = setup();
    // Overlapping GitHub and generic token ranges, punctuation, and a UTF-8 prefix.
    let token = format!("sk-svcacct-_{}-ghp_{}", "z".repeat(22), "A".repeat(36));
    let text = format!("évidence: {token}\n");
    fs::write(&paths.config_path, &text).unwrap();
    let mut opts = options();
    opts.only = Some("config,logs".into());
    opts.redact_logs = true;
    fs::create_dir_all(paths.macc_dir.join("log")).unwrap();
    fs::write(paths.macc_dir.join("log/worker.log"), &text).unwrap();
    opts.handle_secrets = Some(SecretHandlingChoice::Redact);
    let manifest = bundle::create_in(&paths, "redacted", &opts, &saves).unwrap();
    let archived = saves.join("redacted/config/macc.yaml");
    assert_eq!(
        manifest.hashes.config,
        Some(compute_file_sha256(&archived).unwrap())
    );
    assert_eq!(manifest.security.secret_scan.findings, 0);
    for file in [archived, saves.join("redacted/logs/worker.log")] {
        assert_eq!(fs::read_to_string(file).unwrap(), "évidence: [REDACTED]\n");
    }
    assert_eq!(fs::read_to_string(&paths.config_path).unwrap(), text);
}

#[test]
fn final_scan_rejects_a_secret_in_the_manifest_and_preserves_the_previous_save() {
    let (_tmp, paths, saves) = setup();
    let mut opts = options();
    opts.only = Some("config".into());
    bundle::create_in(&paths, "same", &opts, &saves).unwrap();
    let file = saves.join("same/manifest.yaml");
    let previous = fs::read(&file).unwrap();
    opts.description = Some(format!("sk-proj-{}", "a_b-".repeat(10)));
    opts.overwrite = true;
    opts.handle_secrets = Some(SecretHandlingChoice::Redact);
    opts.dry_run = true;
    let preview = bundle::create_in(&paths, "same", &opts, &saves).unwrap();
    assert_eq!(preview.security.secret_scan.findings, 1);
    opts.dry_run = false;
    let error = bundle::create_in(&paths, "same", &opts, &saves).unwrap_err();
    assert!(error.to_string().contains("blocking secret"));
    assert_eq!(fs::read(file).unwrap(), previous);
}

#[test]
fn legacy_session_redaction_handles_extended_token_and_checks_its_hash() {
    let temp = tempfile::tempdir().unwrap();
    let paths = ProjectPaths::from_root(temp.path().join("project"));
    let session = paths.macc_dir.join("state/tool-sessions.json");
    fs::create_dir_all(session.parent().unwrap()).unwrap();
    fs::write(&paths.config_path, "tools: {}\n").unwrap();
    fs::write(&session, r#"{"note":"sk-proj-ab_cd-12345678901234567890"}"#).unwrap();
    let mut opts = options();
    opts.only = Some("sessions".into());
    opts.handle_secrets = Some(SecretHandlingChoice::Redact);
    let saves = temp.path().join("saves");
    let manifest = bundle::create_in(&paths, "legacy", &opts, &saves).unwrap();
    let file = saves.join("legacy/state/tool-sessions.json");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        r#"{"note":"[REDACTED]"}"#
    );
    assert_eq!(
        manifest.hashes.coordinator_sessions,
        Some(compute_file_sha256(&file).unwrap())
    );
}

#[test]
fn nested_phase_logs_round_trip_with_deterministic_budget_omissions() {
    let (_tmp, paths, saves) = setup();
    let root = paths.macc_dir.join("log");
    let evidence = "performer/T-1/phases/review/outcome.json";
    // Create in reverse selection order. The oversized file must not starve the evidence.
    for (name, text) in [
        ("z.log", "late"),
        (evidence, "{}"),
        ("coordinator/run.log", "run"),
        ("a.log", "oversized"),
        ("unrelated/app.log", "private"),
    ] {
        let file = root.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("unrelated"), root.join("performer/external")).unwrap();
    let opts = SaveOptions {
        only: Some("logs".into()),
        log_max_size: "5B".into(),
        ..options()
    };
    let manifest = bundle::create_in(&paths, "evidence", &opts, &saves).unwrap();
    assert!(manifest.includes.logs);
    assert_eq!(manifest.logs_omitted_by_size, ["a.log", "z.log"]);
    let archive = saves.join("evidence/logs");
    assert_eq!(
        fs::read(archive.join("coordinator/run.log")).unwrap(),
        b"run"
    );
    assert_eq!(fs::read(archive.join(evidence)).unwrap(), b"{}");
    for excluded in ["a.log", "z.log", "unrelated", "performer/external"] {
        assert!(!archive.join(excluded).exists());
    }

    let restore_opts = RestoreOptions {
        include_logs: true,
        ..restore_options()
    };
    restore::restore_from(&paths, "evidence", &restore_opts, &saves).unwrap();
    assert_eq!(
        fs::read(paths.macc_dir.join("restored-logs/evidence").join(evidence)).unwrap(),
        b"{}"
    );

    let empty = bundle::create_in(
        &paths,
        "zero",
        &SaveOptions {
            log_max_size: "0".into(),
            ..opts
        },
        &saves,
    )
    .unwrap();
    assert!(!empty.includes.logs);
    assert!(empty.paths.logs_archive.is_none());
    assert_eq!(empty.logs_omitted_by_size.len(), 4);
}

#[test]
fn log_archive_io_failure_identifies_the_path() {
    let (_tmp, paths, saves) = setup();
    let log = paths.macc_dir.join("log");
    fs::write(&log, "not a directory").unwrap();
    let error = bundle::create_in(
        &paths,
        "invalid",
        &SaveOptions {
            only: Some("logs".into()),
            ..options()
        },
        &saves,
    )
    .unwrap_err();
    assert!(matches!(error, MaccError::Io { path, .. } if Path::new(&path) == log));
    assert!(!saves.join("invalid").exists());
}

#[test]
fn excluded_phase_evidence_leaves_no_log_section() {
    let (_tmp, paths, saves) = setup();
    let phase = paths.macc_dir.join("log/performer/T-1/phases/review");
    fs::create_dir_all(&phase).unwrap();
    fs::write(
        phase.join("projected.txt"),
        format!("ghp_{}", "a".repeat(36)),
    )
    .unwrap();
    let opts = SaveOptions {
        only: Some("logs".into()),
        redact_logs: false,
        handle_secrets: Some(SecretHandlingChoice::Exclude),
        ..options()
    };
    let manifest = bundle::create_in(&paths, "excluded", &opts, &saves).unwrap();
    assert!(!manifest.includes.logs);
    assert!(manifest.paths.logs_archive.is_none());
}

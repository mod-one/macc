// Integration tests for L6-DAEMON-001
//
// Verifies that the ownership layer does not regress the property that
// "processes run independently of any client". Three scenarios are exercised
// via the service layer directly, using manufactured timestamps so no actual
// sleep is needed. Scenario (3) — shell disconnect via setsid — is covered
// by the companion script automat/tests/test_daemon_ownership.sh which
// requires a built macc binary.

use chrono::{Duration, Utc};
use macc_core::process_ownership::{
    ClientIdentity, ClientKind, OwnershipStore, ProcessHandle, ProcessKind,
};
use macc_core::service::process_ownership::{
    claim_owner, is_current_owner, list_records, project_lease, register_process,
    request_takeover, respond_takeover, unregister_process,
};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn unique_repo_root(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("macc-daemon-ownership-{label}-{nanos}"));
    fs::create_dir_all(root.join(".macc/state")).expect("create temp repo root");
    root
}

fn fresh_client(client_id: &str, kind: ClientKind) -> ClientIdentity {
    let now = Utc::now().to_rfc3339();
    ClientIdentity {
        client_id: client_id.to_string(),
        kind,
        connected_at: now.clone(),
        last_heartbeat: now,
    }
}

fn coordinator_handle(repo_root: &std::path::Path) -> ProcessHandle {
    ProcessHandle {
        kind: ProcessKind::Coordinator,
        project_root: repo_root.to_path_buf(),
        pid: Some(12345),
    }
}

fn supervisor_handle(repo_root: &std::path::Path) -> ProcessHandle {
    ProcessHandle {
        kind: ProcessKind::Supervisor,
        project_root: repo_root.to_path_buf(),
        pid: Some(12346),
    }
}

fn project_handle(repo_root: &std::path::Path) -> ProcessHandle {
    ProcessHandle {
        kind: ProcessKind::Project,
        project_root: repo_root.to_path_buf(),
        pid: None,
    }
}

// Write a stale heartbeat for a given client ID in the project lease record.
// Uses load_and_modify + get_record/upsert_record to stay within the public API.
fn write_stale_heartbeat_for_client(repo_root: &std::path::Path, client_id: &str) {
    let stale_ts = (Utc::now() - Duration::seconds(90)).to_rfc3339();
    let proj_handle = project_handle(repo_root);
    OwnershipStore::load_and_modify(repo_root, |store| {
        if let Some(record) = store.get_record(&proj_handle).cloned() {
            let mut updated = record;
            if let Some(owner) = updated.owner.as_mut() {
                if owner.client_id == client_id {
                    owner.last_heartbeat = stale_ts.clone();
                }
            }
            for viewer in &mut updated.viewers {
                if viewer.client_id == client_id {
                    viewer.last_heartbeat = stale_ts.clone();
                }
            }
            store.upsert_record(updated);
        }
        Ok(())
    })
    .expect("write stale heartbeat");
}

// Write stale heartbeats for ALL clients in the project lease record.
fn write_all_stale_heartbeats(repo_root: &std::path::Path) {
    let stale_ts = (Utc::now() - Duration::seconds(90)).to_rfc3339();
    let proj_handle = project_handle(repo_root);
    OwnershipStore::load_and_modify(repo_root, |store| {
        if let Some(record) = store.get_record(&proj_handle).cloned() {
            let mut updated = record;
            if let Some(owner) = updated.owner.as_mut() {
                owner.last_heartbeat = stale_ts.clone();
            }
            for viewer in &mut updated.viewers {
                viewer.last_heartbeat = stale_ts.clone();
            }
            store.upsert_record(updated);
        }
        Ok(())
    })
    .expect("write all stale heartbeats");
}

fn coordinator_event_types(repo_root: &std::path::Path) -> Vec<String> {
    let events_path = repo_root
        .join(".macc")
        .join("log")
        .join("coordinator")
        .join("events.jsonl");
    let raw = fs::read_to_string(events_path).expect("read events.jsonl");
    raw.lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("valid event json"))
        .map(|event| {
            event
                .get("type")
                .and_then(Value::as_str)
                .expect("event type")
                .to_string()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Scenario 1: Coordinator runs unowned; record persists; cleanup on stop
//
// Models what happens when `macc coordinator --no-tui` starts without any
// interactive client. The process registers itself. No client claims ownership.
// The record persists across repeated loads with owner=None and viewers=[].
// When the coordinator exits it unregisters and the per-process record is gone.
// ---------------------------------------------------------------------------

#[test]
fn scenario_1_coordinator_runs_unowned_and_record_persists_then_clears() {
    let repo_root = unique_repo_root("s1");
    let handle = coordinator_handle(&repo_root);

    // Step 1: process registers itself (no client yet).
    register_process(&repo_root, handle.clone()).expect("register coordinator process");

    // Step 2: verify per-process inventory record is present with no owner.
    let record = macc_core::service::process_ownership::get_record(&repo_root, &handle)
        .expect("load record")
        .expect("record exists after registration");
    assert!(
        record.owner.is_none(),
        "no owner should be set on newly registered process: {:?}",
        record.owner
    );
    assert!(
        record.viewers.is_empty(),
        "no viewers should be present on newly registered process"
    );

    // Step 3: repeated loads keep the record stable (simulates 30 s of polling).
    for _ in 0..3 {
        let store = OwnershipStore::load(&repo_root).expect("re-load store");
        let reloaded = store
            .get_record(&handle)
            .expect("coordinator record must remain after re-load");
        assert!(
            reloaded.owner.is_none(),
            "record must stay unowned across re-loads"
        );
        assert!(
            reloaded.viewers.is_empty(),
            "viewers must stay empty across re-loads"
        );
    }

    // Step 4: project-wide lease record was also seeded with no owner.
    let lease = project_lease(&repo_root)
        .expect("load project lease")
        .expect("project lease seeded by register_process");
    assert!(
        lease.owner.is_none(),
        "project lease owner must be None when no client has connected"
    );
    assert!(
        lease.viewers.is_empty(),
        "project lease viewers must be empty when no client has connected"
    );

    // Step 5: coordinator stops — per-process inventory record is removed.
    unregister_process(&repo_root, &handle).expect("unregister coordinator process");
    let after_stop = macc_core::service::process_ownership::get_record(&repo_root, &handle)
        .expect("load after unregister");
    assert!(
        after_stop.is_none(),
        "per-process record must be removed after unregister"
    );
}

// ---------------------------------------------------------------------------
// Scenario 2: Stale owner is evicted on next load(); second client takes over
//
// Models a TUI (owner) being killed with SIGKILL. Its heartbeat goes stale.
// On the next OwnershipStore::load() the eviction logic fires and clears the
// owner. A second TUI can then claim ownership.
// ---------------------------------------------------------------------------

#[test]
fn scenario_2_stale_owner_evicted_and_second_client_becomes_owner() {
    let repo_root = unique_repo_root("s2");
    let handle = coordinator_handle(&repo_root);

    // Step 1: register coordinator (no interactive client yet).
    register_process(&repo_root, handle.clone()).expect("register coordinator");

    // Step 2: first TUI claims ownership via service layer.
    let status = claim_owner(&repo_root, &handle, fresh_client("tui-1", ClientKind::Tui))
        .expect("first TUI claims ownership");
    assert_eq!(
        status,
        macc_core::process_ownership::OwnershipStatus::Owner,
        "first TUI must become Owner"
    );
    let lease_before = project_lease(&repo_root)
        .expect("load lease")
        .expect("lease exists");
    assert_eq!(
        lease_before.owner.as_ref().map(|o| o.client_id.as_str()),
        Some("tui-1"),
        "project lease owner must be tui-1"
    );

    // Step 3: simulate TUI killed (SIGKILL) — write a stale heartbeat.
    // This is equivalent to the TTL expiring after the process died.
    write_stale_heartbeat_for_client(&repo_root, "tui-1");

    // Step 4: next OwnershipStore::load() triggers evict_stale_records().
    // The stale owner should be cleared.
    let records_after = list_records(&repo_root).expect("list records triggers eviction");
    let lease_record = records_after
        .iter()
        .find(|r| r.process.kind == ProcessKind::Project)
        .expect("project lease record must remain after eviction");
    assert!(
        lease_record.owner.is_none(),
        "stale tui-1 owner must be evicted on load: found {:?}",
        lease_record.owner
    );

    // Step 5: second TUI connects and claims ownership.
    let status2 = claim_owner(&repo_root, &handle, fresh_client("tui-2", ClientKind::Tui))
        .expect("second TUI claims ownership");
    assert_eq!(
        status2,
        macc_core::process_ownership::OwnershipStatus::Owner,
        "second TUI must become Owner after first is evicted"
    );

    let lease_final = project_lease(&repo_root)
        .expect("load final lease")
        .expect("lease exists");
    assert_eq!(
        lease_final.owner.as_ref().map(|o| o.client_id.as_str()),
        Some("tui-2"),
        "tui-2 must now be the project lease owner"
    );
    assert!(
        lease_final.viewers.is_empty(),
        "no lingering viewers after tui-1 eviction and tui-2 ownership"
    );

    // Cleanup.
    unregister_process(&repo_root, &handle).expect("unregister coordinator");
}

// ---------------------------------------------------------------------------
// Scenario 3: Coordinator + supervisor both registered with owner=None
//
// This test exercises the pure service-layer variant: both processes register
// without any client claiming ownership. Both inventory records have owner=None.
// The full shell-based scenario (setsid + stdin close) lives in
// automat/tests/test_daemon_ownership.sh.
// ---------------------------------------------------------------------------

#[test]
fn scenario_3_coordinator_and_supervisor_both_run_unowned() {
    let repo_root = unique_repo_root("s3");
    let coord_handle = coordinator_handle(&repo_root);
    let sup_handle = supervisor_handle(&repo_root);

    // Both processes register themselves (no client connected).
    register_process(&repo_root, coord_handle.clone()).expect("register coordinator");
    register_process(&repo_root, sup_handle.clone()).expect("register supervisor");

    // Verify both inventory records exist with owner=None.
    let coord_record = macc_core::service::process_ownership::get_record(&repo_root, &coord_handle)
        .expect("load coordinator record")
        .expect("coordinator inventory record exists");
    assert!(
        coord_record.owner.is_none(),
        "coordinator must have owner=None"
    );

    let sup_record = macc_core::service::process_ownership::get_record(&repo_root, &sup_handle)
        .expect("load supervisor record")
        .expect("supervisor inventory record exists");
    assert!(
        sup_record.owner.is_none(),
        "supervisor must have owner=None"
    );

    // Project lease was auto-seeded and has no owner.
    let lease = project_lease(&repo_root)
        .expect("load project lease")
        .expect("project lease seeded");
    assert!(
        lease.owner.is_none(),
        "project lease must have owner=None when no client connected"
    );
    assert!(
        lease.viewers.is_empty(),
        "project lease viewers must be empty when no client connected"
    );

    // Both records survive repeated loads (parent shell disconnect simulation).
    for _ in 0..3 {
        let store = OwnershipStore::load(&repo_root).expect("re-load store");
        assert!(
            store.get_record(&coord_handle).is_some(),
            "coordinator record must survive re-load"
        );
        assert!(
            store.get_record(&sup_handle).is_some(),
            "supervisor record must survive re-load"
        );
    }

    // Both unregister cleanly on exit.
    unregister_process(&repo_root, &coord_handle).expect("unregister coordinator");
    unregister_process(&repo_root, &sup_handle).expect("unregister supervisor");

    assert!(
        macc_core::service::process_ownership::get_record(&repo_root, &coord_handle)
            .expect("load coordinator record after stop")
            .is_none(),
        "coordinator record must be removed after unregister"
    );
    assert!(
        macc_core::service::process_ownership::get_record(&repo_root, &sup_handle)
            .expect("load supervisor record after stop")
            .is_none(),
        "supervisor record must be removed after unregister"
    );
}

// ---------------------------------------------------------------------------
// Scenario 2b: Stale viewer and stale owner are both evicted; fresh client owns
//
// Defensive check: a stale viewer that never claimed ownership doesn't block
// a fresh client from becoming owner.
// ---------------------------------------------------------------------------

#[test]
fn stale_viewer_evicted_and_fresh_client_becomes_owner() {
    let repo_root = unique_repo_root("s2b");
    let handle = coordinator_handle(&repo_root);

    register_process(&repo_root, handle.clone()).expect("register coordinator");

    // First client becomes owner.
    claim_owner(
        &repo_root,
        &handle,
        fresh_client("tui-owner", ClientKind::Tui),
    )
    .expect("owner claim");

    // Second client joins as viewer.
    claim_owner(
        &repo_root,
        &handle,
        fresh_client("web-viewer", ClientKind::Web),
    )
    .expect("viewer join");

    // Write stale heartbeats for all clients in the project lease.
    write_all_stale_heartbeats(&repo_root);

    // Load triggers eviction of all stale clients.
    let _ = list_records(&repo_root).expect("load triggers eviction");

    // New client claims ownership.
    let status = claim_owner(
        &repo_root,
        &handle,
        fresh_client("tui-new", ClientKind::Tui),
    )
    .expect("new owner claim after eviction");
    assert_eq!(
        status,
        macc_core::process_ownership::OwnershipStatus::Owner,
        "fresh client must become owner after stale clients are evicted"
    );

    let lease = project_lease(&repo_root)
        .expect("load lease")
        .expect("lease exists");
    assert_eq!(
        lease.owner.as_ref().map(|o| o.client_id.as_str()),
        Some("tui-new"),
    );
    assert!(lease.viewers.is_empty());
}

#[test]
fn first_connect_takes_ownership_and_later_clients_become_viewers() {
    let repo_root = unique_repo_root("first-connect");
    let handle = coordinator_handle(&repo_root);

    register_process(&repo_root, handle.clone()).expect("register coordinator");

    let first_status = claim_owner(&repo_root, &handle, fresh_client("cli-owner", ClientKind::Cli))
        .expect("first cli claims ownership");
    let second_status =
        claim_owner(&repo_root, &handle, fresh_client("cli-viewer", ClientKind::Cli))
            .expect("second cli joins as viewer");
    let third_status = claim_owner(&repo_root, &handle, fresh_client("web-viewer", ClientKind::Web))
        .expect("web joins as viewer");

    assert_eq!(first_status, macc_core::process_ownership::OwnershipStatus::Owner);
    assert_eq!(
        second_status,
        macc_core::process_ownership::OwnershipStatus::Viewer
    );
    assert_eq!(
        third_status,
        macc_core::process_ownership::OwnershipStatus::Viewer
    );

    let lease = project_lease(&repo_root)
        .expect("load lease")
        .expect("lease exists");
    assert_eq!(
        lease.owner.as_ref().map(|owner| owner.client_id.as_str()),
        Some("cli-owner")
    );
    assert_eq!(lease.viewers.len(), 2);
    assert_eq!(lease.viewers[0].client_id, "cli-viewer");
    assert_eq!(lease.viewers[0].kind, ClientKind::Cli);
    assert_eq!(lease.viewers[1].client_id, "web-viewer");
    assert_eq!(lease.viewers[1].kind, ClientKind::Web);
}

#[test]
fn takeover_acceptance_transfers_ownership_and_emits_events() {
    let repo_root = unique_repo_root("takeover-accept");
    let handle = coordinator_handle(&repo_root);

    register_process(&repo_root, handle.clone()).expect("register coordinator");
    claim_owner(&repo_root, &handle, fresh_client("cli-owner", ClientKind::Cli))
        .expect("owner claim");
    let viewer_status =
        claim_owner(&repo_root, &handle, fresh_client("cli-requester", ClientKind::Cli))
            .expect("requester joins as viewer");
    assert_eq!(
        viewer_status,
        macc_core::process_ownership::OwnershipStatus::Viewer
    );

    let request_id = request_takeover(
        &repo_root,
        &handle,
        fresh_client("cli-requester", ClientKind::Cli),
    )
    .expect("request takeover");
    let pending = project_lease(&repo_root)
        .expect("load pending lease")
        .expect("lease exists");
    assert_eq!(
        pending
            .takeover_request
            .as_ref()
            .map(|request| request.request_id.as_str()),
        Some(request_id.as_str())
    );

    respond_takeover(&repo_root, &handle, "cli-owner", &request_id, true)
        .expect("accept takeover");

    let lease = project_lease(&repo_root)
        .expect("load final lease")
        .expect("lease exists");
    assert_eq!(
        lease.owner.as_ref().map(|owner| owner.client_id.as_str()),
        Some("cli-requester")
    );
    assert!(lease.takeover_request.is_none());
    assert!(
        !is_current_owner(&repo_root, &handle, "cli-owner").expect("check old owner")
    );
    assert!(is_current_owner(&repo_root, &handle, "cli-requester").expect("check new owner"));

    let event_types = coordinator_event_types(&repo_root);
    assert!(event_types.iter().any(|event| event == "takeover_requested"));
    assert!(event_types.iter().any(|event| event == "takeover_accepted"));
}

#[test]
fn takeover_rejection_keeps_owner_and_emits_rejection_event() {
    let repo_root = unique_repo_root("takeover-reject");
    let handle = coordinator_handle(&repo_root);

    register_process(&repo_root, handle.clone()).expect("register coordinator");
    claim_owner(&repo_root, &handle, fresh_client("cli-owner", ClientKind::Cli))
        .expect("owner claim");
    let viewer_status =
        claim_owner(&repo_root, &handle, fresh_client("cli-requester", ClientKind::Cli))
            .expect("requester joins as viewer");
    assert_eq!(
        viewer_status,
        macc_core::process_ownership::OwnershipStatus::Viewer
    );

    let request_id = request_takeover(
        &repo_root,
        &handle,
        fresh_client("cli-requester", ClientKind::Cli),
    )
    .expect("request takeover");

    respond_takeover(&repo_root, &handle, "cli-owner", &request_id, false)
        .expect("reject takeover");

    let lease = project_lease(&repo_root)
        .expect("load final lease")
        .expect("lease exists");
    assert_eq!(
        lease.owner.as_ref().map(|owner| owner.client_id.as_str()),
        Some("cli-owner")
    );
    assert!(lease.takeover_request.is_none());
    assert!(is_current_owner(&repo_root, &handle, "cli-owner").expect("check owner"));
    assert!(
        !is_current_owner(&repo_root, &handle, "cli-requester").expect("check requester")
    );

    let event_types = coordinator_event_types(&repo_root);
    assert!(event_types.iter().any(|event| event == "takeover_requested"));
    assert!(event_types.iter().any(|event| event == "takeover_rejected"));
}

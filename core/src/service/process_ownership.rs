use crate::process_ownership::{
    emit_ownership_claimed, emit_ownership_released, emit_takeover_accepted,
    emit_takeover_rejected, emit_takeover_requested, emit_viewer_joined, emit_viewer_left,
    ClientIdentity, OwnershipEventBroadcaster, OwnershipRecord, OwnershipStatus, OwnershipStore,
    ProcessHandle, ProcessKind, TakeoverRequest,
};
use crate::{MaccError, Result};
use chrono::Utc;
use serde_json::Map;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// L6-OWN-008: the single project-wide control lease handle.
/// All claim/release/viewer/takeover/heartbeat operations route through this
/// handle regardless of which per-process handle the caller passes in.
fn project_lease_handle(repo_root: &Path) -> ProcessHandle {
    ProcessHandle {
        kind: ProcessKind::Project,
        project_root: repo_root.to_path_buf(),
        pid: None,
    }
}

fn ensure_project_lease_record(store: &mut OwnershipStore, repo_root: &Path) {
    let handle = project_lease_handle(repo_root);
    if store.get_record(&handle).is_none() {
        store.upsert_record(OwnershipRecord {
            process: handle,
            owner: None,
            viewers: Vec::new(),
            takeover_request: None,
            started_at: now_rfc3339(),
        });
    }
}

pub fn register_process(repo_root: &Path, handle: ProcessHandle) -> Result<()> {
    let record = OwnershipRecord {
        process: handle.clone(),
        owner: None,
        viewers: Vec::new(),
        takeover_request: None,
        started_at: now_rfc3339(),
    };
    OwnershipStore::load_and_modify(repo_root, |store| {
        // Always make sure the project-wide lease record exists so the gate can
        // operate against a stable handle.
        ensure_project_lease_record(store, repo_root);
        store.upsert_record(record);
        Ok(())
    })?;
    OwnershipEventBroadcaster::emit(repo_root, "process_registered", &handle, Map::new());
    Ok(())
}

pub fn unregister_process(repo_root: &Path, handle: &ProcessHandle) -> Result<()> {
    OwnershipStore::load_and_modify(repo_root, |store| {
        store.remove_record(handle);
        Ok(())
    })
}

pub fn claim_owner(
    repo_root: &Path,
    handle: &ProcessHandle,
    identity: ClientIdentity,
) -> Result<OwnershipStatus> {
    // Route to the project lease. The `handle` is preserved purely for event
    // payload context (so callers see which process the ownership relates to).
    let lease = project_lease_handle(repo_root);
    let status = OwnershipStore::load_and_modify(repo_root, |store| {
        ensure_project_lease_record(store, repo_root);
        let record = find_record_mut(store, &lease).expect("project lease record ensured");

        if record.owner.is_none() {
            let owner = refreshed_identity(identity);
            record.owner = Some(owner.clone());
            record
                .viewers
                .retain(|viewer| viewer.client_id != owner.client_id);
            record.takeover_request = None;
            emit_ownership_claimed(repo_root, handle, &owner);
            return Ok(OwnershipStatus::Owner);
        }

        if record
            .owner
            .as_ref()
            .is_some_and(|owner| owner.client_id == identity.client_id)
        {
            return Ok(OwnershipStatus::Owner);
        }

        if add_viewer_if_missing(record, identity.clone()) {
            let viewer = record
                .viewers
                .iter()
                .find(|viewer| viewer.client_id == identity.client_id)
                .cloned()
                .expect("viewer added");
            emit_viewer_joined(repo_root, handle, &viewer);
        }

        Ok(OwnershipStatus::Viewer)
    })?;

    Ok(status)
}

pub fn release_owner(repo_root: &Path, handle: &ProcessHandle, client_id: &str) -> Result<()> {
    let lease = project_lease_handle(repo_root);
    OwnershipStore::load_and_modify(repo_root, |store| {
        let Some(record) = find_record_mut(store, &lease) else {
            return Ok(());
        };

        let Some(owner) = record.owner.as_ref() else {
            return Ok(());
        };

        if owner.client_id != client_id {
            return Ok(());
        }

        let owner = record.owner.take().expect("owner present");
        record.takeover_request = None;
        emit_ownership_released(repo_root, handle, &owner);
        Ok(())
    })
}

pub fn register_viewer(
    repo_root: &Path,
    handle: &ProcessHandle,
    identity: ClientIdentity,
) -> Result<()> {
    let lease = project_lease_handle(repo_root);
    OwnershipStore::load_and_modify(repo_root, |store| {
        ensure_project_lease_record(store, repo_root);
        let record = find_record_mut(store, &lease).expect("project lease ensured");
        if add_viewer_if_missing(record, identity.clone()) {
            let viewer = record
                .viewers
                .iter()
                .find(|viewer| viewer.client_id == identity.client_id)
                .cloned()
                .expect("viewer added");
            emit_viewer_joined(repo_root, handle, &viewer);
        }
        Ok(())
    })
}

pub fn unregister_viewer(repo_root: &Path, handle: &ProcessHandle, client_id: &str) -> Result<()> {
    let lease = project_lease_handle(repo_root);
    OwnershipStore::load_and_modify(repo_root, |store| {
        let Some(record) = find_record_mut(store, &lease) else {
            return Ok(());
        };

        if let Some(index) = record
            .viewers
            .iter()
            .position(|viewer| viewer.client_id == client_id)
        {
            let viewer = record.viewers.remove(index);
            emit_viewer_left(repo_root, handle, &viewer);
        }

        Ok(())
    })
}

pub fn request_takeover(
    repo_root: &Path,
    handle: &ProcessHandle,
    requester: ClientIdentity,
) -> Result<String> {
    let lease = project_lease_handle(repo_root);
    let request_id = Uuid::new_v4().to_string();
    let request = TakeoverRequest {
        request_id: request_id.clone(),
        requester: refreshed_identity(requester),
        requested_at: now_rfc3339(),
    };

    OwnershipStore::load_and_modify(repo_root, |store| {
        ensure_project_lease_record(store, repo_root);
        let record = find_record_mut(store, &lease).expect("project lease ensured");
        record.takeover_request = Some(request.clone());
        emit_takeover_requested(repo_root, handle, &request.requester, &request.request_id);
        Ok(())
    })?;

    Ok(request_id)
}

pub fn respond_takeover(
    repo_root: &Path,
    handle: &ProcessHandle,
    owner_client_id: &str,
    request_id: &str,
    accept: bool,
) -> Result<()> {
    let lease = project_lease_handle(repo_root);
    OwnershipStore::load_and_modify(repo_root, |store| {
        let record = find_record_mut(store, &lease).ok_or_else(|| {
            MaccError::Validation("Project control lease is not registered".to_string())
        })?;
        let owner = record.owner.as_ref().ok_or_else(|| {
            MaccError::Validation("Cannot respond to takeover without an owner".into())
        })?;
        if owner.client_id != owner_client_id {
            return Err(MaccError::Validation(format!(
                "Client '{}' is not the current owner",
                owner_client_id
            )));
        }

        let request = record
            .takeover_request
            .take()
            .ok_or_else(|| MaccError::Validation("No pending takeover request".into()))?;
        if request.request_id != request_id {
            return Err(MaccError::Validation(format!(
                "Takeover request '{}' does not match pending request '{}'",
                request_id, request.request_id
            )));
        }

        if accept {
            record.owner = Some(request.requester.clone());
            record
                .viewers
                .retain(|viewer| viewer.client_id != request.requester.client_id);
            emit_takeover_accepted(repo_root, handle, &request.requester, &request.request_id);
        } else {
            emit_takeover_rejected(repo_root, handle, &request.requester, &request.request_id);
        }

        Ok(())
    })
}

pub fn heartbeat(repo_root: &Path, _handle: &ProcessHandle, client_id: &str) -> Result<()> {
    let lease = project_lease_handle(repo_root);
    OwnershipStore::load_and_modify(repo_root, |store| {
        let Some(record) = find_record_mut(store, &lease) else {
            return Ok(());
        };

        let now = now_rfc3339();
        if let Some(owner) = record
            .owner
            .as_mut()
            .filter(|owner| owner.client_id == client_id)
        {
            owner.last_heartbeat = now.clone();
            return Ok(());
        }

        if let Some(viewer) = record
            .viewers
            .iter_mut()
            .find(|viewer| viewer.client_id == client_id)
        {
            viewer.last_heartbeat = now;
        }

        Ok(())
    })
}

/// Force-clear the project-wide owner without checking whether the caller is
/// the current owner. Returns the evicted owner's client_id, or None if there
/// was no owner. Use this as a recovery tool when a client died without
/// releasing ownership and you cannot wait for the heartbeat TTL to expire.
pub fn force_release_project_owner(repo_root: &Path) -> Result<Option<String>> {
    let lease = project_lease_handle(repo_root);
    let mut evicted: Option<String> = None;
    OwnershipStore::load_and_modify(repo_root, |store| {
        if let Some(record) = store
            .records
            .iter_mut()
            .find(|r| r.process == lease)
        {
            if let Some(owner) = record.owner.take() {
                evicted = Some(owner.client_id);
                record.takeover_request = None;
            }
        }
        Ok(())
    })?;
    Ok(evicted)
}

pub fn get_record(repo_root: &Path, handle: &ProcessHandle) -> Result<Option<OwnershipRecord>> {
    let store = OwnershipStore::load(repo_root)?;
    Ok(store.get_record(handle).cloned())
}

pub fn list_records(repo_root: &Path) -> Result<Vec<OwnershipRecord>> {
    let store = OwnershipStore::load(repo_root)?;
    Ok(store.records.clone())
}

pub fn is_current_owner(
    repo_root: &Path,
    _handle: &ProcessHandle,
    client_id: &str,
) -> Result<bool> {
    let lease = project_lease_handle(repo_root);
    let record = get_record(repo_root, &lease)?;
    Ok(record
        .and_then(|record| record.owner)
        .is_some_and(|owner| owner.client_id == client_id))
}

/// Returns the current project-wide control lease (or None if no project lease
/// exists yet — i.e. no client has ever claimed control).
pub fn project_lease(repo_root: &Path) -> Result<Option<OwnershipRecord>> {
    get_record(repo_root, &project_lease_handle(repo_root))
}

#[derive(Debug, Clone)]
pub struct RegisteredProcessGuard {
    repo_root: PathBuf,
    handle: ProcessHandle,
}

impl RegisteredProcessGuard {
    pub fn new(repo_root: &Path, handle: ProcessHandle) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            handle,
        }
    }
}

impl Drop for RegisteredProcessGuard {
    fn drop(&mut self) {
        if let Err(err) = unregister_process(&self.repo_root, &self.handle) {
            tracing::warn!(
                process_kind = ?self.handle.kind,
                pid = self.handle.pid,
                "failed to unregister process during drop: {}",
                err
            );
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessOwnershipGuard {
    repo_root: PathBuf,
    handle: ProcessHandle,
    client_id: String,
}

impl ProcessOwnershipGuard {
    pub fn new(repo_root: &Path, handle: ProcessHandle, client_id: impl Into<String>) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            handle,
            client_id: client_id.into(),
        }
    }
}

impl Drop for ProcessOwnershipGuard {
    fn drop(&mut self) {
        if let Err(err) = release_owner(&self.repo_root, &self.handle, &self.client_id) {
            tracing::warn!(
                process_kind = ?self.handle.kind,
                pid = self.handle.pid,
                client_id = self.client_id,
                "failed to release ownership during drop: {}",
                err
            );
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProcessViewerGuard {
    repo_root: PathBuf,
    handle: ProcessHandle,
    client_id: String,
}

impl ProcessViewerGuard {
    pub fn new(repo_root: &Path, handle: ProcessHandle, client_id: impl Into<String>) -> Self {
        Self {
            repo_root: repo_root.to_path_buf(),
            handle,
            client_id: client_id.into(),
        }
    }
}

impl Drop for ProcessViewerGuard {
    fn drop(&mut self) {
        if let Err(err) = unregister_viewer(&self.repo_root, &self.handle, &self.client_id) {
            tracing::warn!(
                process_kind = ?self.handle.kind,
                pid = self.handle.pid,
                client_id = self.client_id,
                "failed to unregister viewer during drop: {}",
                err
            );
        }
    }
}

fn find_record_mut<'a>(
    store: &'a mut OwnershipStore,
    handle: &ProcessHandle,
) -> Option<&'a mut OwnershipRecord> {
    store
        .records
        .iter_mut()
        .find(|record| &record.process == handle)
}

fn add_viewer_if_missing(record: &mut OwnershipRecord, identity: ClientIdentity) -> bool {
    if record
        .viewers
        .iter()
        .any(|viewer| viewer.client_id == identity.client_id)
    {
        return false;
    }

    record.viewers.push(refreshed_identity(identity));
    true
}

fn refreshed_identity(mut identity: ClientIdentity) -> ClientIdentity {
    let now = now_rfc3339();
    if identity.connected_at.is_empty() {
        identity.connected_at = now.clone();
    }
    identity.last_heartbeat = now;
    identity
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::{
        claim_owner, get_record, list_records, project_lease, register_process, release_owner,
        request_takeover, respond_takeover, unregister_process,
    };
    use crate::process_ownership::{
        ClientIdentity, ClientKind, OwnershipStatus, ProcessHandle, ProcessKind,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn register_process_creates_inventory_record_and_seeds_project_lease() {
        let repo_root = temp_repo_root("register-process");
        let handle = sample_handle(&repo_root, 1001);

        register_process(&repo_root, handle.clone()).expect("register process");

        // Per-process record exists with no owner (inventory only).
        let record = get_record(&repo_root, &handle)
            .expect("load record")
            .expect("record exists");
        assert!(record.owner.is_none());
        assert!(record.viewers.is_empty());

        // Project lease auto-seeded with no owner.
        let lease = project_lease(&repo_root)
            .expect("load lease")
            .expect("lease exists");
        assert!(lease.owner.is_none());

        cleanup(repo_root);
    }

    #[test]
    fn first_client_claims_project_lease_via_any_handle() {
        let repo_root = temp_repo_root("first-owner");
        let handle = sample_handle(&repo_root, 1002);
        register_process(&repo_root, handle.clone()).expect("register process");

        let status =
            claim_owner(&repo_root, &handle, sample_client("client-1")).expect("claim ownership");

        assert_eq!(status, OwnershipStatus::Owner);
        // Per-process record stays inventory.
        let record = get_record(&repo_root, &handle)
            .expect("load record")
            .expect("record exists");
        assert!(record.owner.is_none());
        // Project lease records the owner.
        let lease = project_lease(&repo_root)
            .expect("load lease")
            .expect("lease exists");
        assert_eq!(
            lease.owner.as_ref().map(|o| o.client_id.as_str()),
            Some("client-1")
        );

        cleanup(repo_root);
    }

    #[test]
    fn second_client_becomes_viewer_of_project_lease() {
        let repo_root = temp_repo_root("second-viewer");
        let handle = sample_handle(&repo_root, 1003);
        register_process(&repo_root, handle.clone()).expect("register process");
        claim_owner(&repo_root, &handle, sample_client("client-1")).expect("first owner");

        let status =
            claim_owner(&repo_root, &handle, sample_client("client-2")).expect("claim as viewer");

        assert_eq!(status, OwnershipStatus::Viewer);
        let lease = project_lease(&repo_root)
            .expect("load lease")
            .expect("lease exists");
        assert_eq!(
            lease.owner.as_ref().map(|o| o.client_id.as_str()),
            Some("client-1")
        );
        assert_eq!(lease.viewers.len(), 1);
        assert_eq!(lease.viewers[0].client_id, "client-2");

        cleanup(repo_root);
    }

    #[test]
    fn claim_without_registered_process_still_creates_lease() {
        let repo_root = temp_repo_root("unregistered-claim");
        let handle = sample_handle(&repo_root, 1004);

        let status = claim_owner(&repo_root, &handle, sample_client("client-1"))
            .expect("claim creates lease");

        assert_eq!(status, OwnershipStatus::Owner);
        let lease = project_lease(&repo_root)
            .expect("load lease")
            .expect("lease exists");
        assert_eq!(
            lease.owner.as_ref().map(|o| o.client_id.as_str()),
            Some("client-1")
        );

        cleanup(repo_root);
    }

    #[test]
    fn release_owner_clears_project_lease_owner() {
        let repo_root = temp_repo_root("release-owner");
        let handle = sample_handle(&repo_root, 1005);
        register_process(&repo_root, handle.clone()).expect("register process");
        claim_owner(&repo_root, &handle, sample_client("client-1")).expect("claim");

        release_owner(&repo_root, &handle, "client-1").expect("release owner");

        let lease = project_lease(&repo_root)
            .expect("load lease")
            .expect("lease exists");
        assert!(lease.owner.is_none());

        cleanup(repo_root);
    }

    #[test]
    fn unregister_process_removes_inventory_but_keeps_project_lease() {
        let repo_root = temp_repo_root("unregister-process");
        let handle = sample_handle(&repo_root, 1006);
        register_process(&repo_root, handle.clone()).expect("register process");

        unregister_process(&repo_root, &handle).expect("unregister process");

        assert!(get_record(&repo_root, &handle)
            .expect("load record")
            .is_none());
        // Project lease is preserved (registers persist controller state).
        let records = list_records(&repo_root).expect("list records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].process.kind, ProcessKind::Project);

        cleanup(repo_root);
    }

    #[test]
    fn takeover_accepted_transfers_project_lease_owner() {
        let repo_root = temp_repo_root("takeover-accepted");
        let handle = sample_handle(&repo_root, 1007);
        register_process(&repo_root, handle.clone()).expect("register process");
        claim_owner(&repo_root, &handle, sample_client("owner-1")).expect("claim");
        let request_id =
            request_takeover(&repo_root, &handle, sample_client("requester")).expect("request");

        respond_takeover(&repo_root, &handle, "owner-1", &request_id, true)
            .expect("accept takeover");

        let lease = project_lease(&repo_root)
            .expect("load lease")
            .expect("lease exists");
        assert_eq!(
            lease.owner.as_ref().map(|o| o.client_id.as_str()),
            Some("requester")
        );
        assert!(lease.takeover_request.is_none());

        cleanup(repo_root);
    }

    #[test]
    fn takeover_rejected_keeps_project_lease_owner() {
        let repo_root = temp_repo_root("takeover-rejected");
        let handle = sample_handle(&repo_root, 1008);
        register_process(&repo_root, handle.clone()).expect("register process");
        claim_owner(&repo_root, &handle, sample_client("owner-1")).expect("claim");
        let request_id =
            request_takeover(&repo_root, &handle, sample_client("requester")).expect("request");

        respond_takeover(&repo_root, &handle, "owner-1", &request_id, false)
            .expect("reject takeover");

        let lease = project_lease(&repo_root)
            .expect("load lease")
            .expect("lease exists");
        assert_eq!(
            lease.owner.as_ref().map(|o| o.client_id.as_str()),
            Some("owner-1")
        );
        assert!(lease.takeover_request.is_none());

        cleanup(repo_root);
    }

    fn sample_handle(repo_root: &std::path::Path, pid: i32) -> ProcessHandle {
        ProcessHandle {
            kind: ProcessKind::Coordinator,
            project_root: repo_root.to_path_buf(),
            pid: Some(pid),
        }
    }

    fn sample_client(client_id: &str) -> ClientIdentity {
        let now = "2026-05-21T00:00:00Z".to_string();
        ClientIdentity {
            client_id: client_id.to_string(),
            kind: ClientKind::Cli,
            connected_at: now.clone(),
            last_heartbeat: now,
        }
    }

    fn temp_repo_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "macc-process-ownership-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create temp repo root");
        root
    }

    fn cleanup(repo_root: PathBuf) {
        let _ = fs::remove_dir_all(repo_root);
    }
}

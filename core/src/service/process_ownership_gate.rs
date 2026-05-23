use crate::process_ownership::{ProcessHandle, ProcessKind};
use crate::service::process_ownership;
use crate::{MaccError, Result};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientContext {
    pub client_id: String,
    pub project_root: PathBuf,
}

/// L6-OWN-008: a single project-wide control lease (`ProcessKind::Project`)
/// authoritatively gates all mutating actions for the project.
///
/// Behavior:
/// * If no project lease record exists, the gate falls back to the target
///   process record for backwards compatibility with older per-process stores.
///   If neither record exists, the gate passes.
/// * If the lease exists but has no owner, the gate passes (first-client-takes-control).
/// * If the lease has an owner, only that owner may pass; others get
///   `MaccError::NotProcessOwner`.
///
/// The `handle` is preserved only so the error payload tells callers which
/// process they were trying to mutate.
pub fn gate_owner_action(ctx: &ClientContext, handle: &ProcessHandle) -> Result<()> {
    let project_handle = ProcessHandle {
        kind: ProcessKind::Project,
        project_root: ctx.project_root.clone(),
        pid: None,
    };

    let record = match process_ownership::get_record(&ctx.project_root, &project_handle)? {
        Some(record) => record,
        None => match process_ownership::get_record(&ctx.project_root, handle)? {
            Some(record) => record,
            None => return Ok(()),
        },
    };
    let Some(owner) = record.owner else {
        return Ok(());
    };
    if owner.client_id == ctx.client_id {
        return Ok(());
    }

    Err(MaccError::NotProcessOwner {
        handle: handle.clone(),
        current_owner: Some(owner.client_id),
    })
}

#[cfg(test)]
mod tests {
    use super::{gate_owner_action, ClientContext};
    use crate::process_ownership::{
        ClientIdentity, ClientKind, OwnershipRecord, OwnershipStore, ProcessHandle, ProcessKind,
    };
    use crate::service::process_ownership::{claim_owner, register_process};
    use crate::MaccError;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn gate_passes_when_no_project_lease_exists() {
        let repo_root = temp_repo_root("no-lease");
        let handle = sample_handle(&repo_root, Some(4242));
        let ctx = ClientContext {
            client_id: "client-A".to_string(),
            project_root: repo_root.clone(),
        };
        gate_owner_action(&ctx, &handle).expect("no lease means open mode");
        cleanup(&repo_root);
    }

    #[test]
    fn gate_falls_back_to_legacy_per_process_owner_when_project_lease_is_missing() {
        let repo_root = temp_repo_root("legacy-per-process");
        let handle = sample_handle(&repo_root, Some(4242));
        let owner = sample_client("client-A");
        let viewer_ctx = ClientContext {
            client_id: "client-B".to_string(),
            project_root: repo_root.clone(),
        };
        let owner_ctx = ClientContext {
            client_id: owner.client_id.clone(),
            project_root: repo_root.clone(),
        };

        OwnershipStore::load_and_modify(&repo_root, |store| {
            store.upsert_record(OwnershipRecord {
                process: handle.clone(),
                owner: Some(owner.clone()),
                viewers: Vec::new(),
                takeover_request: None,
                started_at: chrono::Utc::now().to_rfc3339(),
            });
            Ok(())
        })
        .expect("seed legacy per-process owner");

        match gate_owner_action(&viewer_ctx, &handle) {
            Err(MaccError::NotProcessOwner {
                handle,
                current_owner,
            }) => {
                assert_eq!(handle.kind, ProcessKind::Coordinator);
                assert_eq!(current_owner.as_deref(), Some("client-A"));
            }
            other => panic!("expected NotProcessOwner, got {other:?}"),
        }

        gate_owner_action(&owner_ctx, &handle).expect("legacy owner should pass gate");

        cleanup(&repo_root);
    }

    #[test]
    fn project_level_lease_authoritatively_gates_per_process_actions() {
        let repo_root = temp_repo_root("project-lease");
        let coord_handle = sample_handle(&repo_root, Some(4242));
        register_process(&repo_root, coord_handle.clone()).expect("register coord");
        // Any claim routes to the project lease.
        claim_owner(&repo_root, &coord_handle, sample_client("client-A")).expect("claim");

        let viewer_ctx = ClientContext {
            client_id: "client-B".to_string(),
            project_root: repo_root.clone(),
        };
        let owner_ctx = ClientContext {
            client_id: "client-A".to_string(),
            project_root: repo_root.clone(),
        };

        match gate_owner_action(&viewer_ctx, &coord_handle) {
            Err(MaccError::NotProcessOwner {
                handle,
                current_owner,
            }) => {
                assert_eq!(handle.kind, ProcessKind::Coordinator);
                assert_eq!(current_owner.as_deref(), Some("client-A"));
            }
            other => panic!("expected NotProcessOwner, got {other:?}"),
        }

        gate_owner_action(&owner_ctx, &coord_handle).expect("owner should pass gate");

        cleanup(&repo_root);
    }

    fn sample_handle(repo_root: &Path, pid: Option<i32>) -> ProcessHandle {
        ProcessHandle {
            kind: ProcessKind::Coordinator,
            project_root: repo_root.to_path_buf(),
            pid,
        }
    }

    fn sample_client(client_id: &str) -> ClientIdentity {
        let now = chrono::Utc::now().to_rfc3339();
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
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("macc-process-gate-{label}-{unique}"));
        fs::create_dir_all(&root).expect("create temp repo");
        root
    }

    fn cleanup(repo_root: &Path) {
        let _ = fs::remove_dir_all(repo_root);
    }
}

use crate::process_ownership::ProcessHandle;
use crate::service::process_ownership;
use crate::{MaccError, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientContext {
    pub client_id: String,
    pub project_root: PathBuf,
}

pub fn gate_owner_action(ctx: &ClientContext, handle: &ProcessHandle) -> Result<()> {
    let resolved_handle =
        resolve_process_handle(&ctx.project_root, handle)?.unwrap_or_else(|| handle.clone());

    if process_ownership::is_current_owner(&ctx.project_root, &resolved_handle, &ctx.client_id)? {
        return Ok(());
    }

    let current_owner = process_ownership::get_record(&ctx.project_root, &resolved_handle)?
        .and_then(|record| record.owner.map(|owner| owner.client_id));

    Err(MaccError::NotProcessOwner {
        handle: resolved_handle,
        current_owner,
    })
}

fn resolve_process_handle(
    repo_root: &Path,
    handle: &ProcessHandle,
) -> Result<Option<ProcessHandle>> {
    if process_ownership::get_record(repo_root, handle)?.is_some() {
        return Ok(Some(handle.clone()));
    }

    let records = process_ownership::list_records(repo_root)?;
    Ok(records
        .into_iter()
        .find(|record| {
            record.process.kind == handle.kind
                && record.process.project_root == handle.project_root
                && (handle.pid.is_none() || record.process.pid == handle.pid)
        })
        .map(|record| record.process))
}

#[cfg(test)]
mod tests {
    use super::{gate_owner_action, ClientContext};
    use crate::process_ownership::{ClientIdentity, ClientKind, ProcessHandle, ProcessKind};
    use crate::service::process_ownership::{claim_owner, register_process};
    use crate::MaccError;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn gate_rejects_non_owner_and_accepts_owner_for_wildcard_handle() {
        let repo_root = temp_repo_root("owner-gate");
        let registered_handle = sample_handle(&repo_root, Some(4242));
        register_process(&repo_root, registered_handle.clone()).expect("register process");
        claim_owner(&repo_root, &registered_handle, sample_client("client-A"))
            .expect("claim owner");

        let wildcard_handle = sample_handle(&repo_root, None);
        let viewer_ctx = ClientContext {
            client_id: "client-B".to_string(),
            project_root: repo_root.clone(),
        };
        let owner_ctx = ClientContext {
            client_id: "client-A".to_string(),
            project_root: repo_root.clone(),
        };

        match gate_owner_action(&viewer_ctx, &wildcard_handle) {
            Err(MaccError::NotProcessOwner {
                handle,
                current_owner,
            }) => {
                assert_eq!(handle, registered_handle);
                assert_eq!(current_owner.as_deref(), Some("client-A"));
            }
            other => panic!("expected NotProcessOwner, got {other:?}"),
        }

        gate_owner_action(&owner_ctx, &wildcard_handle).expect("owner should pass gate");

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

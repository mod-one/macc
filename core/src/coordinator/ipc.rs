use crate::coordinator::control_plane::CoordinatorLog;
use crate::coordinator::runtime::{
    raw_event_identity, raw_event_to_runtime_event, CoordinatorRunState,
};
use crate::coordinator_storage;
use crate::{MaccError, ProjectPaths, Result};
use std::fs;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

pub const COORDINATOR_IPC_ADDR_ENV: &str = "MACC_COORDINATOR_IPC_ADDR";
pub const COORDINATOR_IPC_ADDR_REL_PATH: &str = ".macc/state/coordinator.ipc.addr";

fn performer_ipc_addr_path(repo_root: &Path) -> std::path::PathBuf {
    repo_root.join(COORDINATOR_IPC_ADDR_REL_PATH)
}

fn write_performer_ipc_addr(repo_root: &Path, addr: &str) -> Result<()> {
    let path = performer_ipc_addr_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| MaccError::Io {
            path: parent.to_string_lossy().into(),
            action: "create coordinator ipc state dir".into(),
            source: e,
        })?;
    }
    fs::write(&path, addr).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "write coordinator ipc addr".into(),
        source: e,
    })?;
    Ok(())
}

pub fn read_performer_ipc_addr(repo_root: &Path) -> Option<String> {
    let path = performer_ipc_addr_path(repo_root);
    let raw = fs::read_to_string(path).ok()?;
    let addr = raw.trim();
    if addr.is_empty() {
        None
    } else {
        Some(addr.to_string())
    }
}

pub async fn ensure_performer_ipc_listener(
    repo_root: &Path,
    state: &mut CoordinatorRunState,
    logger: Option<&dyn CoordinatorLog>,
) -> Result<()> {
    if state.performer_ipc_listener_started {
        return Ok(());
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| {
        MaccError::Validation(format!("Failed to bind coordinator IPC listener: {}", e))
    })?;
    let local_addr = listener.local_addr().map_err(|e| {
        MaccError::Validation(format!("Failed to resolve coordinator IPC address: {}", e))
    })?;
    let addr = local_addr.to_string();
    let runtime_event_bus_tx = state.runtime_event_bus_tx.clone();
    let project_paths = ProjectPaths::from_root(repo_root);

    tokio::spawn(async move {
        loop {
            let accepted = listener.accept().await;
            let (stream, _) = match accepted {
                Ok(pair) => pair,
                Err(err) => {
                    tracing::warn!("coordinator IPC accept failed: {}", err);
                    break;
                }
            };
            let runtime_event_bus_tx = runtime_event_bus_tx.clone();
            let project_paths = project_paths.clone();
            tokio::spawn(async move {
                if let Err(err) =
                    handle_ipc_connection(stream, &project_paths, &runtime_event_bus_tx).await
                {
                    tracing::warn!("coordinator IPC connection failed: {}", err);
                }
            });
        }
    });

    state.performer_ipc_addr = Some(addr.clone());
    state.performer_ipc_listener_started = true;
    write_performer_ipc_addr(repo_root, &addr)?;
    if let Some(log) = logger {
        let _ = log.note(format!("- Performer IPC listener ready addr={}", addr));
    }
    Ok(())
}

async fn handle_ipc_connection(
    stream: TcpStream,
    project_paths: &ProjectPaths,
    runtime_event_bus_tx: &tokio::sync::broadcast::Sender<
        crate::coordinator::runtime::CoordinatorRuntimeEvent,
    >,
) -> Result<()> {
    let reader = BufReader::new(stream);
    let mut lines = reader.lines();
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| MaccError::Validation(format!("Failed to read IPC event line: {}", e)))?
    {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event = serde_json::from_str::<crate::coordinator::CoordinatorEventRecord>(trimmed)
            .map_err(|e| {
                MaccError::Validation(format!("Failed to parse IPC event payload as JSON: {}", e))
            })?;
        if raw_event_identity(&event).is_none() {
            continue;
        }
        let _ = coordinator_storage::append_event_record_sqlite(project_paths, &event);
        if let Some(runtime_event) = raw_event_to_runtime_event(&event) {
            let _ = runtime_event_bus_tx.send(runtime_event);
        }
    }
    Ok(())
}

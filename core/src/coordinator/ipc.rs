use crate::coordinator::control_plane::CoordinatorLog;
use crate::coordinator::runtime::{
    raw_event_identity, raw_event_to_runtime_event, CoordinatorRunState,
};
use crate::coordinator_storage;
use crate::{MaccError, ProjectPaths, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

pub const COORDINATOR_IPC_ADDR_ENV: &str = "MACC_COORDINATOR_IPC_ADDR";
pub const COORDINATOR_IPC_ADDR_REL_PATH: &str = ".macc/state/coordinator.ipc.addr";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PerformerIpcAck {
    ok: bool,
    event_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

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
    // If the listener was started but has since died, reset so we can restart it.
    if state.performer_ipc_listener_started
        && !state
            .performer_ipc_listener_alive
            .load(std::sync::atomic::Ordering::Relaxed)
    {
        if let Some(log) = logger {
            let _ = log.note(
                "- WARNING: IPC listener died, restarting...".to_string(),
            );
        }
        tracing::warn!("coordinator IPC listener died, restarting");
        state.performer_ipc_listener_started = false;
    }
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
    let alive_flag = state.performer_ipc_listener_alive.clone();
    alive_flag.store(true, std::sync::atomic::Ordering::Relaxed);

    tokio::spawn({
        let alive_flag = alive_flag.clone();
        async move {
        // Ensure the alive flag is cleared when this task exits for any reason.
        struct AliveGuard(std::sync::Arc<std::sync::atomic::AtomicBool>);
        impl Drop for AliveGuard {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::Relaxed);
                tracing::warn!("coordinator IPC listener task exiting");
            }
        }
        let _guard = AliveGuard(alive_flag);
        let mut consecutive_errors: usize = 0;
        const MAX_CONSECUTIVE_ACCEPT_ERRORS: usize = 50;
        loop {
            let accepted = listener.accept().await;
            let (stream, _) = match accepted {
                Ok(pair) => {
                    consecutive_errors = 0;
                    pair
                }
                Err(err) => {
                    consecutive_errors += 1;
                    tracing::warn!(
                        consecutive = consecutive_errors,
                        "coordinator IPC accept failed: {}", err
                    );
                    if consecutive_errors >= MAX_CONSECUTIVE_ACCEPT_ERRORS {
                        tracing::error!(
                            "coordinator IPC listener giving up after {} consecutive accept errors",
                            consecutive_errors
                        );
                        break;
                    }
                    // Back off briefly before retrying to avoid a tight error loop.
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
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
    }});

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
    let (read_half, mut write_half) = stream.into_split();
    let reader = BufReader::new(read_half);
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
        let ack = process_ipc_event(trimmed, project_paths, runtime_event_bus_tx);
        write_ipc_ack(&mut write_half, &ack)
            .await
            .map_err(|e| MaccError::Validation(format!("Failed to write IPC ack: {}", e)))?;
        if !ack.ok {
            return Err(MaccError::Validation(
                ack.error
                    .unwrap_or_else(|| "Rejected performer IPC event".to_string()),
            ));
        }
    }
    Ok(())
}

fn process_ipc_event(
    raw_line: &str,
    project_paths: &ProjectPaths,
    runtime_event_bus_tx: &tokio::sync::broadcast::Sender<
        crate::coordinator::runtime::CoordinatorRuntimeEvent,
    >,
) -> PerformerIpcAck {
    let event = match serde_json::from_str::<crate::coordinator::CoordinatorEventRecord>(raw_line) {
        Ok(event) => event,
        Err(err) => {
            return PerformerIpcAck {
                ok: false,
                event_id: String::new(),
                error: Some(format!(
                    "Failed to parse IPC event payload as JSON: {}",
                    err
                )),
            };
        }
    };
    let event_id = event.event_id.clone();
    if let Err(err) = event.validate_performer_runtime_event() {
        return PerformerIpcAck {
            ok: false,
            event_id,
            error: Some(format!("Rejected performer IPC event: {}", err)),
        };
    }
    if raw_event_identity(&event).is_none() {
        return PerformerIpcAck {
            ok: false,
            event_id,
            error: Some("Rejected performer IPC event: missing identity".to_string()),
        };
    }
    if let Err(err) = coordinator_storage::append_event_record_sqlite(project_paths, &event) {
        return PerformerIpcAck {
            ok: false,
            event_id,
            error: Some(format!(
                "Failed to persist performer IPC event to SQLite: {}",
                err
            )),
        };
    }
    if let Some(runtime_event) = raw_event_to_runtime_event(&event) {
        let _ = runtime_event_bus_tx.send(runtime_event);
    }
    PerformerIpcAck {
        ok: true,
        event_id,
        error: None,
    }
}

async fn write_ipc_ack<W>(writer: &mut W, ack: &PerformerIpcAck) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let line = serde_json::to_string(ack)
        .map_err(|err| std::io::Error::other(format!("serialize IPC ack: {}", err)))?;
    writer.write_all(line.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::process_ipc_event;
    use crate::coordinator_storage::CoordinatorStorage;
    use crate::ProjectPaths;
    use std::fs;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "macc-ipc-{}-{}",
            label,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ))
    }

    fn build_shell_event_line(
        event_id: &str,
        source: &str,
        task_id: &str,
        event_type: &str,
        phase: &str,
        status: &str,
        payload_json: &str,
    ) -> String {
        let output = Command::new("bash")
            .arg("-lc")
            .arg(
                r#"
jq -nc \
  --arg schema_version "1" \
  --arg event_id "$EVENT_ID" \
  --arg run_id "run-test" \
  --argjson seq 1 \
  --arg ts "2026-03-16T00:00:00Z" \
  --arg source "$SOURCE" \
  --arg task_id "$TASK_ID" \
  --arg type "$EVENT_TYPE" \
  --arg phase "$PHASE" \
  --arg status "$STATUS" \
  --argjson payload "$PAYLOAD_JSON" \
  '({
    schema_version:$schema_version,
    event_id:$event_id,
    run_id:$run_id,
    seq:$seq,
    ts:$ts,
    source:$source,
    task_id:$task_id,
    type:$type,
    phase:($phase|select(length>0)),
    status:$status,
    payload:$payload
  })'
"#,
            )
            .env("EVENT_ID", event_id)
            .env("SOURCE", source)
            .env("TASK_ID", task_id)
            .env("EVENT_TYPE", event_type)
            .env("PHASE", phase)
            .env("STATUS", status)
            .env("PAYLOAD_JSON", payload_json)
            .output()
            .expect("build shell event line");
        assert!(
            output.status.success(),
            "shell event build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("utf8 event line")
            .trim()
            .to_string()
    }

    #[test]
    fn process_ipc_event_accepts_valid_started_event() {
        let root = temp_root("started");
        fs::create_dir_all(&root).expect("create root");
        let paths = ProjectPaths::from_root(&root);
        let (tx, _) = tokio::sync::broadcast::channel(16);
        let ack = process_ipc_event(
            &serde_json::json!({
                "schema_version": "1",
                "event_id": "evt-1",
                "ts": "2026-03-15T00:00:00Z",
                "source": "coordinator-worktree:T1:1",
                "task_id": "T1",
                "type": "started",
                "phase": "dev",
                "status": "started",
                "payload": { "tool": "codex", "worktree": "/tmp/wt" }
            })
            .to_string(),
            &paths,
            &tx,
        );
        assert!(ack.ok);
        assert_eq!(ack.event_id, "evt-1");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn process_ipc_event_rejects_success_phase_result_without_result_kind() {
        let root = temp_root("phase-result");
        fs::create_dir_all(&root).expect("create root");
        let paths = ProjectPaths::from_root(&root);
        let (tx, _) = tokio::sync::broadcast::channel(16);
        let ack = process_ipc_event(
            &serde_json::json!({
                "schema_version": "1",
                "event_id": "evt-2",
                "ts": "2026-03-15T00:00:00Z",
                "source": "coordinator-worktree:T1:1",
                "task_id": "T1",
                "type": "phase_result",
                "phase": "dev",
                "status": "done",
                "payload": { "attempt": 1 }
            })
            .to_string(),
            &paths,
            &tx,
        );
        assert!(!ack.ok);
        assert_eq!(ack.event_id, "evt-2");
        assert!(ack
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("payload.result_kind"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn process_ipc_phase_result_persists_to_sqlite() {
        let root = temp_root("phase-result-sqlite");
        fs::create_dir_all(&root).expect("create root");
        let paths = ProjectPaths::from_root(&root);
        let (tx, _) = tokio::sync::broadcast::channel(16);
        let ack = process_ipc_event(
            &serde_json::json!({
                "schema_version": "1",
                "event_id": "evt-3",
                "ts": "2026-03-15T00:00:00Z",
                "source": "coordinator-worktree:T9:1",
                "task_id": "T9",
                "type": "phase_result",
                "phase": "dev",
                "status": "done",
                "payload": { "attempt": 1, "result_kind": "already_satisfied", "message": "ok" }
            })
            .to_string(),
            &paths,
            &tx,
        );
        assert!(ack.ok);
        let storage_paths =
            crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&paths);
        let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);
        let snapshot = sqlite.load_snapshot().expect("load snapshot");
        let event = snapshot
            .events
            .iter()
            .find(|event| event.event_id == "evt-3")
            .expect("persisted event");
        assert_eq!(event.event_type, "phase_result");
        assert_eq!(event.task_id.as_deref(), Some("T9"));
        assert_eq!(
            event.payload_result_kind(),
            Some(crate::coordinator::PerformerCompletionKind::AlreadySatisfied)
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn process_ipc_event_accepts_shell_built_started_event() {
        let root = temp_root("shell-started");
        fs::create_dir_all(&root).expect("create root");
        let paths = ProjectPaths::from_root(&root);
        let (tx, _) = tokio::sync::broadcast::channel(16);
        let event_line = build_shell_event_line(
            "evt-shell-started",
            "coordinator-worktree:T1:1",
            "T1",
            "started",
            "dev",
            "started",
            r#"{"tool":"codex","worktree":"/tmp/wt"}"#,
        );
        let ack = process_ipc_event(&event_line, &paths, &tx);
        assert!(ack.ok, "shell started event rejected: {:?}", ack.error);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn process_ipc_event_accepts_shell_built_phase_result_event() {
        let root = temp_root("shell-phase-result");
        fs::create_dir_all(&root).expect("create root");
        let paths = ProjectPaths::from_root(&root);
        let (tx, _) = tokio::sync::broadcast::channel(16);
        let event_line = build_shell_event_line(
            "evt-shell-phase-result",
            "coordinator-worktree:T2:1",
            "T2",
            "phase_result",
            "dev",
            "done",
            r#"{"attempt":1,"result_kind":"already_satisfied","message":"ok"}"#,
        );
        let ack = process_ipc_event(&event_line, &paths, &tx);
        assert!(ack.ok, "shell phase_result event rejected: {:?}", ack.error);
        let storage_paths =
            crate::coordinator_storage::CoordinatorStoragePaths::from_project_paths(&paths);
        let sqlite = crate::coordinator_storage::SqliteStorage::new(storage_paths);
        let snapshot = sqlite.load_snapshot().expect("load snapshot");
        let event = snapshot
            .events
            .iter()
            .find(|event| event.event_id == "evt-shell-phase-result")
            .expect("persisted event");
        assert_eq!(
            event.payload_result_kind(),
            Some(crate::coordinator::PerformerCompletionKind::AlreadySatisfied)
        );
        let _ = fs::remove_dir_all(root);
    }
}

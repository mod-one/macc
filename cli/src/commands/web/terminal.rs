use super::errors::ApiError;
use super::WebState;
use axum::body::Bytes;
use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    Path, State,
};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use futures_util::StreamExt;
use macc_core::service::worktree::{canonicalize_path_fallback, resolve_worktree_path};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Component, Path as StdPath, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Semaphore};
use tracing::{info, warn};

const DEFAULT_MAX_SESSIONS: usize = 5;
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub(super) struct TerminalSessionStore {
    inner: Arc<TerminalSessionStoreInner>,
}

struct TerminalSessionStoreInner {
    sessions: Mutex<HashMap<String, Arc<TerminalSession>>>,
    session_slots: Arc<Semaphore>,
    idle_timeout: Duration,
    next_id: AtomicU64,
}

impl Default for TerminalSessionStore {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_SESSIONS, DEFAULT_IDLE_TIMEOUT)
    }
}

impl TerminalSessionStore {
    pub(super) fn new(max_sessions: usize, idle_timeout: Duration) -> Self {
        Self {
            inner: Arc::new(TerminalSessionStoreInner {
                sessions: Mutex::new(HashMap::new()),
                session_slots: Arc::new(Semaphore::new(max_sessions)),
                idle_timeout,
                next_id: AtomicU64::new(1),
            }),
        }
    }

    pub(super) fn create(
        &self,
        session_type: TerminalTargetType,
        terminal_dir: PathBuf,
        worktree_id: Option<String>,
    ) -> Result<TerminalSessionCreated, ApiError> {
        let permit = self.inner.session_slots.clone().try_acquire_owned().map_err(|_| {
            ApiError::terminal_conflict(
                "Terminal session limit reached",
                Some(json!({
                    "limit": DEFAULT_MAX_SESSIONS,
                    "requested": session_type.as_str(),
                })),
            )
        })?;

        let session_id = self.next_session_id();
        let session = TerminalSession::spawn(
            session_id.clone(),
            session_type,
            terminal_dir.clone(),
            worktree_id.clone(),
            permit,
        )
        .map_err(|err: Box<dyn std::error::Error + Send + Sync>| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "MACC-WEB-4002",
                "Terminal",
                format!("Failed to start terminal session: {}", err),
                true,
                Some("Retry after checking the local shell and terminal directory".to_string()),
                Some(json!({
                    "terminalType": session_type.as_str(),
                    "path": terminal_dir,
                    "worktreeId": worktree_id,
                })),
                Some(err.to_string()),
            )
        })?;

        self.inner
            .sessions
            .lock()
            .expect("terminal sessions lock")
            .insert(session_id.clone(), Arc::new(session));

        let response = TerminalSessionCreated {
            session_id,
            terminal_type: session_type,
            path: terminal_dir,
            worktree_id,
        };

        info!(
            session_id = %response.session_id,
            terminal_type = %response.terminal_type.as_str(),
            path = %response.path.display(),
            worktree_id = ?response.worktree_id,
            "created terminal session"
        );

        self.start_idle_watcher(&response.session_id);
        Ok(response)
    }

    fn get(&self, session_id: &str) -> Option<Arc<TerminalSession>> {
        self.inner
            .sessions
            .lock()
            .expect("terminal sessions lock")
            .get(session_id)
            .cloned()
    }

    pub(super) fn remove(&self, session_id: &str, reason: &str) -> bool {
        let session = self
            .inner
            .sessions
            .lock()
            .expect("terminal sessions lock")
            .remove(session_id);

        if let Some(session) = session {
            session.close(reason);
            true
        } else {
            false
        }
    }

    fn next_session_id(&self) -> String {
        let counter = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("term-{:x}-{:x}", nanos, counter)
    }

    fn start_idle_watcher(&self, session_id: &str) {
        let inner = Arc::downgrade(&self.inner);
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(IDLE_CHECK_INTERVAL).await;
                let Some(inner) = inner.upgrade() else {
                    break;
                };
                let Some(session) = inner
                    .sessions
                    .lock()
                    .expect("terminal sessions lock")
                    .get(&session_id)
                    .cloned()
                else {
                    break;
                };
                if session.is_closed() {
                    break;
                }
                if session.idle_for() >= inner.idle_timeout {
                    warn!(
                        session_id = %session_id,
                        idle_timeout_secs = inner.idle_timeout.as_secs(),
                        "terminal session idle timeout"
                    );
                    let store = TerminalSessionStore { inner };
                    store.remove(&session_id, "idle timeout");
                    break;
                }
            }
        });
    }
}

impl Drop for TerminalSessionStore {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) != 1 {
            return;
        }
        let mut sessions = self.inner.sessions.lock().expect("terminal sessions lock");
        let sessions = std::mem::take(&mut *sessions);
        for (_session_id, session) in sessions {
            session.close("terminal store dropped");
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum TerminalTargetType {
    Project,
    Worktree,
}

impl TerminalTargetType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Worktree => "worktree",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTerminalRequest {
    terminal_type: TerminalTargetType,
    worktree_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TerminalSessionCreated {
    session_id: String,
    terminal_type: TerminalTargetType,
    path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    worktree_id: Option<String>,
}

struct TerminalSession {
    session_id: String,
    terminal_type: TerminalTargetType,
    path: PathBuf,
    worktree_id: Option<String>,
    child: Arc<Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>>,
    reader: Arc<Mutex<Option<Box<dyn Read + Send>>>>,
    writer: Arc<Mutex<Option<Box<dyn Write + Send>>>>,
    _permit: tokio::sync::OwnedSemaphorePermit,
    closed: AtomicBool,
    attached: AtomicBool,
    last_activity: Arc<Mutex<Instant>>,
}

impl TerminalSession {
    fn spawn(
        session_id: String,
        terminal_type: TerminalTargetType,
        path: PathBuf,
        worktree_id: Option<String>,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = CommandBuilder::new("bash");
        cmd.cwd(&path);
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd)?;
        let reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        Ok(Self {
            session_id,
            terminal_type,
            path,
            worktree_id,
            child: Arc::new(Mutex::new(Some(child))),
            reader: Arc::new(Mutex::new(Some(reader))),
            writer: Arc::new(Mutex::new(Some(writer))),
            _permit: permit,
            closed: AtomicBool::new(false),
            attached: AtomicBool::new(false),
            last_activity: Arc::new(Mutex::new(Instant::now())),
        })
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn idle_for(&self) -> Duration {
        Instant::now()
            .checked_duration_since(*self.last_activity.lock().expect("terminal activity lock"))
            .unwrap_or_default()
    }

    fn mark_activity(&self) {
        *self.last_activity.lock().expect("terminal activity lock") = Instant::now();
    }

    fn begin_attach(&self) -> Result<TerminalAttachment, ApiError> {
        if self
            .attached
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(ApiError::terminal_conflict(
                "Terminal session is already attached",
                Some(json!({
                    "sessionId": self.session_id,
                })),
            ));
        }

        let reader = self.reader.lock().expect("terminal reader lock").take();
        let writer = self.writer.lock().expect("terminal writer lock").take();
        let Some(reader) = reader else {
            self.attached.store(false, Ordering::SeqCst);
            self.close("terminal attachment failed");
            return Err(ApiError::terminal_not_found(
                "Terminal session is no longer available",
                Some(json!({
                    "sessionId": self.session_id,
                })),
            ));
        };
        let Some(writer) = writer else {
            self.attached.store(false, Ordering::SeqCst);
            self.close("terminal attachment failed");
            return Err(ApiError::terminal_not_found(
                "Terminal session is no longer available",
                Some(json!({
                    "sessionId": self.session_id,
                })),
            ));
        };

        self.mark_activity();
        Ok(TerminalAttachment {
            reader,
            writer,
            activity: self.last_activity.clone(),
        })
    }

    fn close(&self, reason: &str) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }

        let session_id = self.session_id.clone();
        let child = Arc::clone(&self.child);
        let reader = Arc::clone(&self.reader);
        let writer = Arc::clone(&self.writer);
        info!(
            session_id = %session_id,
            terminal_type = %self.terminal_type.as_str(),
            path = %self.path.display(),
            worktree_id = ?self.worktree_id,
            reason,
            "closing terminal session"
        );
        std::thread::spawn(move || {
            reader.lock().expect("terminal reader lock").take();
            writer.lock().expect("terminal writer lock").take();
            if let Some(mut child) = child.lock().expect("terminal child lock").take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        });
    }
}

struct TerminalAttachment {
    reader: Box<dyn Read + Send>,
    writer: Box<dyn Write + Send>,
    activity: Arc<Mutex<Instant>>,
}

pub(super) async fn create_terminal_handler(
    State(state): State<WebState>,
    body: Bytes,
) -> std::result::Result<(StatusCode, Json<TerminalSessionCreated>), ApiError> {
    let request: CreateTerminalRequest = serde_json::from_slice(&body).map_err(|err| {
        ApiError::validation(format!("Invalid terminal create request body: {}", err))
    })?;

    let terminal = resolve_terminal_target(&state, &request)?;
    let created = state.terminal_sessions.create(
        request.terminal_type,
        terminal.path,
        terminal.worktree_id,
    )?;
    Ok((StatusCode::CREATED, Json(created)))
}

pub(super) async fn terminal_ws_handler(
    State(state): State<WebState>,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> std::result::Result<Response, ApiError> {
    let Some(session) = state.terminal_sessions.get(&session_id) else {
        return Err(ApiError::terminal_not_found(
            format!("terminal session '{}' was not found", session_id),
            Some(json!({ "sessionId": session_id })),
        ));
    };

    if session.is_closed() {
        return Err(ApiError::terminal_not_found(
            format!("terminal session '{}' is closed", session_id),
            Some(json!({ "sessionId": session_id })),
        ));
    }

    let attachment = session.begin_attach()?;

    Ok(ws.on_upgrade(move |socket| {
        let state = state.clone();
        async move {
            if let Err(err) =
                handle_terminal_socket(state, session_id.clone(), session, attachment, socket).await
            {
                warn!(session_id = %session_id, error = ?err, "terminal websocket closed with error");
            }
        }
    }))
}

async fn handle_terminal_socket(
    state: WebState,
    session_id: String,
    session: Arc<TerminalSession>,
    attachment: TerminalAttachment,
    socket: WebSocket,
) -> std::result::Result<(), ApiError> {
    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (input_tx, input_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let TerminalAttachment {
        reader,
        writer,
        activity,
    } = attachment;
    let session_for_reader = Arc::clone(&session);
    let session_for_writer = Arc::clone(&session);
    let writer_session_id = session_id.clone();
    let reader_activity = Arc::clone(&activity);
    let writer_activity = Arc::clone(&activity);

    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    *reader_activity.lock().expect("terminal activity lock") = Instant::now();
                    if output_tx.send(buffer[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    warn!(session_id = %session_for_reader.session_id, error = ?err, "terminal reader stopped");
                    break;
                }
            }
        }
    });

    std::thread::spawn(move || {
        let mut writer = writer;
        while let Ok(chunk) = input_rx.recv() {
            *writer_activity.lock().expect("terminal activity lock") = Instant::now();
            if let Err(err) = writer.write_all(&chunk) {
                warn!(session_id = %writer_session_id, error = ?err, "terminal writer stopped");
                break;
            }
            let _ = writer.flush();
        }
    });

    let result = terminal_socket_loop(&mut output_rx, socket, input_tx, session_for_writer).await;
    state.terminal_sessions.remove(&session_id, "websocket detached");
    result
}

async fn terminal_socket_loop(
    output_rx: &mut mpsc::UnboundedReceiver<Vec<u8>>,
    mut socket: WebSocket,
    input_tx: std::sync::mpsc::Sender<Vec<u8>>,
    session: Arc<TerminalSession>,
) -> std::result::Result<(), ApiError> {
    loop {
        tokio::select! {
            maybe_output = output_rx.recv() => {
                match maybe_output {
                    Some(chunk) => {
                        session.mark_activity();
                        socket
                            .send(Message::Binary(chunk))
                            .await
                            .map_err(|err| ApiError::validation(format!("Failed to send terminal output: {}", err)))?;
                    }
                    None => break,
                }
            }
            maybe_message = socket.next() => {
                match maybe_message {
                    Some(Ok(Message::Text(text))) => {
                        session.mark_activity();
                        input_tx.send(text.into_bytes()).map_err(|_| ApiError::terminal_not_found(
                            "Terminal session is no longer available",
                            Some(json!({ "sessionId": session.session_id })),
                        ))?;
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        session.mark_activity();
                        input_tx.send(bytes).map_err(|_| ApiError::terminal_not_found(
                            "Terminal session is no longer available",
                            Some(json!({ "sessionId": session.session_id })),
                        ))?;
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        socket
                            .send(Message::Pong(payload))
                            .await
                            .map_err(|err| ApiError::validation(format!("Failed to respond to terminal ping: {}", err)))?;
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) => break,
                    Some(Err(err)) => {
                        warn!(session_id = %session.session_id, error = ?err, "terminal websocket receive error");
                        break;
                    }
                    None => break,
                }
            }
        }
    }
    Ok(())
}

struct ResolvedTerminalTarget {
    path: PathBuf,
    worktree_id: Option<String>,
}

fn resolve_terminal_target(
    state: &WebState,
    request: &CreateTerminalRequest,
) -> std::result::Result<ResolvedTerminalTarget, ApiError> {
    match request.terminal_type {
        TerminalTargetType::Project => {
            let path = canonical_project_root(&state.paths.root)?;
            Ok(ResolvedTerminalTarget {
                path,
                worktree_id: None,
            })
        }
        TerminalTargetType::Worktree => {
            let Some(worktree_id) = request.worktree_id.as_ref() else {
                return Err(ApiError::validation(
                    "worktreeId is required when terminalType is worktree",
                ));
            };
            validate_worktree_id(worktree_id)?;
            let path = resolve_worktree_path(&state.paths.root, worktree_id)?;
            let canonical = canonicalize_path_fallback(&path);
            let entries = state
                .engine
                .list_worktrees(&state.paths.root)
                .map_err(ApiError::from)?;
            let exists = entries
                .iter()
                .any(|entry| canonicalize_path_fallback(&entry.path) == canonical);
            if !exists {
                return Err(ApiError::worktree_not_found(
                    format!("worktree '{}' was not found", worktree_id),
                    Some(json!({
                        "worktreeId": worktree_id,
                        "path": canonical,
                    })),
                ));
            }
            Ok(ResolvedTerminalTarget {
                path: canonical,
                worktree_id: Some(worktree_id.clone()),
            })
        }
    }
}

fn canonical_project_root(root: &StdPath) -> std::result::Result<PathBuf, ApiError> {
    if !root.exists() {
        return Err(ApiError::validation(format!(
            "Project root '{}' does not exist",
            root.display()
        )));
    }
    Ok(canonicalize_path_fallback(root))
}

fn validate_worktree_id(worktree_id: &str) -> std::result::Result<(), ApiError> {
    let trimmed = worktree_id.trim();
    if trimmed.is_empty() {
        return Err(ApiError::validation("worktreeId cannot be empty"));
    }
    if StdPath::new(trimmed)
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
    {
        return Err(ApiError::validation(
            "worktreeId must be a simple identifier without path separators",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[tokio::test]
    async fn terminal_attach_conflict_is_rejected_before_upgrade() {
        let store = TerminalSessionStore::new(1, Duration::from_secs(60));
        let root = std::env::temp_dir().join(format!(
            "macc-terminal-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("create root");

        let created = store
            .create(TerminalTargetType::Project, root.clone(), None)
            .expect("create session");
        let session = store.get(&created.session_id).expect("session present");

        assert!(session.begin_attach().is_ok(), "first attach should succeed");

        let conflict = session.begin_attach();
        assert!(conflict.is_err(), "second attach should fail");
        let response = conflict.err().expect("conflict error").into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        store.remove(&created.session_id, "test cleanup");
        let _ = std::fs::remove_dir_all(&root);
    }
}

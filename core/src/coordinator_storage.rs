use crate::coordinator::model::{ResourceLock, Task, TaskRegistry};
use crate::coordinator::{CoordinatorCursor, CoordinatorEventPayload, CoordinatorEventRecord};
use crate::{MaccError, ProjectPaths, Result};
use chrono::Utc;
use rusqlite::{params, Connection, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Stored in SQLite's `user_version` once `init_schema` has run. Bump it
/// whenever `init_schema` gains a table or an index, so existing databases
/// pick the change up.
const SQLITE_SCHEMA_VERSION: i64 = 2;

/// Event types emitted on every performer or coordinator heartbeat. They are
/// most of the event log and never carry a result, an error or a decision.
pub const HEARTBEAT_EVENT_TYPES: [&str; 2] = ["heartbeat", "coordinator_heartbeat"];

/// Which events `SqliteStorage::scan_events_newest_first` visits.
#[derive(Debug, Clone, Copy, Default)]
pub struct EventQuery<'a> {
    /// Only this task's events.
    pub task_id: Option<&'a str>,
    /// Only events of this type.
    pub event_type: Option<&'a str>,
    /// Leave out `HEARTBEAT_EVENT_TYPES`.
    pub skip_heartbeats: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorControl {
    pub mode: String,
    pub requested_at: Option<String>,
    pub requested_by: Option<String>,
    pub drain_snapshot_json: Option<String>,
    pub force_grace_seconds: Option<u64>,
    pub cleanup_after_force: Option<bool>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorRun {
    pub run_id: String,
    pub pid: i64,
    pub hostname: String,
    pub started_at: String,
    pub last_tick_at: Option<String>,
    pub stopped_at: Option<String>,
    pub status: String,
    pub epoch: i64,
    pub version: String,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorStorageMode {
    Json,
    DualWrite,
    Sqlite,
}

impl FromStr for CoordinatorStorageMode {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "dual-write" | "dual_write" => Ok(Self::DualWrite),
            "sqlite" => Ok(Self::Sqlite),
            other => Err(format!(
                "Unknown coordinator storage mode '{}'. Expected json|dual-write|sqlite.",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorStoragePhase {
    Pre,
    Post,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinatorStorageTransfer {
    ImportJsonToSqlite,
    ExportSqliteToJson,
    VerifyParity,
}

impl FromStr for CoordinatorStorageTransfer {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "import" | "json-to-sqlite" | "json_to_sqlite" => Ok(Self::ImportJsonToSqlite),
            "export" | "sqlite-to-json" | "sqlite_to_json" => Ok(Self::ExportSqliteToJson),
            "verify" | "parity" => Ok(Self::VerifyParity),
            other => Err(format!(
                "Unknown coordinator storage transfer '{}'. Expected import|export|verify.",
                other
            )),
        }
    }
}

impl FromStr for CoordinatorStoragePhase {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pre" => Ok(Self::Pre),
            "post" => Ok(Self::Post),
            other => Err(format!(
                "Unknown coordinator storage phase '{}'. Expected pre|post.",
                other
            )),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CoordinatorStoragePaths {
    pub registry_json_path: PathBuf,
    pub events_jsonl_path: PathBuf,
    pub cursor_json_path: PathBuf,
    pub sqlite_path: PathBuf,
}

impl CoordinatorStoragePaths {
    pub fn from_project_paths(paths: &ProjectPaths) -> Self {
        Self {
            registry_json_path: paths
                .root
                .join(".macc")
                .join("automation")
                .join("task")
                .join("task_registry.json"),
            events_jsonl_path: paths
                .root
                .join(".macc")
                .join("log")
                .join("coordinator")
                .join("events.jsonl"),
            cursor_json_path: paths
                .root
                .join(".macc")
                .join("state")
                .join("coordinator.cursor"),
            sqlite_path: paths
                .root
                .join(".macc")
                .join("state")
                .join("coordinator.sqlite"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinatorSnapshot {
    pub registry: TaskRegistry,
    pub events: Vec<CoordinatorEventRecord>,
    pub cursor: Option<CoordinatorCursor>,
}

impl CoordinatorSnapshot {
    pub fn empty() -> Self {
        Self {
            registry: default_registry(),
            events: Vec::new(),
            cursor: None,
        }
    }
}

pub trait CoordinatorStorage {
    fn load_snapshot(&self) -> Result<CoordinatorSnapshot>;
    fn save_snapshot(&self, snapshot: &CoordinatorSnapshot) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct TransitionMutation {
    pub task_id: String,
    pub new_state: String,
    pub pr_url: String,
    pub reviewer: String,
    pub reason: String,
    pub now: String,
}

#[derive(Debug, Clone)]
pub struct RuntimeMutation {
    pub task_id: String,
    pub runtime_status: String,
    pub phase: String,
    pub pid: Option<i64>,
    pub last_error: String,
    pub heartbeat_ts: String,
    pub attempt: Option<i64>,
    pub now: String,
    pub message: Option<String>,
    pub progress: Option<u8>,
    pub worker_id: Option<String>,
    pub tool: Option<String>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub stdout_log: Option<String>,
    pub stderr_log: Option<String>,
    pub events_log: Option<String>,
    pub last_event_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MergePendingMutation {
    pub task_id: String,
    pub result_file: String,
    pub pid: Option<i64>,
    pub now: String,
}

#[derive(Debug, Clone)]
pub struct MergeProcessedMutation {
    pub task_id: String,
    pub result_file: String,
    pub status: String,
    pub rc: Option<i64>,
    pub now: String,
}

#[derive(Debug, Clone)]
pub struct RetryIncrementMutation {
    pub task_id: String,
    pub now: String,
}

#[derive(Debug, Clone)]
pub struct SloWarningMutation {
    pub task_id: String,
    pub metric: String,
    pub threshold: i64,
    pub value: i64,
    pub suggestion: String,
    pub now: String,
}

#[derive(Debug, Clone)]
pub struct EventMutation {
    pub event_id: Option<String>,
    pub run_id: Option<String>,
    pub seq: Option<i64>,
    pub ts: Option<String>,
    pub source: String,
    pub task_id: String,
    pub event_type: String,
    pub phase: String,
    pub status: String,
    pub payload: CoordinatorEventPayload,
}

/// SQL filter keeping events that a replay cursor still points at: deleting
/// one would make the next replay start over from the first event.
const NOT_A_CURSOR_EVENT: &str =
    "event_id NOT IN (SELECT last_event_id FROM event_cursor WHERE last_event_id IS NOT NULL)
     AND event_id NOT IN (SELECT last_event_id FROM cursors WHERE last_event_id IS NOT NULL)";

fn heartbeat_type_list() -> String {
    HEARTBEAT_EVENT_TYPES
        .iter()
        .map(|t| format!("'{t}'"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A heartbeat only says "still alive at this time": once a newer one of the
/// same kind is stored for a task, the older ones carry nothing. Dropping them
/// keeps the event log from growing for as long as a performer runs.
///
/// "Newer" is arrival order (`rowid`), not `seq`: performers number their own
/// events, so their heartbeats all share one `seq`.
fn prune_superseded_heartbeats(
    tx: &rusqlite::Transaction<'_>,
    event_type: &str,
    task_id: Option<&str>,
    inserted_rowid: i64,
) -> Result<()> {
    if !HEARTBEAT_EVENT_TYPES.contains(&event_type) {
        return Ok(());
    }
    tx.execute(
        &format!(
            "DELETE FROM events
             WHERE task_id IS ?1 AND event_type = ?2 AND rowid < ?3 AND {NOT_A_CURSOR_EVENT}"
        ),
        params![task_id, event_type, inserted_rowid],
    )
    .map_err(sql_err)?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct JsonStorage {
    paths: CoordinatorStoragePaths,
}

impl JsonStorage {
    pub fn new(paths: CoordinatorStoragePaths) -> Self {
        Self { paths }
    }
}

impl CoordinatorStorage for JsonStorage {
    fn load_snapshot(&self) -> Result<CoordinatorSnapshot> {
        let registry = read_registry_or_default(
            &self.paths.registry_json_path,
            "read coordinator registry json",
        )?;

        let mut events = Vec::new();
        if self.paths.events_jsonl_path.exists() {
            let raw =
                fs::read_to_string(&self.paths.events_jsonl_path).map_err(|e| MaccError::Io {
                    path: self.paths.events_jsonl_path.to_string_lossy().into(),
                    action: "read coordinator events jsonl".into(),
                    source: e,
                })?;
            for line in raw.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<CoordinatorEventRecord>(line) {
                    events.push(v);
                }
            }
        }

        let cursor = if self.paths.cursor_json_path.exists() {
            Some(read_json_typed_or_default(
                &self.paths.cursor_json_path,
                "read coordinator cursor json",
                CoordinatorCursor::default(),
            )?)
        } else {
            None
        };

        Ok(CoordinatorSnapshot {
            registry,
            events,
            cursor,
        })
    }

    fn save_snapshot(&self, snapshot: &CoordinatorSnapshot) -> Result<()> {
        ensure_parent_dir(&self.paths.registry_json_path)?;
        ensure_parent_dir(&self.paths.events_jsonl_path)?;
        ensure_parent_dir(&self.paths.cursor_json_path)?;

        write_json_atomic(&self.paths.registry_json_path, &snapshot.registry)?;

        let mut events_buf = String::new();
        for event in &snapshot.events {
            let line = serde_json::to_string(event).map_err(|e| MaccError::Storage {
                backend: "json",
                message: format!("Failed to serialize event json: {}", e),
            })?;
            events_buf.push_str(&line);
            events_buf.push('\n');
        }
        write_text_atomic(&self.paths.events_jsonl_path, &events_buf)?;

        match &snapshot.cursor {
            Some(cursor) => write_json_atomic(&self.paths.cursor_json_path, cursor)?,
            None => {
                if self.paths.cursor_json_path.exists() {
                    fs::remove_file(&self.paths.cursor_json_path).map_err(|e| MaccError::Io {
                        path: self.paths.cursor_json_path.to_string_lossy().into(),
                        action: "remove coordinator cursor json".into(),
                        source: e,
                    })?;
                }
            }
        }
        Ok(())
    }
}

/// Idle connections kept per thread and per database path. Opening one costs
/// a file open plus the WAL and busy-timeout pragmas, and every storage call
/// used to pay it.
const MAX_IDLE_CONNECTIONS: usize = 4;

type FileIdentity = (u64, u64);

thread_local! {
    static CONNECTION_POOL: std::cell::RefCell<HashMap<PathBuf, Vec<(FileIdentity, Connection)>>> =
        std::cell::RefCell::new(HashMap::new());
}

fn database_file_identity(path: &Path) -> Option<FileIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        // Without an inode, never reuse: a replaced file would go unnoticed.
        let _ = path;
        None
    }
}

/// A coordinator database connection that goes back to its thread's pool
/// when dropped. It dereferences to `rusqlite::Connection`.
pub struct PooledConnection {
    conn: Option<Connection>,
    path: PathBuf,
    identity: Option<FileIdentity>,
}

impl std::ops::Deref for PooledConnection {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn.as_ref().expect("connection present until drop")
    }
}

impl std::ops::DerefMut for PooledConnection {
    fn deref_mut(&mut self) -> &mut Connection {
        self.conn.as_mut().expect("connection present until drop")
    }
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        let (Some(conn), Some(identity)) = (self.conn.take(), self.identity) else {
            return;
        };
        // A connection left inside a transaction is not reusable.
        if !conn.is_autocommit() {
            return;
        }
        let path = std::mem::take(&mut self.path);
        let _ = CONNECTION_POOL.try_with(|pool| {
            let mut pool = pool.borrow_mut();
            let idle = pool.entry(path).or_default();
            if idle.len() < MAX_IDLE_CONNECTIONS {
                idle.push((identity, conn));
            }
        });
    }
}

#[derive(Debug, Clone)]
pub struct SqliteStorage {
    pub paths: CoordinatorStoragePaths,
}

impl SqliteStorage {
    pub fn new(paths: CoordinatorStoragePaths) -> Self {
        Self { paths }
    }

    pub fn has_snapshot_data(&self) -> Result<bool> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let registry_meta_exists: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM metadata WHERE key='registry_json'",
                [],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        let task_count: i64 = conn
            .query_row("SELECT COUNT(1) FROM tasks", [], |row| row.get(0))
            .map_err(sql_err)?;
        Ok(registry_meta_exists > 0 || task_count > 0)
    }

    pub fn append_event(&self, event: &Value) -> Result<bool> {
        let record =
            CoordinatorEventRecord::from_value(event.clone()).map_err(|e| MaccError::Storage {
                backend: "sqlite",
                message: e,
            })?;
        self.append_event_record(&record)
    }

    pub fn append_event_record(&self, event: &CoordinatorEventRecord) -> Result<bool> {
        let mut conn = self.open()?;
        self.init_schema(&conn)?;
        let tx = write_transaction(&mut conn)?;
        let inserted = self.append_event_record_in_tx(&tx, event)?;
        tx.commit().map_err(sql_err)?;
        Ok(inserted)
    }

    fn append_event_record_in_tx(
        &self,
        tx: &rusqlite::Transaction<'_>,
        event: &CoordinatorEventRecord,
    ) -> Result<bool> {
        let now = now_iso_string();
        let seq = if event.seq == 0 {
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        } else {
            event.seq
        };
        let ts = if event.ts.trim().is_empty() {
            now.as_str()
        } else {
            event.ts.as_str()
        };
        let run_id = event
            .run_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .or_else(|| std::env::var("COORDINATOR_RUN_ID").ok())
            .unwrap_or_else(|| {
                format!(
                    "run-{}-{}",
                    Utc::now().timestamp_nanos_opt().unwrap_or_default(),
                    std::process::id()
                )
            });
        let event_id = if event.event_id.trim().is_empty() {
            format!(
                "evt-{}-{}-{}",
                event.event_type,
                event.task_id.as_deref().unwrap_or_default(),
                seq
            )
        } else {
            event.event_id.clone()
        };
        let mut raw_event = event.clone();
        raw_event.schema_version = if raw_event.schema_version.trim().is_empty() {
            crate::coordinator::COORDINATOR_EVENT_SCHEMA_VERSION.to_string()
        } else {
            raw_event.schema_version
        };
        raw_event.event_id = event_id.clone();
        raw_event.run_id = Some(run_id.clone());
        raw_event.seq = seq;
        raw_event.ts = ts.to_string();
        let payload_raw =
            serde_json::to_string(&raw_event.payload).map_err(|e| MaccError::Storage {
                backend: "sqlite",
                message: format!("Failed to serialize event payload: {}", e),
            })?;
        let raw_json = serde_json::to_string(&raw_event).map_err(|e| MaccError::Storage {
            backend: "sqlite",
            message: format!("Failed to serialize event json: {}", e),
        })?;
        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO events (event_id, seq, ts, source, task_id, event_type, phase, status, payload_json, raw_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    event_id,
                    seq,
                    ts,
                    raw_event.source,
                    raw_event.task_id,
                    raw_event.event_type,
                    raw_event.phase,
                    raw_event.status,
                    payload_raw,
                    raw_json
                ],
            )
            .map_err(sql_err)?;
        if inserted > 0 {
            prune_superseded_heartbeats(
                tx,
                &raw_event.event_type,
                raw_event.task_id.as_deref(),
                tx.last_insert_rowid(),
            )?;
        }
        Ok(inserted > 0)
    }

    fn append_event_in_tx(
        &self,
        tx: &rusqlite::Transaction<'_>,
        event: &EventMutation,
    ) -> Result<bool> {
        let now = now_iso_string();
        let seq = event
            .seq
            .unwrap_or_else(|| Utc::now().timestamp_nanos_opt().unwrap_or_default());
        let ts = event.ts.as_deref().unwrap_or(now.as_str());
        let run_id = event
            .run_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .or_else(|| std::env::var("COORDINATOR_RUN_ID").ok())
            .unwrap_or_else(|| {
                format!(
                    "run-{}-{}",
                    Utc::now().timestamp_nanos_opt().unwrap_or_default(),
                    std::process::id()
                )
            });
        let event_id = event
            .event_id
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("evt-{}-{}-{}", event.event_type, event.task_id, seq));
        let payload_raw =
            serde_json::to_string(event.payload.as_value()).map_err(|e| MaccError::Storage {
                backend: "sqlite",
                message: format!("Failed to serialize event payload: {}", e),
            })?;
        let raw_json = serde_json::to_string(&json!({
            "schema_version":"1",
            "event_id": event_id,
            "run_id": run_id,
            "seq": seq,
            "ts": ts,
            "source": event.source,
            "task_id": event.task_id,
            "type": event.event_type,
            "phase": event.phase,
            "status": event.status,
            "payload": event.payload.as_value()
        }))
        .map_err(|e| MaccError::Storage {
            backend: "sqlite",
            message: format!("Failed to serialize event json: {}", e),
        })?;

        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO events (event_id, seq, ts, source, task_id, event_type, phase, status, payload_json, raw_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    event_id,
                    seq,
                    ts,
                    event.source,
                    event.task_id,
                    event.event_type,
                    event.phase,
                    event.status,
                    payload_raw,
                    raw_json
                ],
            )
            .map_err(sql_err)?;
        if inserted > 0 {
            prune_superseded_heartbeats(
                tx,
                &event.event_type,
                Some(event.task_id.as_str()),
                tx.last_insert_rowid(),
            )?;
        }
        Ok(inserted > 0)
    }

    /// Has this task already reported a terminal `phase_result` for the given
    /// claim?
    ///
    /// The performer's IPC event is persisted here the moment it is received
    /// (see `coordinator::ipc`), well before the control-plane loop drains its
    /// event bus. A task whose result is already on record has finished -- it
    /// must never be treated as a dead process, however long ago it last sent a
    /// heartbeat.
    ///
    /// Scoped by `claim_id` so a result from an earlier attempt cannot vouch for
    /// the current one. Events carrying a stale claim are already rejected at
    /// ingest (E418), so anything stored under this claim genuinely belongs to
    /// it.
    ///
    /// Returns `false` on any read failure: an unreadable event log must not
    /// keep a genuinely dead task alive forever.
    pub fn task_has_terminal_result_for_claim(&self, task_id: &str, claim_id: &str) -> bool {
        if task_id.is_empty() || claim_id.is_empty() {
            return false;
        }
        let Ok(conn) = self.open() else {
            return false;
        };
        let mut stmt = match conn.prepare(
            "SELECT raw_json FROM events WHERE task_id = ?1 AND event_type = 'phase_result'",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return false,
        };
        let Ok(rows) = stmt.query_map(params![task_id], |row| row.get::<_, String>(0)) else {
            return false;
        };
        for raw in rows.flatten() {
            let Ok(value) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            if value.get("claim_id").and_then(Value::as_str) == Some(claim_id) {
                return true;
            }
        }
        false
    }

    /// A connection to the coordinator database, reused across calls on the
    /// same thread when the database file is still the same one.
    pub fn open(&self) -> Result<PooledConnection> {
        ensure_parent_dir(&self.paths.sqlite_path)?;
        let path = self.paths.sqlite_path.clone();
        if let Some(file) = database_file_identity(&path) {
            let reused = CONNECTION_POOL.with(|pool| {
                let mut pool = pool.borrow_mut();
                let idle = pool.get_mut(&path)?;
                // Connections to a database file that was since replaced
                // would read the old file: drop them.
                idle.retain(|(identity, _)| *identity == file);
                idle.pop()
            });
            if let Some((identity, conn)) = reused {
                return Ok(PooledConnection {
                    conn: Some(conn),
                    path,
                    identity: Some(identity),
                });
            }
        }
        let conn = Connection::open(&path).map_err(sql_err)?;
        // WAL mode allows concurrent readers and writers without blocking.
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(sql_err)?;
        // Prevent SQLITE_BUSY errors when multiple performers write concurrently.
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(sql_err)?;
        let identity = database_file_identity(&path);
        Ok(PooledConnection {
            conn: Some(conn),
            path,
            identity,
        })
    }

    pub fn init_schema(&self, conn: &Connection) -> Result<()> {
        // Every storage call opens a connection and lands here. Once a database
        // carries the current schema, skip the DDL batch and the control-row
        // write: on a busy project they cost a write lock on every read.
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(sql_err)?;
        if version >= SQLITE_SCHEMA_VERSION {
            return Ok(());
        }
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS metadata (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tasks (
              task_id TEXT PRIMARY KEY,
              state TEXT,
              title TEXT,
              priority TEXT,
              tool TEXT,
              payload_json TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS task_runtime (
              task_id TEXT NOT NULL,
              claim_id TEXT PRIMARY KEY,
              run_id TEXT NOT NULL,
              coordinator_epoch INTEGER NOT NULL,
              workflow_state TEXT NOT NULL,
              runtime_status TEXT NOT NULL,
              phase TEXT,
              tool TEXT,
              worktree_slot_id TEXT,
              worktree_path TEXT,
              branch TEXT,
              base_branch TEXT,
              pid INTEGER,
              process_group_id INTEGER,
              started_at TEXT,
              updated_at TEXT NOT NULL,
              last_heartbeat_at TEXT,
              heartbeat_seq INTEGER DEFAULT 0,
              lease_expires_at TEXT,
              attempt INTEGER DEFAULT 0,
              locked_resources_json TEXT,
              last_error_code TEXT,
              last_error_message TEXT,
              payload_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS resource_locks (
              resource TEXT NOT NULL,
              task_id TEXT NOT NULL,
              claim_id TEXT NOT NULL,
              acquired_at TEXT NOT NULL,
              expires_at TEXT,
              worktree_path TEXT,
              locked_at TEXT,
              payload_json TEXT NOT NULL,
              updated_at TEXT NOT NULL,
              PRIMARY KEY(resource, claim_id)
            );
            CREATE TABLE IF NOT EXISTS worktree_slots (
              slot_id TEXT PRIMARY KEY,
              path TEXT NOT NULL,
              base_branch TEXT NOT NULL,
              current_branch TEXT,
              assigned_task_id TEXT,
              assigned_claim_id TEXT,
              tool TEXT,
              status TEXT NOT NULL,
              last_checked_at TEXT,
              cleanup_error TEXT
            );
            CREATE TABLE IF NOT EXISTS event_cursor (
              stream TEXT PRIMARY KEY,
              last_event_id TEXT,
              last_read_at TEXT
            );
            CREATE TABLE IF NOT EXISTS process_registry (
              process_id TEXT PRIMARY KEY,
              task_id TEXT,
              claim_id TEXT,
              pid INTEGER NOT NULL,
              process_group_id INTEGER,
              parent_pid INTEGER,
              command TEXT,
              launched_at TEXT,
              last_seen_at TEXT,
              status TEXT
            );
            CREATE TABLE IF NOT EXISTS events (
              event_id TEXT PRIMARY KEY,
              seq INTEGER,
              ts TEXT,
              source TEXT,
              task_id TEXT,
              event_type TEXT,
              phase TEXT,
              status TEXT,
              payload_json TEXT NOT NULL,
              raw_json TEXT NOT NULL
            );
            -- Ghost cleanup looks up whether a task already reported a result,
            -- once per dead-PID candidate. Without this the lookup scans the
            -- whole event log, which is dominated by heartbeats and grows all
            -- run long.
            CREATE INDEX IF NOT EXISTS idx_events_task_type
              ON events (task_id, event_type);
            -- Event readers walk the log in `seq` order, usually from the newest
            -- end and only for the last few rows. Without this, every read sorts
            -- the whole table first.
            CREATE INDEX IF NOT EXISTS idx_events_seq
              ON events (seq, event_id);
            -- Latest-event-of-a-type lookups (concurrency, cleanup markers).
            CREATE INDEX IF NOT EXISTS idx_events_type_seq
              ON events (event_type, seq);
            CREATE TABLE IF NOT EXISTS cursors (
              name TEXT PRIMARY KEY,
              path TEXT,
              inode INTEGER,
              offset INTEGER,
              last_event_id TEXT,
              updated_at TEXT,
              payload_json TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS jobs (
              job_key TEXT PRIMARY KEY,
              task_id TEXT,
              job_type TEXT NOT NULL,
              pid INTEGER,
              status TEXT,
              payload_json TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tool_throttle (
              tool_id TEXT PRIMARY KEY,
              throttled_until INTEGER NOT NULL,
              payload_json TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS coordinator_runs (
              run_id TEXT PRIMARY KEY,
              pid INTEGER NOT NULL,
              hostname TEXT NOT NULL,
              started_at TEXT NOT NULL,
              last_tick_at TEXT,
              stopped_at TEXT,
              status TEXT NOT NULL,
              epoch INTEGER NOT NULL,
              version TEXT NOT NULL,
              stop_reason TEXT
            );
            CREATE TABLE IF NOT EXISTS gate_decisions (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              task_id TEXT NOT NULL,
              decision TEXT NOT NULL,
              role TEXT NOT NULL,
              actor TEXT NOT NULL,
              revision TEXT,
              evidence TEXT,
              reason TEXT,
              recorded_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_gate_decisions_task ON gate_decisions (task_id, id);
            CREATE TABLE IF NOT EXISTS coordinator_control (
              id INTEGER PRIMARY KEY CHECK (id = 1),
              mode TEXT NOT NULL,
              requested_at TEXT,
              requested_by TEXT,
              drain_snapshot_json TEXT,
              force_grace_seconds INTEGER,
              cleanup_after_force INTEGER,
              reason TEXT
            );
            ",
        )
        .map_err(sql_err)?;
        conn.execute(
            "INSERT OR IGNORE INTO coordinator_control (id, mode) VALUES (1, 'running')",
            [],
        )
        .map_err(sql_err)?;
        if version < 2 {
            // Logs written before heartbeats were pruned on append: keep the
            // last heartbeat of each task and give the space back.
            conn.execute(
                &format!(
                    "DELETE FROM events
                     WHERE event_type IN ({types})
                       AND rowid NOT IN (SELECT MAX(rowid) FROM events
                                         WHERE event_type IN ({types})
                                         GROUP BY task_id, event_type)
                       AND {not_a_cursor}",
                    types = heartbeat_type_list(),
                    not_a_cursor = NOT_A_CURSOR_EVENT,
                ),
                [],
            )
            .map_err(sql_err)?;
            // Best effort: another process holding the database only delays it.
            let _ = conn.execute_batch("VACUUM;");
        }
        conn.execute_batch(&format!("PRAGMA user_version = {SQLITE_SCHEMA_VERSION};"))
            .map_err(sql_err)?;
        Ok(())
    }

    pub fn get_coordinator_control(&self) -> Result<Option<CoordinatorControl>> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let mut stmt = conn
            .prepare("SELECT mode, requested_at, requested_by, drain_snapshot_json, force_grace_seconds, cleanup_after_force, reason FROM coordinator_control WHERE id = 1")
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        if let Some(row) = rows.next().map_err(sql_err)? {
            let mode: String = row.get(0).map_err(sql_err)?;
            let requested_at: Option<String> = row.get(1).map_err(sql_err)?;
            let requested_by: Option<String> = row.get(2).map_err(sql_err)?;
            let drain_snapshot_json: Option<String> = row.get(3).map_err(sql_err)?;
            let force_grace_seconds_i64: Option<i64> = row.get(4).map_err(sql_err)?;
            let cleanup_after_force_i64: Option<i64> = row.get(5).map_err(sql_err)?;
            let reason: Option<String> = row.get(6).map_err(sql_err)?;
            Ok(Some(CoordinatorControl {
                mode,
                requested_at,
                requested_by,
                drain_snapshot_json,
                force_grace_seconds: force_grace_seconds_i64.map(|v| v as u64),
                cleanup_after_force: cleanup_after_force_i64.map(|v| v != 0),
                reason,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn set_coordinator_control(&self, ctrl: &CoordinatorControl) -> Result<()> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        conn.execute(
            "INSERT INTO coordinator_control (id, mode, requested_at, requested_by, drain_snapshot_json, force_grace_seconds, cleanup_after_force, reason)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
               mode=excluded.mode,
               requested_at=excluded.requested_at,
               requested_by=excluded.requested_by,
               drain_snapshot_json=excluded.drain_snapshot_json,
               force_grace_seconds=excluded.force_grace_seconds,
               cleanup_after_force=excluded.cleanup_after_force,
               reason=excluded.reason",
            rusqlite::params![
                ctrl.mode,
                ctrl.requested_at,
                ctrl.requested_by,
                ctrl.drain_snapshot_json,
                ctrl.force_grace_seconds.map(|v| v as i64),
                ctrl.cleanup_after_force.map(|v| if v { 1 } else { 0 }),
                ctrl.reason,
            ],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    pub fn get_coordinator_run(&self, run_id: &str) -> Result<Option<CoordinatorRun>> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let mut stmt = conn
            .prepare("SELECT run_id, pid, hostname, started_at, last_tick_at, stopped_at, status, epoch, version, stop_reason FROM coordinator_runs WHERE run_id = ?1")
            .map_err(sql_err)?;
        let mut rows = stmt.query([run_id]).map_err(sql_err)?;
        if let Some(row) = rows.next().map_err(sql_err)? {
            let run_id: String = row.get(0).map_err(sql_err)?;
            let pid: i64 = row.get(1).map_err(sql_err)?;
            let hostname: String = row.get(2).map_err(sql_err)?;
            let started_at: String = row.get(3).map_err(sql_err)?;
            let last_tick_at: Option<String> = row.get(4).map_err(sql_err)?;
            let stopped_at: Option<String> = row.get(5).map_err(sql_err)?;
            let status: String = row.get(6).map_err(sql_err)?;
            let epoch: i64 = row.get(7).map_err(sql_err)?;
            let version: String = row.get(8).map_err(sql_err)?;
            let stop_reason: Option<String> = row.get(9).map_err(sql_err)?;
            Ok(Some(CoordinatorRun {
                run_id,
                pid,
                hostname,
                started_at,
                last_tick_at,
                stopped_at,
                status,
                epoch,
                version,
                stop_reason,
            }))
        } else {
            Ok(None)
        }
    }

    /// Append one human approval decision to the gate ledger.
    ///
    /// The ledger is append-only by construction: there is no update or delete
    /// API. Gate state is derived from it on every cycle, so a decision can be
    /// superseded only by a later decision from the same person and role.
    pub fn append_gate_decision(
        &self,
        task_id: &str,
        record: &crate::coordinator::model::ApprovalRecord,
    ) -> Result<()> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        conn.execute(
            "INSERT INTO gate_decisions (task_id, decision, role, actor, revision, evidence, reason, recorded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                task_id,
                record.decision.as_str(),
                record.role,
                record.actor,
                record.revision,
                record.evidence,
                record.reason,
                record.recorded_at,
            ],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    /// Every recorded decision, oldest first, grouped by task id.
    pub fn gate_decisions_by_task(
        &self,
    ) -> Result<std::collections::BTreeMap<String, Vec<crate::coordinator::model::ApprovalRecord>>>
    {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT task_id, decision, role, actor, revision, evidence, reason, recorded_at
                 FROM gate_decisions ORDER BY id ASC",
            )
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        let mut out: std::collections::BTreeMap<String, Vec<_>> = Default::default();
        while let Some(row) = rows.next().map_err(sql_err)? {
            let task_id: String = row.get(0).map_err(sql_err)?;
            let decision: String = row.get(1).map_err(sql_err)?;
            let Ok(decision) = decision.parse() else {
                continue;
            };
            out.entry(task_id)
                .or_default()
                .push(crate::coordinator::model::ApprovalRecord {
                    decision,
                    role: row.get(2).map_err(sql_err)?,
                    actor: row.get(3).map_err(sql_err)?,
                    revision: row.get(4).map_err(sql_err)?,
                    evidence: row.get(5).map_err(sql_err)?,
                    reason: row.get(6).map_err(sql_err)?,
                    recorded_at: row.get(7).map_err(sql_err)?,
                });
        }
        Ok(out)
    }

    pub fn upsert_coordinator_run(&self, run: &CoordinatorRun) -> Result<()> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        conn.execute(
            "INSERT INTO coordinator_runs (run_id, pid, hostname, started_at, last_tick_at, stopped_at, status, epoch, version, stop_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(run_id) DO UPDATE SET
               pid=excluded.pid,
               hostname=excluded.hostname,
               started_at=excluded.started_at,
               last_tick_at=excluded.last_tick_at,
               stopped_at=excluded.stopped_at,
               status=excluded.status,
               epoch=excluded.epoch,
               version=excluded.version,
               stop_reason=excluded.stop_reason",
            rusqlite::params![
                run.run_id,
                run.pid,
                run.hostname,
                run.started_at,
                run.last_tick_at,
                run.stopped_at,
                run.status,
                run.epoch,
                run.version,
                run.stop_reason,
            ],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    pub fn get_active_coordinator_run(&self) -> Result<Option<CoordinatorRun>> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let mut stmt = conn
            .prepare("SELECT run_id, pid, hostname, started_at, last_tick_at, stopped_at, status, epoch, version, stop_reason FROM coordinator_runs WHERE status = 'running' OR status = 'draining' ORDER BY started_at DESC LIMIT 1")
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        if let Some(row) = rows.next().map_err(sql_err)? {
            let run_id: String = row.get(0).map_err(sql_err)?;
            let pid: i64 = row.get(1).map_err(sql_err)?;
            let hostname: String = row.get(2).map_err(sql_err)?;
            let started_at: String = row.get(3).map_err(sql_err)?;
            let last_tick_at: Option<String> = row.get(4).map_err(sql_err)?;
            let stopped_at: Option<String> = row.get(5).map_err(sql_err)?;
            let status: String = row.get(6).map_err(sql_err)?;
            let epoch: i64 = row.get(7).map_err(sql_err)?;
            let version: String = row.get(8).map_err(sql_err)?;
            let stop_reason: Option<String> = row.get(9).map_err(sql_err)?;
            Ok(Some(CoordinatorRun {
                run_id,
                pid,
                hostname,
                started_at,
                last_tick_at,
                stopped_at,
                status,
                epoch,
                version,
                stop_reason,
            }))
        } else {
            Ok(None)
        }
    }

    /// Returns the most recent coordinator run regardless of status. Unlike
    /// `get_active_coordinator_run` (which filters to running/draining), this
    /// surfaces the last finished run so clients can report why it stopped.
    pub fn get_latest_coordinator_run(&self) -> Result<Option<CoordinatorRun>> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let mut stmt = conn
            .prepare("SELECT run_id, pid, hostname, started_at, last_tick_at, stopped_at, status, epoch, version, stop_reason FROM coordinator_runs ORDER BY started_at DESC LIMIT 1")
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        if let Some(row) = rows.next().map_err(sql_err)? {
            Ok(Some(CoordinatorRun {
                run_id: row.get(0).map_err(sql_err)?,
                pid: row.get(1).map_err(sql_err)?,
                hostname: row.get(2).map_err(sql_err)?,
                started_at: row.get(3).map_err(sql_err)?,
                last_tick_at: row.get(4).map_err(sql_err)?,
                stopped_at: row.get(5).map_err(sql_err)?,
                status: row.get(6).map_err(sql_err)?,
                epoch: row.get(7).map_err(sql_err)?,
                version: row.get(8).map_err(sql_err)?,
                stop_reason: row.get(9).map_err(sql_err)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_recent_coordinator_runs(&self, limit: usize) -> Result<Vec<CoordinatorRun>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let mut stmt = conn
            .prepare("SELECT run_id, pid, hostname, started_at, last_tick_at, stopped_at, status, epoch, version, stop_reason FROM coordinator_runs ORDER BY started_at DESC LIMIT ?1")
            .map_err(sql_err)?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                Ok(CoordinatorRun {
                    run_id: row.get(0)?,
                    pid: row.get(1)?,
                    hostname: row.get(2)?,
                    started_at: row.get(3)?,
                    last_tick_at: row.get(4)?,
                    stopped_at: row.get(5)?,
                    status: row.get(6)?,
                    epoch: row.get(7)?,
                    version: row.get(8)?,
                    stop_reason: row.get(9)?,
                })
            })
            .map_err(sql_err)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_err)
    }

    pub fn get_cursor(&self, name: &str) -> Result<Option<(u64, String)>> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let mut stmt = conn
            .prepare("SELECT offset, last_event_id FROM cursors WHERE name = ?1")
            .map_err(sql_err)?;
        let mut rows = stmt.query([name]).map_err(sql_err)?;
        if let Some(row) = rows.next().map_err(sql_err)? {
            let offset: i64 = row.get(0).map_err(sql_err)?;
            let last_id: String = row.get(1).map_err(sql_err)?;
            Ok(Some((offset as u64, last_id)))
        } else {
            Ok(None)
        }
    }

    pub fn set_cursor(&self, name: &str, offset: u64, last_id: &str) -> Result<()> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let now = chrono::Utc::now().to_rfc3339();
        let payload_json = serde_json::to_string(&CoordinatorCursor {
            offset,
            last_event_id: (!last_id.is_empty()).then(|| last_id.to_string()),
            updated_at: Some(now.clone()),
            ..CoordinatorCursor::default()
        })
        .map_err(|e| MaccError::Storage {
            backend: "sqlite",
            message: format!("Failed to serialize cursor payload: {}", e),
        })?;
        conn.execute(
            "INSERT INTO cursors (name, offset, last_event_id, updated_at, payload_json) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(name) DO UPDATE SET offset=?2, last_event_id=?3, updated_at=?4",
            rusqlite::params![name, offset as i64, last_id, now, payload_json],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    pub fn apply_transition(&self, change: &TransitionMutation) -> Result<()> {
        self.apply_transition_with_event(change, None)
    }

    pub fn apply_transition_with_event(
        &self,
        change: &TransitionMutation,
        event: Option<&EventMutation>,
    ) -> Result<()> {
        let mut conn = self.open()?;
        self.init_schema(&conn)?;
        let tx = write_transaction(&mut conn)?;

        let task_raw: String = tx
            .query_row(
                "SELECT payload_json FROM tasks WHERE task_id=?1",
                params![change.task_id],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        let mut task = parse_task_payload(&change.task_id, &task_raw)?;

        task.state = change.new_state.clone();
        task.updated_at = Some(change.now.clone());
        task.state_changed_at = Some(change.now.clone());

        if change.new_state == "pr_open" && !change.pr_url.is_empty() {
            task.pr_url = Some(change.pr_url.clone());
        }

        if change.new_state == "changes_requested" {
            let mut review = task.review.take().unwrap_or_default();
            review.changed = Some(true);
            review.last_reviewed_at = Some(change.now.clone());
            if !change.reviewer.is_empty() {
                review.reviewer = Some(change.reviewer.clone());
            }
            if !change.reason.is_empty() {
                review.reason = Some(change.reason.clone());
            }
            task.review = Some(review);
        }

        if matches!(change.new_state.as_str(), "merged" | "abandoned" | "todo") {
            task.assignee = None;
            task.claimed_at = None;
            task.worktree = None;
            task.task_runtime.status = Some("idle".to_string());
            task.task_runtime.pid = None;
            task.task_runtime.started_at = None;
            task.task_runtime.current_phase = None;
            task.task_runtime.merge_result_pending = Some(false);
            task.task_runtime.merge_result_file = None;
        }

        let title = task.title.clone().unwrap_or_default();
        let priority = task.priority.clone().unwrap_or_default();
        let tool = task.tool.clone().unwrap_or_default();
        let payload = serde_json::to_string(&task).map_err(|e| MaccError::Storage {
            backend: "sqlite",
            message: format!("Failed to serialize task payload: {}", e),
        })?;
        tx.execute(
            "UPDATE tasks SET state=?2, title=?3, priority=?4, tool=?5, payload_json=?6, updated_at=?7 WHERE task_id=?1",
            params![
                change.task_id,
                change.new_state,
                title,
                priority,
                tool,
                payload,
                change.now
            ],
        )
        .map_err(sql_err)?;

        self.upsert_task_runtime_row(&tx, &task, &change.now)?;
        self.recompute_resource_locks(&tx, &change.now)?;
        if let Some(event) = event {
            self.append_event_in_tx(&tx, event)?;
        }
        tx.commit().map_err(sql_err)?;
        Ok(())
    }

    pub fn set_runtime(&self, change: &RuntimeMutation) -> Result<()> {
        self.set_runtime_with_event(change, None)
    }

    pub fn set_runtime_with_event(
        &self,
        change: &RuntimeMutation,
        event: Option<&EventMutation>,
    ) -> Result<()> {
        let mut conn = self.open()?;
        self.init_schema(&conn)?;
        let tx = write_transaction(&mut conn)?;

        let task_raw: String = tx
            .query_row(
                "SELECT payload_json FROM tasks WHERE task_id=?1",
                params![change.task_id],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        let mut task = parse_task_payload(&change.task_id, &task_raw)?;

        task.task_runtime.status = Some(change.runtime_status.clone());
        if !change.phase.is_empty() {
            task.task_runtime.current_phase = Some(change.phase.clone());
        }
        if let Some(msg) = &change.message {
            task.task_runtime.message = Some(msg.clone());
        }
        if let Some(prog) = change.progress {
            task.task_runtime.progress = Some(prog);
        }
        if let Some(wid) = &change.worker_id {
            task.task_runtime.worker_id = Some(wid.clone());
        }
        if let Some(t) = &change.tool {
            task.task_runtime.tool = Some(t.clone());
        }
        if let Some(wt) = &change.worktree {
            task.task_runtime.worktree = Some(wt.clone());
        }
        if let Some(br) = &change.branch {
            task.task_runtime.branch = Some(br.clone());
        }
        if let Some(sol) = &change.stdout_log {
            task.task_runtime.stdout_log = Some(sol.clone());
        }
        if let Some(sel) = &change.stderr_log {
            task.task_runtime.stderr_log = Some(sel.clone());
        }
        if let Some(evl) = &change.events_log {
            task.task_runtime.events_log = Some(evl.clone());
        }
        if let Some(lea) = &change.last_event_at {
            task.task_runtime.last_event_at = Some(lea.clone());
        }
        task.task_runtime.registry_updated_at = Some(change.now.clone());

        match change.pid {
            Some(pid) => task.task_runtime.pid = Some(pid),
            None => {
                if matches!(
                    change.runtime_status.as_str(),
                    "idle" | "phase_done" | "failed" | "stale"
                ) {
                    task.task_runtime.pid = None;
                }
            }
        }
        if !change.last_error.is_empty() {
            task.task_runtime.last_error = Some(change.last_error.clone());
        }
        if !change.heartbeat_ts.is_empty() {
            task.task_runtime.last_heartbeat = Some(change.heartbeat_ts.clone());
        }
        if let Some(attempt) = change.attempt {
            task.task_runtime.attempt = Some(attempt);
        }
        if change.runtime_status == "running"
            && task
                .task_runtime
                .started_at
                .as_deref()
                .unwrap_or_default()
                .is_empty()
        {
            task.task_runtime.started_at = Some(change.now.clone());
        }
        if matches!(
            change.runtime_status.as_str(),
            "idle" | "phase_done" | "failed" | "stale"
        ) {
            task.task_runtime.phase_started_at = None;
        } else if change.runtime_status == "running" {
            task.task_runtime.phase_started_at = Some(change.now.clone());
        }

        task.updated_at = Some(change.now.clone());

        let state = task.state.clone();
        let title = task.title.clone().unwrap_or_default();
        let priority = task.priority.clone().unwrap_or_default();
        let tool = task.tool.clone().unwrap_or_default();
        let payload = serde_json::to_string(&task).map_err(|e| MaccError::Storage {
            backend: "sqlite",
            message: format!("Failed to serialize task payload: {}", e),
        })?;
        tx.execute(
            "UPDATE tasks SET state=?2, title=?3, priority=?4, tool=?5, payload_json=?6, updated_at=?7 WHERE task_id=?1",
            params![change.task_id, state, title, priority, tool, payload, change.now],
        )
        .map_err(sql_err)?;

        self.upsert_task_runtime_row(&tx, &task, &change.now)?;
        if let Some(event) = event {
            self.append_event_in_tx(&tx, event)?;
        }
        tx.commit().map_err(sql_err)?;
        Ok(())
    }

    pub fn set_merge_pending(&self, change: &MergePendingMutation) -> Result<()> {
        let mut conn = self.open()?;
        self.init_schema(&conn)?;
        let tx = write_transaction(&mut conn)?;

        let task_raw: String = tx
            .query_row(
                "SELECT payload_json FROM tasks WHERE task_id=?1",
                params![change.task_id],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        let mut task = parse_task_payload(&change.task_id, &task_raw)?;
        task.task_runtime.merge_result_pending = Some(true);
        task.task_runtime.merge_result_file = Some(change.result_file.clone());
        task.task_runtime.merge_worker_pid = change.pid;
        task.task_runtime.merge_result_started_at = Some(change.now.clone());
        task.updated_at = Some(change.now.clone());

        let state = task.state.clone();
        let title = task.title.clone().unwrap_or_default();
        let priority = task.priority.clone().unwrap_or_default();
        let tool = task.tool.clone().unwrap_or_default();
        let payload = serde_json::to_string(&task).map_err(|e| MaccError::Storage {
            backend: "sqlite",
            message: format!("Failed to serialize task payload: {}", e),
        })?;
        tx.execute(
            "UPDATE tasks SET state=?2, title=?3, priority=?4, tool=?5, payload_json=?6, updated_at=?7 WHERE task_id=?1",
            params![change.task_id, state, title, priority, tool, payload, change.now],
        )
        .map_err(sql_err)?;

        self.upsert_task_runtime_row(&tx, &task, &change.now)?;
        tx.commit().map_err(sql_err)?;
        Ok(())
    }

    pub fn set_merge_processed(&self, change: &MergeProcessedMutation) -> Result<()> {
        let mut conn = self.open()?;
        self.init_schema(&conn)?;
        let tx = write_transaction(&mut conn)?;

        let task_raw: String = tx
            .query_row(
                "SELECT payload_json FROM tasks WHERE task_id=?1",
                params![change.task_id],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        let mut task = parse_task_payload(&change.task_id, &task_raw)?;
        task.task_runtime.merge_result_pending = Some(false);
        task.task_runtime.merge_result_file = None;
        task.task_runtime.merge_worker_pid = None;
        if !change.result_file.is_empty() {
            task.task_runtime.last_merge_result_file = Some(change.result_file.clone());
        }
        if !change.status.is_empty() {
            task.task_runtime.last_merge_result_status = Some(change.status.clone());
        }
        if let Some(rc) = change.rc {
            task.task_runtime.last_merge_result_rc = Some(rc);
        }
        task.task_runtime.last_merge_result_at = Some(change.now.clone());
        task.updated_at = Some(change.now.clone());

        let state = task.state.clone();
        let title = task.title.clone().unwrap_or_default();
        let priority = task.priority.clone().unwrap_or_default();
        let tool = task.tool.clone().unwrap_or_default();
        let payload = serde_json::to_string(&task).map_err(|e| MaccError::Storage {
            backend: "sqlite",
            message: format!("Failed to serialize task payload: {}", e),
        })?;
        tx.execute(
            "UPDATE tasks SET state=?2, title=?3, priority=?4, tool=?5, payload_json=?6, updated_at=?7 WHERE task_id=?1",
            params![change.task_id, state, title, priority, tool, payload, change.now],
        )
        .map_err(sql_err)?;

        self.upsert_task_runtime_row(&tx, &task, &change.now)?;
        tx.commit().map_err(sql_err)?;
        Ok(())
    }

    pub fn increment_retries(&self, change: &RetryIncrementMutation) -> Result<()> {
        let mut conn = self.open()?;
        self.init_schema(&conn)?;
        let tx = write_transaction(&mut conn)?;

        let task_raw: String = tx
            .query_row(
                "SELECT payload_json FROM tasks WHERE task_id=?1",
                params![change.task_id],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        let mut task = parse_task_payload(&change.task_id, &task_raw)?;
        task.task_runtime.increment_retries();
        task.updated_at = Some(change.now.clone());

        let state = task.state.clone();
        let title = task.title.clone().unwrap_or_default();
        let priority = task.priority.clone().unwrap_or_default();
        let tool = task.tool.clone().unwrap_or_default();
        let payload = serde_json::to_string(&task).map_err(|e| MaccError::Storage {
            backend: "sqlite",
            message: format!("Failed to serialize task payload: {}", e),
        })?;
        tx.execute(
            "UPDATE tasks SET state=?2, title=?3, priority=?4, tool=?5, payload_json=?6, updated_at=?7 WHERE task_id=?1",
            params![change.task_id, state, title, priority, tool, payload, change.now],
        )
        .map_err(sql_err)?;
        self.upsert_task_runtime_row(&tx, &task, &change.now)?;
        tx.commit().map_err(sql_err)?;
        Ok(())
    }

    pub fn upsert_slo_warning(&self, change: &SloWarningMutation) -> Result<()> {
        let mut conn = self.open()?;
        self.init_schema(&conn)?;
        let tx = write_transaction(&mut conn)?;

        let task_raw: String = tx
            .query_row(
                "SELECT payload_json FROM tasks WHERE task_id=?1",
                params![change.task_id],
                |row| row.get(0),
            )
            .map_err(sql_err)?;
        let mut task = parse_task_payload(&change.task_id, &task_raw)?;
        task.task_runtime.upsert_slo_warning(
            &change.metric,
            change.threshold,
            change.value,
            &change.suggestion,
            &change.now,
        );
        task.updated_at = Some(change.now.clone());

        let state = task.state.clone();
        let title = task.title.clone().unwrap_or_default();
        let priority = task.priority.clone().unwrap_or_default();
        let tool = task.tool.clone().unwrap_or_default();
        let payload = serde_json::to_string(&task).map_err(|e| MaccError::Storage {
            backend: "sqlite",
            message: format!("Failed to serialize task payload: {}", e),
        })?;
        tx.execute(
            "UPDATE tasks SET state=?2, title=?3, priority=?4, tool=?5, payload_json=?6, updated_at=?7 WHERE task_id=?1",
            params![change.task_id, state, title, priority, tool, payload, change.now],
        )
        .map_err(sql_err)?;
        self.upsert_task_runtime_row(&tx, &task, &change.now)?;
        tx.commit().map_err(sql_err)?;
        Ok(())
    }

    fn upsert_task_runtime_row(
        &self,
        tx: &rusqlite::Transaction<'_>,
        task: &Task,
        now: &str,
    ) -> Result<()> {
        let task_id = &task.id;
        let runtime = &task.task_runtime;

        let claim_id = runtime
            .claim_id
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(runtime.active_session_id.as_deref())
            .unwrap_or(&format!("unclaimed-{}", task_id))
            .to_string();

        let run_id = runtime.run_id.as_deref().unwrap_or("-");
        let coordinator_epoch = runtime.coordinator_epoch.unwrap_or(0);
        let workflow_state = &task.state;
        let runtime_status = runtime.status.as_deref().unwrap_or("idle");
        let phase = runtime.current_phase.as_deref();
        let tool = task.tool.as_deref();

        let worktree_slot_id = runtime
            .extra
            .get("worktree_slot_id")
            .and_then(Value::as_str);

        let worktree_path = task
            .worktree
            .as_ref()
            .and_then(|w| w.worktree_path.as_deref());
        let branch = task.worktree.as_ref().and_then(|w| w.branch.as_deref());
        let base_branch = task
            .worktree
            .as_ref()
            .and_then(|w| w.base_branch.as_deref());

        let pid = runtime.pid;
        let process_group_id = runtime.process_group_id;
        let started_at = runtime.started_at.as_deref();
        let last_heartbeat_at = runtime.last_heartbeat.as_deref();
        let heartbeat_seq = runtime.heartbeat_seq.unwrap_or(0);
        let lease_expires_at = runtime.lease_expires_at.as_deref();
        let attempt = runtime.attempt.unwrap_or(0);

        let locked_resources_json = runtime
            .locked_resources_json
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                serde_json::to_string(&task.exclusive_resources)
                    .unwrap_or_else(|_| "[]".to_string())
            });

        let last_error_code = runtime.last_error_code.as_deref();
        let last_error_message = runtime
            .last_error_message
            .as_deref()
            .or(runtime.last_error.as_deref());

        let runtime_raw = serde_json::to_string(runtime).map_err(|e| MaccError::Storage {
            backend: "sqlite",
            message: format!(
                "Failed to serialize task_runtime payload for '{}': {}",
                task_id, e
            ),
        })?;

        // Delete any existing row with the same task_id to prevent duplicates when claim_id changes
        tx.execute(
            "DELETE FROM task_runtime WHERE task_id=?1",
            params![task_id],
        )
        .map_err(sql_err)?;

        tx.execute(
            "INSERT INTO task_runtime (
               task_id, claim_id, run_id, coordinator_epoch, workflow_state,
               runtime_status, phase, tool, worktree_slot_id, worktree_path,
               branch, base_branch, pid, process_group_id, started_at,
               updated_at, last_heartbeat_at, heartbeat_seq, lease_expires_at, attempt,
               locked_resources_json, last_error_code, last_error_message, payload_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)
             ON CONFLICT(claim_id) DO UPDATE SET
               task_id=excluded.task_id,
               run_id=excluded.run_id,
               coordinator_epoch=excluded.coordinator_epoch,
               workflow_state=excluded.workflow_state,
               runtime_status=excluded.runtime_status,
               phase=excluded.phase,
               tool=excluded.tool,
               worktree_slot_id=excluded.worktree_slot_id,
               worktree_path=excluded.worktree_path,
               branch=excluded.branch,
               base_branch=excluded.base_branch,
               pid=excluded.pid,
               process_group_id=excluded.process_group_id,
               started_at=excluded.started_at,
               updated_at=excluded.updated_at,
               last_heartbeat_at=excluded.last_heartbeat_at,
               heartbeat_seq=excluded.heartbeat_seq,
               lease_expires_at=excluded.lease_expires_at,
               attempt=excluded.attempt,
               locked_resources_json=excluded.locked_resources_json,
               last_error_code=excluded.last_error_code,
               last_error_message=excluded.last_error_message,
               payload_json=excluded.payload_json",
            params![
                task_id,
                claim_id,
                run_id,
                coordinator_epoch,
                workflow_state,
                runtime_status,
                phase,
                tool,
                worktree_slot_id,
                worktree_path,
                branch,
                base_branch,
                pid,
                process_group_id,
                started_at,
                now,
                last_heartbeat_at,
                heartbeat_seq,
                lease_expires_at,
                attempt,
                locked_resources_json,
                last_error_code,
                last_error_message,
                runtime_raw
            ],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    fn recompute_resource_locks(&self, tx: &rusqlite::Transaction<'_>, now: &str) -> Result<()> {
        tx.execute("DELETE FROM resource_locks", [])
            .map_err(sql_err)?;
        let mut stmt = tx
            .prepare("SELECT payload_json FROM tasks ORDER BY task_id")
            .map_err(sql_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sql_err)?;
        let mut existing = std::collections::BTreeSet::new();
        for row in rows {
            let raw = row.map_err(sql_err)?;
            let task: Task = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if !is_active_state(&task.state) {
                continue;
            }
            if task.worktree.is_none() {
                continue;
            }
            if task.id.is_empty() {
                continue;
            }
            let worktree_path = task
                .worktree
                .as_ref()
                .and_then(|v| v.worktree_path.clone())
                .unwrap_or_default();
            let locked_at = task
                .claimed_at
                .as_deref()
                .filter(|v| !v.is_empty())
                .unwrap_or(now)
                .to_string();
            let claim_id = task
                .task_runtime
                .claim_id
                .clone()
                .filter(|s| !s.is_empty())
                .or(task.task_runtime.active_session_id.clone())
                .unwrap_or_else(|| format!("unclaimed-{}", task.id));

            let expires_at = task.task_runtime.lease_expires_at.clone();

            for resource_name in &task.exclusive_resources {
                if resource_name.is_empty() || existing.contains(resource_name) {
                    continue;
                }
                let lock = ResourceLock {
                    task_id: task.id.clone(),
                    claim_id: claim_id.clone(),
                    worktree_path: worktree_path.clone(),
                    locked_at: locked_at.clone(),
                    expires_at: expires_at.clone(),
                    extra: Default::default(),
                };
                let lock_json = serde_json::to_value(&lock).map_err(|e| MaccError::Storage {
                    backend: "sqlite",
                    message: format!(
                        "Failed to serialize typed resource lock '{}': {}",
                        resource_name, e
                    ),
                })?;
                let payload = if worktree_path.is_empty() {
                    // Keep historical shape where missing worktree_path becomes null in payload.
                    let mut with_null = lock_json;
                    with_null["worktree_path"] = Value::Null;
                    with_null
                } else {
                    lock_json
                };
                tx.execute(
                    "INSERT INTO resource_locks (resource, task_id, claim_id, acquired_at, expires_at, worktree_path, locked_at, payload_json, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        resource_name,
                        task.id,
                        claim_id,
                        locked_at, // acquired_at
                        expires_at,
                        worktree_path,
                        locked_at,
                        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string()),
                        now
                    ],
                )
                .map_err(sql_err)?;
                existing.insert(resource_name.to_string());
            }
        }
        Ok(())
    }

    fn load_registry_from_tables(&self, conn: &Connection) -> Result<TaskRegistry> {
        let mut registry = TaskRegistry::default();
        let mut stmt = conn
            .prepare("SELECT payload_json FROM tasks ORDER BY task_id")
            .map_err(sql_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sql_err)?;
        for row in rows {
            let raw = row.map_err(sql_err)?;
            if let Ok(task) = serde_json::from_str::<Task>(&raw) {
                registry.tasks.push(task);
            }
        }

        let mut stmt = conn
            .prepare("SELECT resource, payload_json FROM resource_locks ORDER BY resource")
            .map_err(sql_err)?;
        let rows = stmt
            .query_map([], |row| {
                let resource: String = row.get(0)?;
                let payload: String = row.get(1)?;
                Ok((resource, payload))
            })
            .map_err(sql_err)?;
        for row in rows {
            let (resource, payload) = row.map_err(sql_err)?;
            if let Ok(lock) = serde_json::from_str::<ResourceLock>(&payload) {
                registry.resource_locks.insert(resource, lock);
            }
        }
        registry.updated_at = Some(now_iso_string());
        registry
            .extra
            .insert("schema_version".into(), Value::from(1));
        registry
            .extra
            .entry("processed_event_ids".into())
            .or_insert_with(|| json!({}));
        registry
            .extra
            .entry("state_mapping".into())
            .or_insert_with(|| json!({}));
        Ok(registry)
    }

    pub fn get_running_task_pids(&self) -> Result<Vec<(String, i64)>> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let mut stmt = conn
            .prepare("SELECT task_id, pid FROM task_runtime WHERE status = 'running' AND pid IS NOT NULL")
            .map_err(sql_err)?;
        let mut rows = stmt.query([]).map_err(sql_err)?;
        let mut pids = Vec::new();
        while let Some(row) = rows.next().map_err(sql_err)? {
            let task_id: String = row.get(0).map_err(sql_err)?;
            let pid: i64 = row.get(1).map_err(sql_err)?;
            pids.push((task_id, pid));
        }
        Ok(pids)
    }
}

impl SqliteStorage {
    /// The task registry alone, without the event log: what most readers need.
    pub fn load_registry(&self) -> Result<TaskRegistry> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        self.load_registry_with_conn(&conn)
    }

    fn load_registry_with_conn(&self, conn: &Connection) -> Result<TaskRegistry> {
        let metadata_registry = match conn.query_row(
            "SELECT value FROM metadata WHERE key='registry_json'",
            [],
            |row| row.get::<_, String>(0),
        ) {
            Ok(raw) => Some(serde_json::from_str::<TaskRegistry>(&raw).map_err(|e| {
                MaccError::Storage {
                    backend: "sqlite",
                    message: format!("Failed to parse registry_json metadata: {}", e),
                }
            })?),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(sql_err(e)),
        };
        let mut registry = self.load_registry_from_tables(conn)?;
        if let Some(meta) = metadata_registry {
            let TaskRegistry {
                tasks: meta_tasks,
                resource_locks: meta_resource_locks,
                external_merged_task_ids: meta_external_merged_task_ids,
                updated_at: meta_updated_at,
                extra: meta_extra,
            } = meta;
            let table_tasks_len = registry.tasks.len();
            let meta_tasks_len = meta_tasks.len();
            if table_tasks_len == 0 && meta_tasks_len > 0 {
                registry = TaskRegistry {
                    tasks: meta_tasks,
                    resource_locks: meta_resource_locks,
                    external_merged_task_ids: meta_external_merged_task_ids,
                    updated_at: meta_updated_at,
                    extra: meta_extra,
                };
            } else {
                if registry.resource_locks.is_empty() && !meta_resource_locks.is_empty() {
                    registry.resource_locks = meta_resource_locks;
                }
                registry
                    .external_merged_task_ids
                    .extend(meta_external_merged_task_ids);
                registry.updated_at = meta_updated_at.or(registry.updated_at);
                for (key, value) in meta_extra {
                    registry.extra.insert(key, value);
                }
            }
        }

        Ok(registry)
    }

    /// The registry and the cursor without the event log.
    ///
    /// For read-modify-write callers: `save_snapshot` never deletes events, so
    /// a snapshot saved with no events leaves the stored log untouched instead
    /// of re-inserting every row of it.
    pub fn load_snapshot_without_events(&self) -> Result<CoordinatorSnapshot> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        Ok(CoordinatorSnapshot {
            registry: self.load_registry_with_conn(&conn)?,
            events: Vec::new(),
            cursor: self.load_cursor_with_conn(&conn)?,
        })
    }

    fn load_cursor_with_conn(&self, conn: &Connection) -> Result<Option<CoordinatorCursor>> {
        match conn.query_row(
            "SELECT payload_json FROM cursors WHERE name='coordinator'",
            [],
            |row| row.get::<_, String>(0),
        ) {
            Ok(raw) => Ok(Some(
                serde_json::from_str::<CoordinatorCursor>(&raw).map_err(|e| {
                    MaccError::Storage {
                        backend: "sqlite",
                        message: format!("Failed to parse cursor payload_json: {}", e),
                    }
                })?,
            )),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(sql_err(e)),
        }
    }

    /// The last `limit` events, oldest first (the order `load_snapshot` uses).
    pub fn load_recent_events(&self, limit: usize) -> Result<Vec<CoordinatorEventRecord>> {
        let mut events = Vec::with_capacity(limit.min(1024));
        if limit == 0 {
            return Ok(events);
        }
        self.scan_events_newest_first(EventQuery::default(), |event| {
            events.push(event);
            if events.len() >= limit {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })?;
        events.reverse();
        Ok(events)
    }

    /// Up to `limit` events with a `seq` above `after_seq`, oldest first: what
    /// a live stream has not delivered yet.
    pub fn load_events_after(
        &self,
        after_seq: i64,
        limit: usize,
    ) -> Result<Vec<CoordinatorEventRecord>> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT raw_json FROM events WHERE seq > ?1 ORDER BY seq ASC, event_id ASC LIMIT ?2",
            )
            .map_err(sql_err)?;
        let rows = stmt
            .query_map(params![after_seq, limit as i64], |row| {
                row.get::<_, String>(0)
            })
            .map_err(sql_err)?;
        let mut events = Vec::new();
        for row in rows {
            if let Ok(event) =
                serde_json::from_str::<CoordinatorEventRecord>(&row.map_err(sql_err)?)
            {
                events.push(event);
            }
        }
        Ok(events)
    }

    /// The `seq` of a stored event.
    pub fn event_seq(&self, event_id: &str) -> Result<Option<i64>> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        match conn.query_row(
            "SELECT seq FROM events WHERE event_id = ?1",
            [event_id],
            |row| row.get::<_, Option<i64>>(0),
        ) {
            Ok(seq) => Ok(seq),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(sql_err(e)),
        }
    }

    /// Visit the events matching `query`, newest first, until `visit` breaks.
    ///
    /// Readers looking for the latest event of some kind stop at the first
    /// match instead of loading and parsing the whole log, which grows with
    /// every heartbeat for as long as the project runs.
    pub fn scan_events_newest_first<F>(&self, query: EventQuery<'_>, mut visit: F) -> Result<()>
    where
        F: FnMut(CoordinatorEventRecord) -> ControlFlow<()>,
    {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let mut clauses: Vec<String> = Vec::new();
        let mut values: Vec<&str> = Vec::new();
        if let Some(task_id) = query.task_id {
            values.push(task_id);
            clauses.push(format!("task_id = ?{}", values.len()));
        }
        if let Some(event_type) = query.event_type {
            values.push(event_type);
            clauses.push(format!("event_type = ?{}", values.len()));
        }
        if query.skip_heartbeats {
            clauses.push(format!(
                "IFNULL(event_type, '') NOT IN ({})",
                heartbeat_type_list()
            ));
        }
        let filter = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        let mut stmt = conn
            .prepare(&format!(
                "SELECT raw_json FROM events {filter} ORDER BY seq DESC, event_id DESC"
            ))
            .map_err(sql_err)?;
        let mut rows = stmt
            .query(rusqlite::params_from_iter(values))
            .map_err(sql_err)?;
        while let Some(row) = rows.next().map_err(sql_err)? {
            let raw: String = row.get(0).map_err(sql_err)?;
            let Ok(event) = serde_json::from_str::<CoordinatorEventRecord>(&raw) else {
                continue;
            };
            if visit(event).is_break() {
                break;
            }
        }
        Ok(())
    }
}

impl CoordinatorStorage for SqliteStorage {
    fn load_snapshot(&self) -> Result<CoordinatorSnapshot> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let registry = self.load_registry_with_conn(&conn)?;

        let mut events = Vec::new();
        let mut stmt = conn
            .prepare("SELECT raw_json FROM events ORDER BY seq ASC, event_id ASC")
            .map_err(sql_err)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(sql_err)?;
        for row in rows {
            let raw = row.map_err(sql_err)?;
            if let Ok(v) = serde_json::from_str::<CoordinatorEventRecord>(&raw) {
                events.push(v);
            }
        }

        let cursor = self.load_cursor_with_conn(&conn)?;

        Ok(CoordinatorSnapshot {
            registry,
            events,
            cursor,
        })
    }

    fn save_snapshot(&self, snapshot: &CoordinatorSnapshot) -> Result<()> {
        let mut conn = self.open()?;
        self.init_schema(&conn)?;
        let tx = write_transaction(&mut conn)?;

        // The coordinator saves the registry many times per tick while only a
        // task or two change: rewrite just the rows whose task changed.
        let stored_tasks: HashMap<String, String> = {
            let mut stmt = tx
                .prepare("SELECT task_id, payload_json FROM tasks")
                .map_err(sql_err)?;
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(sql_err)?;
            rows.collect::<std::result::Result<_, _>>()
                .map_err(sql_err)?
        };
        let kept: HashSet<&str> = snapshot
            .registry
            .tasks
            .iter()
            .map(|task| task.id.as_str())
            .filter(|id| !id.is_empty())
            .collect();
        for task_id in stored_tasks.keys().filter(|id| !kept.contains(id.as_str())) {
            tx.execute("DELETE FROM tasks WHERE task_id = ?1", [task_id])
                .map_err(sql_err)?;
        }
        // Runtime rows whose task is gone, including orphans of older saves.
        tx.execute(
            "DELETE FROM task_runtime WHERE task_id NOT IN (SELECT task_id FROM tasks)",
            [],
        )
        .map_err(sql_err)?;
        tx.execute("DELETE FROM resource_locks", [])
            .map_err(sql_err)?;
        // Events are append-only — never delete them during snapshot save.
        // IPC handlers write events concurrently; deleting + re-inserting
        // creates a race window that loses events (including phase_results).
        tx.execute("DELETE FROM cursors", []).map_err(sql_err)?;
        tx.execute("DELETE FROM jobs", []).map_err(sql_err)?;

        let now = now_iso_string();
        // The tasks live in their own table; `registry_json` keeps the rest of
        // the registry, and is only rewritten when that changes.
        let registry_meta = TaskRegistry {
            tasks: Vec::new(),
            ..snapshot.registry.clone()
        };
        let registry_raw =
            serde_json::to_string(&registry_meta).map_err(|e| MaccError::Storage {
                backend: "sqlite",
                message: format!("Failed to serialize registry json: {}", e),
            })?;
        tx.execute(
            "INSERT INTO metadata (key, value, updated_at) VALUES ('registry_json', ?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
             WHERE metadata.value <> excluded.value",
            params![registry_raw, now],
        )
        .map_err(sql_err)?;

        for task in &snapshot.registry.tasks {
            let task_id = task.id.clone();
            if task_id.is_empty() {
                continue;
            }
            let state = task.state.clone();
            let title = task.title.clone().unwrap_or_default();
            let priority = task.priority.clone().unwrap_or_default();
            let tool = task.tool.clone().unwrap_or_default();
            let task_updated = task.updated_at.as_deref().unwrap_or(now.as_str());
            let task_raw = serde_json::to_string(task).map_err(|e| MaccError::Storage {
                backend: "sqlite",
                message: format!("Failed to serialize task payload: {}", e),
            })?;
            let runtime_status = task.task_runtime.status.as_deref().unwrap_or("idle");
            let pid = task.task_runtime.pid;
            let runtime_updated = task
                .task_runtime
                .extra
                .get("updated_at")
                .and_then(Value::as_str)
                .unwrap_or(task_updated);
            if stored_tasks.get(&task_id) != Some(&task_raw) {
                tx.execute(
                    "INSERT OR REPLACE INTO tasks (task_id, state, title, priority, tool, payload_json, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![task_id, state, title, priority, tool, task_raw, task_updated],
                )
                .map_err(sql_err)?;
                self.upsert_task_runtime_row(&tx, task, runtime_updated)?;
            }

            if let Some(pid) = pid {
                let job_payload = json!({
                    "task_id": task_id,
                    "job_type": "performer",
                    "pid": pid,
                    "status": runtime_status,
                });
                tx.execute(
                    "INSERT INTO jobs (job_key, task_id, job_type, pid, status, payload_json, updated_at)
                     VALUES (?1, ?2, 'performer', ?3, ?4, ?5, ?6)",
                    params![
                        format!("{}:performer", task_id),
                        task_id,
                        pid,
                        runtime_status,
                        serde_json::to_string(&job_payload).unwrap_or_else(|_| "{}".to_string()),
                        runtime_updated
                    ],
                )
                .map_err(sql_err)?;
            }
            if let Some(merge_pid) = task.task_runtime.merge_worker_pid {
                let job_payload = json!({
                    "task_id": task_id,
                    "job_type": "merge_worker",
                    "pid": merge_pid,
                    "status": runtime_status,
                });
                tx.execute(
                    "INSERT INTO jobs (job_key, task_id, job_type, pid, status, payload_json, updated_at)
                     VALUES (?1, ?2, 'merge_worker', ?3, ?4, ?5, ?6)",
                    params![
                        format!("{}:merge", task_id),
                        task_id,
                        merge_pid,
                        runtime_status,
                        serde_json::to_string(&job_payload).unwrap_or_else(|_| "{}".to_string()),
                        runtime_updated
                    ],
                )
                .map_err(sql_err)?;
            }
        }

        for (resource, lock_value) in &snapshot.registry.resource_locks {
            let task_id = lock_value.task_id.as_str();
            let claim_id = if lock_value.claim_id.is_empty() {
                format!("unclaimed-{}", task_id)
            } else {
                lock_value.claim_id.clone()
            };
            let worktree_path = lock_value.worktree_path.as_str();
            let locked_at = lock_value.locked_at.as_str();
            let expires_at = lock_value.expires_at.as_deref();
            let lock_raw = serde_json::to_string(lock_value).map_err(|e| MaccError::Storage {
                backend: "sqlite",
                message: format!("Failed to serialize resource lock payload: {}", e),
            })?;
            tx.execute(
                "INSERT INTO resource_locks (resource, task_id, claim_id, acquired_at, expires_at, worktree_path, locked_at, payload_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    resource,
                    task_id,
                    claim_id,
                    locked_at, // acquired_at
                    expires_at,
                    worktree_path,
                    locked_at,
                    lock_raw,
                    now
                ],
            )
            .map_err(sql_err)?;
        }

        for (idx, event) in snapshot.events.iter().enumerate() {
            let event_id = if event.event_id.is_empty() {
                format!("event-{}", idx + 1)
            } else {
                event.event_id.clone()
            };
            let seq = if event.seq == 0 {
                (idx + 1) as i64
            } else {
                event.seq
            };
            let ts = event.ts.as_str();
            let source = event.source.as_str();
            let task_id = event.task_id.as_deref().unwrap_or("");
            let event_type = event.event_type.as_str();
            let phase = event.phase.as_deref().unwrap_or("");
            let status = event.status.as_str();
            let payload = event.payload.clone();
            let payload_raw = serde_json::to_string(&payload).map_err(|e| MaccError::Storage {
                backend: "sqlite",
                message: format!("Failed to serialize event payload: {}", e),
            })?;
            let event_raw = serde_json::to_string(event).map_err(|e| MaccError::Storage {
                backend: "sqlite",
                message: format!("Failed to serialize raw event: {}", e),
            })?;
            tx.execute(
                "INSERT OR IGNORE INTO events (event_id, seq, ts, source, task_id, event_type, phase, status, payload_json, raw_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    event_id,
                    seq,
                    ts,
                    source,
                    task_id,
                    event_type,
                    phase,
                    status,
                    payload_raw,
                    event_raw
                ],
            )
            .map_err(sql_err)?;
        }

        if let Some(cursor) = &snapshot.cursor {
            let cursor_raw = serde_json::to_string(cursor).map_err(|e| MaccError::Storage {
                backend: "sqlite",
                message: format!("Failed to serialize cursor payload: {}", e),
            })?;
            let path = cursor.path.as_deref().unwrap_or("");
            let inode = cursor.inode.unwrap_or(0);
            let offset = i64::try_from(cursor.offset).unwrap_or(i64::MAX);
            let last_event_id = cursor.last_event_id.as_deref().unwrap_or("");
            let updated_at = cursor.updated_at.as_deref().unwrap_or(now.as_str());
            tx.execute(
                "INSERT INTO cursors (name, path, inode, offset, last_event_id, updated_at, payload_json)
                 VALUES ('coordinator', ?1, ?2, ?3, ?4, ?5, ?6)",
                params![path, inode, offset, last_event_id, updated_at, cursor_raw],
            )
            .map_err(sql_err)?;
        }

        tx.commit().map_err(sql_err)?;
        Ok(())
    }
}

// ── Tool throttle persistence (inherent impl, not part of the trait) ───

impl SqliteStorage {
    /// Load the persisted tool throttle registry from SQLite.
    pub fn load_throttle_registry(
        &self,
    ) -> Result<crate::coordinator::rate_limit::ToolThrottleRegistry> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let mut registry = crate::coordinator::rate_limit::ToolThrottleRegistry::new();
        let mut stmt = conn
            .prepare("SELECT tool_id, payload_json FROM tool_throttle")
            .map_err(sql_err)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(sql_err)?;
        for row in rows {
            let (tool_id, payload) = row.map_err(sql_err)?;
            if let Ok(state) =
                serde_json::from_str::<crate::coordinator::rate_limit::ToolThrottleState>(&payload)
            {
                registry.insert(tool_id, state);
            }
        }
        Ok(registry)
    }

    /// Persist the full tool throttle registry to SQLite (replace-all).
    pub fn save_throttle_registry(
        &self,
        registry: &crate::coordinator::rate_limit::ToolThrottleRegistry,
    ) -> Result<()> {
        let mut conn = self.open()?;
        self.init_schema(&conn)?;
        let tx = write_transaction(&mut conn)?;
        tx.execute("DELETE FROM tool_throttle", [])
            .map_err(sql_err)?;
        let now = now_iso_string();
        for (tool_id, state) in registry {
            let payload = serde_json::to_string(state).map_err(|e| MaccError::Storage {
                backend: "sqlite",
                message: format!("Failed to serialize throttle state for {}: {}", tool_id, e),
            })?;
            tx.execute(
                "INSERT INTO tool_throttle (tool_id, throttled_until, payload_json, updated_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![tool_id, state.throttled_until as i64, payload, now],
            )
            .map_err(sql_err)?;
        }
        tx.commit().map_err(sql_err)?;
        Ok(())
    }

    /// Upsert a single tool's throttle state.
    pub fn upsert_tool_throttle(
        &self,
        tool_id: &str,
        state: &crate::coordinator::rate_limit::ToolThrottleState,
    ) -> Result<()> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let now = now_iso_string();
        let payload = serde_json::to_string(state).map_err(|e| MaccError::Storage {
            backend: "sqlite",
            message: format!("Failed to serialize throttle state for {}: {}", tool_id, e),
        })?;
        conn.execute(
            "INSERT OR REPLACE INTO tool_throttle (tool_id, throttled_until, payload_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![tool_id, state.throttled_until as i64, payload, now],
        )
        .map_err(sql_err)?;
        Ok(())
    }

    /// Remove a single tool from the throttle registry.
    pub fn delete_tool_throttle(&self, tool_id: &str) -> Result<bool> {
        let conn = self.open()?;
        self.init_schema(&conn)?;
        let changed = conn
            .execute(
                "DELETE FROM tool_throttle WHERE tool_id = ?1",
                params![tool_id],
            )
            .map_err(sql_err)?;
        Ok(changed > 0)
    }
}

pub fn sync_coordinator_storage(
    project_paths: &ProjectPaths,
    mode: CoordinatorStorageMode,
    phase: CoordinatorStoragePhase,
) -> Result<()> {
    // Backward-compatible entry point. New code should call explicit transfer APIs.
    match (mode, phase) {
        (CoordinatorStorageMode::Json, _) => Ok(()),
        (CoordinatorStorageMode::DualWrite, _) => {
            coordinator_storage_import_json_to_sqlite(project_paths)
        }
        (CoordinatorStorageMode::Sqlite, CoordinatorStoragePhase::Pre) => {
            let imported = coordinator_storage_bootstrap_sqlite_from_json(project_paths)?;
            if !imported {
                coordinator_storage_export_sqlite_to_json(project_paths)?;
            }
            Ok(())
        }
        (CoordinatorStorageMode::Sqlite, CoordinatorStoragePhase::Post) => {
            coordinator_storage_export_sqlite_to_json(project_paths)
        }
    }
}

pub fn coordinator_storage_bootstrap_sqlite_from_json(
    project_paths: &ProjectPaths,
) -> Result<bool> {
    let paths = CoordinatorStoragePaths::from_project_paths(project_paths);
    let json_store = JsonStorage::new(paths.clone());
    let sqlite_store = SqliteStorage::new(paths);
    if sqlite_store.has_snapshot_data()? {
        return Ok(false);
    }
    let json_snapshot = json_store.load_snapshot()?;
    sqlite_store.save_snapshot(&json_snapshot)?;
    Ok(true)
}

pub fn coordinator_storage_import_json_to_sqlite(project_paths: &ProjectPaths) -> Result<()> {
    let paths = CoordinatorStoragePaths::from_project_paths(project_paths);
    let json_store = JsonStorage::new(paths.clone());
    let sqlite_store = SqliteStorage::new(paths);
    let json_snapshot = json_store.load_snapshot()?;
    sqlite_store.save_snapshot(&json_snapshot)
}

pub fn coordinator_storage_export_sqlite_to_json(project_paths: &ProjectPaths) -> Result<()> {
    let paths = CoordinatorStoragePaths::from_project_paths(project_paths);
    let json_store = JsonStorage::new(paths.clone());
    let sqlite_store = SqliteStorage::new(paths);
    let sqlite_snapshot = sqlite_store.load_snapshot()?;
    json_store.save_snapshot(&sqlite_snapshot)
}

pub fn coordinator_storage_verify_parity(project_paths: &ProjectPaths) -> Result<()> {
    let paths = CoordinatorStoragePaths::from_project_paths(project_paths);
    let json_store = JsonStorage::new(paths.clone());
    let sqlite_store = SqliteStorage::new(paths);
    let json_snapshot = json_store.load_snapshot()?;
    let sqlite_snapshot = sqlite_store.load_snapshot()?;
    if json_snapshot != sqlite_snapshot {
        return Err(MaccError::Storage {
            backend: "sqlite",
            message: "Coordinator storage mismatch: json and sqlite snapshots differ".into(),
        });
    }
    Ok(())
}

pub fn append_event_sqlite(project_paths: &ProjectPaths, event: &Value) -> Result<bool> {
    let paths = CoordinatorStoragePaths::from_project_paths(project_paths);
    let sqlite = SqliteStorage::new(paths);
    sqlite.append_event(event)
}

pub fn append_event_record_sqlite(
    project_paths: &ProjectPaths,
    event: &CoordinatorEventRecord,
) -> Result<bool> {
    let paths = CoordinatorStoragePaths::from_project_paths(project_paths);
    let sqlite = SqliteStorage::new(paths);
    sqlite.append_event_record(event)
}

pub fn apply_transition_sqlite(
    project_paths: &ProjectPaths,
    change: &TransitionMutation,
) -> Result<()> {
    apply_transition_sqlite_with_event(project_paths, change, None)
}

pub fn apply_transition_sqlite_with_event(
    project_paths: &ProjectPaths,
    change: &TransitionMutation,
    event: Option<&EventMutation>,
) -> Result<()> {
    let paths = CoordinatorStoragePaths::from_project_paths(project_paths);
    let sqlite = SqliteStorage::new(paths);
    sqlite.apply_transition_with_event(change, event)
}

pub fn set_runtime_sqlite(project_paths: &ProjectPaths, change: &RuntimeMutation) -> Result<()> {
    set_runtime_sqlite_with_event(project_paths, change, None)
}

pub fn set_runtime_sqlite_with_event(
    project_paths: &ProjectPaths,
    change: &RuntimeMutation,
    event: Option<&EventMutation>,
) -> Result<()> {
    let paths = CoordinatorStoragePaths::from_project_paths(project_paths);
    let sqlite = SqliteStorage::new(paths);
    sqlite.set_runtime_with_event(change, event)
}

pub fn set_merge_pending_sqlite(
    project_paths: &ProjectPaths,
    change: &MergePendingMutation,
) -> Result<()> {
    let paths = CoordinatorStoragePaths::from_project_paths(project_paths);
    let sqlite = SqliteStorage::new(paths);
    sqlite.set_merge_pending(change)
}

pub fn set_merge_processed_sqlite(
    project_paths: &ProjectPaths,
    change: &MergeProcessedMutation,
) -> Result<()> {
    let paths = CoordinatorStoragePaths::from_project_paths(project_paths);
    let sqlite = SqliteStorage::new(paths);
    sqlite.set_merge_processed(change)
}

pub fn increment_retries_sqlite(
    project_paths: &ProjectPaths,
    change: &RetryIncrementMutation,
) -> Result<()> {
    let paths = CoordinatorStoragePaths::from_project_paths(project_paths);
    let sqlite = SqliteStorage::new(paths);
    sqlite.increment_retries(change)
}

pub fn upsert_slo_warning_sqlite(
    project_paths: &ProjectPaths,
    change: &SloWarningMutation,
) -> Result<()> {
    let paths = CoordinatorStoragePaths::from_project_paths(project_paths);
    let sqlite = SqliteStorage::new(paths);
    sqlite.upsert_slo_warning(change)
}

#[cfg(test)]
fn read_json_or_default(path: &Path, action: &str, default: Value) -> Result<Value> {
    if !path.exists() {
        return Ok(default);
    }
    let raw = fs::read_to_string(path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: action.into(),
        source: e,
    })?;
    serde_json::from_str::<Value>(&raw).map_err(|e| MaccError::Storage {
        backend: "json",
        message: format!("Failed to parse {}: {}", path.display(), e),
    })
}

fn read_json_typed_or_default<T>(path: &Path, action: &str, default: T) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    if !path.exists() {
        return Ok(default);
    }
    let raw = fs::read_to_string(path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: action.into(),
        source: e,
    })?;
    serde_json::from_str::<T>(&raw).map_err(|e| MaccError::Storage {
        backend: "json",
        message: format!("Failed to parse {}: {}", path.display(), e),
    })
}

fn read_registry_or_default(path: &Path, action: &str) -> Result<TaskRegistry> {
    if !path.exists() {
        return Ok(default_registry());
    }
    let raw = fs::read_to_string(path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: action.into(),
        source: e,
    })?;
    serde_json::from_str::<TaskRegistry>(&raw).map_err(|e| MaccError::Storage {
        backend: "json",
        message: format!("Failed to parse {}: {}", path.display(), e),
    })
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| MaccError::Io {
            path: parent.to_string_lossy().into(),
            action: "create parent directory".into(),
            source: e,
        })?;
    }
    Ok(())
}

fn write_text_atomic(path: &Path, content: &str) -> Result<()> {
    ensure_parent_dir(path)?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content).map_err(|e| MaccError::Io {
        path: tmp.to_string_lossy().into(),
        action: "write temp file".into(),
        source: e,
    })?;
    fs::rename(&tmp, path).map_err(|e| MaccError::Io {
        path: path.to_string_lossy().into(),
        action: "replace destination file".into(),
        source: e,
    })?;
    Ok(())
}

fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let content = serde_json::to_string_pretty(value).map_err(|e| MaccError::Storage {
        backend: "json",
        message: format!("Failed to serialize json: {}", e),
    })?;
    write_text_atomic(path, &content)
}

fn now_iso_string() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn is_active_state(state: &str) -> bool {
    matches!(
        state,
        "claimed" | "in_progress" | "pr_open" | "changes_requested" | "queued"
    )
}

fn default_registry() -> TaskRegistry {
    let mut registry = TaskRegistry {
        updated_at: Some(now_iso_string()),
        ..TaskRegistry::default()
    };
    registry
        .extra
        .insert("schema_version".into(), Value::from(1));
    registry
        .extra
        .insert("processed_event_ids".into(), json!({}));
    registry.extra.insert("state_mapping".into(), json!({}));
    registry
}

fn parse_task_payload(task_id: &str, raw: &str) -> Result<Task> {
    serde_json::from_str::<Task>(raw).map_err(|e| MaccError::Storage {
        backend: "sqlite",
        message: format!(
            "Failed to parse typed task payload for '{}': {}",
            task_id, e
        ),
    })
}

/// Start a write transaction holding the write lock from its first statement.
///
/// A deferred transaction that reads before it writes cannot upgrade once
/// another connection (a performer event, the TUI, the web server) has
/// committed in between: SQLite fails it at once with "database is locked",
/// without waiting on the busy timeout. Taking the lock up front makes the
/// transaction wait its turn instead.
fn write_transaction(conn: &mut Connection) -> Result<rusqlite::Transaction<'_>> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_err)
}

fn sql_err(e: rusqlite::Error) -> MaccError {
    MaccError::Coordinator {
        code: "E411",
        message: format!(
            "SQLite coordinator storage error (Runtime ledger write failed): {}",
            e
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_project_root(prefix: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("{}_{}", prefix, nonce));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn gate_decision_ledger_round_trips_in_order() {
        use crate::coordinator::model::{ApprovalDecision, ApprovalRecord};
        let root = temp_project_root("macc_gate_ledger");
        let storage = SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(
            &crate::ProjectPaths::from_root(&root),
        ));
        let rec = |decision, actor: &str, at: &str| ApprovalRecord {
            decision,
            role: "PRODUCT_OWNER".into(),
            actor: actor.into(),
            revision: Some("abcdef1234".into()),
            evidence: Some("https://example/pr/1".into()),
            reason: None,
            recorded_at: at.into(),
        };
        storage
            .append_gate_decision(
                "GATE-1",
                &rec(
                    ApprovalDecision::ChangesRequested,
                    "alice",
                    "2026-09-21T10:00:00Z",
                ),
            )
            .unwrap();
        storage
            .append_gate_decision(
                "GATE-1",
                &rec(ApprovalDecision::Approved, "alice", "2026-09-21T11:00:00Z"),
            )
            .unwrap();
        storage
            .append_gate_decision(
                "GATE-2",
                &rec(ApprovalDecision::Rejected, "bob", "2026-09-21T12:00:00Z"),
            )
            .unwrap();
        let ledger = storage.gate_decisions_by_task().unwrap();
        let g1 = &ledger["GATE-1"];
        assert_eq!(g1.len(), 2);
        assert_eq!(
            g1[0].decision,
            ApprovalDecision::ChangesRequested,
            "oldest first"
        );
        assert_eq!(g1[1].decision, ApprovalDecision::Approved);
        assert_eq!(g1[1].evidence.as_deref(), Some("https://example/pr/1"));
        assert_eq!(ledger["GATE-2"][0].actor, "bob");
        let _ = fs::remove_dir_all(root);
    }

    fn seed_files(paths: &CoordinatorStoragePaths) {
        ensure_parent_dir(&paths.registry_json_path).unwrap();
        ensure_parent_dir(&paths.events_jsonl_path).unwrap();
        ensure_parent_dir(&paths.cursor_json_path).unwrap();
        let registry = json!({
            "schema_version": 1,
            "tasks": [
                {
                    "id": "TASK-1",
                    "title": "Task One",
                    "state": "in_progress",
                    "tool": "codex",
                    "gate": {
                        "required_verdict": "accepted",
                        "description": "Release gate"
                    },
                    "blocked_on_external": {
                        "reason": "Waiting for production evidence",
                        "clears_when": "GAP-1 is accepted",
                        "tracking_id": "GAP-1",
                        "source": "operator"
                    },
                    "task_runtime": {
                        "status": "running",
                        "pid": 1234,
                        "current_phase": "dev",
                        "last_heartbeat": "2026-02-20T00:00:05Z",
                        "metrics": {
                            "retries": 2
                        },
                        "gate_verdict": "rejected",
                        "external_block_resolution": {
                            "evidence": "Approved in release review",
                            "resolved_at": "2026-02-20T00:00:04Z",
                            "block_reason": "Waiting for production evidence",
                            "tracking_id": "GAP-1"
                        },
                        "last_error": null
                    }
                }
            ],
            "resource_locks": {
                "service-skeleton": {
                    "task_id": "TASK-1",
                    "worktree_path": "/tmp/wt-1",
                    "locked_at": "2026-02-20T00:00:00Z"
                }
            },
            "processed_event_ids": {},
            "state_mapping": {},
            "updated_at": "2026-02-20T00:00:00Z"
        });
        write_json_atomic(&paths.registry_json_path, &registry).unwrap();
        write_text_atomic(
            &paths.events_jsonl_path,
            "{\"event_id\":\"evt-1\",\"seq\":1,\"ts\":\"2026-02-20T00:00:01Z\",\"source\":\"coordinator\",\"type\":\"task_dispatched\",\"task_id\":\"TASK-1\",\"status\":\"started\",\"payload\":{}}\n",
        )
        .unwrap();
        write_json_atomic(
            &paths.cursor_json_path,
            &CoordinatorCursor {
                path: Some(paths.events_jsonl_path.to_string_lossy().to_string()),
                inode: Some(1),
                offset: 100,
                last_event_id: Some("evt-1".to_string()),
                updated_at: Some("2026-02-20T00:00:01Z".to_string()),
                extra: Default::default(),
            },
        )
        .unwrap();
    }

    #[test]
    fn dual_write_preserves_equivalence() {
        let root = temp_project_root("macc_coord_storage_dual");
        let project_paths = ProjectPaths::from_root(&root);
        let storage_paths = CoordinatorStoragePaths::from_project_paths(&project_paths);
        seed_files(&storage_paths);

        sync_coordinator_storage(
            &project_paths,
            CoordinatorStorageMode::DualWrite,
            CoordinatorStoragePhase::Post,
        )
        .unwrap();

        let json_snapshot = JsonStorage::new(storage_paths.clone())
            .load_snapshot()
            .unwrap();
        let sqlite_snapshot = SqliteStorage::new(storage_paths).load_snapshot().unwrap();
        assert_eq!(json_snapshot, sqlite_snapshot);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sqlite_pre_phase_exports_existing_sqlite_snapshot() {
        let root = temp_project_root("macc_coord_storage_sqlite");
        let project_paths = ProjectPaths::from_root(&root);
        let storage_paths = CoordinatorStoragePaths::from_project_paths(&project_paths);
        seed_files(&storage_paths);

        sync_coordinator_storage(
            &project_paths,
            CoordinatorStorageMode::DualWrite,
            CoordinatorStoragePhase::Post,
        )
        .unwrap();

        write_json_atomic(&storage_paths.registry_json_path, &json!({"broken": true})).unwrap();

        sync_coordinator_storage(
            &project_paths,
            CoordinatorStorageMode::Sqlite,
            CoordinatorStoragePhase::Pre,
        )
        .unwrap();

        let restored = read_json_or_default(
            &storage_paths.registry_json_path,
            "read restored registry",
            json!({}),
        )
        .unwrap();
        assert!(restored.get("tasks").is_some());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sqlite_roundtrip_preserves_cursor_payload() {
        let root = temp_project_root("macc_coord_storage_cursor");
        let project_paths = ProjectPaths::from_root(&root);
        let storage_paths = CoordinatorStoragePaths::from_project_paths(&project_paths);
        seed_files(&storage_paths);

        sync_coordinator_storage(
            &project_paths,
            CoordinatorStorageMode::DualWrite,
            CoordinatorStoragePhase::Post,
        )
        .unwrap();

        let sqlite_snapshot = SqliteStorage::new(storage_paths.clone())
            .load_snapshot()
            .unwrap();
        let cursor = sqlite_snapshot.cursor.expect("cursor from sqlite");
        assert_eq!(cursor.offset, 100, "cursor offset must roundtrip",);
        assert_eq!(cursor.inode, Some(1), "cursor inode must roundtrip",);
        assert_eq!(
            cursor.last_event_id.as_deref(),
            Some("evt-1"),
            "cursor last_event_id must roundtrip",
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sqlite_roundtrip_preserves_runtime_fields() {
        let root = temp_project_root("macc_coord_storage_runtime");
        let project_paths = ProjectPaths::from_root(&root);
        let storage_paths = CoordinatorStoragePaths::from_project_paths(&project_paths);
        seed_files(&storage_paths);

        sync_coordinator_storage(
            &project_paths,
            CoordinatorStorageMode::DualWrite,
            CoordinatorStoragePhase::Post,
        )
        .unwrap();

        // Simulate restart in sqlite mode: restore JSON from sqlite snapshot.
        write_json_atomic(&storage_paths.registry_json_path, &json!({"broken": true})).unwrap();
        sync_coordinator_storage(
            &project_paths,
            CoordinatorStorageMode::Sqlite,
            CoordinatorStoragePhase::Pre,
        )
        .unwrap();

        let restored = read_json_or_default(
            &storage_paths.registry_json_path,
            "read restored registry",
            json!({}),
        )
        .unwrap();
        let task = restored["tasks"][0].clone();
        assert_eq!(
            task["task_runtime"]["status"].as_str(),
            Some("running"),
            "runtime status should survive sqlite roundtrip"
        );
        assert_eq!(
            task["task_runtime"]["pid"].as_i64(),
            Some(1234),
            "runtime pid should survive sqlite roundtrip"
        );
        assert_eq!(
            task["task_runtime"]["current_phase"].as_str(),
            Some("dev"),
            "runtime current phase should survive sqlite roundtrip"
        );
        assert_eq!(
            task["task_runtime"]["last_heartbeat"].as_str(),
            Some("2026-02-20T00:00:05Z"),
            "runtime heartbeat should survive sqlite roundtrip"
        );
        assert_eq!(
            task["task_runtime"]["metrics"]["retries"].as_i64(),
            Some(2),
            "runtime retries metric should survive sqlite roundtrip"
        );
        assert_eq!(
            task["gate"]["required_verdict"].as_str(),
            Some("accepted"),
            "gate contract should survive sqlite roundtrip"
        );
        assert_eq!(
            task["task_runtime"]["gate_verdict"].as_str(),
            Some("rejected"),
            "gate verdict should survive sqlite roundtrip"
        );
        assert_eq!(
            task["task_runtime"]["external_block_resolution"]["tracking_id"].as_str(),
            Some("GAP-1"),
            "external block resolution should survive sqlite roundtrip"
        );
        assert_eq!(
            task["blocked_on_external"]["source"].as_str(),
            Some("operator"),
            "external block provenance should survive sqlite roundtrip"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn migration_same_event_stream_same_final_state() {
        let root_json = temp_project_root("macc_coord_storage_json_baseline");
        let paths_json = ProjectPaths::from_root(&root_json);
        let storage_paths_json = CoordinatorStoragePaths::from_project_paths(&paths_json);
        seed_files(&storage_paths_json);

        let root_sqlite = temp_project_root("macc_coord_storage_sqlite_migration");
        let paths_sqlite = ProjectPaths::from_root(&root_sqlite);
        let storage_paths_sqlite = CoordinatorStoragePaths::from_project_paths(&paths_sqlite);
        seed_files(&storage_paths_sqlite);

        let baseline_snapshot = JsonStorage::new(storage_paths_json)
            .load_snapshot()
            .unwrap();

        sync_coordinator_storage(
            &paths_sqlite,
            CoordinatorStorageMode::Sqlite,
            CoordinatorStoragePhase::Pre,
        )
        .unwrap();
        sync_coordinator_storage(
            &paths_sqlite,
            CoordinatorStorageMode::Sqlite,
            CoordinatorStoragePhase::Post,
        )
        .unwrap();

        let migrated_snapshot = JsonStorage::new(storage_paths_sqlite.clone())
            .load_snapshot()
            .unwrap();
        assert_eq!(baseline_snapshot.registry, migrated_snapshot.registry);
        assert_eq!(baseline_snapshot.events, migrated_snapshot.events);
        assert_eq!(
            baseline_snapshot.cursor.as_ref().map(|c| c.offset),
            migrated_snapshot.cursor.as_ref().map(|c| c.offset)
        );
        assert_eq!(
            baseline_snapshot.cursor.as_ref().and_then(|c| c.inode),
            migrated_snapshot.cursor.as_ref().and_then(|c| c.inode)
        );
        assert_eq!(
            baseline_snapshot
                .cursor
                .as_ref()
                .and_then(|c| c.last_event_id.clone()),
            migrated_snapshot
                .cursor
                .as_ref()
                .and_then(|c| c.last_event_id.clone())
        );

        sync_coordinator_storage(
            &paths_sqlite,
            CoordinatorStorageMode::Sqlite,
            CoordinatorStoragePhase::Post,
        )
        .unwrap();
        let replay_snapshot = JsonStorage::new(storage_paths_sqlite)
            .load_snapshot()
            .unwrap();
        assert_eq!(migrated_snapshot, replay_snapshot);

        let _ = fs::remove_dir_all(root_json);
        let _ = fs::remove_dir_all(root_sqlite);
    }

    #[test]
    fn sqlite_pre_recovers_after_json_loss() {
        let root = temp_project_root("macc_coord_storage_restart");
        let project_paths = ProjectPaths::from_root(&root);
        let storage_paths = CoordinatorStoragePaths::from_project_paths(&project_paths);
        seed_files(&storage_paths);

        // Persist canonical snapshot into SQLite.
        sync_coordinator_storage(
            &project_paths,
            CoordinatorStorageMode::DualWrite,
            CoordinatorStoragePhase::Post,
        )
        .unwrap();

        let baseline = JsonStorage::new(storage_paths.clone())
            .load_snapshot()
            .unwrap();

        // Simulate crash/file loss on JSON side.
        std::fs::remove_file(&storage_paths.registry_json_path).unwrap();
        std::fs::remove_file(&storage_paths.events_jsonl_path).unwrap();
        std::fs::remove_file(&storage_paths.cursor_json_path).unwrap();

        // Restart: pre-phase should rehydrate JSON from SQLite source.
        sync_coordinator_storage(
            &project_paths,
            CoordinatorStorageMode::Sqlite,
            CoordinatorStoragePhase::Pre,
        )
        .unwrap();

        let restored = JsonStorage::new(storage_paths).load_snapshot().unwrap();
        assert_eq!(baseline.registry, restored.registry);
        assert_eq!(baseline.events, restored.events);
        assert_eq!(baseline.cursor, restored.cursor);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn set_runtime_with_event_is_atomic_and_idempotent() {
        let root = temp_project_root("macc_coord_storage_runtime_event");
        let project_paths = ProjectPaths::from_root(&root);
        let storage_paths = CoordinatorStoragePaths::from_project_paths(&project_paths);
        seed_files(&storage_paths);

        coordinator_storage_import_json_to_sqlite(&project_paths).unwrap();
        let now = "2026-02-25T00:00:00Z".to_string();
        let runtime = RuntimeMutation {
            task_id: "TASK-1".to_string(),
            runtime_status: "phase_done".to_string(),
            phase: "dev".to_string(),
            pid: None,
            last_error: "".to_string(),
            heartbeat_ts: now.clone(),
            attempt: Some(1),
            now: now.clone(),
            message: None,
            progress: None,
            worker_id: None,
            tool: None,
            worktree: None,
            branch: None,
            stdout_log: None,
            stderr_log: None,
            events_log: None,
            last_event_at: None,
        };
        let event = EventMutation {
            event_id: Some("evt-runtime-done-1".to_string()),
            run_id: Some("run-test".to_string()),
            seq: Some(42),
            ts: Some(now.clone()),
            source: "test".to_string(),
            task_id: "TASK-1".to_string(),
            event_type: "phase_result".to_string(),
            phase: "dev".to_string(),
            status: "done".to_string(),
            payload: json!({"message":"phase done"}).into(),
        };
        set_runtime_sqlite_with_event(&project_paths, &runtime, Some(&event)).unwrap();
        set_runtime_sqlite_with_event(&project_paths, &runtime, Some(&event)).unwrap();

        let snapshot = SqliteStorage::new(storage_paths).load_snapshot().unwrap();
        let task = snapshot.registry.find_task("TASK-1").unwrap();
        assert_eq!(task.task_runtime.status.as_deref(), Some("phase_done"));
        let matching_events = snapshot
            .events
            .iter()
            .filter(|e| e.event_id == "evt-runtime-done-1")
            .count();
        assert_eq!(matching_events, 1, "event insert must stay idempotent");

        let _ = fs::remove_dir_all(root);
    }

    /// Ghost cleanup asks this question to tell "the performer crashed" apart
    /// from "the performer finished and exited". Both leave a dead PID and a
    /// stale heartbeat; only the event log distinguishes them.
    #[test]
    fn terminal_result_lookup_is_scoped_to_the_claim() {
        let root = temp_project_root("macc_terminal_result");
        let project_paths = ProjectPaths::from_root(&root);
        let sqlite =
            SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&project_paths));

        let mut event = CoordinatorEventRecord {
            event_id: "T1-1-abc".to_string(),
            seq: 1,
            ts: "2026-08-14T11:39:12Z".to_string(),
            source: "coordinator-worktree:T1".to_string(),
            task_id: Some("T1".to_string()),
            event_type: "phase_result".to_string(),
            claim_id: Some("claim-1".to_string()),
            ..Default::default()
        };
        sqlite.append_event_record(&event).expect("append event");

        assert!(
            sqlite.task_has_terminal_result_for_claim("T1", "claim-1"),
            "the claim that produced the result must be recognised"
        );
        assert!(
            !sqlite.task_has_terminal_result_for_claim("T1", "claim-2"),
            "a later claim must not inherit an earlier claim's result"
        );
        assert!(
            !sqlite.task_has_terminal_result_for_claim("T2", "claim-1"),
            "results must not leak across tasks"
        );
        assert!(
            !sqlite.task_has_terminal_result_for_claim("", "claim-1")
                && !sqlite.task_has_terminal_result_for_claim("T1", ""),
            "an unidentified task or claim proves nothing"
        );

        // A non-terminal event for the same claim must not count as a result.
        event.event_id = "T1-2-def".to_string();
        event.seq = 2;
        event.event_type = "heartbeat".to_string();
        sqlite
            .append_event_record(&event)
            .expect("append heartbeat");
        assert!(
            !sqlite.task_has_terminal_result_for_claim("T1", "claim-9"),
            "heartbeats are not results"
        );

        let _ = fs::remove_dir_all(root);
    }

    /// An unreadable or absent event log must not keep a dead task alive.
    #[test]
    fn terminal_result_lookup_fails_closed() {
        let root = temp_project_root("macc_terminal_result_missing");
        let project_paths = ProjectPaths::from_root(&root);
        let sqlite =
            SqliteStorage::new(CoordinatorStoragePaths::from_project_paths(&project_paths));
        assert!(
            !sqlite.task_has_terminal_result_for_claim("T1", "claim-1"),
            "no event log means no proof of completion"
        );
        let _ = fs::remove_dir_all(root);
    }

    fn event_log_storage(prefix: &str) -> (PathBuf, SqliteStorage) {
        let root = temp_project_root(prefix);
        let paths = CoordinatorStoragePaths::from_project_paths(&ProjectPaths::from_root(&root));
        let sqlite = SqliteStorage::new(paths);
        let records = [
            (1, "T-1", "started"),
            (2, "T-1", "heartbeat"),
            (3, "T-2", "phase_result"),
            (4, "T-1", "phase_result"),
            (5, "", "coordinator_heartbeat"),
            (6, "T-1", "heartbeat"),
        ];
        for (seq, task, kind) in records {
            let record: CoordinatorEventRecord = serde_json::from_value(json!({
                "event_id": format!("evt-{seq}"),
                "run_id": "run-1",
                "seq": seq,
                "ts": "2026-10-14T00:00:00Z",
                "type": kind,
                "task_id": (!task.is_empty()).then_some(task),
                "payload": {}
            }))
            .unwrap();
            sqlite.append_event_record(&record).unwrap();
        }
        (root, sqlite)
    }

    fn scanned_ids(
        sqlite: &SqliteStorage,
        query: EventQuery<'_>,
        stop_after: usize,
    ) -> Vec<String> {
        let mut ids = Vec::new();
        sqlite
            .scan_events_newest_first(query, |event| {
                ids.push(event.event_id);
                if ids.len() >= stop_after {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            })
            .unwrap();
        ids
    }

    #[test]
    fn recent_events_are_the_newest_rows_in_log_order() {
        let (root, sqlite) = event_log_storage("macc_recent_events");
        let ids: Vec<String> = sqlite
            .load_recent_events(3)
            .unwrap()
            .into_iter()
            .map(|e| e.event_id)
            .collect();
        assert_eq!(ids, ["evt-4", "evt-5", "evt-6"]);
        assert!(sqlite.load_recent_events(0).unwrap().is_empty());
        // evt-2 was superseded by the later T-1 heartbeat evt-6.
        assert_eq!(sqlite.load_recent_events(100).unwrap().len(), 5);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn event_scans_filter_and_stop_early() {
        let (root, sqlite) = event_log_storage("macc_scan_events");
        let task_only = EventQuery {
            task_id: Some("T-1"),
            ..Default::default()
        };
        assert_eq!(
            scanned_ids(&sqlite, task_only, usize::MAX),
            ["evt-6", "evt-4", "evt-1"]
        );
        let no_heartbeats = EventQuery {
            task_id: Some("T-1"),
            skip_heartbeats: true,
            ..Default::default()
        };
        assert_eq!(
            scanned_ids(&sqlite, no_heartbeats, usize::MAX),
            ["evt-4", "evt-1"]
        );
        let all_without_heartbeats = EventQuery {
            skip_heartbeats: true,
            ..Default::default()
        };
        assert_eq!(
            scanned_ids(&sqlite, all_without_heartbeats, usize::MAX),
            ["evt-4", "evt-3", "evt-1"]
        );
        let latest_result = EventQuery {
            event_type: Some("phase_result"),
            ..Default::default()
        };
        assert_eq!(scanned_ids(&sqlite, latest_result, 1), ["evt-4"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn schema_setup_runs_once_and_indexes_the_event_log() {
        let (root, sqlite) = event_log_storage("macc_schema_version");
        let conn = sqlite.open().unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SQLITE_SCHEMA_VERSION);
        let plan: String = conn
            .query_row(
                "EXPLAIN QUERY PLAN SELECT raw_json FROM events ORDER BY seq DESC, event_id DESC LIMIT 5",
                [],
                |row| row.get(3),
            )
            .unwrap();
        assert!(plan.contains("idx_events_seq"), "{plan}");

        // A database written before the version stamp gets the new indexes.
        conn.execute_batch("DROP INDEX idx_events_seq; PRAGMA user_version = 0;")
            .unwrap();
        sqlite.init_schema(&conn).unwrap();
        let restored: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM sqlite_master WHERE name = 'idx_events_seq'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(restored, 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_registry_save_without_events_keeps_the_event_log() {
        let (root, sqlite) = event_log_storage("macc_save_without_events");
        let mut snapshot = sqlite.load_snapshot_without_events().unwrap();
        assert!(snapshot.events.is_empty());
        snapshot
            .registry
            .tasks
            .push(serde_json::from_value(json!({"id": "T-1", "state": "todo"})).unwrap());
        sqlite.save_snapshot(&snapshot).unwrap();
        let reloaded = sqlite.load_snapshot().unwrap();
        assert_eq!(reloaded.events.len(), 5);
        assert_eq!(sqlite.load_registry().unwrap().tasks.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn only_the_last_heartbeat_of_a_task_is_kept() {
        let root = temp_project_root("macc_heartbeat_prune");
        let paths = CoordinatorStoragePaths::from_project_paths(&ProjectPaths::from_root(&root));
        let sqlite = SqliteStorage::new(paths);
        let append = |seq: i64, task: &str, kind: &str| {
            let record: CoordinatorEventRecord = serde_json::from_value(json!({
                "event_id": format!("evt-{seq}"),
                "seq": seq,
                "ts": "2026-10-14T00:00:00Z",
                "type": kind,
                "task_id": task,
                "payload": {}
            }))
            .unwrap();
            sqlite.append_event_record(&record).unwrap();
        };
        let ids = |sqlite: &SqliteStorage| -> Vec<String> {
            sqlite
                .load_recent_events(100)
                .unwrap()
                .into_iter()
                .map(|e| e.event_id)
                .collect()
        };
        append(1, "A", "heartbeat");
        append(2, "B", "heartbeat");
        append(3, "A", "progress");
        append(4, "A", "heartbeat");
        assert_eq!(ids(&sqlite), ["evt-2", "evt-3", "evt-4"]);

        // A heartbeat the replay cursor points at survives newer ones.
        let conn = sqlite.open().unwrap();
        conn.execute(
            "INSERT INTO event_cursor (stream, last_event_id) VALUES ('coordinator', 'evt-4')",
            [],
        )
        .unwrap();
        append(5, "A", "heartbeat");
        append(6, "A", "heartbeat");
        assert_eq!(ids(&sqlite), ["evt-2", "evt-3", "evt-4", "evt-6"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn older_logs_are_pruned_once_on_upgrade() {
        let root = temp_project_root("macc_heartbeat_migration");
        let paths = CoordinatorStoragePaths::from_project_paths(&ProjectPaths::from_root(&root));
        let sqlite = SqliteStorage::new(paths);
        let conn = sqlite.open().unwrap();
        sqlite.init_schema(&conn).unwrap();
        // Rows written by a binary that kept every heartbeat.
        for (seq, task, kind) in [
            (1, "A", "heartbeat"),
            (2, "A", "heartbeat"),
            (3, "A", "started"),
        ] {
            conn.execute(
                "INSERT INTO events (event_id, seq, ts, source, task_id, event_type, phase, status, payload_json, raw_json)
                 VALUES (?1, ?2, '', '', ?3, ?4, '', '', '{}', ?5)",
                params![
                    format!("evt-{seq}"),
                    seq,
                    task,
                    kind,
                    json!({"event_id": format!("evt-{seq}"), "seq": seq, "type": kind, "task_id": task}).to_string()
                ],
            )
            .unwrap();
        }
        conn.execute_batch("PRAGMA user_version = 1;").unwrap();
        sqlite.init_schema(&conn).unwrap();
        let left: Vec<String> = sqlite
            .load_recent_events(10)
            .unwrap()
            .into_iter()
            .map(|e| e.event_id)
            .collect();
        assert_eq!(left, ["evt-2", "evt-3"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn events_after_a_seq_come_in_log_order() {
        let (root, sqlite) = event_log_storage("macc_events_after");
        let ids: Vec<String> = sqlite
            .load_events_after(3, 10)
            .unwrap()
            .into_iter()
            .map(|e| e.event_id)
            .collect();
        assert_eq!(ids, ["evt-4", "evt-5", "evt-6"]);
        assert_eq!(sqlite.load_events_after(3, 1).unwrap().len(), 1);
        assert_eq!(sqlite.event_seq("evt-4").unwrap(), Some(4));
        assert_eq!(sqlite.event_seq("missing").unwrap(), None);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_replaced_database_file_gets_a_fresh_connection() {
        let (root, sqlite) = event_log_storage("macc_pool_replace");
        assert!(!sqlite.load_recent_events(10).unwrap().is_empty());
        // The pooled connection points at the old file; a new one must be used.
        let db = sqlite.paths.sqlite_path.clone();
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{}", db.display(), suffix));
        }
        assert!(sqlite.load_recent_events(10).unwrap().is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_save_rewrites_only_the_tasks_that_changed() {
        let root = temp_project_root("macc_incremental_save");
        let paths = CoordinatorStoragePaths::from_project_paths(&ProjectPaths::from_root(&root));
        let sqlite = SqliteStorage::new(paths);
        let task = |id: &str, state: &str| -> Task {
            serde_json::from_value(json!({"id": id, "state": state, "updated_at": "t0"})).unwrap()
        };
        let mut snapshot = CoordinatorSnapshot::empty();
        snapshot.registry.tasks = vec![task("A", "todo"), task("B", "todo"), task("C", "todo")];
        sqlite.save_snapshot(&snapshot).unwrap();

        // Mark every stored row, then save with one task changed and one gone.
        let conn = sqlite.open().unwrap();
        conn.execute("UPDATE tasks SET updated_at = 'untouched'", [])
            .unwrap();
        snapshot.registry.tasks = vec![task("A", "todo"), task("B", "in_progress")];
        sqlite.save_snapshot(&snapshot).unwrap();

        let rows: Vec<(String, String, String)> = conn
            .prepare("SELECT task_id, state, updated_at FROM tasks ORDER BY task_id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert_eq!(
            rows,
            [
                ("A".into(), "todo".into(), "untouched".into()),
                ("B".into(), "in_progress".into(), "t0".into()),
            ]
        );
        let runtime_tasks: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM task_runtime WHERE task_id = 'C'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(runtime_tasks, 0);
        // The tasks live in their table only.
        let meta: String = conn
            .query_row(
                "SELECT value FROM metadata WHERE key = 'registry_json'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!meta.contains("in_progress"), "{meta}");
        assert_eq!(sqlite.load_registry().unwrap().tasks.len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn performer_heartbeats_sharing_a_seq_are_pruned_by_arrival() {
        let root = temp_project_root("macc_heartbeat_same_seq");
        let paths = CoordinatorStoragePaths::from_project_paths(&ProjectPaths::from_root(&root));
        let sqlite = SqliteStorage::new(paths);
        // Performers number their own events: every heartbeat carries seq 1.
        for n in 1..=3 {
            let record: CoordinatorEventRecord = serde_json::from_value(json!({
                "event_id": format!("T-1-hb-{n}"),
                "seq": 1,
                "type": "heartbeat",
                "task_id": "T-1",
                "payload": {}
            }))
            .unwrap();
            sqlite.append_event_record(&record).unwrap();
        }
        let left: Vec<String> = sqlite
            .load_recent_events(10)
            .unwrap()
            .into_iter()
            .map(|e| e.event_id)
            .collect();
        assert_eq!(left, ["T-1-hb-3"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_save_waits_for_a_concurrent_writer_instead_of_failing() {
        let root = temp_project_root("macc_concurrent_save");
        let paths = CoordinatorStoragePaths::from_project_paths(&ProjectPaths::from_root(&root));
        let sqlite = SqliteStorage::new(paths);
        let mut snapshot = CoordinatorSnapshot::empty();
        snapshot.registry.tasks =
            vec![serde_json::from_value(json!({"id": "A", "state": "todo"})).unwrap()];
        sqlite.save_snapshot(&snapshot).unwrap();

        // Another process writes, holding the write lock for a while, and
        // commits while the save is under way.
        let other = sqlite.clone();
        let (locked_tx, locked_rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let mut conn = other.open().unwrap();
            let tx = write_transaction(&mut conn).unwrap();
            tx.execute(
                "UPDATE tasks SET updated_at = 'other' WHERE task_id = 'A'",
                [],
            )
            .unwrap();
            locked_tx.send(()).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(300));
            tx.commit().unwrap();
        });
        locked_rx.recv().unwrap();
        snapshot.registry.tasks[0].state = "in_progress".into();
        sqlite
            .save_snapshot(&snapshot)
            .expect("the save waits for the other writer");
        writer.join().unwrap();
        assert_eq!(
            sqlite.load_registry().unwrap().tasks[0].state,
            "in_progress"
        );
        let _ = fs::remove_dir_all(root);
    }
}

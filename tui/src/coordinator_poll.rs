//! Coordinator polling off the UI thread.
//!
//! The live view refreshes the task registry and the recent events every
//! second while a coordinator runs, and the run status every few seconds.
//! Each refresh reads the coordinator database; done on the UI thread, every
//! read froze rendering and key handling for its duration. A poll runs on a
//! worker thread instead and the UI applies its result on a later tick.

use macc_core::coordinator::model::TaskRegistry;
use macc_core::coordinator::CoordinatorEventRecord;
use macc_core::coordinator_storage::{
    CoordinatorStorage, CoordinatorStoragePaths, JsonStorage, SqliteStorage,
};
use macc_core::service::coordinator_workflow::CoordinatorStatus;
use macc_core::ProjectPaths;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{Duration, Instant};

/// How often the run status (summary, pending approvals) is re-read.
pub const STATUS_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// What a poll reads.
pub struct PollRequest {
    pub paths: ProjectPaths,
    pub legacy_json_fallback: bool,
    pub events_window: usize,
    pub include_status: bool,
}

/// What a poll read. `events` is `None` when the project has no coordinator
/// database: the UI then reads events through the engine.
pub struct PollResult {
    pub registry: Result<TaskRegistry, String>,
    pub events: Option<macc_core::Result<Vec<CoordinatorEventRecord>>>,
    pub status: Option<macc_core::Result<CoordinatorStatus>>,
}

/// At most one poll in flight at a time.
#[derive(Default)]
pub struct CoordinatorPoller {
    in_flight: Option<Receiver<PollResult>>,
    last_status_poll: Option<Instant>,
}

impl CoordinatorPoller {
    pub fn is_busy(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Whether the next poll should also read the run status.
    pub fn status_due(&self) -> bool {
        self.last_status_poll
            .map(|at| at.elapsed() >= STATUS_POLL_INTERVAL)
            .unwrap_or(true)
    }

    /// Start a poll unless one is already running.
    pub fn start(&mut self, request: PollRequest) {
        if self.in_flight.is_some() {
            return;
        }
        if request.include_status {
            self.last_status_poll = Some(Instant::now());
        }
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("macc-tui-coordinator-poll".into())
            .spawn(move || {
                let _ = tx.send(poll(&request));
            });
        if spawned.is_ok() {
            self.in_flight = Some(rx);
        }
    }

    /// The finished poll's result, if any.
    pub fn take_result(&mut self) -> Option<PollResult> {
        let rx = self.in_flight.as_ref()?;
        match rx.try_recv() {
            Ok(result) => {
                self.in_flight = None;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.in_flight = None;
                None
            }
        }
    }
}

fn poll(request: &PollRequest) -> PollResult {
    let storage_paths = CoordinatorStoragePaths::from_project_paths(&request.paths);
    let sqlite = SqliteStorage::new(storage_paths.clone());
    let registry = match sqlite.load_registry() {
        Ok(registry) => Ok(registry),
        Err(err) if request.legacy_json_fallback => JsonStorage::new(storage_paths)
            .load_snapshot()
            .map(|snapshot| snapshot.registry)
            .map_err(|json_err| {
                format!(
                    "failed to load coordinator registry (sqlite={}, json={})",
                    err, json_err
                )
            }),
        Err(err) => Err(format!(
            "failed to load coordinator registry from sqlite: {}",
            err
        )),
    };
    let events = match sqlite.has_snapshot_data() {
        Ok(true) => Some(sqlite.load_recent_events(request.events_window)),
        Ok(false) => None,
        Err(err) => Some(Err(err)),
    };
    // The engine's status call is this same function; it is called directly
    // because the engine handle is not shareable with a worker thread.
    let status = request
        .include_status
        .then(|| macc_core::service::coordinator_workflow::get_coordinator_status(&request.paths));
    PollResult {
        registry,
        events,
        status,
    }
}

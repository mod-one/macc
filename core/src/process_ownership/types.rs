use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ClientKind {
    Tui,
    Web,
    Cli,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ClientIdentity {
    pub client_id: String,
    pub kind: ClientKind,
    pub connected_at: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ProcessKind {
    Coordinator,
    Supervisor,
    WebServer,
    TerminalSession,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ProcessHandle {
    pub kind: ProcessKind,
    pub project_root: PathBuf,
    pub pid: Option<i32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum OwnershipStatus {
    Owner,
    Viewer,
    Unregistered,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct OwnershipRecord {
    pub process: ProcessHandle,
    pub owner: Option<ClientIdentity>,
    pub viewers: Vec<ClientIdentity>,
    pub takeover_request: Option<TakeoverRequest>,
    pub started_at: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TakeoverRequest {
    pub request_id: String,
    pub requester: ClientIdentity,
    pub requested_at: String,
}

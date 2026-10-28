//! Revision-checked commands are separate from replayable runtime notifications.
mod transport;
use crate::{
    config::CoordinatorConfig,
    coordinator::{model::Task, runtime::CoordinatorRunState, types::CoordinatorEnvConfig},
    MaccError, ProjectPaths, Result,
};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{Duration, Instant},
};
pub use transport::{send, send_sync};
const BUDGET: Duration = Duration::from_secs(5);
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub macc_control: u32,
    pub command: String,
    pub request_id: String,
    pub task_id: String,
    pub expected_revision: String,
    pub evidence: String,
}
impl Request {
    pub fn new(task: &Task, evidence: &str) -> Result<Self> {
        let request = Self {
            macc_control: 1,
            command: "unblock_task".into(),
            request_id: uuid::Uuid::new_v4().to_string(),
            task_id: task.id.clone(),
            expected_revision: task_revision(task)?,
            evidence: evidence.into(),
        };
        request.validate()?;
        Ok(request)
    }
    pub fn validate(&self) -> Result<()> {
        if self.macc_control != 1
            || self.command != "unblock_task"
            || uuid::Uuid::parse_str(&self.request_id).is_err()
            || self.task_id.is_empty()
            || self.task_id.len() > 256
            || self.evidence.trim().is_empty()
            || self.evidence.len() > 16384
            || self.expected_revision.len() != 64
            || !self
                .expected_revision
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(error("Invalid unblock control request"));
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<String> {
        digest(self)
    }
}
fn digest(value: &impl Serialize) -> Result<String> {
    use sha2::{Digest, Sha256};
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|e| error(e.to_string()))?)
    ))
}
pub fn task_revision(task: &Task) -> Result<String> {
    digest(task)
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub request_id: String,
    pub error: Option<String>,
}
#[derive(Debug)]
pub struct Pending {
    request: Request,
    deadline: Instant,
    reply: tokio::sync::oneshot::Sender<Reply>,
}
pub(crate) fn is_request(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line).is_ok_and(|v| v.get("macc_control").is_some())
}
pub(crate) async fn receive(line: &str, tx: &tokio::sync::mpsc::Sender<Pending>) -> Reply {
    let request: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return Reply {
                request_id: String::new(),
                error: Some(format!("Invalid control command: {e}")),
            }
        }
    };
    let id = request.request_id.clone();
    if let Err(e) = request.validate() {
        return Reply {
            request_id: id,
            error: Some(e.to_string()),
        };
    }
    let (reply, rx) = tokio::sync::oneshot::channel();
    let pending = Pending {
        request,
        deadline: Instant::now() + BUDGET,
        reply,
    };
    if tx.try_send(pending).is_err() {
        return Reply {
            request_id: id,
            error: Some("Coordinator control queue is unavailable or full".into()),
        };
    }
    match tokio::time::timeout(BUDGET, rx).await {
        Ok(Ok(reply)) => reply,
        _ => Reply {
            request_id: id,
            error: Some(
                "Coordinator control acknowledgement timed out; reload task state before retrying"
                    .into(),
            ),
        },
    }
}
/// Called only at owner-loop safe points, never by telemetry replay or clients.
pub fn drain(
    root: &Path,
    env: &CoordinatorEnvConfig,
    config: Option<&CoordinatorConfig>,
    state: &mut CoordinatorRunState,
) {
    while let Ok(pending) = state.control_rx.try_recv() {
        if pending.reply.is_closed() || Instant::now() >= pending.deadline {
            continue;
        }
        let result = crate::service::coordinator_workflow::unblocking::apply(
            &ProjectPaths::from_root(root),
            config,
            env,
            &pending.request,
        );
        let error = result.err().map(|e| e.to_string());
        tracing::info!(request_id=%pending.request.request_id,task_id=%pending.request.task_id,success=error.is_none(),"Coordinator control command processed");
        let _ = pending.reply.send(Reply {
            request_id: pending.request.request_id,
            error,
        });
    }
}
fn error(message: impl Into<String>) -> MaccError {
    MaccError::Validation(message.into())
}
#[cfg(test)]
mod tests;

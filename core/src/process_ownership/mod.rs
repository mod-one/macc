pub mod events;
pub mod heartbeat;
pub mod store;
pub mod types;

pub use events::*;
pub use heartbeat::{
    evict_stale_clients, owner_responded_to_takeover, HeartbeatConfig, TakeoverDefaultResponse,
};
pub use store::{ownership_change_token, OwnershipStore};
pub use types::{
    ClientIdentity, ClientKind, OwnershipRecord, OwnershipStatus, ProcessHandle, ProcessKind,
    TakeoverRequest,
};

pub(crate) fn process_identity(pid: i64) -> Option<String> {
    if pid <= 0 || !crate::coordinator::helpers::is_pid_running(pid) {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        let raw = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let tail = raw
            .rsplit_once(')')?
            .1
            .split_whitespace()
            .collect::<Vec<_>>();
        if matches!(*tail.first()?, "Z" | "X") {
            return None;
        }
        let boot = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
        Some(format!("{pid}:{}:{}", boot.trim(), tail.get(19)?))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let out = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "lstart="])
            .output()
            .ok()?;
        let start = String::from_utf8(out.stdout).ok()?;
        if !out.status.success() || start.trim().is_empty() {
            return None;
        }
        Some(format!("{pid}:{}", start.trim()))
    }
}

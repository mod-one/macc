pub mod heartbeat;
pub mod store;
pub mod types;

pub use heartbeat::{HeartbeatConfig, evict_stale_clients};
pub use store::OwnershipStore;
pub use types::{
    ClientIdentity, ClientKind, OwnershipRecord, OwnershipStatus, ProcessHandle, ProcessKind,
    TakeoverRequest,
};

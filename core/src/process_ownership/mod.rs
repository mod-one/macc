pub mod store;
pub mod types;

pub use store::OwnershipStore;
pub use types::{
    ClientIdentity, ClientKind, OwnershipRecord, OwnershipStatus, ProcessHandle, ProcessKind,
    TakeoverRequest,
};

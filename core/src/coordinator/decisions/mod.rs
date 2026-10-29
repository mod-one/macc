//! Durable decision/graph authority; coordinator state and files are projections.
mod approvals;
mod backup;
pub use backup::*;
mod context;
mod graph;
mod model;
mod runtime;
mod service;
mod store;
pub use approvals::approval_ledger_for_project;
pub use model::*;
pub use runtime::*;
pub use service::*;
pub use store::{digest, Store};

pub fn error(message: impl Into<String>) -> crate::MaccError {
    crate::MaccError::Validation(format!("Decision control: {}", message.into()))
}

#[cfg(test)]
mod tests;

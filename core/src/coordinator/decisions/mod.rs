//! Durable decision/graph authority; coordinator state and files are projections.
mod approvals;
mod backup;
pub use backup::*;
mod context;
mod graph;
mod investigation;
mod model;
mod operations;
mod presentation;
mod runtime;
mod secure;
mod service;
mod store;
mod transport;
mod triage;
pub use approvals::{approval_challenge, approval_ledger_for_project};
pub use investigation::{evidence_digest, FactEvidence};
pub use model::*;
pub use presentation::graph_diff;
pub use runtime::*;
pub use secure::{answer_challenge, graph_challenge, Attestation, AttestationBody, TrustRegistry};
pub use service::*;
pub use store::{digest, Store};
pub use transport::{Archive, SignedArchive};

pub fn error(message: impl Into<String>) -> crate::MaccError {
    crate::MaccError::Validation(format!("Decision control: {}", message.into()))
}

#[cfg(test)]
mod tests;

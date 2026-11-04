//! Durable decision/graph authority; coordinator state and files are projections.
mod analytics;
mod approvals;
mod backup;
pub use backup::*;
mod context;
mod effects;
mod graph;
mod intake;
mod investigation;
pub use intake::validate_initial_requests;
mod promotion;
pub(crate) use promotion::{guard_prd_promotion, promotion_destination};
mod interaction;
pub use interaction::EvidencePermission;
mod lifecycle;
mod model;
pub use effects::effects_challenge;
pub use lifecycle::{revision_challenge, withdraw_challenge};
mod broker;
mod operations;
mod operator;
pub use broker::{ApprovalTicket, BrokerConfig};
pub use operator::OperatorIntent;
pub use secure::broker_available;
mod presentation;
mod reconciliation;
mod runtime;
mod secure;
mod service;
mod store;
mod transport;
pub use reconciliation::{ArchiveConflict, ArchiveReview, MergeSide};
mod triage;
pub use approvals::{approval_challenge, approval_ledger_for_project};
pub use investigation::{evidence_digest, FactEvidence};
pub use model::*;
pub use presentation::graph_diff;
pub use runtime::*;
pub use secure::{
    answer_challenge, graph_challenge, mutation_challenge, operation_challenge, revoke_challenge,
    Attestation, AttestationBody, TrustRegistry,
};
pub use service::*;
pub use store::{digest, Store};
pub use transport::{Archive, SignedArchive};

pub fn error(message: impl Into<String>) -> crate::MaccError {
    crate::MaccError::Validation(format!("Decision control: {}", message.into()))
}

#[cfg(test)]
mod tests;

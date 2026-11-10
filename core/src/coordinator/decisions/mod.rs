//! Durable decision/graph authority; coordinator state and files are projections.
mod analytics;
mod approvals;
mod backup;
pub use backup::*;
mod context;
mod delegation;
mod diagnostic;
pub(crate) use delegation::verified as verified_delegated_gates;
pub use diagnostic::DiagnosticDelivery;
mod context_budget;
mod context_render;
mod context_sources;
pub use context_budget::{context_budget, ContextBudget, CONTEXT_BUDGET_CODE};
mod context_identity;
pub(crate) use context_identity::validate as validate_context_identity;
mod effects;
mod graph;
mod intake;
mod investigation;
pub use intake::validate_initial_requests;
mod promotion;
pub(crate) use promotion::{guard_prd_promotion, promotion_destination};
mod interaction;
mod response;
pub use interaction::EvidencePermission;
pub use response::{intervention_response, InterventionResponse, RESPONSE_MODE};
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
mod source_recovery;
mod waiting;
pub use waiting::{
    gate_revision, human_wait_count, pending_for_task, refresh_waits, PendingDecision,
};
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
#[path = "tests_delegation.rs"]
mod delegation_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) mod tests_context_budget;

#[cfg(test)]
mod tests_diagnostic;

#[cfg(test)]
mod tests_response_wait;

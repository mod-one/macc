//! Provider-neutral O2 services; the canonical SQLite authority remains in session_store.
pub mod actions;
pub mod agent;
pub mod compaction;
pub mod context;
pub mod extraction;
pub mod mcp;
pub mod memory;
pub mod overview;
mod paths;
pub mod policy;
#[cfg(test)]
mod probe_tests;
pub mod providers;
mod schema;
pub mod semantic;
#[cfg(test)]
mod tests;
pub mod tuning;
pub use schema::{ready, upgrade};
pub(crate) fn initial_schema() -> &'static str {
    schema::SCHEMA
}
use crate::{session_store as store, Result};
use std::path::Path;
pub(crate) use store::{digest, error, now, sql};
pub(crate) fn database(root: &Path) -> Result<(rusqlite::Connection, store::Authority)> {
    let (db, a) = store::active_database(root)?;
    if !ready(&db)? {
        return Err(error(
            "O2 schema upgrade required; preview and apply before enabling O2",
        ));
    }
    Ok((db, a))
}

/// Read-side ports must not migrate or alter SQLite authority/permissions.
pub(crate) fn readonly_database(root: &Path) -> Result<(rusqlite::Connection, store::Authority)> {
    let (db, a) = store::readonly_database(root)?;
    if !ready(&db)? {
        return Err(error("O2 schema upgrade required"));
    }
    Ok((db, a))
}

pub(crate) mod context_usage;

mod context_cache;
mod context_prompt;

mod policy_fields;

#[cfg(test)]
mod integration_tests;

pub(crate) mod health;

pub mod output;

#[cfg(test)]
mod regression_tests;

#[cfg(test)]
mod acceptance_tests;
#[cfg(test)]
mod pipeline_tests;

#[cfg(test)]
mod p3_tests;

#[cfg(test)]
mod preflight_tests;

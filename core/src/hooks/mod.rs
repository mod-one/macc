//! Tool-neutral hook catalogue, activation claims and deterministic execution.
mod actions;
mod catalog;
pub mod native;
mod runtime;
mod types;
pub use actions::{execute, Action};
pub use catalog::{catalog, status, validate, HookState, Status};
pub use runtime::{evidence, selected, transform};
pub use types::*;
#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod tests;
fn error(message: impl Into<String>) -> crate::MaccError {
    crate::MaccError::Validation(message.into())
}
/// Review an external manifest before requesting an immutable import.
pub fn review(definition: Hook) -> crate::Result<ExternalHook> {
    let snapshot = ExternalHook {
        sha256: checksum(&definition)?,
        definition,
    };
    catalog::validate_external(&snapshot)?;
    if catalog(&crate::config::CanonicalConfig::default())?
        .iter()
        .any(|h| h.id == snapshot.definition.id)
    {
        return Err(error("External hook ID is reserved by MACC"));
    }
    Ok(snapshot)
}

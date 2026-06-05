pub mod builder;
pub mod snapshot;

pub use builder::RuntimeSnapshotBuilder;
pub use snapshot::*;

use crate::{ProjectPaths, Result};

/// Spec §2.11: canonical trait for snapshot consumers (CLI, TUI, Web).
/// `engine.runtime_snapshot(paths)` satisfies `current_snapshot()`; the `Engine`
/// trait is the production implementation.  This trait exists so test doubles can
/// substitute a canned snapshot without touching SQLite or git.
pub trait RuntimeSnapshotProvider {
    fn current_snapshot(&self) -> Result<RuntimeSnapshot>;
}

/// Convenience blanket: any `ProjectPaths` reference acts as a snapshot provider
/// by delegating directly to the builder.  This lets call-sites that only have a
/// `&ProjectPaths` avoid constructing a full engine.
impl RuntimeSnapshotProvider for ProjectPaths {
    fn current_snapshot(&self) -> Result<RuntimeSnapshot> {
        RuntimeSnapshotBuilder::build(self)
    }
}

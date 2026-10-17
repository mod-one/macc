//! Optional local retrieval. Gate receipts remain in the canonical O2 audit store.
mod backend;
mod benchmark;
mod gate;
mod lsa;
mod retrieval;
#[cfg(test)]
mod tests;
use super::*;
pub use backend::{Descriptor, EmbeddingBackend, EmbeddingLimits, EmbeddingSpace, Registration};
pub use benchmark::{benchmark, Case, CaseResult, Report, Suite};
pub use gate::{apply_gate, status};
use memory::{Access, Hit, Memory, Search};
use policy::MemoryPolicy;
pub(crate) use retrieval::{augment, current, expand, prepare_pool};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Default)]
pub(crate) struct Selection {
    pub scores: BTreeMap<String, f64>,
    pub diagnostic: String,
    pub key: String,
    pub event: String,
    pub canonical: String,
    pub pool: Vec<String>,
    pub query: String,
    pub settings: String,
}
pub(crate) fn settings(p: &MemoryPolicy) -> Result<String> {
    let mut config = p.clone();
    // A benchmark can be reviewed while retrieval is still disabled.
    config.semantic_enabled = false;
    Ok(digest(
        &serde_json::to_vec(&config).map_err(|e| error(e.to_string()))?,
    ))
}
fn corpus(records: &[Memory], access: &Access) -> Result<String> {
    let mut values = records
        .iter()
        .map(|m| {
            memory::hit(m.clone(), 0, "").map(|h| {
                (
                    m.id.clone(),
                    h.checksum,
                    access.task_id.is_some() && m.proposal.source_task == access.task_id,
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    values.sort();
    Ok(digest(
        &serde_json::to_vec(&values).map_err(|e| error(e.to_string()))?,
    ))
}

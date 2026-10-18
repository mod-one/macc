//! Bind confirmation to the exact actions and the file contents shown in a preview.
use crate::{
    plan::{ActionPlan, PlannedOp},
    MaccError, Result,
};
use sha2::{Digest, Sha256};
pub fn fingerprint(plan: &ActionPlan, operations: &[PlannedOp]) -> Result<String> {
    let bytes = serde_json::to_vec(&(plan, operations))
        .map_err(|e| MaccError::Validation(format!("Cannot fingerprint apply review: {e}")))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

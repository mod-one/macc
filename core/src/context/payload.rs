//! Canonical agent JSON transport: compact JSON and exactly one terminating newline.
use crate::{MaccError, Result};
use serde::Serialize;

pub fn json_line<T: Serialize>(value: &T) -> Result<String> {
    let mut text = serde_json::to_string(value)
        .map_err(|e| MaccError::Validation(format!("Context serialization failed: {e}")))?;
    text.push('\n');
    Ok(text)
}

pub fn json_tokens<T: Serialize>(value: &T) -> Result<usize> {
    Ok(super::estimate_tokens(&json_line(value)?))
}

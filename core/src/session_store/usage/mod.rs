//! Invocation-level usage. Native-format knowledge belongs to ToolSpec profiles.
mod model;
mod normalization;
mod parser;
mod persistence;
mod schema;
#[cfg(test)]
mod tests;
use super::*;
pub use model::*;
pub use parser::render;
pub use persistence::{begin, end, inspect};
pub(super) use schema::ready;
pub use schema::upgrade;
pub(super) fn initial_schema() -> &'static str {
    schema::SCHEMA
}

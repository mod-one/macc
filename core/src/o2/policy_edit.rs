//! Typed single-field edits using the shared catalog and canonical transaction.
use super::{error, policy, Result};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Serialize)]
pub struct FieldView {
    pub revision: String,
    pub field: policy::Field,
    pub value: Value,
}
#[derive(Serialize)]
pub struct Preview {
    pub revision: String,
    pub path: String,
    pub before: Value,
    pub after: Value,
    pub activation: String,
}
fn field(path: &str) -> Result<policy::Field> {
    policy::fields()
        .into_iter()
        .find(|f| f.path == path)
        .ok_or_else(|| {
            error(format!(
                "Unknown O2 field '{path}'; use macc o2 policy list"
            ))
        })
}
pub fn get(root: &Path, path: &str) -> Result<FieldView> {
    let field = field(path)?;
    let current = policy::inspect(root)?;
    let value = serde_json::to_value(current.policy).map_err(|e| error(e.to_string()))?;
    Ok(FieldView {
        revision: current.revision,
        value: value
            .pointer(&format!("/{}", path.replace('.', "/")))
            .unwrap()
            .clone(),
        field,
    })
}
fn prepare(
    root: &Path,
    path: &str,
    input: &str,
    revision: Option<&str>,
) -> Result<(Preview, policy::Policy)> {
    let field = field(path)?;
    let current = policy::inspect(root)?;
    if revision.is_some_and(|r| r != current.revision) {
        return Err(error("Configuration revision conflict; reload and review"));
    }
    let after = match field.kind.as_str() {
        "string" | "enum" => Value::String(input.into()),
        "bool" | "number" => {
            let value: Value = serde_json::from_str(input)
                .map_err(|_| error(format!("{} requires a {} value", path, field.kind)))?;
            if (field.kind == "bool" && !value.is_boolean())
                || (field.kind == "number" && !value.is_number())
            {
                return Err(error(format!("{} requires a {} value", path, field.kind)));
            }
            value
        }
        _ => return Err(error("Unsupported policy field type")),
    };
    let mut draft = serde_json::to_value(&current.policy).map_err(|e| error(e.to_string()))?;
    let slot = draft
        .pointer_mut(&format!("/{}", path.replace('.', "/")))
        .ok_or_else(|| error("Unknown policy field"))?;
    let before = std::mem::replace(slot, after.clone());
    let policy: policy::Policy =
        serde_json::from_value(draft).map_err(|e| error(format!("Invalid {path}: {e}")))?;
    policy.validate()?;
    Ok((
        Preview {
            revision: current.revision,
            path: path.into(),
            before,
            after,
            activation: field.activation,
        },
        policy,
    ))
}
pub fn preview(root: &Path, path: &str, input: &str, revision: Option<&str>) -> Result<Preview> {
    prepare(root, path, input, revision).map(|(preview, _)| preview)
}
pub fn apply(
    root: &Path,
    path: &str,
    input: &str,
    revision: &str,
    confirmed: bool,
) -> Result<policy::View> {
    if !confirmed {
        return Err(error("Confirm the reviewed O2 policy edit before applying"));
    }
    let (_, draft) = prepare(root, path, input, Some(revision))?;
    crate::config::transaction::update(
        &crate::ProjectPaths::from_root(root),
        Some(revision),
        |c| {
            // Preserve unrelated groups, including their absent/inherited state.
            match path.split('.').next().unwrap_or_default() {
                "memory" => c.memory = Some(draft.memory.clone()),
                "context_engine" => c.context_engine = Some(draft.context_engine.clone()),
                "adaptive_compaction" => {
                    c.adaptive_compaction = Some(draft.adaptive_compaction.clone())
                }
                "code_intelligence" => c.code_intelligence = Some(draft.code_intelligence.clone()),
                _ => return Err(error("Unknown O2 policy group")),
            }
            Ok(())
        },
    )?;
    policy::inspect(root)
}

#[cfg(test)]
#[path = "policy_edit_tests.rs"]
mod tests;

use super::*;
use crate::{config::transaction, ProjectPaths, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Status,
    Set {
        id: String,
        enabled: Option<bool>,
        revision: String,
    },
    Import {
        definition: Hook,
        sha256: String,
        revision: String,
    },
    Remove {
        id: String,
        revision: String,
    },
}
impl Action {
    pub fn is_mutation(&self) -> bool {
        !matches!(self, Self::Status)
    }
}
pub fn execute(root: &Path, action: Action) -> Result<Status> {
    let paths = ProjectPaths::from_root(root);
    if matches!(action, Action::Status) {
        return status(&crate::load_canonical_config(&paths.config_path)?);
    }
    let revision = match &action {
        Action::Set { revision, .. }
        | Action::Import { revision, .. }
        | Action::Remove { revision, .. } => revision.clone(),
        Action::Status => unreachable!(),
    };
    let (operation, hook_id) = match &action {
        Action::Set { id, .. } => ("set", id.clone()),
        Action::Import { definition, .. } => ("import", definition.id.clone()),
        Action::Remove { id, .. } => ("remove", id.clone()),
        Action::Status => unreachable!(),
    };
    let config = transaction::update(&paths, Some(&revision), |config| {
        let known = catalog(config)?;
        let states = status(config)?;
        let requested_id = match &action {
            Action::Set {
                id,
                enabled: Some(false),
                ..
            }
            | Action::Remove { id, .. } => Some(id),
            _ => None,
        };
        if let Some(id) = requested_id {
            if let Some(hook) = states
                .hooks
                .iter()
                .find(|h| h.hook.id == *id && !h.required_by.is_empty())
            {
                return Err(error(format!("Hook {id} is required by {}. Disable the owning feature or output contract first.",hook.required_by.join(", "))));
            }
        }
        let settings = config.hooks.get_or_insert_with(Default::default);
        match action {
            Action::Set { id, enabled, .. } => {
                if !known.iter().any(|h| h.id == id) {
                    return Err(error(format!("Unknown hook: {id}")));
                }
                if let Some(enabled) = enabled {
                    settings.overrides.insert(id, enabled);
                } else {
                    settings.overrides.remove(&id);
                }
            }
            Action::Import {
                definition, sha256, ..
            } => {
                if known.iter().any(|h| h.id == definition.id) {
                    return Err(error("Hook ID already exists; remove the external snapshot explicitly before importing a replacement"));
                }
                settings.external.push(ExternalHook { definition, sha256 });
            }
            Action::Remove { id, .. } => {
                if !settings.external.iter().any(|h| h.definition.id == id) {
                    return Err(error("Only external hook snapshots can be removed"));
                }
                settings.external.retain(|h| h.definition.id != id);
                settings.overrides.remove(&id);
            }
            Action::Status => unreachable!(),
        }
        validate(config)
    })?;
    tracing::info!(operation, hook=%hook_id, "Hook configuration updated with revision check");
    status(&config)
}

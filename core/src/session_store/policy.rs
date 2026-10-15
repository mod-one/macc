use super::*;
use crate::{config::CanonicalConfig, ProjectPaths};
use std::path::Path;
pub(super) fn load(root: &Path) -> Result<SessionPolicy> {
    let paths = ProjectPaths::from_root(root);
    let config = if paths.config_path.exists() {
        crate::load_canonical_config(&paths.config_path)?
    } else {
        CanonicalConfig::default()
    };
    let p = config.session_management.unwrap_or_default();
    p.validate()?;
    Ok(p)
}
/// One compare-and-swap transaction on the canonical configuration. Clients use
/// this instead of maintaining an independent session settings file.
pub fn update_policy(root: &Path, expected: &str, policy: &SessionPolicy) -> Result<StoreView> {
    replace_policy(root, expected, Some(policy))
}
pub fn remove_policy_override(root: &Path, expected: &str) -> Result<StoreView> {
    replace_policy(root, expected, None)
}
fn replace_policy(
    root: &Path,
    expected: &str,
    policy: Option<&SessionPolicy>,
) -> Result<StoreView> {
    if let Some(p) = policy {
        p.validate()?;
    }
    let paths = ProjectPaths::from_root(root);
    crate::config::transaction::update(&paths, Some(expected), |config| {
        config.session_management = policy.cloned();
        Ok(())
    })?;
    inspect(root)
}
pub fn inspect(root: &Path) -> Result<StoreView> {
    let _guard = lock(root)?;
    let paths = ProjectPaths::from_root(root);
    let config = if paths.config_path.exists() {
        crate::load_canonical_config(&paths.config_path)?
    } else {
        CanonicalConfig::default()
    };
    let revision = crate::config::transaction::revision(&config)?;
    let policy_source = if config.session_management.is_some() {
        "project"
    } else {
        "defaults"
    }
    .to_string();
    let policy = config.session_management.clone().unwrap_or_default();
    policy.validate()?;
    let a = authority(root)?;
    let sqlite = a.as_ref().is_some_and(|a| a.mode == "sqlite");
    let sessions = if sqlite {
        let (db, _) = db::active(root)?;
        db::list(&db)?
    } else {
        migration::legacy_views(root)?
    };
    Ok(StoreView {
        project_root: root
            .canonicalize()
            .map_err(|e| error(e.to_string()))?
            .display()
            .to_string(),
        backend: if sqlite { "sqlite" } else { "json" }.into(),
        project_id: a.map(|a| a.project_id),
        revision,
        policy,
        policy_source,
        fields: fields(),
        sessions,
        checkpoints_ready: if sqlite {
            let (db, _) = db::active(root)?;
            checkpoints::ready(&db)?
        } else {
            false
        },
        checkpoints: if sqlite {
            let (db, _) = db::active(root)?;
            checkpoints::list(&db)?
        } else {
            vec![]
        },
    })
}

//! Reviewed apply prerequisite shared by CLI, TUI and Web engines.
use super::*;
use serde::{Deserialize, Serialize};

/// Persistent user history is not generated adapter output, even if an old
/// managed-path manifest accidentally listed it.
pub(crate) fn retained_state(relative: &str) -> bool {
    matches!(
        relative,
        ".macc/memory.db"
            | ".macc/memory.db-wal"
            | ".macc/memory.db-shm"
            | ".macc/state/tool-sessions.json"
            | ".macc/state/tool-sessions.json.lock"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Activation {
    pub config_revision: String,
    pub operation: String,
    pub source_sha256: Option<String>,
}

pub fn requested(config: &crate::config::CanonicalConfig) -> bool {
    let p = policy::from_config(config);
    p.memory.enabled
        || p.context_engine.enabled
        || p.adaptive_compaction.enabled
        || p.code_intelligence.enabled
        || config
            .session_management
            .as_ref()
            .is_some_and(|policy| policy.usage.capture_mode > 0)
}

/// Preview only. Existing ownership, identity and lease checks remain authoritative.
pub fn preview(root: &Path) -> Result<Option<Activation>> {
    // Managed worker adapter setup never owns a second session database. Its
    // caller checks the repository authority; runners receive that repository.
    if root.join(".macc/worktree.json").is_file() {
        return Ok(None);
    }
    let paths = crate::ProjectPaths::from_root(root);
    if !paths.config_path.exists() {
        return Ok(None);
    }
    let config = crate::load_canonical_config(&paths.config_path)?;
    policy::from_config(&config).validate()?;
    if !requested(&config) {
        return Ok(None);
    }
    let config_revision = store::policy_revision(&config)?;
    let (operation, source_sha256) = if !store::is_sqlite(root)? {
        let report = store::migrate(root, false, None)?;
        (
            "migrate sessions to SQLite and install O2 (backup legacy sessions)",
            Some(report.source_sha256),
        )
    } else {
        let (db, _) = store::readonly_database(root)?;
        if ready(&db)? {
            ("O2 ready", None)
        } else {
            drop(db);
            let report = upgrade(root, false, None)?;
            (
                "upgrade O2 schema (backup SQLite)",
                Some(report.source_sha256),
            )
        }
    };
    Ok(Some(Activation {
        config_revision,
        operation: operation.into(),
        source_sha256,
    }))
}

pub fn apply(root: &Path, reviewed: &Activation) -> Result<()> {
    if preview(root)?.as_ref() != Some(reviewed) {
        return Err(error(
            "O2 activation changed after preview; review macc apply again",
        ));
    }
    if let Some(source) = reviewed.source_sha256.as_deref() {
        if store::is_sqlite(root)? {
            upgrade(root, true, Some(source))?;
        } else {
            store::migrate(root, true, Some(source))?;
        }
    }
    require_ready(root)
}

pub fn require_ready(root: &Path) -> Result<()> {
    let paths = crate::ProjectPaths::from_root(root);
    if !paths.config_path.exists() {
        return Ok(());
    }
    let config = crate::load_canonical_config(&paths.config_path)?;
    if requested(&config) {
        super::readonly_database(root).map_err(|e| error(format!(
            "Configured O2 features are not operational: {e}. Run macc apply and confirm the SQLite/O2 activation preview before starting the coordinator."
        )))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "activation_tests.rs"]
mod tests;

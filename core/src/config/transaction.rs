//! Canonical configuration transactions shared by interactive clients.
use crate::{config::CanonicalConfig, fs_lock::AdvisoryLock, MaccError, ProjectPaths, Result};
use std::time::Duration;
/// Stable revision of the complete canonical configuration.
pub fn revision(config: &CanonicalConfig) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(config)
        .map_err(|e| MaccError::Validation(format!("Serialize configuration revision: {e}")))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
pub fn update(
    paths: &ProjectPaths,
    expected: Option<&str>,
    edit: impl FnOnce(&mut CanonicalConfig) -> Result<()>,
) -> Result<CanonicalConfig> {
    let _guard = AdvisoryLock::acquire_with_retry(
        &paths.config_path.with_extension("yaml.lock"),
        Duration::from_secs(5),
        Duration::from_millis(50),
        "canonical configuration",
    )?;
    let mut config = if paths.config_path.exists() {
        crate::load_canonical_config(&paths.config_path)?
    } else {
        CanonicalConfig::default()
    };
    if let Some(revision) = expected {
        if revision != self::revision(&config)? {
            return Err(MaccError::Validation(
                "Configuration revision conflict; reload and review your draft".into(),
            ));
        }
    }
    edit(&mut config)?;
    crate::save_canonical_config(paths, &config)?;
    Ok(config)
}

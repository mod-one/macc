//! Explicit validation commands shared by supervisor readiness and execution.
use super::SupervisorConfig;
use crate::{MaccError, Result};
use std::path::Path;

pub fn commands(_root: &Path, config: &SupervisorConfig) -> Result<Vec<String>> {
    config.validate()?;
    if !config.tests_enabled {
        return Ok(Vec::new());
    }
    if config.validation_commands.is_empty() {
        return Err(MaccError::Validation(
            super::settings::MISSING_TEST_COMMANDS.into(),
        ));
    }
    Ok(config.validation_commands.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_commands_default_to_disabled_even_with_a_root_workflow() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        assert!(commands(dir.path(), &SupervisorConfig::default())
            .unwrap()
            .is_empty());
        let mut config = SupervisorConfig::default();
        config.tests_enabled = true;
        assert!(commands(dir.path(), &config)
            .unwrap_err()
            .to_string()
            .contains("Add at least one validation command"));
        config.validation_commands = vec!["cd web && npm test".into()];
        assert_eq!(
            commands(dir.path(), &config).unwrap(),
            config.validation_commands
        );
        config.tests_enabled = false;
        assert!(commands(dir.path(), &config).unwrap().is_empty());
    }
}

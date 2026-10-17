//! Project validation discovery shared by supervisor readiness and execution.
use super::SupervisorConfig;
use crate::{MaccError, Result};
use std::path::Path;
fn io(path: &Path, source: std::io::Error) -> MaccError {
    MaccError::Io {
        path: path.display().to_string(),
        action: "discover supervisor validation".into(),
        source,
    }
}
pub fn commands(root: &Path, config: &SupervisorConfig) -> Result<Vec<String>> {
    if !config.tests_enabled {
        return Ok(Vec::new());
    }
    if !config.validation_commands.is_empty() {
        return Ok(config.validation_commands.clone());
    }
    if root.join("Makefile").exists()
        && std::fs::read_to_string(root.join("Makefile"))
            .map_err(|e| io(root, e))?
            .lines()
            .any(|l| l.starts_with("check:"))
    {
        return Ok(vec!["make check".into()]);
    }
    if root.join("Cargo.toml").exists() {
        return Ok(vec!["cargo test --workspace --locked".into()]);
    }
    let package = root.join("package.json");
    if package.exists() {
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&package).map_err(|e| io(&package, e))?)
                .map_err(|e| MaccError::Validation(format!("Invalid package.json: {e}")))?;
        let manager = if root.join("pnpm-lock.yaml").exists() {
            "pnpm"
        } else {
            "npm"
        };
        let scripts = v
            .get("scripts")
            .and_then(|s| s.as_object())
            .ok_or_else(|| MaccError::Validation("No project validation scripts".into()))?;
        if !scripts.contains_key("test") && !scripts.contains_key("test:e2e") {
            return Err(MaccError::Validation(
                "Supervisor requires a test command; configure validation_commands".into(),
            ));
        }
        return Ok([
            "lint",
            "typecheck",
            "build",
            "test",
            "test:e2e",
            "contracts:check",
        ]
        .iter()
        .filter(|s| scripts.contains_key(**s))
        .map(|s| format!("{manager} run {s}"))
        .collect());
    }
    Err(MaccError::Validation(
        "Supervisor requires validation_commands for this project".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn monorepo_requires_commands_only_when_tests_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = SupervisorConfig::default();
        assert!(commands(dir.path(), &config).is_err());
        config.tests_enabled = false;
        assert!(commands(dir.path(), &config).unwrap().is_empty());
        config.tests_enabled = true;
        config.validation_commands = vec!["cd web && npm test".into()];
        assert_eq!(
            commands(dir.path(), &config).unwrap(),
            config.validation_commands
        );
    }
    #[test]
    fn discovery_includes_test_and_uses_root_package_manager() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"build":"build", "test":"test"}}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(
            commands(dir.path(), &SupervisorConfig::default()).unwrap(),
            vec!["pnpm run build", "pnpm run test"]
        );
    }
}

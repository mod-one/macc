//! Shared supervisor settings contract for CLI and interactive clients.
use super::SupervisorConfig;
use crate::{MaccError, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Component, Path};

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: &'static str,
    pub help: &'static str,
}
macro_rules! fields {
    ($(($key:literal, $label:literal, $kind:literal, $help:literal)),* $(,)?) => {
        pub const FIELDS: &[Field] = &[$(Field { key: $key, label: $label, kind: $kind, help: $help }),*];
    };
}
fields![
    ("tool", "AI tool", "optional", "Empty: inherit coordinator tool, then first enabled tool. Uses ToolSpec adapters."),
    ("model", "Model override", "optional", "Empty: inherit the selected tool model."),
    ("effort", "Effort override", "optional", "Empty: inherit the selected tool effort. Use a value supported by that tool."),
    ("repair_enabled", "Automatic repair", "bool", "Off: diagnose only; keep the incident waiting without editing source."),
    ("tests_enabled", "Tests and checks", "bool", "Adding validation commands enables tests; clearing them disables tests. Enabling tests manually requires at least one command. Off: integration may proceed without test evidence."),
    ("ai_verification_enabled", "AI verification", "bool", "Off: skip the separate AI review; integration relies on the remaining enabled checks."),
    ("auto_restart", "Automatic restart", "bool", "Off: integrate a successful repair and requeue tasks, but leave the coordinator stopped. Also disables restart-only recovery."),
    ("validation_commands", "Validation commands", "commands", "Commands run from the isolated worktree root. Adding commands enables tests; an empty list disables tests. Use relative subproject paths. TUI: separate commands with ;; ."),
    ("intervention_timeout_seconds", "AI call timeout (seconds)", "number", "Positive timeout per AI invocation."),
    ("validation_timeout_seconds", "Check timeout (seconds)", "number", "Positive timeout per validation command."),
    ("max_repair_rounds", "Repair rounds", "number", "Maximum repair/check/review rounds per attempt; at least 1."),
    ("max_restart_attempts", "Intervention attempts", "number", "Maximum attempts per incident context; unchanged permanent failures wait for new evidence."),
    ("watchdog_interval_seconds", "Watchdog interval (seconds)", "number", "Positive delay between monitoring cycles."),
    ("log_analysis_window_seconds", "Analysis window (seconds)", "number", "Also controls stalled-tick detection, with a 30-second minimum threshold."),
    ("report_output_path", "Report path", "path", "JSON report path; relative paths start at the project root. Absolute paths remain supported."),
    ("crash_debounce_checks", "Legacy crash debounce", "number", "Compatibility setting for legacy supervisors; unused by the canonical incident loop."),
    ("events_log_path", "Legacy events path", "path", "Compatibility path; unused by the canonical SQLite incident loop."),
];

pub const MISSING_TEST_COMMANDS: &str = "Tests cannot be enabled without validation_commands. Add at least one validation command, or leave tests_enabled=false.";

impl SupervisorConfig {
    pub fn validate(&self) -> Result<()> {
        let value = serde_json::to_value(self).map_err(|e| MaccError::Validation(e.to_string()))?;
        for field in FIELDS {
            let v = &value[field.key];
            let invalid = match field.kind {
                "number" => v.as_u64().is_none_or(|n| n == 0),
                "optional" => v.as_str().is_some_and(|s| s.trim().is_empty()),
                "path" => v.as_str().is_none_or(|s| {
                    s.trim().is_empty()
                        || Path::new(s).file_name().is_none()
                        || !Path::new(s).components().all(|c| {
                            matches!(
                                c,
                                Component::Normal(_)
                                    | Component::CurDir
                                    | Component::RootDir
                                    | Component::Prefix(_)
                            )
                        })
                }),
                _ => false,
            };
            if invalid {
                return Err(MaccError::Validation(format!(
                    "Invalid automation.supervisor.{}: {}",
                    field.key, field.help
                )));
            }
        }
        if self
            .validation_commands
            .iter()
            .any(|c| c.trim().is_empty() || c.contains('\0'))
        {
            return Err(MaccError::Validation(
                "Supervisor validation commands must not be blank or contain NUL".into(),
            ));
        }
        Ok(())
    }
}

/// Merge a typed JSON patch atomically; omitted fields keep their saved values.
pub fn merge(config: &mut SupervisorConfig, patch: &Value) -> Result<()> {
    let object = patch
        .as_object()
        .ok_or_else(|| MaccError::Validation("Supervisor settings must be an object".into()))?;
    let mut draft =
        serde_json::to_value(&*config).map_err(|e| MaccError::Validation(e.to_string()))?;
    for (key, value) in object {
        if !FIELDS.iter().any(|f| f.key == key) {
            return Err(MaccError::Validation(format!(
                "Unknown supervisor setting: {key}"
            )));
        }
        draft[key] = value.clone();
    }
    if object.contains_key("validation_commands") && !object.contains_key("tests_enabled") {
        if let Some(commands) = draft["validation_commands"].as_array() {
            draft["tests_enabled"] = json!(!commands.is_empty());
        }
    }
    let next: SupervisorConfig = serde_json::from_value(draft)
        .map_err(|e| MaccError::Validation(format!("Invalid supervisor settings: {e}")))?;
    next.validate()?;
    if object.get("tests_enabled") == Some(&json!(true)) && next.validation_commands.is_empty() {
        return Err(MaccError::Validation(MISSING_TEST_COMMANDS.into()));
    }
    *config = next;
    Ok(())
}

/// Parse one user-entered value without discarding the previous valid draft.
/// Commands support a JSON array (CLI) or the interactive `;;` separator.
pub fn set(config: &mut SupervisorConfig, key: &str, input: &str) -> Result<()> {
    let field = FIELDS
        .iter()
        .find(|f| f.key == key)
        .ok_or_else(|| MaccError::Validation(format!("Unknown supervisor setting: {key}")))?;
    let input = input.trim();
    let error = || MaccError::Validation(format!("Invalid {key}: {}", field.help));
    let value = match field.kind {
        "bool" => json!(input.parse::<bool>().map_err(|_| error())?),
        "number" => json!(input.parse::<u64>().map_err(|_| error())?),
        "optional" if input.is_empty() || input == "null" => Value::Null,
        "commands" if input.starts_with('[') => {
            json!(serde_json::from_str::<Vec<String>>(input).map_err(|_| error())?)
        }
        "commands" => json!(if input.is_empty() {
            Vec::new()
        } else {
            input.split(";;").map(|s| s.trim().to_owned()).collect()
        }),
        _ => json!(input),
    };
    merge(config, &json!({(key): value}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_configs_infer_tests_from_commands_and_preserve_explicit_choices() {
        let c: SupervisorConfig = serde_json::from_str("{}").unwrap();
        assert!(!c.tests_enabled);
        assert!(c.ai_verification_enabled && c.repair_enabled && c.auto_restart);
        c.validate().unwrap();
        let c: SupervisorConfig =
            serde_yaml::from_str("validation_commands: ['npm test']").unwrap();
        assert!(c.tests_enabled);
        let c: SupervisorConfig =
            serde_yaml::from_str("validation_commands: ['npm test']\ntests_enabled: false")
                .unwrap();
        assert!(!c.tests_enabled);
        assert_eq!(
            c,
            serde_json::from_value(serde_json::to_value(&c).unwrap()).unwrap()
        );
        let c: SupervisorConfig = serde_yaml::from_str("tests_enabled: true").unwrap();
        assert!(!c.tests_enabled);
        assert!(super::super::validation::commands(Path::new("."), &c)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn editing_is_atomic_and_supports_subprojects() {
        let mut c = SupervisorConfig::default();
        set(
            &mut c,
            "validation_commands",
            "cd web && npm test ;; cargo test --manifest-path service/Cargo.toml",
        )
        .unwrap();
        assert_eq!(c.validation_commands.len(), 2);
        let before = c.clone();
        for (k, v) in [
            ("max_repair_rounds", "0"),
            ("report_output_path", "../outside.json"),
            ("tests_enabled", "yes"),
            ("validation_commands", "[\"\"]"),
        ] {
            assert!(set(&mut c, k, v).is_err());
            assert_eq!(c, before);
        }
        set(&mut c, "tool", "").unwrap();
        assert_eq!(c.tool, None);
    }

    #[test]
    fn command_edits_toggle_tests_and_manual_activation_requires_commands() {
        let mut c = SupervisorConfig::default();
        let before = c.clone();
        let message = set(&mut c, "tests_enabled", "true")
            .unwrap_err()
            .to_string();
        assert!(message.contains("Add at least one validation command"));
        assert_eq!(c, before);
        set(&mut c, "validation_commands", "npm test").unwrap();
        assert!(c.tests_enabled);
        set(&mut c, "tests_enabled", "false").unwrap();
        assert!(!c.tests_enabled);
        merge(&mut c, &json!({"validation_commands":["npm run check"]})).unwrap();
        assert!(c.tests_enabled);
        merge(&mut c, &json!({"validation_commands":[]})).unwrap();
        assert!(!c.tests_enabled);
        let before = c.clone();
        assert!(merge(
            &mut c,
            &json!({"tests_enabled":true,"validation_commands":[]})
        )
        .is_err());
        assert_eq!(c, before);
        assert!(c.ai_verification_enabled && c.repair_enabled && c.auto_restart);
    }
}

//! Supervisor settings editor, separated from the main state/navigation module.
use super::*;
use macc_core::supervisor::{settings, SupervisorConfig};

impl AppState {
    fn supervisor_config(&self) -> SupervisorConfig {
        self.working_copy
            .as_ref()
            .and_then(|c| c.automation.supervisor.clone())
            .unwrap_or_default()
    }
    pub fn supervisor_field_value(&self, index: usize) -> String {
        let Some(field) = settings::FIELDS.get(index) else {
            return String::new();
        };
        let value = serde_json::to_value(self.supervisor_config()).unwrap_or_default();
        match &value[field.key] {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            Value::Array(a) if a.iter().filter_map(Value::as_str).any(|c| c.contains(";;")) => {
                value[field.key].to_string()
            }
            Value::Array(a) => a
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ;; "),
            v => v.to_string(),
        }
    }
    pub fn edit_supervisor_field(&mut self, index: usize, toggle: bool) {
        let Some(field) = settings::FIELDS.get(index) else {
            return;
        };
        if toggle && field.kind == "commands" {
            self.open_supervisor_commands();
        } else if toggle && field.kind == "bool" {
            let value = self.supervisor_field_value(index) != "true";
            self.save_supervisor_field(index, &value.to_string());
        } else if toggle
            && matches!(field.key, "tool" | "model" | "effort")
            && self.supervisor_choices(field.key).len() > 1
        {
            let choices = self.supervisor_choices(field.key);
            let current = self.supervisor_field_value(index);
            let pos = choices.iter().position(|v| *v == current).unwrap_or(0);
            self.save_supervisor_field(index, &choices[(pos + 1) % choices.len()]);
        } else {
            self.settings_field_input = self.supervisor_field_value(index);
            self.settings_field_editing = true;
        }
    }
    fn supervisor_choices(&self, key: &str) -> Vec<String> {
        let mut choices = vec![String::new()];
        let Some(canonical) = &self.working_copy else {
            return choices;
        };
        if key == "tool" {
            choices.extend(canonical.tools.enabled.clone());
            return choices;
        }
        let tool = self
            .supervisor_config()
            .tool
            .or_else(|| {
                canonical
                    .automation
                    .coordinator
                    .as_ref()
                    .and_then(|c| c.coordinator_tool.clone())
            })
            .or_else(|| canonical.tools.enabled.first().cloned());
        if let Some(descriptor) = self
            .tool_descriptors
            .iter()
            .find(|d| Some(&d.id) == tool.as_ref())
        {
            if let Some(field) = descriptor.fields.iter().find(|f| {
                if key == "model" {
                    f.id == "model"
                } else {
                    f.id.to_ascii_lowercase().contains("effort")
                }
            }) {
                if let FieldKind::Enum(options) = &field.kind {
                    choices.extend(options.clone());
                }
            }
        }
        choices
    }
    pub fn supervisor_field_info(&self, index: usize) -> String {
        let field = &settings::FIELDS[index];
        let config = self.supervisor_config();
        let readiness = self
            .project_paths
            .as_ref()
            .map(|p| {
                if !config.repair_enabled {
                    return "Diagnosis only; automatic repair disabled".into();
                }
                match macc_core::supervisor::validation::commands(&p.root, &config) {
                    Ok(commands) if commands.is_empty() => if config.validation_commands.is_empty() {
                        "Tests disabled: validation_commands is empty. Add a command to enable tests.".into()
                    } else { "Tests disabled".into() },
                    Ok(commands) => format!("Checks (not executed):\n{}", commands.join("\n")),
                    Err(error) => format!("Repair not ready: {error}"),
                }
            })
            .unwrap_or_default();
        let choices = self.supervisor_choices(field.key);
        let choices = if choices.len() > 1 {
            format!("\nChoices: inherit, {}", choices[1..].join(", "))
        } else {
            String::new()
        };
        format!("{}\n\n{}\n\nValue: {}{}\n\n{}\n\nEnter/Space: toggle or edit. Left/Right: adjust. e: exact entry. Empty overrides inherit. Save to apply.\n\nTests: {}; AI verification: {}; automatic restart: {}", field.label, field.help, self.supervisor_field_value(index), choices, readiness, config.tests_enabled, config.ai_verification_enabled, config.auto_restart)
    }
    pub(super) fn save_supervisor_field(&mut self, index: usize, input: &str) -> bool {
        let Some(field) = settings::FIELDS.get(index) else {
            return false;
        };
        let mut config = self.supervisor_config();
        if let Err(error) = settings::set(&mut config, field.key, input) {
            self.set_status(UiStatusLevel::Error, error.to_string(), None);
            return false;
        }
        self.snapshot_before_config_change();
        if let Some(c) = &mut self.working_copy {
            c.automation.supervisor = Some(config.clone());
        }
        let warning = self
            .project_paths
            .as_ref()
            .filter(|_| config.repair_enabled)
            .and_then(|p| macc_core::supervisor::validation::commands(&p.root, &config).err());
        if let Some(error) = warning {
            self.set_status(UiStatusLevel::Warning, format!("Draft updated; repair not ready: {error}. Configure Validation commands, then save."), None);
        } else {
            let message = if field.key == "validation_commands" {
                format!(
                    "Validation commands updated; tests_enabled={}. Save configuration to apply.",
                    config.tests_enabled
                )
            } else {
                "Supervisor draft updated. Save configuration to apply; active interventions keep their current settings.".into()
            };
            self.set_status(UiStatusLevel::Info, message, Some(Duration::from_secs(5)));
        }
        true
    }
    pub fn commit_supervisor_edit(&mut self, index: usize) {
        let input = self.settings_field_input.clone();
        if self.save_supervisor_field(index, &input) {
            self.settings_field_editing = false;
        }
    }
    /// Arrow keys adjust bounded numbers or cycle tools/toggles without typing.
    pub fn adjust_supervisor_field(&mut self, index: usize, increase: bool) {
        let Some(field) = settings::FIELDS.get(index) else {
            return;
        };
        if field.kind == "number" {
            let n = self
                .supervisor_field_value(index)
                .parse::<u64>()
                .unwrap_or(1);
            let next = if increase {
                n.saturating_add(1)
            } else {
                n.saturating_sub(1).max(1)
            };
            self.save_supervisor_field(index, &next.to_string());
        } else if field.kind == "bool" || matches!(field.key, "tool" | "model" | "effort") {
            self.edit_supervisor_field(index, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::{MaccEngine, ToolRegistry};
    #[test]
    fn supervisor_tab_edits_are_typed_and_undoable() {
        let mut state = AppState::with_engine(Arc::new(MaccEngine::new(ToolRegistry::new())));
        let mut config = CanonicalConfig::default();
        config.tools.enabled = vec!["fake".into()];
        state.working_copy = Some(config);
        state.jump_config_tab(6);
        assert_eq!(AppState::config_tab_fields(6).len(), settings::FIELDS.len());
        state.toggle_current_config_field();
        assert_eq!(state.supervisor_config().tool.as_deref(), Some("fake"));
        let edits_before_rejected_toggle = state.undo_stack.len();
        let tests_index = settings::FIELDS
            .iter()
            .position(|f| f.key == "tests_enabled")
            .unwrap();
        state.config_view_index = tests_index;
        state.toggle_current_config_field();
        assert!(!state.supervisor_config().tests_enabled);
        assert_eq!(state.undo_stack.len(), edits_before_rejected_toggle);
        assert!(state
            .ui_status
            .as_ref()
            .is_some_and(|status| status.message.contains("validation command")));
        let rounds_index = settings::FIELDS
            .iter()
            .position(|f| f.key == "max_repair_rounds")
            .unwrap();
        state.config_view_index = rounds_index;
        state.begin_current_config_edit();
        state.settings_field_input = "0".into();
        state.commit_config_edit();
        assert!(
            state.settings_field_editing,
            "invalid edit remains open for correction"
        );
        assert_eq!(state.supervisor_config().max_repair_rounds, 3);
        state.settings_field_input = "2".into();
        state.commit_config_edit();
        assert!(!state.settings_field_editing);
        assert_eq!(state.supervisor_config().max_repair_rounds, 2);
        assert_eq!(state.undo_stack.len(), edits_before_rejected_toggle + 1);
        state.undo_config_change();
        assert_eq!(state.supervisor_config().max_repair_rounds, 3);
    }
}

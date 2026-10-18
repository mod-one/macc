//! Interactive validation-command rows; no shell execution in the editor.
use super::*;
use crossterm::event::KeyCode;

pub(super) struct CommandEditor {
    commands: Vec<String>,
    selected: usize,
    input: Option<String>,
}
impl AppState {
    pub fn supervisor_command_bindings(&self) -> Vec<(&'static str, &'static str)> {
        if self
            .supervisor_commands_editor
            .as_ref()
            .is_some_and(|e| e.input.is_some())
        {
            return vec![
                ("Enter", "Accept command"),
                ("Backspace", "Delete character"),
                ("Esc", "Cancel input"),
            ];
        }
        vec![
            ("↑↓", "Select command"),
            ("←→", "Reorder"),
            ("a", "Add command"),
            ("Enter", "Edit command"),
            ("d", "Remove command"),
            ("s", "Accept draft"),
            ("Esc", "Cancel"),
        ]
    }
    pub fn supervisor_commands_active(&self) -> bool {
        self.supervisor_commands_editor.is_some()
    }
    pub(super) fn open_supervisor_commands(&mut self) {
        let commands = self
            .working_copy
            .as_ref()
            .and_then(|c| c.automation.supervisor.as_ref())
            .map(|c| c.validation_commands.clone())
            .unwrap_or_default();
        self.supervisor_commands_editor = Some(CommandEditor {
            commands,
            selected: 0,
            input: None,
        });
    }
    pub fn supervisor_commands_key(&mut self, key: KeyCode) {
        let Some(editor) = &mut self.supervisor_commands_editor else {
            return;
        };
        if let Some(input) = &mut editor.input {
            match key {
                KeyCode::Char(ch) => input.push(ch),
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Esc => editor.input = None,
                KeyCode::Enter if !input.trim().is_empty() => {
                    if editor.selected == editor.commands.len() {
                        editor.commands.push(input.trim().to_string());
                    } else {
                        editor.commands[editor.selected] = input.trim().to_string();
                    }
                    editor.input = None;
                }
                _ => {}
            }
            return;
        }
        match key {
            KeyCode::Up => editor.selected = editor.selected.saturating_sub(1),
            KeyCode::Down => {
                editor.selected = (editor.selected + 1).min(editor.commands.len().saturating_sub(1))
            }
            KeyCode::Char('a') => {
                editor.selected = editor.commands.len();
                editor.input = Some(String::new());
            }
            KeyCode::Enter if editor.selected < editor.commands.len() => {
                editor.input = Some(editor.commands[editor.selected].clone())
            }
            KeyCode::Char('d') if editor.selected < editor.commands.len() => {
                editor.commands.remove(editor.selected);
                editor.selected = editor.selected.min(editor.commands.len().saturating_sub(1));
            }
            KeyCode::Left if editor.selected > 0 && editor.selected < editor.commands.len() => {
                editor.commands.swap(editor.selected - 1, editor.selected);
                editor.selected -= 1;
            }
            KeyCode::Right if editor.selected + 1 < editor.commands.len() => {
                editor.commands.swap(editor.selected, editor.selected + 1);
                editor.selected += 1;
            }
            KeyCode::Char('s') => {
                let input = serde_json::to_string(&editor.commands).expect("string list");
                let index = macc_core::supervisor::settings::FIELDS
                    .iter()
                    .position(|f| f.key == "validation_commands")
                    .expect("commands field");
                if self.save_supervisor_field(index, &input) {
                    self.supervisor_commands_editor = None;
                }
            }
            KeyCode::Esc => self.supervisor_commands_editor = None,
            _ => {}
        }
    }
    pub fn supervisor_commands_info(&self) -> Option<String> {
        let editor = self.supervisor_commands_editor.as_ref()?;
        let mut lines = vec![
            "Validation commands (draft)".to_string(),
            "a: add | Enter: edit | d: remove | Left/Right: reorder | s: accept | Esc: cancel"
                .into(),
            "After accepting, save the project configuration to apply.".into(),
            String::new(),
        ];
        if editor.commands.is_empty() {
            lines.push(
                "Empty list: discover the root test workflow. Press a to add a subproject command."
                    .into(),
            );
        }
        for (i, command) in editor.commands.iter().enumerate() {
            lines.push(format!(
                "{} {}. {}",
                if i == editor.selected { ">" } else { " " },
                i + 1,
                command
            ));
        }
        if let Some(input) = &editor.input {
            lines.push(format!(
                "\nCommand: {input}_\nEnter confirms a nonblank command; Esc cancels this edit."
            ));
        }
        Some(lines.join("\n"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use macc_core::{MaccEngine, ToolRegistry};
    #[test]
    fn command_rows_add_edit_remove_and_cancel_without_formatting_json() {
        let mut state = AppState::with_engine(Arc::new(MaccEngine::new(ToolRegistry::new())));
        state.working_copy = Some(CanonicalConfig::default());
        state.open_supervisor_commands();
        state.supervisor_commands_key(KeyCode::Char('a'));
        state.supervisor_commands_key(KeyCode::Enter); // blank input stays open
        for ch in "cd web && npm test".chars() {
            state.supervisor_commands_key(KeyCode::Char(ch));
        }
        state.supervisor_commands_key(KeyCode::Enter);
        state.supervisor_commands_key(KeyCode::Char('s'));
        assert!(!state.supervisor_commands_active());
        assert_eq!(
            state
                .working_copy
                .as_ref()
                .unwrap()
                .automation
                .supervisor
                .as_ref()
                .unwrap()
                .validation_commands,
            vec!["cd web && npm test"]
        );
        state.open_supervisor_commands();
        state.supervisor_commands_key(KeyCode::Char('d'));
        state.supervisor_commands_key(KeyCode::Esc);
        assert_eq!(
            state
                .working_copy
                .as_ref()
                .unwrap()
                .automation
                .supervisor
                .as_ref()
                .unwrap()
                .validation_commands
                .len(),
            1
        );
    }
}
